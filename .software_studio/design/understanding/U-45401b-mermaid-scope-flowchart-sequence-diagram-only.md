# U-45401b — Mermaid scope — flowchart + sequence diagram only

_Kind: **Understanding** · Exported 2026-06-05 20:35:19 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-45401b"></a>
## U-45401b — Mermaid scope — flowchart + sequence diagram only

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:28:30

**Client-confirmed scope.** The Client confirms that **flowchart** and **sequence diagram** are sufficient mermaid diagram types for md2pdf. No requirement to support additional mermaid types (gantt, classDiagram, stateDiagram, ER, etc.) at this time.

## What is built today

- `src/mermaid/diagram_type.rs` — sniffer that classifies an incoming mermaid block as flowchart, sequence, or unsupported.
- `src/mermaid/flowchart/` — parser, IR, layout (using `petgraph`), emit. ~3000 LOC.
- `src/mermaid/sequence.rs` — single-file sequence-diagram renderer. ~1954 LOC.
- `src/mermaid/svg_buf.rs` — hand-rolled SVG writer used by both.

## Architecture rationale

- The hand-rolled approach (vs. binding to a JS mermaid renderer or shelling out) is consistent with the supply-chain posture — see U-<minimize-deps Understanding>. This is not arbitrary; it is the expected first-choice approach for new diagram-related work in this codebase.
- `layout-rs` was reportedly considered (referenced in a prior Decision) but is not in use. The deviation from that earlier Decision is acknowledged in `flowchart/layout.rs:5`.

## Out of scope

- Other mermaid diagram types. If a future request lands for one, it is a scope expansion that deserves its own Decision — not a drive-by addition.
- Importing a mermaid-rendering crate or shelling to a JS renderer. Both are presumed-rejected under the supply-chain posture; explicit Decision required to revisit.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-mermaid-scope"
}
```

---

