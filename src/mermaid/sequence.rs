//! `sequenceDiagram` sub-renderer (Work W-4c3f16).
//!
//! Pipeline per D-fb4ebb §1 and the W-4c3f16 spec:
//!
//! ```text
//!   mermaid sequenceDiagram source
//!     → tokenize (line-oriented)
//!     → recursive-descent parse → SequenceIr
//!     → deterministic top-down layout → LaidOutDiagram
//!     → SVG emit (constrained subset) → bytes
//! ```
//!
//! ## Scope (per W-4c3f16)
//!
//! - `participant <id> [as <label>]`, `actor <id> [as <label>]`
//! - sync messages (`->`, `->>`)
//! - async messages (`-->>`)
//! - dotted no-arrow lines (`-->`) — accepted as a permissive
//!   superset because they are mermaid-spec syntax and excluding them
//!   would force authors to round-trip syntactically valid input
//! - self-messages (same actor on both sides)
//! - activations: `activate <id>`, `deactivate <id>`, and the `+` /
//!   `-` shorthand on the message (`A->>+B: ...`, `A->>-B: ...`)
//! - notes: `Note over <a>[, <b>]: text`, `Note left of <a>: text`,
//!   `Note right of <a>: text`
//! - simple loops: `loop <label>` ... `end`
//! - simple alts: `alt <label>` ... `else <label>` ... `end`
//!
//! ## Out of scope (typed errors)
//!
//! `par`/`and`, nested loops/alts beyond one level deep, autonumber,
//! links, background highlights, opt blocks, rect blocks, critical
//! blocks, breaks. Each fails with `MermaidError::UnsupportedFeature`
//! naming the construct.
//!
//! ## Hardening
//!
//! Every parse step honors the limits in `super::limits`. The parser
//! never recurses on user input — group nesting is tracked via an
//! explicit stack with a hard depth cap. There is no regex engine; no
//! backtracking; no allocator-quadratic step.

use super::MermaidError;
use super::svg_buf::{Attrs, SvgBuf};

/// Local alias for the parent module's hard caps so the body of this
/// file can keep using `limits::MAX_*` (which is also how the
/// flowchart sub-renderer's parser refers to them).
mod limits {
    pub use super::super::{
        MAX_ACTORS, MAX_EVENTS, MAX_GROUP_DEPTH, MAX_INPUT_BYTES as MAX_SOURCE_BYTES,
        MAX_LABEL_LEN as MAX_LABEL_BYTES, MAX_LINES, MAX_LINE_BYTES,
    };
}

/// Public entry point. Called from `mermaid::render` after the
/// diagram-type sniffer has confirmed `sequenceDiagram`.
pub fn render(source: &str) -> Result<Vec<u8>, MermaidError> {
    let ir = parse(source)?;
    let laid = layout(&ir);
    Ok(emit(&laid))
}

// ---------------------------------------------------------------------------
// IR
// ---------------------------------------------------------------------------

/// Parsed sequenceDiagram. Preserves source order of events.
#[derive(Debug, Default, PartialEq)]
pub struct SequenceIr {
    pub actors: Vec<Actor>,
    pub events: Vec<Event>,
}

#[derive(Debug, PartialEq)]
pub struct Actor {
    /// Canonical id used in messages. mermaid lets `participant Foo`
    /// declare the id `Foo`; later references use the same string.
    pub id: String,
    /// Display label. Defaults to `id` if no `as <label>` was given.
    pub label: String,
    /// `participant` (rectangle head) vs `actor` (stick figure head).
    pub kind: ActorKind,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum ActorKind {
    Participant,
    Actor,
}

/// Index into `SequenceIr::actors`.
pub type ActorIdx = usize;

#[derive(Debug, PartialEq)]
pub enum Event {
    Message {
        from: ActorIdx,
        to: ActorIdx,
        kind: MessageKind,
        text: String,
        /// `+` shorthand: activate `to` immediately after this message.
        activate_target: bool,
        /// `-` shorthand: deactivate `from` immediately after this
        /// message.
        deactivate_source: bool,
    },
    Note {
        placement: NotePlacement,
        text: String,
    },
    Activate(ActorIdx),
    Deactivate(ActorIdx),
    GroupStart {
        kind: GroupKind,
        label: String,
    },
    GroupElse {
        label: String,
    },
    GroupEnd,
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum MessageKind {
    /// `->>` solid arrow.
    SyncSolidArrow,
    /// `->` solid line, no arrow head. Mermaid lists this as
    /// "deprecated/no arrow"; we render a line without an arrow.
    SolidNoArrow,
    /// `-->>` dotted arrow (async).
    AsyncDottedArrow,
    /// `-->` dotted line, no arrow head.
    DottedNoArrow,
}

#[derive(Debug, PartialEq)]
pub enum NotePlacement {
    Over { actors: Vec<ActorIdx> },
    LeftOf { actor: ActorIdx },
    RightOf { actor: ActorIdx },
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum GroupKind {
    Loop,
    Alt,
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

mod parse_impl {
    use super::*;

    pub fn parse(source: &str) -> Result<SequenceIr, MermaidError> {
        // Source-bytes cap is enforced upstream by `mermaid::render`.
        // Re-check in case this function is reachable from another
        // entry point (e.g. tests).
        if source.len() > limits::MAX_SOURCE_BYTES {
            return Err(MermaidError::InputTooLarge { cap: "MAX_SOURCE_BYTES", limit: limits::MAX_SOURCE_BYTES });
        }

        let mut ir = SequenceIr::default();
        // Group-nesting stack: tracks open `loop`/`alt` blocks. Each
        // entry is the source line where the group was opened (used
        // for diagnostics on unmatched `end`).
        let mut group_stack: Vec<usize> = Vec::new();
        // Track maximum depth seen — used for the v1 "one level deep"
        // honest-scope check.
        let mut deepest_depth: usize = 0;

        let mut line_count: usize = 0;
        let mut event_count: usize = 0;
        let mut header_seen = false;

        for (lineno_zero, raw_line) in source.lines().enumerate() {
            line_count += 1;
            if line_count > limits::MAX_LINES {
                return Err(MermaidError::InputTooLarge { cap: "lines", limit: limits::MAX_LINES });
            }
            if raw_line.len() > limits::MAX_LINE_BYTES {
                return Err(MermaidError::InputTooLarge { cap: "line bytes", limit: limits::MAX_LINE_BYTES });
            }
            let lineno = lineno_zero + 1;
            let line = raw_line.trim();

            if line.is_empty() {
                continue;
            }
            if line.starts_with("%%") {
                continue;
            }

            // First non-blank line must be `sequenceDiagram`. The
            // diagram-type sniffer in `mermaid::mod` already guarantees
            // this, but the parser also honors it so it is callable
            // independently.
            if !header_seen {
                if line == "sequenceDiagram"
                    || line.starts_with("sequenceDiagram ")
                    || line.starts_with("sequenceDiagram\t")
                {
                    header_seen = true;
                    continue;
                }
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: "expected `sequenceDiagram` header as first non-blank line"
                        .to_string(),
                });
            }

            parse_one_line(
                lineno,
                line,
                &mut ir,
                &mut group_stack,
                &mut deepest_depth,
                &mut event_count,
            )?;
        }

        if !header_seen {
            // Source was empty after stripping comments.
            return Err(MermaidError::ParseError {
                line: 0,
                reason: "no `sequenceDiagram` header found".to_string(),
            });
        }

        if let Some(open_at) = group_stack.last().copied() {
            return Err(MermaidError::ParseError {
                line: open_at,
                reason: "unclosed group: missing `end`".to_string(),
            });
        }

        // Honest-scope check: per W-4c3f16, v1 supports at most one
        // level of nesting for loop/alt. If the input went deeper, we
        // already accepted it during parse (the parser was generous);
        // surface it as an UnsupportedFeature error so the author
        // sees the limit.
        if deepest_depth > 1 {
            return Err(MermaidError::ParseError { line: 0, reason: format!("unsupported feature: {} ({})", format!("nesting depth {deepest_depth}"), "v1 supports at most one level of `loop`/`alt` nesting; \
                       refactor the diagram or wait for a later release") });
        }

        Ok(ir)
    }

    fn parse_one_line(
        lineno: usize,
        line: &str,
        ir: &mut SequenceIr,
        group_stack: &mut Vec<usize>,
        deepest_depth: &mut usize,
        event_count: &mut usize,
    ) -> Result<(), MermaidError> {
        // Recognize keyword-led directives first; messages are the
        // catch-all path. Order matters: `participant` and `actor` are
        // declarations, not actor names.

        // Out-of-subset constructs we explicitly recognize and reject
        // by name so the diagnostic is actionable.
        for unsupported in [
            ("par ", "par"),
            ("par\t", "par"),
            ("and ", "and"),
            ("and\t", "and"),
            ("autonumber", "autonumber"),
            ("opt ", "opt"),
            ("opt\t", "opt"),
            ("critical ", "critical"),
            ("break ", "break"),
            ("rect ", "rect"),
            ("link ", "link"),
            ("links ", "links"),
            ("properties ", "properties"),
            ("details ", "details"),
        ] {
            let (prefix, kw) = unsupported;
            if line == kw || line.starts_with(prefix) {
                return Err(MermaidError::ParseError { line: lineno, reason: format!("unsupported feature: {} ({})", kw.to_string(), "this sequenceDiagram construct is not in the v1 subset") });
            }
        }

        if let Some(rest) = strip_keyword(line, "participant") {
            ir.add_actor(lineno, rest, ActorKind::Participant)?;
            return Ok(());
        }
        if let Some(rest) = strip_keyword(line, "actor") {
            ir.add_actor(lineno, rest, ActorKind::Actor)?;
            return Ok(());
        }
        if let Some(rest) = strip_keyword(line, "activate") {
            let id = rest.trim();
            if id.is_empty() {
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: "`activate` requires an actor id".to_string(),
                });
            }
            let idx = ir.intern_actor(id, ActorKind::Participant)?;
            push_event(ir, event_count, Event::Activate(idx))?;
            return Ok(());
        }
        if let Some(rest) = strip_keyword(line, "deactivate") {
            let id = rest.trim();
            if id.is_empty() {
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: "`deactivate` requires an actor id".to_string(),
                });
            }
            let idx = ir.intern_actor(id, ActorKind::Participant)?;
            push_event(ir, event_count, Event::Deactivate(idx))?;
            return Ok(());
        }
        if let Some(rest) = strip_keyword_ci(line, "Note") {
            let event = parse_note(lineno, rest, ir)?;
            push_event(ir, event_count, event)?;
            return Ok(());
        }
        if let Some(rest) = strip_keyword(line, "loop") {
            let label = rest.trim().to_string();
            check_label_size(lineno, &label)?;
            push_group(group_stack, deepest_depth, lineno)?;
            push_event(
                ir,
                event_count,
                Event::GroupStart {
                    kind: GroupKind::Loop,
                    label,
                },
            )?;
            return Ok(());
        }
        if let Some(rest) = strip_keyword(line, "alt") {
            let label = rest.trim().to_string();
            check_label_size(lineno, &label)?;
            push_group(group_stack, deepest_depth, lineno)?;
            push_event(
                ir,
                event_count,
                Event::GroupStart {
                    kind: GroupKind::Alt,
                    label,
                },
            )?;
            return Ok(());
        }
        if let Some(rest) = strip_keyword(line, "else") {
            if group_stack.is_empty() {
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: "`else` outside any `alt` block".to_string(),
                });
            }
            let label = rest.trim().to_string();
            check_label_size(lineno, &label)?;
            push_event(ir, event_count, Event::GroupElse { label })?;
            return Ok(());
        }
        if line == "end" {
            if group_stack.pop().is_none() {
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: "`end` without a matching `loop`/`alt`".to_string(),
                });
            }
            push_event(ir, event_count, Event::GroupEnd)?;
            return Ok(());
        }

        // Otherwise it must be a message line.
        let event = parse_message(lineno, line, ir)?;
        push_event(ir, event_count, event)?;
        Ok(())
    }

    /// Strip `keyword` if `line` starts with it followed by whitespace
    /// or end of line. Case-sensitive.
    fn strip_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
        if line == keyword {
            return Some("");
        }
        let kw_bytes = keyword.as_bytes();
        if line.len() <= kw_bytes.len() {
            return None;
        }
        if !line.as_bytes().starts_with(kw_bytes) {
            return None;
        }
        let next_byte = line.as_bytes()[kw_bytes.len()];
        if next_byte == b' ' || next_byte == b'\t' {
            Some(&line[kw_bytes.len()..])
        } else {
            None
        }
    }

    /// Case-insensitive variant for `Note` (mermaid spec uses `Note`
    /// with a capital N but is forgiving).
    fn strip_keyword_ci<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
        if line.len() < keyword.len() {
            return None;
        }
        let head = &line[..keyword.len()];
        if !head.eq_ignore_ascii_case(keyword) {
            return None;
        }
        let rest_bytes = line.as_bytes().get(keyword.len()).copied();
        match rest_bytes {
            None => Some(""),
            Some(b' ') | Some(b'\t') => Some(&line[keyword.len()..]),
            _ => None,
        }
    }

    fn parse_note(
        lineno: usize,
        rest: &str,
        ir: &mut SequenceIr,
    ) -> Result<Event, MermaidError> {
        // Forms:
        //   Note over A[, B]: text
        //   Note left of A: text
        //   Note right of A: text
        let rest = rest.trim_start();
        let (placement_part, text) = split_at_colon(lineno, rest)?;
        let placement_part = placement_part.trim();
        let text = text.trim().to_string();
        check_label_size(lineno, &text)?;

        let placement = if let Some(actors_part) = strip_keyword(placement_part, "over") {
            let names: Vec<&str> = actors_part
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .collect();
            if names.is_empty() {
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: "`Note over` requires at least one actor".to_string(),
                });
            }
            let mut idxs = Vec::with_capacity(names.len());
            for n in names {
                idxs.push(ir.intern_actor(n, ActorKind::Participant)?);
            }
            NotePlacement::Over { actors: idxs }
        } else if let Some(actor_part) =
            strip_two_word_keyword(placement_part, "left", "of")
        {
            let id = actor_part.trim();
            if id.is_empty() {
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: "`Note left of` requires an actor id".to_string(),
                });
            }
            NotePlacement::LeftOf {
                actor: ir.intern_actor(id, ActorKind::Participant)?,
            }
        } else if let Some(actor_part) =
            strip_two_word_keyword(placement_part, "right", "of")
        {
            let id = actor_part.trim();
            if id.is_empty() {
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: "`Note right of` requires an actor id".to_string(),
                });
            }
            NotePlacement::RightOf {
                actor: ir.intern_actor(id, ActorKind::Participant)?,
            }
        } else {
            return Err(MermaidError::ParseError {
                line: lineno,
                reason: format!(
                    "expected `Note over|left of|right of <actor>: text` but got `Note {placement_part}`"
                ),
            });
        };

        Ok(Event::Note { placement, text })
    }

    fn strip_two_word_keyword<'a>(
        s: &'a str,
        first: &str,
        second: &str,
    ) -> Option<&'a str> {
        let after_first = strip_keyword(s, first)?;
        let after_first = after_first.trim_start();
        let after_second = strip_keyword(after_first, second)?;
        Some(after_second)
    }

    fn split_at_colon(lineno: usize, s: &str) -> Result<(&str, &str), MermaidError> {
        match s.find(':') {
            Some(idx) => Ok((&s[..idx], &s[idx + 1..])),
            None => Err(MermaidError::ParseError {
                line: lineno,
                reason: "expected `: <text>` in note/message".to_string(),
            }),
        }
    }

    fn check_label_size(lineno: usize, label: &str) -> Result<(), MermaidError> {
        if label.len() > limits::MAX_LABEL_BYTES {
            return Err(MermaidError::InputTooLarge { cap: "label bytes", limit: limits::MAX_LABEL_BYTES });
        }
        let _ = lineno;
        Ok(())
    }

    fn parse_message(
        lineno: usize,
        line: &str,
        ir: &mut SequenceIr,
    ) -> Result<Event, MermaidError> {
        // Find the arrow operator. We look for the longest match first
        // so `-->>` is preferred over `-->`, and `->>` over `->`.
        // Also detect the `+` / `-` activation shorthand which can
        // appear immediately after the arrow.
        const ARROWS: &[(&str, MessageKind)] = &[
            ("-->>", MessageKind::AsyncDottedArrow),
            ("->>", MessageKind::SyncSolidArrow),
            ("-->", MessageKind::DottedNoArrow),
            ("->", MessageKind::SolidNoArrow),
        ];

        // Reject mermaid arrow forms we do not yet support. We do this
        // by recognizing them and returning a typed error rather than
        // letting them slip through as parse failures.
        const REJECTED_ARROWS: &[&str] = &["-x", "--x", "-)", "--)"];
        for arrow in REJECTED_ARROWS {
            if line.contains(arrow) {
                // Only reject if it appears as an actual arrow (preceded
                // by whitespace or non-arrow chars and followed by a
                // valid context). Cheap check: ensure not embedded in an
                // identifier.
                if arrow_is_token(line, arrow) {
                    return Err(MermaidError::ParseError {
                        line: lineno,
                        reason: format!(
                            "unsupported feature: arrow `{arrow}` (v1 supports `->`, `->>`, `-->`, and `-->>` arrows only)"
                        ),
                    });
                }
            }
        }

        let (arrow, kind, idx) = ARROWS
            .iter()
            .find_map(|(a, k)| line.find(a).map(|i| (*a, *k, i)))
            .ok_or_else(|| MermaidError::ParseError {
                line: lineno,
                reason: format!("could not parse line as a reason: `{line}`"),
            })?;

        let (left_part, after_arrow) = (&line[..idx], &line[idx + arrow.len()..]);
        let left = left_part.trim();
        if left.is_empty() {
            return Err(MermaidError::ParseError {
                line: lineno,
                reason: "missing source actor before arrow".to_string(),
            });
        }

        // Activation shorthand: a single `+` or `-` immediately after
        // the arrow attaches to the *target* (`+`) or *source* (`-`).
        let mut after = after_arrow;
        let mut activate_target = false;
        let mut deactivate_source = false;
        let after_trimmed = after.trim_start();
        if let Some(stripped) = after_trimmed.strip_prefix('+') {
            activate_target = true;
            after = stripped;
        } else if let Some(stripped) = after_trimmed.strip_prefix('-') {
            deactivate_source = true;
            after = stripped;
        }

        let (right_part, text) = split_at_colon(lineno, after)?;
        let right = right_part.trim();
        if right.is_empty() {
            return Err(MermaidError::ParseError {
                line: lineno,
                reason: "missing target actor after arrow".to_string(),
            });
        }
        let text = text.trim().to_string();
        check_label_size(lineno, &text)?;

        let from = ir.intern_actor(left, ActorKind::Participant)?;
        let to = ir.intern_actor(right, ActorKind::Participant)?;

        Ok(Event::Message {
            from,
            to,
            kind,
            text,
            activate_target,
            deactivate_source,
        })
    }

    fn arrow_is_token(line: &str, arrow: &str) -> bool {
        // Check that the substring `arrow` appears at a position that
        // cannot be confused for a longer arrow. Specifically: ensure
        // the character immediately before is not '-' and the character
        // after is not '>' (for the no-arrowhead candidates) or '-'
        // (which would extend `--` further).
        let mut start = 0;
        while let Some(idx) = line[start..].find(arrow) {
            let abs = start + idx;
            let before = if abs == 0 {
                None
            } else {
                line.as_bytes().get(abs - 1).copied()
            };
            let after_idx = abs + arrow.len();
            let after = line.as_bytes().get(after_idx).copied();
            // We only rule it out if the arrow is part of a normal arrow
            // we DO support. Conservatively: any occurrence is a token.
            // (We are looking for `-x`, `--x`, `-)`, `--)`. None of
            // those fragments appear inside `->` `->>` `-->` `-->>`.)
            let _ = before;
            let _ = after;
            return true;
            #[allow(unreachable_code)]
            {
                start = abs + arrow.len();
            }
        }
        false
    }

    fn push_event(
        ir: &mut SequenceIr,
        counter: &mut usize,
        event: Event,
    ) -> Result<(), MermaidError> {
        *counter += 1;
        if *counter > limits::MAX_EVENTS {
            return Err(MermaidError::InputTooLarge { cap: "events", limit: limits::MAX_EVENTS });
        }
        ir.events.push(event);
        Ok(())
    }

    fn push_group(
        stack: &mut Vec<usize>,
        deepest: &mut usize,
        lineno: usize,
    ) -> Result<(), MermaidError> {
        stack.push(lineno);
        if stack.len() > limits::MAX_GROUP_DEPTH {
            return Err(MermaidError::InputTooLarge { cap: "group nesting", limit: limits::MAX_GROUP_DEPTH });
        }
        if stack.len() > *deepest {
            *deepest = stack.len();
        }
        Ok(())
    }
}

pub use parse_impl::parse;

impl SequenceIr {
    /// Add an explicit actor declaration. `rest` is the input after
    /// `participant` / `actor` has been stripped, e.g.
    /// `Alice as Alice the Builder`.
    fn add_actor(
        &mut self,
        lineno: usize,
        rest: &str,
        kind: ActorKind,
    ) -> Result<(), MermaidError> {
        let rest = rest.trim();
        if rest.is_empty() {
            return Err(MermaidError::ParseError {
                line: lineno,
                reason: "`participant`/`actor` requires a name".to_string(),
            });
        }
        let (id, label) = match split_as_alias(rest) {
            Some((id, label)) => (id.to_string(), label.to_string()),
            None => (rest.to_string(), rest.to_string()),
        };
        if id.len() > limits::MAX_LABEL_BYTES || label.len() > limits::MAX_LABEL_BYTES {
            return Err(MermaidError::InputTooLarge { cap: "actor label bytes", limit: limits::MAX_LABEL_BYTES });
        }
        // Re-declarations: if an actor with the same id exists, update
        // the label and possibly upgrade kind.
        if let Some(existing) = self.actors.iter_mut().find(|a| a.id == id) {
            existing.label = label;
            existing.kind = kind;
            return Ok(());
        }
        if self.actors.len() >= limits::MAX_ACTORS {
            return Err(MermaidError::InputTooLarge { cap: "actors", limit: limits::MAX_ACTORS });
        }
        self.actors.push(Actor { id, label, kind });
        Ok(())
    }

    /// Look up or create an actor referenced by id. Used for
    /// implicit-declaration in messages and notes.
    fn intern_actor(&mut self, id: &str, kind: ActorKind) -> Result<ActorIdx, MermaidError> {
        let id = id.trim();
        if id.is_empty() {
            return Err(MermaidError::ParseError {
                line: 0,
                reason: "empty actor id".to_string(),
            });
        }
        if id.len() > limits::MAX_LABEL_BYTES {
            return Err(MermaidError::InputTooLarge { cap: "actor id bytes", limit: limits::MAX_LABEL_BYTES });
        }
        if let Some((idx, _)) = self.actors.iter().enumerate().find(|(_, a)| a.id == id) {
            return Ok(idx);
        }
        if self.actors.len() >= limits::MAX_ACTORS {
            return Err(MermaidError::InputTooLarge { cap: "actors", limit: limits::MAX_ACTORS });
        }
        let idx = self.actors.len();
        self.actors.push(Actor {
            id: id.to_string(),
            label: id.to_string(),
            kind,
        });
        Ok(idx)
    }
}

/// Recognize the mermaid-spec ` as ` separator (case-sensitive,
/// whitespace-padded).
fn split_as_alias(rest: &str) -> Option<(&str, &str)> {
    let rest = rest.trim();
    // We must match ` as ` as a whole word so an id containing "as"
    // (e.g. "TaskMaster") is not split.
    let needle = " as ";
    let idx = rest.find(needle)?;
    let id = rest[..idx].trim();
    let label = rest[idx + needle.len()..].trim();
    if id.is_empty() || label.is_empty() {
        return None;
    }
    Some((id, label))
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

/// Style constants. Everything is in user-space units (≈ pixels at 1×).
mod style {
    // Header (actor box) geometry.
    pub const HEADER_BOX_HEIGHT: f32 = 36.0;
    pub const HEADER_PAD_X: f32 = 16.0;
    /// Padding inside an actor head box (per side).
    pub const HEADER_TEXT_PAD: f32 = 8.0;

    // Column / row spacing.
    pub const ROW_HEIGHT: f32 = 36.0;
    pub const COLUMN_MIN_WIDTH: f32 = 140.0;
    pub const COLUMN_GAP: f32 = 16.0;
    /// Y of the first event row (below header).
    pub const HEADER_TO_FIRST_ROW: f32 = 28.0;
    /// Bottom padding after the last event before the footer header
    /// (mermaid repeats actor heads at the bottom).
    pub const LAST_ROW_TO_FOOTER: f32 = 24.0;

    // Activation bar geometry.
    pub const ACTIVATION_WIDTH: f32 = 10.0;

    // Note geometry.
    pub const NOTE_PAD_X: f32 = 8.0;
    pub const NOTE_PAD_Y: f32 = 6.0;
    pub const NOTE_HEIGHT: f32 = 28.0;
    pub const NOTE_LEFT_RIGHT_OFFSET: f32 = 16.0;
    pub const NOTE_LEFT_RIGHT_WIDTH: f32 = 110.0;

    // Group (loop / alt) geometry.
    pub const GROUP_LEFT_PAD: f32 = 24.0;
    pub const GROUP_RIGHT_PAD: f32 = 24.0;
    pub const GROUP_TOP_PAD: f32 = 16.0;
    pub const GROUP_BOTTOM_PAD: f32 = 8.0;
    pub const GROUP_LABEL_TAB_W: f32 = 56.0;
    pub const GROUP_LABEL_TAB_H: f32 = 18.0;

    // Margins.
    pub const DOC_MARGIN_X: f32 = 24.0;
    pub const DOC_MARGIN_TOP: f32 = 24.0;
    pub const DOC_MARGIN_BOTTOM: f32 = 24.0;

    // Text metrics — approximation. We do not pull rustybuzz here; we
    // approximate label widths as `chars * char_width`, which is good
    // enough for layout deterministically and is the same heuristic
    // used by the flowchart Work for consistency.
    pub const TEXT_FONT_SIZE: f32 = 13.0;
    pub const HEADER_FONT_SIZE: f32 = 14.0;
    pub const NOTE_FONT_SIZE: f32 = 12.0;
    pub const GROUP_FONT_SIZE: f32 = 11.0;
    pub const CHAR_WIDTH_RATIO: f32 = 0.55; // ≈ Helvetica / Arial

    // Colors. All from the constrained palette in D-fb4ebb's SVG
    // subset — solid fills only.
    pub const COLOR_BG: &str = "#ffffff";
    pub const COLOR_ACTOR_FILL: &str = "#ECECFF";
    pub const COLOR_ACTOR_STROKE: &str = "#9370DB";
    pub const COLOR_LIFELINE: &str = "#999999";
    pub const COLOR_MSG_STROKE: &str = "#333333";
    pub const COLOR_TEXT: &str = "#222222";
    pub const COLOR_NOTE_FILL: &str = "#FFF5AD";
    pub const COLOR_NOTE_STROKE: &str = "#AAAA33";
    pub const COLOR_ACTIVATION_FILL: &str = "#F4F4F4";
    pub const COLOR_ACTIVATION_STROKE: &str = "#666666";
    pub const COLOR_GROUP_STROKE: &str = "#7777BB";
    pub const COLOR_GROUP_LABEL_FILL: &str = "#7777BB";
    pub const COLOR_GROUP_LABEL_TEXT: &str = "#ffffff";
}

/// Estimate the pixel width of a label rendered at the given font
/// size. Cheap approximation; deterministic across platforms.
fn measure_text(label: &str, font_size: f32) -> f32 {
    // Count Unicode scalar values, not bytes. Wide CJK glyphs are
    // approximated as the same as Latin; close enough for the v1
    // layout pass.
    let n = label.chars().count() as f32;
    n * font_size * style::CHAR_WIDTH_RATIO
}

/// A diagram with all coordinates fixed.
pub struct LaidOutDiagram<'a> {
    pub ir: &'a SequenceIr,
    pub width: f32,
    pub height: f32,
    /// X of each actor's lifeline (centerline).
    pub actor_x: Vec<f32>,
    /// Width of each actor's head box.
    pub actor_head_w: Vec<f32>,
    /// Y of the top of the first event row.
    pub first_row_y: f32,
    /// Y of the top of the footer (where the actor heads repeat).
    pub footer_y: f32,
    /// Per-event geometry, in source order.
    pub event_layout: Vec<EventLayout>,
}

#[derive(Clone)]
pub enum EventLayout {
    Message {
        y: f32,
        from: ActorIdx,
        to: ActorIdx,
        kind: MessageKind,
        text: String,
    },
    Note {
        y: f32,
        x: f32,
        w: f32,
        h: f32,
        text: String,
    },
    /// Activation start (box opens at `y` on the activation stack of
    /// `actor`). The end-y is filled in by the layout pass once the
    /// matching deactivation is found.
    Activation {
        actor: ActorIdx,
        start_y: f32,
        end_y: f32,
        /// Stack slot at start time — controls how far right of the
        /// lifeline the box is drawn (nested activations stagger).
        depth: usize,
    },
    Group {
        kind: GroupKind,
        labels: Vec<(f32, String)>, // (y of segment-start, label)
        start_y: f32,
        end_y: f32,
        left_x: f32,
        right_x: f32,
    },
}

pub fn layout(ir: &SequenceIr) -> LaidOutDiagram<'_> {
    // ----- Column widths -----
    // 1. Each actor's head-box width = head label width + padding.
    let mut actor_head_w: Vec<f32> = ir
        .actors
        .iter()
        .map(|a| {
            (measure_text(&a.label, style::HEADER_FONT_SIZE)
                + 2.0 * style::HEADER_TEXT_PAD)
                .max(60.0)
        })
        .collect();

    // 2. For each pair of adjacent actors, ensure the gap between them
    //    is wide enough for the widest message label that crosses
    //    exactly that gap (approximation: a message between adjacent
    //    actors needs the gap to fit its label).
    let n = ir.actors.len();
    let mut gap_min: Vec<f32> = vec![style::COLUMN_GAP; n.saturating_sub(1)];
    for ev in &ir.events {
        if let Event::Message {
            from, to, text, ..
        } = ev
        {
            if from == to {
                // Self-message: a small loop draws to the right of the
                // actor's lifeline. Need ~80px of room on the right.
                let need = (measure_text(text, style::TEXT_FONT_SIZE) + 24.0).max(60.0);
                if *from + 1 < n {
                    let i = *from;
                    if need > gap_min[i] {
                        gap_min[i] = need;
                    }
                } else {
                    // Last actor — widen its own column.
                    if let Some(w) = actor_head_w.last_mut() {
                        if *w < need {
                            *w = need;
                        }
                    }
                }
            } else {
                let lo = (*from).min(*to);
                let hi = (*from).max(*to);
                let label_w = measure_text(text, style::TEXT_FONT_SIZE) + 24.0;
                // Distribute label width across the spans it crosses.
                let span = hi - lo;
                if span >= 1 {
                    let per = label_w / span as f32;
                    for i in lo..hi {
                        if per > gap_min[i] {
                            gap_min[i] = per;
                        }
                    }
                }
            }
        }
    }

    // 3. Compute lifeline x-coordinates left to right.
    let mut actor_x: Vec<f32> = Vec::with_capacity(n);
    let mut cursor = style::DOC_MARGIN_X;
    for i in 0..n {
        // Center of head box i.
        let half_w = actor_head_w[i] / 2.0;
        let center = cursor + half_w;
        // Enforce min column width.
        let min_required = if i == 0 {
            half_w
        } else {
            actor_x[i - 1] + gap_min[i - 1]
        };
        let center = center.max(min_required);
        actor_x.push(center);
        cursor = center + half_w + style::HEADER_PAD_X;
        // Apply column-min-width as a floor on the cursor advance.
        if i + 1 < n && (actor_x[i] + gap_min[i]) < (cursor + style::COLUMN_GAP) {
            // not used directly — gap_min already factored in
        }
    }
    let _ = cursor;

    let total_width = if n == 0 {
        2.0 * style::DOC_MARGIN_X + style::COLUMN_MIN_WIDTH
    } else {
        actor_x[n - 1] + actor_head_w[n - 1] / 2.0 + style::DOC_MARGIN_X
    };

    // ----- Row heights / event y-positions -----
    let first_row_y = style::DOC_MARGIN_TOP + style::HEADER_BOX_HEIGHT + style::HEADER_TO_FIRST_ROW;
    let mut y = first_row_y;
    let mut event_layout: Vec<EventLayout> = Vec::with_capacity(ir.events.len());

    // Activation tracking: stack of (event_index_in_event_layout, depth)
    // per actor. We push when an activation opens and pop when one
    // closes; the open EventLayout::Activation entry has its end_y
    // fixed up.
    let mut active_stacks: Vec<Vec<usize>> = vec![Vec::new(); n]; // indices into event_layout

    // Group tracking: stack of indices into event_layout where a
    // GroupStart left a placeholder.
    struct OpenGroup {
        layout_idx: usize,
        start_y: f32,
        actors_lo: usize,
        actors_hi: usize,
    }
    let mut open_groups: Vec<OpenGroup> = Vec::new();

    for ev in &ir.events {
        match ev {
            Event::Message {
                from,
                to,
                kind,
                text,
                activate_target,
                deactivate_source,
            } => {
                // Self-messages take double the row height (loop draws
                // a small bracket to the right of the lifeline).
                let h = if from == to {
                    style::ROW_HEIGHT * 1.5
                } else {
                    style::ROW_HEIGHT
                };
                event_layout.push(EventLayout::Message {
                    y,
                    from: *from,
                    to: *to,
                    kind: *kind,
                    text: text.clone(),
                });
                if let Some(g) = open_groups.last_mut() {
                    g.actors_lo = g.actors_lo.min((*from).min(*to));
                    g.actors_hi = g.actors_hi.max((*from).max(*to));
                }
                let row_bottom = y + h;
                if *activate_target {
                    let depth = active_stacks[*to].len();
                    let idx = event_layout.len();
                    event_layout.push(EventLayout::Activation {
                        actor: *to,
                        start_y: row_bottom - 4.0,
                        end_y: row_bottom + style::ROW_HEIGHT, // placeholder
                        depth,
                    });
                    active_stacks[*to].push(idx);
                }
                if *deactivate_source {
                    if let Some(idx) = active_stacks[*from].pop() {
                        if let EventLayout::Activation { end_y, .. } = &mut event_layout[idx] {
                            *end_y = row_bottom;
                        }
                    }
                }
                y = row_bottom;
            }
            Event::Note { placement, text } => {
                let (x, w) = note_extent(placement, &actor_x, &actor_head_w, text);
                let h = style::NOTE_HEIGHT.max(
                    measure_text(text, style::NOTE_FONT_SIZE) / w.max(1.0) * style::NOTE_FONT_SIZE
                        + 2.0 * style::NOTE_PAD_Y,
                );
                event_layout.push(EventLayout::Note {
                    y,
                    x,
                    w,
                    h,
                    text: text.clone(),
                });
                if let Some(g) = open_groups.last_mut() {
                    let touched: Vec<ActorIdx> = match placement {
                        NotePlacement::Over { actors } => actors.clone(),
                        NotePlacement::LeftOf { actor } | NotePlacement::RightOf { actor } => {
                            vec![*actor]
                        }
                    };
                    for a in touched {
                        g.actors_lo = g.actors_lo.min(a);
                        g.actors_hi = g.actors_hi.max(a);
                    }
                }
                y += h + 8.0;
            }
            Event::Activate(a) => {
                let depth = active_stacks[*a].len();
                let idx = event_layout.len();
                event_layout.push(EventLayout::Activation {
                    actor: *a,
                    start_y: y - 6.0,
                    end_y: y + style::ROW_HEIGHT, // placeholder
                    depth,
                });
                active_stacks[*a].push(idx);
            }
            Event::Deactivate(a) => {
                if let Some(idx) = active_stacks[*a].pop() {
                    if let EventLayout::Activation { end_y, .. } = &mut event_layout[idx] {
                        *end_y = y;
                    }
                }
            }
            Event::GroupStart { kind, label } => {
                let layout_idx = event_layout.len();
                event_layout.push(EventLayout::Group {
                    kind: *kind,
                    labels: vec![(y, label.clone())],
                    start_y: y,
                    end_y: y + style::ROW_HEIGHT,
                    left_x: 0.0,
                    right_x: 0.0,
                });
                open_groups.push(OpenGroup {
                    layout_idx,
                    start_y: y,
                    actors_lo: usize::MAX,
                    actors_hi: 0,
                });
                y += style::GROUP_TOP_PAD;
            }
            Event::GroupElse { label } => {
                if let Some(g) = open_groups.last() {
                    if let EventLayout::Group { labels, .. } = &mut event_layout[g.layout_idx] {
                        labels.push((y, label.clone()));
                    }
                }
                y += style::GROUP_TOP_PAD;
            }
            Event::GroupEnd => {
                if let Some(g) = open_groups.pop() {
                    let end_y = y + style::GROUP_BOTTOM_PAD;
                    let lo = if g.actors_lo == usize::MAX { 0 } else { g.actors_lo };
                    let hi = if g.actors_hi == 0 && g.actors_lo == usize::MAX {
                        n.saturating_sub(1)
                    } else {
                        g.actors_hi
                    };
                    let left_x = if n == 0 {
                        style::DOC_MARGIN_X
                    } else {
                        actor_x[lo] - actor_head_w[lo] / 2.0 - style::GROUP_LEFT_PAD
                    };
                    let right_x = if n == 0 {
                        style::DOC_MARGIN_X + style::COLUMN_MIN_WIDTH
                    } else {
                        actor_x[hi] + actor_head_w[hi] / 2.0 + style::GROUP_RIGHT_PAD
                    };
                    if let EventLayout::Group {
                        start_y,
                        end_y: ge,
                        left_x: lx,
                        right_x: rx,
                        ..
                    } = &mut event_layout[g.layout_idx]
                    {
                        *start_y = g.start_y;
                        *ge = end_y;
                        *lx = left_x;
                        *rx = right_x;
                    }
                    y = end_y;
                }
            }
        }
    }

    // Any activations still open: close them at `y`.
    for stack in &active_stacks {
        for idx in stack {
            if let EventLayout::Activation { end_y, .. } = &mut event_layout[*idx] {
                *end_y = y;
            }
        }
    }

    let footer_y = y + style::LAST_ROW_TO_FOOTER;
    let total_height =
        footer_y + style::HEADER_BOX_HEIGHT + style::DOC_MARGIN_BOTTOM;

    LaidOutDiagram {
        ir,
        width: total_width,
        height: total_height,
        actor_x,
        actor_head_w,
        first_row_y,
        footer_y,
        event_layout,
    }
}

fn note_extent(
    placement: &NotePlacement,
    actor_x: &[f32],
    actor_head_w: &[f32],
    text: &str,
) -> (f32, f32) {
    match placement {
        NotePlacement::Over { actors } if !actors.is_empty() => {
            let lo = *actors.iter().min().unwrap();
            let hi = *actors.iter().max().unwrap();
            let left = actor_x[lo] - actor_head_w[lo] / 2.0;
            let right = actor_x[hi] + actor_head_w[hi] / 2.0;
            let w = (right - left).max(measure_text(text, style::NOTE_FONT_SIZE) + 2.0 * style::NOTE_PAD_X);
            (left, w)
        }
        NotePlacement::LeftOf { actor } => {
            let right = actor_x[*actor] - actor_head_w[*actor] / 2.0 - style::NOTE_LEFT_RIGHT_OFFSET;
            let w = style::NOTE_LEFT_RIGHT_WIDTH
                .max(measure_text(text, style::NOTE_FONT_SIZE) + 2.0 * style::NOTE_PAD_X);
            (right - w, w)
        }
        NotePlacement::RightOf { actor } => {
            let left = actor_x[*actor] + actor_head_w[*actor] / 2.0 + style::NOTE_LEFT_RIGHT_OFFSET;
            let w = style::NOTE_LEFT_RIGHT_WIDTH
                .max(measure_text(text, style::NOTE_FONT_SIZE) + 2.0 * style::NOTE_PAD_X);
            (left, w)
        }
        // Note Over with empty actor list shouldn't be reachable —
        // parser rejects it. Defensive fallback.
        NotePlacement::Over { .. } => (0.0, style::NOTE_LEFT_RIGHT_WIDTH),
    }
}

// ---------------------------------------------------------------------------
// SVG emission
// ---------------------------------------------------------------------------

pub fn emit(diag: &LaidOutDiagram<'_>) -> Vec<u8> {
    let mut svg = SvgBuf::new(diag.width, diag.height);

    // Background.
    svg.rect(
        0.0,
        0.0,
        diag.width,
        diag.height,
        &Attrs::new().set("fill", style::COLOR_BG),
    );

    // Lifelines (drawn before everything else so activations and
    // messages overlay them).
    let lifeline_top = style::DOC_MARGIN_TOP + style::HEADER_BOX_HEIGHT;
    let lifeline_bottom = diag.footer_y;
    for &x in &diag.actor_x {
        svg.line(
            x,
            lifeline_top,
            x,
            lifeline_bottom,
            &Attrs::new()
                .set("stroke", style::COLOR_LIFELINE)
                .set("stroke-width", "1")
                .set("stroke-dasharray", "4,4"),
        );
    }

    // Groups first (they form the visual frame; messages overlay).
    for ev in &diag.event_layout {
        if let EventLayout::Group {
            kind,
            labels,
            start_y,
            end_y,
            left_x,
            right_x,
        } = ev
        {
            // Outer rectangle (open at top so the label tab tucks in).
            svg.rect(
                *left_x,
                *start_y,
                right_x - left_x,
                end_y - start_y,
                &Attrs::new()
                    .set("fill", "none")
                    .set("stroke", style::COLOR_GROUP_STROKE)
                    .set("stroke-width", "1.5"),
            );
            // Label tab.
            let kind_name = match kind {
                GroupKind::Loop => "loop",
                GroupKind::Alt => "alt",
            };
            svg.rect(
                *left_x,
                *start_y,
                style::GROUP_LABEL_TAB_W,
                style::GROUP_LABEL_TAB_H,
                &Attrs::new()
                    .set("fill", style::COLOR_GROUP_LABEL_FILL)
                    .set("stroke", style::COLOR_GROUP_STROKE)
                    .set("stroke-width", "1"),
            );
            svg.text(
                *left_x + 8.0,
                *start_y + style::GROUP_LABEL_TAB_H - 5.0,
                kind_name,
                &Attrs::new()
                    .set("fill", style::COLOR_GROUP_LABEL_TEXT)
                    .set_f("font-size", style::GROUP_FONT_SIZE)
                    .set("font-family", "sans-serif"),
            );
            // Per-segment labels (loop: one; alt: one + each `else`).
            for (seg_y, label) in labels {
                if !label.is_empty() {
                    svg.text(
                        *left_x + style::GROUP_LABEL_TAB_W + 8.0,
                        *seg_y + style::GROUP_LABEL_TAB_H - 5.0,
                        &format!("[{}]", label),
                        &Attrs::new()
                            .set("fill", style::COLOR_GROUP_STROKE)
                            .set_f("font-size", style::GROUP_FONT_SIZE)
                            .set("font-family", "sans-serif"),
                    );
                }
            }
        }
    }

    // Activation bars.
    for ev in &diag.event_layout {
        if let EventLayout::Activation {
            actor,
            start_y,
            end_y,
            depth,
        } = ev
        {
            let cx = diag.actor_x[*actor]
                + (*depth as f32) * (style::ACTIVATION_WIDTH * 0.4);
            svg.rect(
                cx - style::ACTIVATION_WIDTH / 2.0,
                *start_y,
                style::ACTIVATION_WIDTH,
                (end_y - start_y).max(8.0),
                &Attrs::new()
                    .set("fill", style::COLOR_ACTIVATION_FILL)
                    .set("stroke", style::COLOR_ACTIVATION_STROKE)
                    .set("stroke-width", "1"),
            );
        }
    }

    // Header / footer actor heads.
    for (i, actor) in diag.ir.actors.iter().enumerate() {
        let cx = diag.actor_x[i];
        let w = diag.actor_head_w[i];
        for &y_top in &[style::DOC_MARGIN_TOP, diag.footer_y] {
            match actor.kind {
                ActorKind::Participant => {
                    svg.rect(
                        cx - w / 2.0,
                        y_top,
                        w,
                        style::HEADER_BOX_HEIGHT,
                        &Attrs::new()
                            .set("fill", style::COLOR_ACTOR_FILL)
                            .set("stroke", style::COLOR_ACTOR_STROKE)
                            .set("stroke-width", "1"),
                    );
                }
                ActorKind::Actor => {
                    // Stick-figure approximation: a circle for the
                    // head plus a label box. We use the head box for
                    // the label and add a small circle above it.
                    svg.rect(
                        cx - w / 2.0,
                        y_top,
                        w,
                        style::HEADER_BOX_HEIGHT,
                        &Attrs::new()
                            .set("fill", style::COLOR_ACTOR_FILL)
                            .set("stroke", style::COLOR_ACTOR_STROKE)
                            .set("stroke-width", "1"),
                    );
                    svg.circle(
                        cx,
                        y_top - 6.0,
                        5.0,
                        &Attrs::new()
                            .set("fill", style::COLOR_ACTOR_FILL)
                            .set("stroke", style::COLOR_ACTOR_STROKE)
                            .set("stroke-width", "1"),
                    );
                }
            }
            svg.text(
                cx,
                y_top + style::HEADER_BOX_HEIGHT / 2.0 + style::HEADER_FONT_SIZE / 3.0,
                &actor.label,
                &Attrs::new()
                    .set("fill", style::COLOR_TEXT)
                    .set_f("font-size", style::HEADER_FONT_SIZE)
                    .set("text-anchor", "middle")
                    .set("font-family", "sans-serif"),
            );
        }
    }

    // Notes.
    for ev in &diag.event_layout {
        if let EventLayout::Note { y, x, w, h, text } = ev {
            svg.rect(
                *x,
                *y,
                *w,
                *h,
                &Attrs::new()
                    .set("fill", style::COLOR_NOTE_FILL)
                    .set("stroke", style::COLOR_NOTE_STROKE)
                    .set("stroke-width", "1"),
            );
            svg.text(
                *x + *w / 2.0,
                *y + *h / 2.0 + style::NOTE_FONT_SIZE / 3.0,
                text,
                &Attrs::new()
                    .set("fill", style::COLOR_TEXT)
                    .set_f("font-size", style::NOTE_FONT_SIZE)
                    .set("text-anchor", "middle")
                    .set("font-family", "sans-serif"),
            );
        }
    }

    // Messages (drawn last so arrowheads sit on top of activations).
    for ev in &diag.event_layout {
        if let EventLayout::Message {
            y,
            from,
            to,
            kind,
            text,
        } = ev
        {
            draw_message(&mut svg, *y, *from, *to, *kind, text, diag);
        }
    }

    svg.finish()
}

fn draw_message(
    svg: &mut SvgBuf,
    y: f32,
    from: ActorIdx,
    to: ActorIdx,
    kind: MessageKind,
    text: &str,
    diag: &LaidOutDiagram<'_>,
) {
    let line_y = y + style::ROW_HEIGHT - 12.0;
    let dotted = matches!(
        kind,
        MessageKind::AsyncDottedArrow | MessageKind::DottedNoArrow
    );
    let arrow_head = matches!(
        kind,
        MessageKind::AsyncDottedArrow | MessageKind::SyncSolidArrow
    );

    if from == to {
        // Self-message: a small bracket on the right of the lifeline.
        let x = diag.actor_x[from];
        let off = 30.0;
        let top_y = line_y - 12.0;
        let mid_y = line_y;
        let bot_y = line_y + 12.0;
        let mut attrs = Attrs::new()
            .set("stroke", style::COLOR_MSG_STROKE)
            .set("fill", "none")
            .set("stroke-width", "1");
        if dotted {
            attrs = attrs.set("stroke-dasharray", "4,3");
        }
        // Three line segments forming a hook: → out → down → back.
        svg.line(x, top_y, x + off, top_y, &attrs);
        svg.line(x + off, top_y, x + off, bot_y, &attrs);
        svg.line(x + off, bot_y, x, mid_y, &attrs);
        if arrow_head {
            draw_arrow_head(svg, x + 8.0, mid_y, -1.0);
        }
        // Label to the right of the hook.
        svg.text(
            x + off + 6.0,
            mid_y - 4.0,
            text,
            &Attrs::new()
                .set("fill", style::COLOR_TEXT)
                .set_f("font-size", style::TEXT_FONT_SIZE)
                .set("font-family", "sans-serif"),
        );
    } else {
        let x_from = diag.actor_x[from];
        let x_to = diag.actor_x[to];
        let dir = if x_to > x_from { 1.0 } else { -1.0 };
        // Pull endpoints in slightly so arrowhead rests against the
        // lifeline rather than overlapping it.
        let end_offset = 2.0 * dir;
        let mut attrs = Attrs::new()
            .set("stroke", style::COLOR_MSG_STROKE)
            .set("stroke-width", "1");
        if dotted {
            attrs = attrs.set("stroke-dasharray", "4,3");
        }
        svg.line(x_from, line_y, x_to - end_offset, line_y, &attrs);
        if arrow_head {
            draw_arrow_head(svg, x_to - end_offset, line_y, dir);
        }
        // Label centered on the message line.
        let mid = (x_from + x_to) / 2.0;
        svg.text(
            mid,
            line_y - 6.0,
            text,
            &Attrs::new()
                .set("fill", style::COLOR_TEXT)
                .set_f("font-size", style::TEXT_FONT_SIZE)
                .set("text-anchor", "middle")
                .set("font-family", "sans-serif"),
        );
    }
}

fn draw_arrow_head(svg: &mut SvgBuf, tip_x: f32, tip_y: f32, dir: f32) {
    let len = 8.0;
    let half = 4.0;
    let base_x = tip_x - len * dir;
    let p = vec![(tip_x, tip_y), (base_x, tip_y - half), (base_x, tip_y + half)];
    svg.polygon(
        &p,
        &Attrs::new()
            .set("fill", style::COLOR_MSG_STROKE)
            .set("stroke", style::COLOR_MSG_STROKE)
            .set("stroke-width", "1"),
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> SequenceIr {
        parse(s).unwrap_or_else(|e| panic!("parse failed: {e}\nINPUT:\n{s}"))
    }

    #[test]
    fn parses_minimal_diagram() {
        let ir = parse_str("sequenceDiagram\nAlice->>Bob: hi\n");
        assert_eq!(ir.actors.len(), 2);
        assert_eq!(ir.actors[0].id, "Alice");
        assert_eq!(ir.actors[1].id, "Bob");
        assert_eq!(ir.events.len(), 1);
        match &ir.events[0] {
            Event::Message {
                from, to, kind, text, ..
            } => {
                assert_eq!(*from, 0);
                assert_eq!(*to, 1);
                assert_eq!(*kind, MessageKind::SyncSolidArrow);
                assert_eq!(text, "hi");
            }
            other => panic!("expected message, got {other:?}"),
        }
    }

    #[test]
    fn parses_explicit_participants_with_aliases() {
        let ir = parse_str(
            "sequenceDiagram\nparticipant A as Alice\nparticipant B as Bob\nA->>B: hi\n",
        );
        assert_eq!(ir.actors[0].id, "A");
        assert_eq!(ir.actors[0].label, "Alice");
        assert_eq!(ir.actors[1].id, "B");
        assert_eq!(ir.actors[1].label, "Bob");
    }

    #[test]
    fn parses_actor_keyword() {
        let ir = parse_str("sequenceDiagram\nactor U\nU->>S: req\n");
        assert_eq!(ir.actors[0].kind, ActorKind::Actor);
    }

    #[test]
    fn parses_all_supported_arrow_kinds() {
        let ir = parse_str(
            "sequenceDiagram\nA->B: solid\nA->>B: sync\nA-->B: dotted\nA-->>B: async\n",
        );
        let kinds: Vec<_> = ir
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Message { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                MessageKind::SolidNoArrow,
                MessageKind::SyncSolidArrow,
                MessageKind::DottedNoArrow,
                MessageKind::AsyncDottedArrow,
            ]
        );
    }

    #[test]
    fn rejects_unsupported_arrow_x() {
        let err = parse("sequenceDiagram\nA-xB: bye\n").unwrap_err();
        match err {
            MermaidError::ParseError { reason: feature, .. } => {
                assert!(feature.contains("-x"));
            }
            other => panic!("expected UnsupportedFeature, got {other:?}"),
        }
    }

    #[test]
    fn parses_self_message() {
        let ir = parse_str("sequenceDiagram\nA->>A: ping\n");
        match &ir.events[0] {
            Event::Message { from, to, .. } => assert_eq!(from, to),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_activate_deactivate_explicit() {
        let ir = parse_str(
            "sequenceDiagram\nA->>B: req\nactivate B\nB-->>A: resp\ndeactivate B\n",
        );
        let active_count = ir
            .events
            .iter()
            .filter(|e| matches!(e, Event::Activate(_)))
            .count();
        let deactive_count = ir
            .events
            .iter()
            .filter(|e| matches!(e, Event::Deactivate(_)))
            .count();
        assert_eq!(active_count, 1);
        assert_eq!(deactive_count, 1);
    }

    #[test]
    fn parses_activation_shorthand() {
        let ir = parse_str("sequenceDiagram\nA->>+B: open\nB-->>-A: close\n");
        match &ir.events[0] {
            Event::Message {
                activate_target, ..
            } => assert!(*activate_target),
            other => panic!("got {other:?}"),
        }
        match &ir.events[1] {
            Event::Message {
                deactivate_source, ..
            } => assert!(*deactivate_source),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_note_over_one_actor() {
        let ir = parse_str("sequenceDiagram\nA->>B: hi\nNote over A: hello\n");
        match &ir.events[1] {
            Event::Note { placement, text } => {
                assert_eq!(text, "hello");
                match placement {
                    NotePlacement::Over { actors } => assert_eq!(actors, &vec![0]),
                    other => panic!("got {other:?}"),
                }
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn parses_note_over_two_actors() {
        let ir = parse_str("sequenceDiagram\nA->>B: hi\nNote over A,B: shared\n");
        match &ir.events[1] {
            Event::Note { placement, .. } => match placement {
                NotePlacement::Over { actors } => assert_eq!(actors, &vec![0, 1]),
                other => panic!("got {other:?}"),
            },
            _ => panic!("expected note"),
        }
    }

    #[test]
    fn parses_note_left_of_and_right_of() {
        let ir = parse_str(
            "sequenceDiagram\nA->>B: hi\nNote left of A: l\nNote right of B: r\n",
        );
        match &ir.events[1] {
            Event::Note { placement, .. } => {
                assert!(matches!(placement, NotePlacement::LeftOf { actor: 0 }))
            }
            _ => panic!(),
        }
        match &ir.events[2] {
            Event::Note { placement, .. } => {
                assert!(matches!(placement, NotePlacement::RightOf { actor: 1 }))
            }
            _ => panic!(),
        }
    }

    #[test]
    fn parses_loop_block() {
        let ir = parse_str(
            "sequenceDiagram\nA->>B: hi\nloop every minute\nA->>B: ping\nend\n",
        );
        let group_starts = ir
            .events
            .iter()
            .filter(|e| matches!(e, Event::GroupStart { .. }))
            .count();
        let group_ends = ir
            .events
            .iter()
            .filter(|e| matches!(e, Event::GroupEnd))
            .count();
        assert_eq!(group_starts, 1);
        assert_eq!(group_ends, 1);
    }

    #[test]
    fn parses_alt_with_else_block() {
        let ir = parse_str(
            "sequenceDiagram\nalt happy path\nA->>B: ok\nelse fail\nA->>B: err\nend\n",
        );
        let elses = ir
            .events
            .iter()
            .filter(|e| matches!(e, Event::GroupElse { .. }))
            .count();
        assert_eq!(elses, 1);
    }

    #[test]
    fn rejects_par_block() {
        let err = parse("sequenceDiagram\npar branch one\nA->>B: hi\nend\n").unwrap_err();
        match err {
            MermaidError::ParseError { reason, .. } => {
                assert!(reason.contains("par"), "got {reason:?}");
                assert!(reason.contains("unsupported"), "got {reason:?}");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn rejects_autonumber() {
        let err = parse("sequenceDiagram\nautonumber\nA->>B: hi\n").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { ref reason, .. } if reason.contains("unsupported")));
    }

    #[test]
    fn rejects_rect_block() {
        let err =
            parse("sequenceDiagram\nrect rgb(0,0,0)\nA->>B: hi\nend\n").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { ref reason, .. } if reason.contains("unsupported")));
    }

    #[test]
    fn rejects_nested_groups_beyond_one_level() {
        let err = parse(
            "sequenceDiagram\nloop outer\nloop inner\nA->>B: hi\nend\nend\n",
        )
        .unwrap_err();
        match err {
            MermaidError::ParseError { reason: feature, .. } => {
                assert!(feature.contains("nesting depth"));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn accepts_one_level_of_nesting_actually_no() {
        // Per W-4c3f16: v1 supports at most one level of nesting.
        // "One level deep" means a single block, not a block-in-a-block.
        // So a single loop is fine; a loop containing a loop fails.
        let ir = parse_str("sequenceDiagram\nloop tick\nA->>B: hi\nend\n");
        assert!(ir
            .events
            .iter()
            .any(|e| matches!(e, Event::GroupStart { .. })));
    }

    #[test]
    fn rejects_unmatched_end() {
        let err = parse("sequenceDiagram\nA->>B: hi\nend\n").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn rejects_unclosed_loop() {
        let err = parse("sequenceDiagram\nloop forever\nA->>B: hi\n").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn rejects_message_without_colon() {
        let err = parse("sequenceDiagram\nA->>B no colon here\n").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn rejects_missing_header() {
        let err = parse("A->>B: hi\n").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn ignores_blank_lines_and_comments() {
        let ir = parse_str(
            "sequenceDiagram\n\n%% a comment\n  \nA->>B: hi\n%% trailing\n",
        );
        assert_eq!(ir.events.len(), 1);
    }

    #[test]
    fn limit_too_many_actors() {
        let mut src = String::from("sequenceDiagram\n");
        for i in 0..(limits::MAX_ACTORS + 1) {
            src.push_str(&format!("participant A{i}\n"));
        }
        let err = parse(&src).unwrap_err();
        assert!(matches!(err, MermaidError::InputTooLarge { cap: "actors", .. }));
    }

    #[test]
    fn limit_too_many_events() {
        let mut src = String::from("sequenceDiagram\n");
        for _ in 0..(limits::MAX_EVENTS + 1) {
            src.push_str("A->>B: x\n");
        }
        let err = parse(&src).unwrap_err();
        assert!(matches!(err, MermaidError::InputTooLarge { cap: "events", .. }));
    }

    #[test]
    fn limit_oversize_label() {
        let big = "x".repeat(limits::MAX_LABEL_BYTES + 1);
        let src = format!("sequenceDiagram\nA->>B: {big}\n");
        let err = parse(&src).unwrap_err();
        assert!(matches!(err, MermaidError::InputTooLarge { .. }));
    }

    #[test]
    fn limit_oversize_line() {
        let big = "A->>B: ".to_string() + &"y".repeat(limits::MAX_LINE_BYTES);
        let src = format!("sequenceDiagram\n{big}\n");
        let err = parse(&src).unwrap_err();
        match err {
            MermaidError::InputTooLarge { cap, .. } => assert_eq!(cap, "line bytes"),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn xml_injection_attempt_in_label_is_neutralised() {
        let svg = render(
            "sequenceDiagram\nA->>B: </text><script>alert(1)</script>\n",
        )
        .unwrap();
        let s = String::from_utf8(svg).unwrap();
        assert!(!s.contains("<script"));
        assert!(s.contains("&lt;script&gt;"));
    }

    #[test]
    fn renders_minimal_diagram_to_well_formed_svg() {
        let bytes = render("sequenceDiagram\nAlice->>Bob: hello\n").unwrap();
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("<svg"));
        assert!(s.ends_with("</svg>"));
        assert!(s.contains("Alice"));
        assert!(s.contains("Bob"));
        assert!(s.contains("hello"));
    }

    #[test]
    fn renders_full_feature_diagram_without_panic() {
        let src = "sequenceDiagram
participant A as Alice
actor B
A->>+B: open
B-->>-A: close
Note over A,B: shared note
loop every minute
  A->>B: poll
end
alt success
  A->>B: ack
else failure
  A->>B: nack
end
";
        let bytes = render(src).unwrap();
        assert!(bytes.starts_with(b"<svg"));
        assert!(bytes.ends_with(b"</svg>"));
    }

    #[test]
    fn deterministic_output_across_runs() {
        let src = "sequenceDiagram\nA->>B: hi\nB-->>A: ack\n";
        let a = render(src).unwrap();
        let b = render(src).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn hostile_input_shapes_do_not_panic() {
        // A small fuzz battery — we don't pull a fuzzer dep, just hit
        // a representative set of pathological shapes.
        let cases: &[&str] = &[
            "",
            "\n\n\n",
            "%%%%%%%",
            "sequenceDiagram",
            "sequenceDiagram\n",
            "sequenceDiagram\n\n",
            "sequenceDiagram\n->>:\n",
            "sequenceDiagram\nA->>:\n",
            "sequenceDiagram\nA->> B\n",
            "sequenceDiagram\nA->>B:\n",
            "sequenceDiagram\nNote\n",
            "sequenceDiagram\nNote over\n",
            "sequenceDiagram\nNote over: text\n",
            "sequenceDiagram\nNote left of: text\n",
            "sequenceDiagram\nactivate\n",
            "sequenceDiagram\ndeactivate\n",
            "sequenceDiagram\nloop\nA->>B:\n",
            "sequenceDiagram\nelse hi\n",
            "sequenceDiagram\nend\n",
            "sequenceDiagram\nparticipant\n",
            "sequenceDiagram\nparticipant A as\n",
            "sequenceDiagram\nA-xB: bye\n",
            "sequenceDiagram\nA-->>B-->>C: chained\n",
            // High-codepoint unicode in labels.
            "sequenceDiagram\nA->>B: 日本語のテスト 🚀\n",
            // Lots of whitespace.
            "sequenceDiagram\n   A   ->>   B   :   spaced  \n",
        ];
        for c in cases {
            // We don't care whether each case parses; we care that
            // each case's render() returns Ok or Err — never panics
            // and never hangs.
            let _ = std::panic::catch_unwind(|| {
                let _ = render(c);
            })
            .map_err(|_| panic!("render panicked on input: {c:?}"));
        }
    }
}
