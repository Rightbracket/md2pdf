# U-7c65ac — Minimize external dependencies to limit supply-chain attack surface

_Kind: **Understanding** · Exported 2026-06-03 02:16:08 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-7c65ac"></a>
## U-7c65ac — Minimize external dependencies to limit supply-chain attack surface

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:28:03

**Client-stated, load-bearing posture.** The Client wants to keep a limited set of external dependencies to avoid supply-chain attack surface. This is upstream of several specific Decisions in the codebase and retro-explains the existing posture.

## What this implies in the artifact today

- **Hand-rolled `typst::World`** (`src/pipeline/world.rs`) instead of pulling `typst-as-lib` (which would add `reqwest` / `ureq` feature paths to the dep graph) — see Cargo.toml lines 19–24 for the in-source rationale.
- **Hand-rolled mermaid subsystem** (`src/mermaid/`, ~4900 LOC) instead of binding to a JS mermaid renderer or shelling to one. `layout-rs` was apparently floated and is *not* in use.
- **Hand-rolled SVG writer** (`src/mermaid/svg_buf.rs`) used for mermaid emit "rather than" the declared `svg` crate.
- **No Chromium**, which is also independently a stated requirement (see Vision V-7e24cd) but is consistent with this posture.
- **Exact-version pinning + minimal default features** (see U-e61b99) sits in the same posture family even though the Security Engineer authored the specific rule.

## How to apply this Understanding

- New Work that proposes adding a crate should justify it against this posture, especially if the crate is npm-style (many transitive deps) or pulls a TLS / HTTP stack already covered in-tree.
- Hand-rolling is a legitimate first option here, not a last resort. The Client explicitly said: "that could explain some of the preference for home-grown code over adding new crates."
- This does NOT mean zero deps — typst, pulldown-cmark, ureq, image, etc. are accepted load-bearing libraries. The posture is about marginal additions.

## Suspects flagged by the Archaeologist that align with this posture

- `svg` crate declared in Cargo.toml but apparently unused (replaced by hand-rolled `svg_buf.rs`). Cleanup candidate.
- `ecow` declared but no direct `use ecow::` found in src/ (may be transitive-but-declared). Worth confirming.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-supply-chain-posture"
}
```

---

