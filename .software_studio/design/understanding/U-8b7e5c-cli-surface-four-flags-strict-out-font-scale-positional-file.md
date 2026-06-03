# U-8b7e5c — CLI surface — four flags (--strict, --out, --font-scale, positional FILE)

_Kind: **Understanding** · Exported 2026-06-03 02:16:08 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-8b7e5c"></a>
## U-8b7e5c — CLI surface — four flags (--strict, --out, --font-scale, positional FILE)

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:28:11

**Client-confirmed.** The four flags landed over several iterations but constitute the *intended* CLI design. Client believes they work correctly today.

## The CLI surface

```
md2pdf [--strict] [--out PATH] [--font-scale N] <FILE>
```

- `<FILE>` — positional, the input Markdown file. Required.
- `--out PATH` — explicit output path. Validated by `validate_out_path` in `src/cli.rs`.
- (no `--out`) — default output is derived: input `foo.md` becomes `foo.pdf` written next to the input. (`derive_output_path` in `src/cli.rs`.)
- `--font-scale N` — parsed by a `FontScale` parser; the value is injected into the Typst source as `#let md2pdf_body_size = <N>pt`.
- `--strict` — strict-mode gate. Behavioral details to be captured separately when we revisit the strict flag (Client unsure on the exact semantics; suspects it relates to remote-image-fetch failure handling but is not certain).

## What this Understanding does NOT cover

- The exact semantics of `--strict` — deferred to a separate Understanding once we confirm the runtime behavior. The Client's recollection ("`--strict` is related to behavior when the remote image is not fetchable") is partial and explicitly tentative.
- Exit-code mapping — separate Understanding.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-cli-surface"
}
```

---

