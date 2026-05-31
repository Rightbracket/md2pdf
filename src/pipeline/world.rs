//! Hand-rolled `typst::World` implementation.
//!
//! Why hand-rolled rather than `typst-as-lib`: the surface we need for
//! v1 is small (one in-memory main source, no `@preview` packages, no
//! filesystem includes, embedded font set). Hand-rolling keeps the
//! dependency footprint tight and explicit per U-c36357 / U-4dba32.
//!
//! Fonts embedded:
//! - **Twemoji Mozilla** (COLRv0) — the Decision-mandated emoji font
//!   per D-2058a7.
//! - **Typst's built-in fallback fonts** — provided by `typst-library`'s
//!   `default_library` defaults (text fonts come via the embedded
//!   `Library`'s standard set).
//!
//! Body serif/sans/mono fonts are *deferred* to a subsequent Work
//! (the design-engineer + emitter Work). The scaffold relies on
//! Typst's default text rendering for the placeholder document; the
//! *emoji* font is what this scaffold proves end-to-end.

use std::sync::OnceLock;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, Source, VirtualPath};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// Bytes of the bundled Twemoji Mozilla COLRv0 font. The font binary is
/// fetched from the Mozilla project's release tarball during repo
/// preparation (see `assets/fonts/Twemoji.Mozilla.ttf`). Per D-2058a7,
/// CC-BY 4.0 attribution must surface in the binary's eventual
/// `--license` output (a future Work).
const TWEMOJI_MOZILLA_TTF: &[u8] =
    include_bytes!("../../assets/fonts/Twemoji.Mozilla.ttf");

/// All fonts the scaffold makes available to Typst, in the order in
/// which they will appear in the `FontBook`. Order is meaningful: index
/// here matches the `index` argument the World hands back to Typst.
fn build_font_set() -> Vec<Font> {
    let mut fonts = Vec::new();

    // Twemoji Mozilla can hold multiple faces (it is a single-face TTF
    // in v0.7.0, but we iterate to be future-proof).
    let bytes = Bytes::new(TWEMOJI_MOZILLA_TTF.to_vec());
    let mut idx = 0;
    while let Some(font) = Font::new(bytes.clone(), idx) {
        fonts.push(font);
        idx += 1;
    }
    fonts
}

/// In-memory Typst world for the scaffold.
pub struct ScaffoldWorld {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    main_source: Source,
    fonts: Vec<Font>,
}

impl ScaffoldWorld {
    /// Build a world rendering the given Typst source as its main file.
    pub fn new(source_text: String) -> Self {
        // Combine Twemoji with Typst's default embedded fonts (provided
        // by typst-library when its `embed-fonts` feature is on; we
        // don't enable that for now, so Twemoji is the only embedded
        // face — the scaffold body uses Typst's internal text-shaping
        // defaults). When the emitter Work lands, body fonts will be
        // appended here.
        let fonts = build_font_set();
        let book = FontBook::from_fonts(&fonts);

        let main_id = FileId::new(None, VirtualPath::new("/main.typ"));
        let main_source = Source::new(main_id, source_text);

        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            main_source,
            fonts,
        }
    }
}

impl World for ScaffoldWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.book
    }

    fn main(&self) -> FileId {
        self.main_source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main_source.id() {
            Ok(self.main_source.clone())
        } else {
            Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        // Scaffold has no filesystem-backed includes. Image/file
        // resolution lands with the image-pipeline Work.
        Err(FileError::NotFound(id.vpath().as_rootless_path().into()))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<i64>) -> Option<Datetime> {
        // Deterministic empty: today() is unused by the scaffold doc.
        None
    }
}

// Light compile-time sanity: ensure the embedded font bytes are present
// and non-trivial. A zero-byte include_bytes! would silently produce a
// World with no emoji coverage.
const _: () = {
    if TWEMOJI_MOZILLA_TTF.is_empty() {
        panic!("Twemoji.Mozilla.ttf failed to embed (zero bytes)");
    }
};

// Keep the OnceLock import path stable for future cache work.
#[allow(dead_code)]
fn _reserved_for_future_caches() -> &'static OnceLock<()> {
    static CELL: OnceLock<()> = OnceLock::new();
    &CELL
}
