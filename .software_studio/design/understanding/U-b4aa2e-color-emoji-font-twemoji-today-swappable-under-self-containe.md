# U-b4aa2e — Color emoji font — Twemoji today, swappable under self-contained-binary constraints

_Kind: **Understanding** · Exported 2026-06-05 20:35:19 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-b4aa2e"></a>
## U-b4aa2e — Color emoji font — Twemoji today, swappable under self-contained-binary constraints

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:33:11

**Client-confirmed by ratification.** Color emoji rendering is a Vision-level requirement (V-7e24cd). The current bundled font is **Twemoji** (Mozilla's COLRv0 build, `assets/fonts/Twemoji.Mozilla.ttf`, sourced from `mozilla/twemoji-colr` v0.7.0, CC-BY 4.0 attribution).

## Client posture on font choice

- Indifferent on which color-emoji font is used. "I've only seen Twemoji and I think it looks just fine, so I don't see a reason to want to change it, but I'm not married to it."
- A future Work that proposes swapping to e.g. Noto Color Emoji is a **free swap** — it does not require revisiting a Decision; it just needs to maintain the constraints below.

## Hard constraints on any emoji-font choice

- Must be a **color emoji** font (the Vision-level requirement).
- Must be **bundleable into the binary** (per U-<self-contained-binary>) — no font that requires the user to install it separately.
- Must have a **license compatible with binary redistribution** (Twemoji's CC-BY 4.0 attribution is satisfied today via the README citation).

## Open questions deferred

- **Fallback behavior** when an emoji codepoint is not in the bundled font: not specified by the Client ("this issue hasn't come up"). Today's behavior is whatever Typst's font-fallback does when given the World's font set; if a real case surfaces, capture as a separate Understanding then.
- No platform-specific styling requirement (Apple-style vs. Discord-style vs. Google-style). The Client has not asked for one.

## Embedding mechanism

- `include_bytes!` of the .ttf into `src/pipeline/world.rs`, served through the hand-rolled `typst::World`. This pattern is the expected default for any future bundled-font addition (per U-<self-contained-binary>).

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-twemoji"
}
```

---

