# Eddie's shot log — md2pdf

## Day 1 — Blank PDF; missing body font

House walked me in: *"renders pdfs but they are blank"*. Called the
shot before chalking: only Twemoji was loaded into the `typst::World`,
no body font for Latin, content stream comes out empty. Confirmed —
zlib'd the lone stream in the existing README.pdf, got zero bytes.
Easy money.

Pulled `typst-assets` with the `fonts` feature, fed
`typst_assets::fonts()` into `build_font_set()` ahead of Twemoji so
the shaper picks Libertinus for body and falls back to the colour
emoji for codepoints only it covers. Old code commented that
`Library::default()` carries text fonts — it doesn't, never did; left
a note in `world.rs` so nobody re-racks that scratch.

Verified: README.pdf now 26 KB with real `BT…TJ…ET` and embedded
Libertinus. Pipeline tests green.

Pre-existing scratch on the next table: 5 failing tests in
`src/mermaid/` — but that whole module is untracked WIP and not wired
to the binary. Not my hand to play tonight.

— Eddie
