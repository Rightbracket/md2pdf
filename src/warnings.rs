//! Warning collector wired through the Markdown→Typst emitter and the
//! image pipeline. Per D-c3af71 §D this module owns the *shape* of a
//! warning (the `Warning` struct + `WarningSource` enum), the canonical
//! stderr writer, and the run-end accumulator that the `--strict`
//! end-of-run gate (W#7') will eventually consult.
//!
//! ## Why a separate module
//!
//! The image pipeline already collects its own typed warnings
//! (`image_pipeline::ImageWarning`). The emitter has *additional*
//! warning sources (mermaid render failures, unsupported image-attribute
//! syntax, etc.). The strict-mode gate needs a single yes/no answer:
//! "did at least one warning fire across the whole render?" — and a
//! single accumulated list for log/diagnostic purposes. This module
//! provides both, plus a `WarnSink` so tests can capture stderr without
//! actually writing to stderr.
//!
//! ## What this Work wires
//!
//! - `WarningCollector::warn(...)` writes the canonical stderr line and
//!   pushes a `Warning` onto the run-end accumulator.
//! - The emitter (W-58a2ba) routes mermaid + emitter-side warnings here.
//! - The image pipeline keeps its own internal accumulator AND we drain
//!   its warnings into this collector after each `pipeline.resolve(...)`
//!   call so the strict gate sees a unified list. (Choice documented in
//!   the Outcome: drain-after rather than threading a `&mut
//!   WarningCollector` into `Pipeline` itself, to avoid mutating the
//!   image-pipeline crate's public surface for a non-functional plumbing
//!   change.)
//!
//! ## What this Work does NOT do
//!
//! The end-of-run `--strict` gate that turns "any warnings fired?" into
//! exit code 6 is W#7'. This module just wires the collector; pipeline
//! callers consult `WarningCollector::any()` and decide.

/// What part of the system raised the warning. `non_exhaustive` so
/// downstream consumers (notably W#7's strict gate) cannot break the
/// build by depending on closed-set matching.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WarningSource {
    /// Image pipeline (network / filesystem / decode / format /
    /// scheme). The canonical stderr text is owned by
    /// `image_pipeline::ImageWarning::render_stderr_line`; this module
    /// reuses that exact format when bridging.
    Image,
    /// Mermaid sub-renderer (parse/layout/unsupported-type).
    Mermaid,
    /// Emitter-side: e.g. unrecognized image-attribute syntax,
    /// recoverable parse oddities the emitter chose not to drop on the
    /// floor.
    Emitter,
    /// Typst compile step (`typst::compile`) emitted a non-fatal
    /// warning. Bridged from `Warned<...>::warnings` after compile so
    /// the unified strict-mode gate sees them.
    TypstCompile,
}

/// One recorded warning. The collector emits the human-readable form to
/// stderr at `warn(...)` time and keeps the structured form here for
/// post-hoc inspection (the `--strict` gate is the primary consumer).
#[derive(Debug, Clone)]
pub struct Warning {
    pub source: WarningSource,
    pub message: String,
}

/// Where warning lines go. Production wires this to actual stderr;
/// tests wire it to a `Vec<String>` so they can assert on canonical
/// text without an `eprintln!` race against the test harness.
pub trait WarnSink: Send + Sync {
    fn write_line(&mut self, line: &str);
}

/// Default production sink — writes one line at a time to stderr.
pub struct StderrSink;
impl WarnSink for StderrSink {
    fn write_line(&mut self, line: &str) {
        eprintln!("{}", line);
    }
}

/// Test-friendly sink that captures warning lines.
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

/// Accumulator + stderr forwarder.
pub struct WarningCollector {
    sink: Box<dyn WarnSink>,
    warnings: Vec<Warning>,
}

impl WarningCollector {
    pub fn new() -> Self {
        Self {
            sink: Box::new(StderrSink),
            warnings: Vec::new(),
        }
    }
    pub fn with_sink(sink: Box<dyn WarnSink>) -> Self {
        Self {
            sink,
            warnings: Vec::new(),
        }
    }
    /// Record a warning: write the canonical line to the sink and push
    /// onto the accumulator.
    pub fn warn(&mut self, source: WarningSource, message: impl Into<String>) {
        let message = message.into();
        let line = match source {
            WarningSource::Image => format!("md2pdf: warn: {}", message),
            WarningSource::Mermaid => format!("md2pdf: warn: mermaid: {}", message),
            WarningSource::Emitter => format!("md2pdf: warn: {}", message),
            WarningSource::TypstCompile => format!("md2pdf: warn: typst-compile: {}", message),
        };
        self.sink.write_line(&line);
        self.warnings.push(Warning { source, message });
    }

    /// All warnings recorded so far.
    pub fn warnings(&self) -> &[Warning] {
        &self.warnings
    }

    /// Did at least one warning fire? The strict-mode gate (W#7')
    /// consults this.
    pub fn any(&self) -> bool {
        !self.warnings.is_empty()
    }

    pub fn count(&self) -> usize {
        self.warnings.len()
    }

    /// Bridge an image-pipeline warning into the unified accumulator
    /// **without** re-emitting to stderr (the image pipeline already
    /// wrote its own canonical line at the moment the warning fired).
    /// Used by the emitter's drain-after-resolve path; see
    /// `emitter::MarkdownEmitter::drain_image_warnings`.
    pub fn record_image_silently(&mut self, w: crate::image_pipeline::ImageWarning) {
        self.warnings.push(Warning {
            source: WarningSource::Image,
            message: format!("image '{}' could not be loaded: {}", w.src, w.reason),
        });
    }
}

impl Default for WarningCollector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warn_writes_to_sink_and_accumulates() {
        let mut wc = WarningCollector::with_sink(Box::new(CapturingSink::new()));
        wc.warn(WarningSource::Mermaid, "diagram type 'gantt' is not yet supported");
        wc.warn(WarningSource::Emitter, "unrecognized image attr");
        assert_eq!(wc.count(), 2);
        assert!(wc.any());
        assert_eq!(wc.warnings()[0].source, WarningSource::Mermaid);
    }

    #[test]
    fn empty_collector_reports_no_warnings() {
        let wc = WarningCollector::with_sink(Box::new(CapturingSink::new()));
        assert!(!wc.any());
        assert_eq!(wc.count(), 0);
    }
}
