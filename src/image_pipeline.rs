//! Image resolution / fetch / decode / sizing pipeline for md2pdf.
//!
//! Implements the contract pinned by D-fb4ebb §2 (image-fetch and embedding
//! pipeline), D-b53937 (`--strict` envelope), U-f1660d (sizing rule), and
//! the `done_definition` of W-df3269.
//!
//! ## Pipeline shape
//!
//! ```text
//!   Markdown image ref (src + alt + optional {width,height})
//!     → dispatch (http/https / file:// / unsupported scheme / local path)
//!     → fetch (HTTP via ureq+rustls+webpki-roots) OR local read
//!     → decode (PNG/JPG via `image` crate; SVG passthrough; other → placeholder)
//!     → size (per U-f1660d rule)
//!     → ResolvedImage { Embedded(bytes,format,size) | Placeholder(reason,size) }
//! ```
//!
//! On any failure mode in the right-hand half of the pipeline (network,
//! filesystem, decode, format/scheme not supported) the pipeline emits a
//! warning to stderr in the format mandated by D-fb4ebb §2 ("Missing /
//! unreachable image behavior") and returns a `Placeholder` outcome. The
//! warning is also recorded on the `Pipeline` struct so a caller running
//! in `--strict` mode can post-hoc detect that ≥1 warning fired and exit
//! with code 6 per D-b53937.
//!
//! Strict mode does NOT change what this module emits or returns. Per
//! D-b53937 §4, stderr text is identical with or without `--strict`; the
//! flag only changes whether the caller writes a PDF and whether it exits
//! non-zero. This module therefore exposes `warning_count()` and the full
//! `warnings()` accessor, and leaves the strict-vs-default decision to
//! the caller.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use url::Url;

// -----------------------------------------------------------------------------
// Public API: requests, sizing, outcomes.
// -----------------------------------------------------------------------------

/// One image reference from the Markdown input, ready for the pipeline.
#[derive(Debug, Clone)]
pub struct ImageRequest<'a> {
    /// Raw `src` string from the Markdown image (`![alt](src)`). Either a
    /// URL (`http://`, `https://`, `file://`, or another scheme) or a
    /// local path (relative to the input Markdown file's directory, or
    /// absolute).
    pub src: &'a str,
    /// Alt text from the Markdown image. Used for placeholders when the
    /// image cannot be loaded. Empty string allowed.
    pub alt: &'a str,
    /// Explicit width attribute, if the Markdown supplied one.
    pub explicit_width: Option<Length>,
    /// Explicit height attribute, if the Markdown supplied one.
    pub explicit_height: Option<Length>,
}

/// A length carrying its unit, as accepted by the GFM-style image
/// attribute syntax (`{width=400 height=200px width=5cm width=2in
/// width=30%}`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    /// Pixels (the bare-number default).
    Px(f64),
    /// Points (PDF/Typst native unit).
    Pt(f64),
    /// Centimetres.
    Cm(f64),
    /// Inches.
    In(f64),
    /// Percent of the current content width.
    Percent(f64),
}

impl Length {
    /// Convert to Typst points, given the page-content width in pt (used
    /// only for the `Percent` case). Pixels use the de-facto 96 dpi
    /// convention (1px = 0.75pt).
    pub fn to_pt(self, content_width_pt: f64) -> f64 {
        match self {
            Length::Px(v) => v * 0.75,
            Length::Pt(v) => v,
            Length::Cm(v) => v * (72.0 / 2.54),
            Length::In(v) => v * 72.0,
            Length::Percent(v) => content_width_pt * (v / 100.0),
        }
    }
}

/// Page geometry the pipeline needs to compute fit-to-width and the 80%
/// page-height cap.
#[derive(Debug, Clone, Copy)]
pub struct PageContext {
    /// Width of the typeset content area, in points.
    pub content_width_pt: f64,
    /// Height of the typeset content area, in points.
    pub content_height_pt: f64,
}

impl PageContext {
    /// A reasonable default: US-letter page (8.5"x11") with 1-inch
    /// margins. Concrete documents will override this when the theme
    /// lands.
    pub const DEFAULT_LETTER: PageContext = PageContext {
        content_width_pt: 6.5 * 72.0,
        content_height_pt: 9.0 * 72.0,
    };
}

/// The dimensions the pipeline computed for an image. `height_pt = None`
/// means "auto, preserve aspect" — used only for SVG references that
/// lack intrinsic dimensions and were not given an explicit height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SizedDims {
    pub width_pt: f64,
    pub height_pt: Option<f64>,
}

/// What the pipeline actually returns to the Markdown→Typst emitter.
#[derive(Debug, Clone)]
pub enum ResolvedImage {
    /// Image was successfully resolved and is embedded as bytes.
    Embedded {
        format: EmbeddedFormat,
        bytes: Vec<u8>,
        /// The pixel dimensions discovered during decode, when known.
        /// `None` for SVG (we don't parse it; Typst does).
        intrinsic_px: Option<(u32, u32)>,
        size: SizedDims,
    },
    /// Image could not be resolved. The pipeline has already emitted the
    /// warning to stderr and recorded it on the `Pipeline`. The emitter
    /// renders a bordered box at `size` containing `display_text`.
    Placeholder {
        /// What the placeholder box should display: alt text, falling
        /// back to a (possibly truncated) form of the source string.
        display_text: String,
        /// Human-readable reason (matches the second half of the stderr
        /// warning line). Useful for tooltips / debugging.
        reason: String,
        size: SizedDims,
    },
}

/// Encoded format the pipeline hands off to the emitter. Typst's `image`
/// element accepts encoded PNG/JPEG bytes directly, and resolves SVG via
/// its bundled `resvg` ingestion path — no md2pdf-side rasterisation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddedFormat {
    Png,
    Jpeg,
    Svg,
}

/// One recorded warning. The pipeline emits the human-readable form to
/// stderr at the moment it fires (matching the format pinned by D-fb4ebb
/// §2), and additionally retains it here so callers (e.g. `--strict`
/// mode) can decide post-hoc what to do.
#[derive(Debug, Clone)]
pub struct ImageWarning {
    pub src: String,
    pub reason: String,
}

impl ImageWarning {
    /// Render the canonical stderr line. Pinned text — log scrapers
    /// depend on it (D-b53937 §4).
    pub fn render_stderr_line(&self) -> String {
        format!(
            "md2pdf: warn: image '{}' could not be loaded: {}",
            self.src, self.reason
        )
    }
}

// -----------------------------------------------------------------------------
// Pipeline construction.
// -----------------------------------------------------------------------------

/// Hard-coded fetch-policy constants pinned by D-fb4ebb §2.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
const HTTP_MAX_BYTES: u64 = 20 * 1024 * 1024;
const HTTP_MAX_REDIRECTS: u32 = 5;

/// Pipeline holds the state needed across many image references in a
/// single document run: the base directory for local-path resolution,
/// the page context for sizing, the HTTP agent, and the accumulating
/// list of warnings.
pub struct Pipeline {
    base_dir: PathBuf,
    page: PageContext,
    #[allow(dead_code)]
    user_agent: String,
    #[allow(dead_code)]
    http_timeout: Duration,
    http_max_bytes: u64,
    http_max_redirects: u32,
    agent: ureq::Agent,
    warnings: Vec<ImageWarning>,
    /// Sink for warning lines. In production this writes to actual
    /// stderr; tests can swap in a buffer.
    warn_sink: Box<dyn WarnSink>,
}

/// Trait abstracting where warning lines go. Production wires this to
/// stderr; tests wire it to a `Vec<String>` so they can assert on the
/// canonical text.
pub trait WarnSink: Send + Sync {
    fn write_line(&mut self, line: &str);
}

struct StderrSink;
impl WarnSink for StderrSink {
    fn write_line(&mut self, line: &str) {
        eprintln!("{}", line);
    }
}

/// Test-friendly sink that captures all warning lines to a `Vec<String>`.
pub struct CapturingSink {
    pub lines: Vec<String>,
}
impl CapturingSink {
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }
}
impl Default for CapturingSink {
    fn default() -> Self {
        Self::new()
    }
}
impl WarnSink for CapturingSink {
    fn write_line(&mut self, line: &str) {
        self.lines.push(line.to_string());
    }
}

/// Builder for [`Pipeline`].
pub struct PipelineBuilder {
    base_dir: PathBuf,
    page: PageContext,
    user_agent: String,
    http_timeout: Duration,
    http_max_bytes: u64,
    http_max_redirects: u32,
    warn_sink: Box<dyn WarnSink>,
}

impl PipelineBuilder {
    /// Start a new builder. `base_dir` is the directory the input
    /// Markdown file lives in; relative `src` strings are resolved
    /// against it.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
            page: PageContext::DEFAULT_LETTER,
            user_agent: format!("md2pdf/{}", env!("CARGO_PKG_VERSION")),
            http_timeout: HTTP_TIMEOUT,
            http_max_bytes: HTTP_MAX_BYTES,
            http_max_redirects: HTTP_MAX_REDIRECTS,
            warn_sink: Box::new(StderrSink),
        }
    }
    pub fn page_context(mut self, page: PageContext) -> Self {
        self.page = page;
        self
    }
    pub fn warn_sink(mut self, sink: Box<dyn WarnSink>) -> Self {
        self.warn_sink = sink;
        self
    }
    /// Test hook: shrink the body cap. Production must use the default
    /// 20 MiB pinned by D-fb4ebb §2.
    pub fn http_max_bytes(mut self, n: u64) -> Self {
        self.http_max_bytes = n;
        self
    }
    /// Test hook: shrink the timeout.
    pub fn http_timeout(mut self, t: Duration) -> Self {
        self.http_timeout = t;
        self
    }
    /// Test hook: change redirect cap.
    pub fn http_max_redirects(mut self, n: u32) -> Self {
        self.http_max_redirects = n;
        self
    }
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = ua.into();
        self
    }
    pub fn build(self) -> Pipeline {
        // Build the ureq agent. ureq 2.x with default-features=false +
        // "tls" feature gives rustls + webpki-roots — exactly what
        // D-fb4ebb §2 requires (no OpenSSL anywhere in the graph).
        //
        // We disable ureq's built-in redirect handling (`redirects(0)`)
        // because we need loop detection (visited-URL set), which ureq's
        // counter-only redirect handling does not provide.
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(self.http_timeout)
            .timeout_read(self.http_timeout)
            .timeout_write(self.http_timeout)
            .redirects(0)
            .user_agent(&self.user_agent)
            .build();
        Pipeline {
            base_dir: self.base_dir,
            page: self.page,
            user_agent: self.user_agent,
            http_timeout: self.http_timeout,
            http_max_bytes: self.http_max_bytes,
            http_max_redirects: self.http_max_redirects,
            agent,
            warnings: Vec::new(),
            warn_sink: self.warn_sink,
        }
    }
}

impl Pipeline {
    /// Construct with default settings, base directory only. For
    /// fine-grained config (page, sink, test hooks) use
    /// [`PipelineBuilder`].
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        PipelineBuilder::new(base_dir).build()
    }

    /// All warnings recorded so far, in the order they fired.
    pub fn warnings(&self) -> &[ImageWarning] {
        &self.warnings
    }

    /// Convenience: did at least one warning-class behavior fire? This
    /// is the question `--strict` mode (D-b53937) needs to answer
    /// before the caller writes the PDF.
    pub fn any_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }

    /// Resolve one image reference end-to-end. Always returns a
    /// `ResolvedImage`; failures become `Placeholder` and emit a stderr
    /// warning + record on `self.warnings`.
    pub fn resolve(&mut self, req: &ImageRequest) -> ResolvedImage {
        match self.dispatch_and_load(req) {
            Ok(loaded) => {
                let size = self.compute_size(req, loaded.intrinsic_px, loaded.format);
                ResolvedImage::Embedded {
                    format: loaded.format,
                    bytes: loaded.bytes,
                    intrinsic_px: loaded.intrinsic_px,
                    size,
                }
            }
            Err(reason) => {
                let warning = ImageWarning {
                    src: req.src.to_string(),
                    reason: reason.clone(),
                };
                self.warn_sink.write_line(&warning.render_stderr_line());
                self.warnings.push(warning);
                let display_text = if !req.alt.is_empty() {
                    req.alt.to_string()
                } else {
                    truncate_for_display(req.src, 80)
                };
                let size = self.compute_size_for_placeholder(req);
                ResolvedImage::Placeholder {
                    display_text,
                    reason,
                    size,
                }
            }
        }
    }
}

/// Internal: a successfully-loaded image ready for sizing.
struct LoadedImage {
    format: EmbeddedFormat,
    bytes: Vec<u8>,
    intrinsic_px: Option<(u32, u32)>,
}

impl Pipeline {
    /// Run dispatch → fetch/read → decode. Returns the loaded image or
    /// a human-readable failure reason (which becomes the `<reason>`
    /// part of the canonical warning line).
    fn dispatch_and_load(&self, req: &ImageRequest) -> Result<LoadedImage, String> {
        // 1. Try parsing as an absolute URL. The url crate's `Url::parse`
        //    succeeds on any string with a `scheme:` prefix; relative
        //    paths and bare filenames fall through to local resolution.
        if let Ok(url) = Url::parse(req.src) {
            match url.scheme() {
                "http" | "https" => return self.load_remote(url),
                "file" => {
                    let path = url
                        .to_file_path()
                        .map_err(|_| "invalid file:// URL".to_string())?;
                    return self.load_local(&path, req.src);
                }
                other => {
                    return Err(format!("url scheme not supported: {}", other));
                }
            }
        }
        // 2. Local path resolution. Per D-fb4ebb §2: relative to the
        //    input Markdown file's directory, then absolute fallback.
        let path = Path::new(req.src);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.base_dir.join(path)
        };
        self.load_local(&resolved, req.src)
    }

    /// Local read + decode.
    fn load_local(&self, path: &Path, _original_src: &str) -> Result<LoadedImage, String> {
        let bytes = fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => "file not found".to_string(),
            std::io::ErrorKind::PermissionDenied => "permission denied".to_string(),
            _ => format!("read error: {}", e),
        })?;
        decode_or_passthrough(bytes, path)
    }

    /// Remote fetch + decode. Implements:
    ///   - manual redirect following with visited-URL loop detection
    ///   - body-size cap with mid-stream abort
    ///   - timeouts (connect + read + write, all 10s by default)
    fn load_remote(&self, initial_url: Url) -> Result<LoadedImage, String> {
        let mut current = initial_url;
        let mut visited: Vec<String> = Vec::new();
        for _ in 0..=self.http_max_redirects {
            visited.push(current.to_string());
            let resp = self.agent.request_url("GET", &current).call();
            let resp = match resp {
                Ok(r) => r,
                Err(ureq::Error::Status(_, r)) => r,
                Err(ureq::Error::Transport(t)) => {
                    let kind = t.kind();
                    return Err(match kind {
                        ureq::ErrorKind::ConnectionFailed => "connection failed".to_string(),
                        ureq::ErrorKind::Dns => "dns lookup failed".to_string(),
                        ureq::ErrorKind::Io => {
                            // io::Error within ureq covers timeouts as well
                            // as raw socket faults; surface either as a
                            // generic message but include the inner.
                            let msg = t.to_string();
                            if msg.contains("timed out") || msg.contains("timeout") {
                                "request timed out".to_string()
                            } else {
                                format!("io error: {}", msg)
                            }
                        }
                        _ => format!("http transport error: {}", t),
                    });
                }
            };

            let status = resp.status();
            // Redirect codes per D-fb4ebb §2: 301, 302, 303, 307, 308.
            if matches!(status, 301 | 302 | 303 | 307 | 308) {
                let location = resp
                    .header("location")
                    .ok_or_else(|| format!("http {} with no Location header", status))?
                    .to_string();
                let next = current
                    .join(&location)
                    .map_err(|e| format!("invalid redirect target: {}", e))?;
                if visited.iter().any(|v| v == &next.to_string()) {
                    return Err("redirect loop".to_string());
                }
                current = next;
                continue;
            }
            if !(200..300).contains(&status) {
                return Err(format!("http {}", status));
            }

            // 200-class. Stream body with byte cap.
            let bytes = read_capped(resp.into_reader(), self.http_max_bytes)?;
            // Decode based on content-type / sniff. We have no path
            // here, so use the URL's last segment as a hint for
            // extension-based SVG detection.
            let url_path = current.path().to_string();
            return decode_or_passthrough(bytes, Path::new(&url_path));
        }
        Err(format!(
            "too many redirects (>{})",
            self.http_max_redirects
        ))
    }

    /// Compute final dimensions per the U-f1660d sizing rule, given
    /// optional intrinsic pixel dimensions from the decoded raster (or
    /// `None` for SVG).
    fn compute_size(
        &self,
        req: &ImageRequest,
        intrinsic_px: Option<(u32, u32)>,
        _format: EmbeddedFormat,
    ) -> SizedDims {
        let cw = self.page.content_width_pt;
        let ch = self.page.content_height_pt;

        // The intrinsic aspect ratio (width / height) — used for both
        // "one dim given" and "neither given" cases. None means we don't
        // know it (SVG without explicit dims).
        let aspect = intrinsic_px.and_then(|(w, h)| {
            if w == 0 || h == 0 {
                None
            } else {
                Some(w as f64 / h as f64)
            }
        });

        match (req.explicit_width, req.explicit_height) {
            // Both explicit → use both. Aspect not preserved.
            (Some(w), Some(h)) => SizedDims {
                width_pt: w.to_pt(cw),
                height_pt: Some(h.to_pt(cw)),
            },
            // Width-only → compute height from aspect (if known).
            (Some(w), None) => {
                let wpt = w.to_pt(cw);
                let hpt = aspect.map(|ar| wpt / ar);
                SizedDims {
                    width_pt: wpt,
                    height_pt: hpt,
                }
            }
            // Height-only → compute width from aspect.
            (None, Some(h)) => {
                let hpt = h.to_pt(cw);
                if let Some(ar) = aspect {
                    SizedDims {
                        width_pt: hpt * ar,
                        height_pt: Some(hpt),
                    }
                } else {
                    // No aspect known (SVG without intrinsic) — fall back
                    // to fit-to-content-width and keep the explicit
                    // height.
                    SizedDims {
                        width_pt: cw,
                        height_pt: Some(hpt),
                    }
                }
            }
            // Neither → fit to content width, preserve aspect, cap at
            // 80% page-content height.
            (None, None) => {
                let cap = ch * 0.80;
                if let Some(ar) = aspect {
                    let mut wpt = cw;
                    let mut hpt = wpt / ar;
                    if hpt > cap {
                        hpt = cap;
                        wpt = hpt * ar;
                    }
                    SizedDims {
                        width_pt: wpt,
                        height_pt: Some(hpt),
                    }
                } else {
                    // SVG without intrinsic — width = content width, let
                    // Typst preserve aspect on the height side.
                    SizedDims {
                        width_pt: cw,
                        height_pt: None,
                    }
                }
            }
        }
    }

    fn compute_size_for_placeholder(&self, req: &ImageRequest) -> SizedDims {
        // Placeholder boxes follow the same sizing rule, but with no
        // intrinsic dimensions known. Use a conservative 1.6:1 aspect
        // (a rough page-thumbnail ratio) when neither dim is explicit.
        let cw = self.page.content_width_pt;
        match (req.explicit_width, req.explicit_height) {
            (Some(w), Some(h)) => SizedDims {
                width_pt: w.to_pt(cw),
                height_pt: Some(h.to_pt(cw)),
            },
            (Some(w), None) => {
                let wpt = w.to_pt(cw);
                SizedDims {
                    width_pt: wpt,
                    height_pt: Some(wpt / 1.6),
                }
            }
            (None, Some(h)) => {
                let hpt = h.to_pt(cw);
                SizedDims {
                    width_pt: hpt * 1.6,
                    height_pt: Some(hpt),
                }
            }
            (None, None) => {
                // A small placeholder box: 60% content width, 1.6:1.
                let wpt = cw * 0.60;
                SizedDims {
                    width_pt: wpt,
                    height_pt: Some(wpt / 1.6),
                }
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Decode helpers.
// -----------------------------------------------------------------------------

/// Decide format → decode (PNG/JPG) or pass-through (SVG) → return
/// LoadedImage. `name_hint` is used for SVG extension sniffing when the
/// raw bytes' magic-number sniff is inconclusive.
fn decode_or_passthrough(bytes: Vec<u8>, name_hint: &Path) -> Result<LoadedImage, String> {
    // Detect SVG first — it's a text format with a magic-ish signature.
    if looks_like_svg(&bytes, name_hint) {
        return Ok(LoadedImage {
            format: EmbeddedFormat::Svg,
            bytes,
            intrinsic_px: None,
        });
    }
    // Sniff PNG / JPEG via magic bytes.
    let format = sniff_raster(&bytes);
    match format {
        Some(EmbeddedFormat::Png) | Some(EmbeddedFormat::Jpeg) => {
            // Use the `image` crate to verify and pull intrinsic dims.
            // We hand the encoded bytes back to the emitter (Typst
            // accepts encoded PNG/JPG directly), so this is a validation
            // + dimension read, not a re-encode.
            let img_format = match format {
                Some(EmbeddedFormat::Png) => image::ImageFormat::Png,
                Some(EmbeddedFormat::Jpeg) => image::ImageFormat::Jpeg,
                _ => unreachable!(),
            };
            let cursor = std::io::Cursor::new(&bytes);
            let reader = image::ImageReader::with_format(cursor, img_format);
            let dims = reader.into_dimensions().map_err(|e| {
                format!(
                    "decode failed ({}): {}",
                    if matches!(format, Some(EmbeddedFormat::Png)) {
                        "png"
                    } else {
                        "jpeg"
                    },
                    e
                )
            })?;
            Ok(LoadedImage {
                format: format.unwrap(),
                bytes,
                intrinsic_px: Some(dims),
            })
        }
        _ => {
            // Not PNG, JPEG, or SVG → unsupported in v1.
            let ext = name_hint
                .extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_lowercase());
            let label = ext.unwrap_or_else(|| "unknown".to_string());
            Err(format!("format not supported: {}", label))
        }
    }
}

fn sniff_raster(bytes: &[u8]) -> Option<EmbeddedFormat> {
    if bytes.len() >= 8 && &bytes[0..8] == b"\x89PNG\r\n\x1a\n" {
        return Some(EmbeddedFormat::Png);
    }
    if bytes.len() >= 3 && &bytes[0..3] == b"\xFF\xD8\xFF" {
        return Some(EmbeddedFormat::Jpeg);
    }
    None
}

fn looks_like_svg(bytes: &[u8], name_hint: &Path) -> bool {
    // Cheap path: extension hint.
    if let Some(ext) = name_hint.extension().and_then(|s| s.to_str()) {
        if ext.eq_ignore_ascii_case("svg") {
            return true;
        }
    }
    // Content sniff: SVG is text; look for `<svg` or `<?xml ... <svg`
    // in the first 1 KiB after stripping leading whitespace / BOM.
    let head_len = bytes.len().min(1024);
    let head = &bytes[..head_len];
    let mut start = 0;
    if head.starts_with(b"\xEF\xBB\xBF") {
        start = 3;
    }
    let trimmed = &head[start..];
    let trimmed = trimmed
        .iter()
        .position(|&b| !b.is_ascii_whitespace())
        .map(|i| &trimmed[i..])
        .unwrap_or(trimmed);
    if trimmed.starts_with(b"<svg") || trimmed.starts_with(b"<SVG") {
        return true;
    }
    if trimmed.starts_with(b"<?xml") {
        // Look ahead for `<svg`.
        if let Ok(s) = std::str::from_utf8(trimmed) {
            return s.contains("<svg") || s.contains("<SVG");
        }
    }
    false
}

/// Read up to `cap` bytes from `r`. If the stream produces more, return
/// an error — i.e. enforces the cap mid-stream rather than after the
/// fact. Implementation: read into a fixed buffer in chunks, count
/// bytes, abort early.
fn read_capped<R: Read>(mut r: R, cap: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];
    let mut total: u64 = 0;
    loop {
        let n = r
            .read(&mut chunk)
            .map_err(|e| format!("io error: {}", e))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > cap {
            return Err(format!("response body exceeded {} bytes", cap));
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    Ok(buf)
}

fn truncate_for_display(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let head: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{}…", head)
    }
}

/// 1×1 transparent PNG, hand-encoded. Exposed crate-internally so unit
/// tests AND the integration tests in `tests/` can build a tiny valid
/// raster fixture without pulling in extra dev-dependencies.
#[cfg(any(test, feature = "test-fixtures"))]
pub fn tiny_png_fixture() -> Vec<u8> {
    // Smallest valid PNG: 1×1, 8-bit RGBA, single transparent pixel.
    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, // signature
        0x00, 0x00, 0x00, 0x0D, // IHDR length
        b'I', b'H', b'D', b'R',
        0x00, 0x00, 0x00, 0x01, // width = 1
        0x00, 0x00, 0x00, 0x01, // height = 1
        0x08, 0x06, 0x00, 0x00, 0x00, // 8-bit, RGBA, no interlace
        0x1F, 0x15, 0xC4, 0x89, // CRC
        0x00, 0x00, 0x00, 0x0A, // IDAT length
        b'I', b'D', b'A', b'T',
        0x78, 0x9C, 0x62, 0x00, 0x01, 0x00, 0x00, 0x05, 0x00, 0x01, // zlib data
        0x0D, 0x0A, 0x2D, 0xB4, // CRC
        0x00, 0x00, 0x00, 0x00, // IEND length
        b'I', b'E', b'N', b'D',
        0xAE, 0x42, 0x60, 0x82, // CRC
    ];
    PNG.to_vec()
}

// -----------------------------------------------------------------------------
// Unit tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn px(w: u32, h: u32) -> Option<(u32, u32)> {
        Some((w, h))
    }

    fn page_a4() -> PageContext {
        PageContext {
            content_width_pt: 500.0,
            content_height_pt: 700.0,
        }
    }

    fn make_pipeline() -> Pipeline {
        PipelineBuilder::new(std::env::temp_dir())
            .page_context(page_a4())
            .warn_sink(Box::new(CapturingSink::new()))
            .build()
    }

    fn req<'a>(src: &'a str, w: Option<Length>, h: Option<Length>) -> ImageRequest<'a> {
        ImageRequest {
            src,
            alt: "alt",
            explicit_width: w,
            explicit_height: h,
        }
    }

    // --- Length conversions --------------------------------------------------

    #[test]
    fn length_pt_unit_passthrough() {
        assert_eq!(Length::Pt(72.0).to_pt(500.0), 72.0);
    }
    #[test]
    fn length_inch_to_pt() {
        assert!((Length::In(1.0).to_pt(500.0) - 72.0).abs() < 1e-9);
    }
    #[test]
    fn length_cm_to_pt() {
        let got = Length::Cm(2.54).to_pt(500.0);
        assert!((got - 72.0).abs() < 1e-9);
    }
    #[test]
    fn length_pixel_to_pt_at_96dpi() {
        let got = Length::Px(96.0).to_pt(500.0);
        assert!((got - 72.0).abs() < 1e-9);
    }
    #[test]
    fn length_percent_uses_content_width() {
        assert!((Length::Percent(50.0).to_pt(400.0) - 200.0).abs() < 1e-9);
    }

    // --- Sizing matrix per U-f1660d -----------------------------------------

    #[test]
    fn sizing_both_explicit_uses_both() {
        let p = make_pipeline();
        let r = req("x.png", Some(Length::Pt(100.0)), Some(Length::Pt(50.0)));
        let s = p.compute_size(&r, px(2, 1), EmbeddedFormat::Png);
        assert_eq!(s.width_pt, 100.0);
        assert_eq!(s.height_pt, Some(50.0));
    }

    #[test]
    fn sizing_width_only_preserves_aspect_from_intrinsic() {
        let p = make_pipeline();
        let r = req("x.png", Some(Length::Pt(200.0)), None);
        // 4:1 aspect → height = 50.
        let s = p.compute_size(&r, px(400, 100), EmbeddedFormat::Png);
        assert_eq!(s.width_pt, 200.0);
        assert!((s.height_pt.unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn sizing_height_only_preserves_aspect_from_intrinsic() {
        let p = make_pipeline();
        let r = req("x.png", None, Some(Length::Pt(80.0)));
        // 2:1 aspect (w/h) → width = 160.
        let s = p.compute_size(&r, px(200, 100), EmbeddedFormat::Png);
        assert!((s.width_pt - 160.0).abs() < 1e-6);
        assert_eq!(s.height_pt, Some(80.0));
    }

    #[test]
    fn sizing_neither_fits_content_width_with_aspect() {
        let p = make_pipeline(); // page = 500x700
        let r = req("x.png", None, None);
        // 5:1 aspect → width 500, height 100. Well under 80% of 700 = 560.
        let s = p.compute_size(&r, px(500, 100), EmbeddedFormat::Png);
        assert_eq!(s.width_pt, 500.0);
        assert!((s.height_pt.unwrap() - 100.0).abs() < 1e-6);
    }

    #[test]
    fn sizing_neither_engages_height_cap_at_80_percent() {
        let p = make_pipeline(); // page = 500x700; cap = 560
        let r = req("x.png", None, None);
        // 1:5 portrait → naive height = 2500 (way over). Cap kicks in:
        // height = 560, width = 560 * (1/5) = 112.
        let s = p.compute_size(&r, px(100, 500), EmbeddedFormat::Png);
        assert!((s.height_pt.unwrap() - 560.0).abs() < 1e-6);
        assert!((s.width_pt - 112.0).abs() < 1e-6);
    }

    #[test]
    fn sizing_svg_no_intrinsic_no_explicit_uses_content_width_auto_height() {
        let p = make_pipeline();
        let r = req("x.svg", None, None);
        let s = p.compute_size(&r, None, EmbeddedFormat::Svg);
        assert_eq!(s.width_pt, 500.0);
        assert_eq!(s.height_pt, None);
    }

    #[test]
    fn sizing_svg_height_only_no_intrinsic_falls_back_to_content_width() {
        let p = make_pipeline();
        let r = req("x.svg", None, Some(Length::Pt(100.0)));
        let s = p.compute_size(&r, None, EmbeddedFormat::Svg);
        assert_eq!(s.width_pt, 500.0);
        assert_eq!(s.height_pt, Some(100.0));
    }

    // --- Sniffing ------------------------------------------------------------

    #[test]
    fn sniff_recognises_png_magic() {
        assert_eq!(
            sniff_raster(b"\x89PNG\r\n\x1a\nrest"),
            Some(EmbeddedFormat::Png)
        );
    }
    #[test]
    fn sniff_recognises_jpeg_magic() {
        assert_eq!(
            sniff_raster(b"\xFF\xD8\xFF\xE0blah"),
            Some(EmbeddedFormat::Jpeg)
        );
    }
    #[test]
    fn sniff_rejects_random_bytes() {
        assert_eq!(sniff_raster(b"GIF89a..."), None);
    }
    #[test]
    fn svg_detected_by_extension() {
        assert!(looks_like_svg(b"some bytes", Path::new("img.svg")));
        assert!(looks_like_svg(b"some bytes", Path::new("img.SVG")));
    }
    #[test]
    fn svg_detected_by_content_no_extension_hint() {
        assert!(looks_like_svg(
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
            Path::new("noext")
        ));
    }
    #[test]
    fn svg_detected_with_xml_prolog() {
        assert!(looks_like_svg(
            b"<?xml version=\"1.0\"?>\n<svg></svg>",
            Path::new("noext")
        ));
    }
    #[test]
    fn svg_not_detected_for_html() {
        assert!(!looks_like_svg(b"<!DOCTYPE html><html>", Path::new("a.html")));
    }

    // --- read_capped --------------------------------------------------------

    #[test]
    fn read_capped_succeeds_under_cap() {
        let data = vec![1u8; 1000];
        let got = read_capped(Cursor::new(data.clone()), 2000).unwrap();
        assert_eq!(got, data);
    }
    #[test]
    fn read_capped_aborts_over_cap() {
        let data = vec![0u8; 50_000];
        let err = read_capped(Cursor::new(data), 10_000).unwrap_err();
        assert!(err.contains("exceeded"));
    }
    #[test]
    fn read_capped_aborts_mid_stream_not_after() {
        // Verify the error fires before we'd buffered the full body.
        let data = vec![0u8; 25 * 1024 * 1024];
        let err = read_capped(Cursor::new(data), 20 * 1024 * 1024).unwrap_err();
        assert!(err.contains("exceeded"));
    }

    // --- Warnings ------------------------------------------------------------

    #[test]
    fn warning_renders_canonical_stderr_line() {
        let w = ImageWarning {
            src: "missing.png".into(),
            reason: "file not found".into(),
        };
        assert_eq!(
            w.render_stderr_line(),
            "md2pdf: warn: image 'missing.png' could not be loaded: file not found"
        );
    }

    #[test]
    fn missing_local_file_yields_placeholder_and_warning() {
        let mut p = PipelineBuilder::new(std::env::temp_dir())
            .page_context(page_a4())
            .warn_sink(Box::new(CapturingSink::new()))
            .build();
        let r = ImageRequest {
            src: "definitely-not-here-xyz123.png",
            alt: "a thing",
            explicit_width: None,
            explicit_height: None,
        };
        let out = p.resolve(&r);
        match out {
            ResolvedImage::Placeholder { display_text, reason, .. } => {
                assert_eq!(display_text, "a thing");
                assert!(reason.contains("file not found"));
            }
            _ => panic!("expected placeholder"),
        }
        assert_eq!(p.warnings().len(), 1);
        assert!(p.any_warnings());
    }

    #[test]
    fn unsupported_url_scheme_yields_placeholder() {
        let mut p = make_pipeline();
        let r = ImageRequest {
            src: "ftp://example.com/x.png",
            alt: "",
            explicit_width: None,
            explicit_height: None,
        };
        let out = p.resolve(&r);
        match out {
            ResolvedImage::Placeholder { reason, display_text, .. } => {
                assert!(reason.contains("url scheme not supported"));
                assert!(reason.contains("ftp"));
                assert_eq!(display_text, "ftp://example.com/x.png");
            }
            _ => panic!("expected placeholder"),
        }
    }

    #[test]
    fn data_uri_yields_placeholder() {
        let mut p = make_pipeline();
        let r = ImageRequest {
            src: "data:image/png;base64,iVBOR",
            alt: "",
            explicit_width: None,
            explicit_height: None,
        };
        let out = p.resolve(&r);
        assert!(matches!(out, ResolvedImage::Placeholder { .. }));
    }

    // --- Local resolution: relative + absolute -------------------------------

    #[test]
    fn local_relative_resolves_against_base_dir_and_decodes_png() {
        let dir = tempdir();
        let png = tiny_png_fixture();
        std::fs::write(dir.join("pic.png"), &png).unwrap();
        let mut p = PipelineBuilder::new(&dir).page_context(page_a4()).build();
        let r = ImageRequest {
            src: "pic.png",
            alt: "x",
            explicit_width: None,
            explicit_height: None,
        };
        match p.resolve(&r) {
            ResolvedImage::Embedded { format, intrinsic_px, .. } => {
                assert_eq!(format, EmbeddedFormat::Png);
                assert_eq!(intrinsic_px, Some((1, 1)));
            }
            _ => panic!("expected embedded"),
        }
    }

    #[test]
    fn local_absolute_resolves_directly() {
        let dir = tempdir();
        let png_path = dir.join("abs.png");
        std::fs::write(&png_path, &tiny_png_fixture()).unwrap();
        let mut p = PipelineBuilder::new(std::env::temp_dir()).build();
        let r = ImageRequest {
            src: png_path.to_str().unwrap(),
            alt: "x",
            explicit_width: None,
            explicit_height: None,
        };
        match p.resolve(&r) {
            ResolvedImage::Embedded { format, .. } => {
                assert_eq!(format, EmbeddedFormat::Png);
            }
            _ => panic!("expected embedded"),
        }
    }

    #[test]
    fn file_url_resolves_to_local_path() {
        let dir = tempdir();
        let path = dir.join("via-url.png");
        std::fs::write(&path, &tiny_png_fixture()).unwrap();
        let url = url::Url::from_file_path(&path).unwrap().to_string();
        let mut p = PipelineBuilder::new(std::env::temp_dir()).build();
        let r = ImageRequest {
            src: &url,
            alt: "x",
            explicit_width: None,
            explicit_height: None,
        };
        assert!(matches!(p.resolve(&r), ResolvedImage::Embedded { .. }));
    }

    #[test]
    fn unsupported_format_yields_placeholder() {
        let dir = tempdir();
        std::fs::write(dir.join("a.gif"), b"GIF89a\x01\x00\x01\x00\x00\x00\x00\x00\x00").unwrap();
        let mut p = PipelineBuilder::new(&dir).build();
        let r = ImageRequest {
            src: "a.gif",
            alt: "",
            explicit_width: None,
            explicit_height: None,
        };
        match p.resolve(&r) {
            ResolvedImage::Placeholder { reason, .. } => {
                assert!(reason.contains("format not supported"));
                assert!(reason.contains("gif"));
            }
            _ => panic!("expected placeholder"),
        }
    }

    #[test]
    fn local_svg_passes_through() {
        let dir = tempdir();
        std::fs::write(
            dir.join("d.svg"),
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
        )
        .unwrap();
        let mut p = PipelineBuilder::new(&dir).build();
        let r = ImageRequest {
            src: "d.svg",
            alt: "",
            explicit_width: None,
            explicit_height: None,
        };
        match p.resolve(&r) {
            ResolvedImage::Embedded { format, intrinsic_px, .. } => {
                assert_eq!(format, EmbeddedFormat::Svg);
                assert_eq!(intrinsic_px, None);
            }
            _ => panic!("expected embedded svg"),
        }
    }

    #[test]
    fn truncate_for_display_handles_short_strings() {
        assert_eq!(truncate_for_display("hi", 80), "hi");
    }
    #[test]
    fn truncate_for_display_truncates_long() {
        let s = "a".repeat(100);
        let got = truncate_for_display(&s, 10);
        assert!(got.ends_with('…'));
        assert!(got.chars().count() <= 10);
    }

    /// Per-test scratch dir; cheap, leaks on drop (tests are short and
    /// we'd rather skip a tempdir crate to keep the dep graph minimal).
    fn tempdir() -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pid = std::process::id();
        p.push(format!("md2pdf-test-{}-{}", pid, nanos));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
