# U-6a598c — Self-contained binary — no external runtime dependencies

_Kind: **Understanding** · Exported 2026-06-03 02:16:08 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-6a598c"></a>
## U-6a598c — Self-contained binary — no external runtime dependencies

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:33:11

**Client-confirmed posture.** The md2pdf binary is meant to be fully shippable with no external runtime dependencies. A user should be able to download the binary, run it, and have it work — no separate font files, no JS runtime, no system libraries beyond what every modern OS ships, no Chromium.

## What this implies in the artifact today

- **Bundled fonts.** `Twemoji.Mozilla.ttf` is embedded via `include_bytes!` in `src/pipeline/world.rs`; `typst-assets` is pulled with the `fonts` feature so default Latin/serif/mono fonts are also bundled. No `.ttf` files need to ship alongside the binary.
- **Hand-rolled `typst::World`** (also justified by U-7c65ac) carries fonts in-memory rather than loading from a system font directory.
- **No Chromium / no JS runtime** — also stated independently in the Vision; consistent with this posture.
- **TLS via rustls + webpki-roots** — no OpenSSL / no system trust store dependency, so HTTP image fetch works on a fresh machine without `libssl` installed.
- **Release profile** uses `strip + thin LTO + codegen-units=1` to keep the binary small while still being self-contained.

## Distinct from U-7c65ac

- **U-7c65ac (supply-chain):** about what crates we *build* against. Concerns attack surface, transitive trust.
- **This Understanding (self-contained runtime):** about what a *user* needs to run the binary. Concerns distribution friction, portability.
- They overlap (no-Chromium satisfies both) but a Work item could satisfy one without the other. New Work proposals should be evaluated against both postures separately.

## How to apply this Understanding

- Anything that would require the user to install something extra (a font, a runtime, a config file in a known location, an external binary) is suspect. Default first option is to bundle / embed / vendor.
- Acceptable exceptions: things the OS already provides (libc), things the user explicitly asks for at the CLI (input file, output path), and network resources fetched at runtime when the user supplies a URL (image fetch).
- The Client did not explicitly request font embedding; they said "I'm happy that they are — it sounds like the previous builders did the right thing." Treat that as Client ratification of the embedding pattern as a whole.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-self-contained-binary"
}
```

---

