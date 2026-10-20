# ADR-0005: Text box to Loro text binding

- **Status:** proposed
- **Date:** 2026-10-20
- **Spike:** S5
- **Owner:** Core

## Question

Can a native text box drive a Loro text container without losing or doubling
text, including emoji, CJK input and remote updates?

## Context

Loro's `LoroText` indexes are Unicode scalar positions. A browser or native
text box reports the entire new value after an input event and uses UTF-16
positions in JavaScript, so the binding must not confuse the two.

## Decision

A text box value is converted to exactly one replacement: the common prefix and
suffix are found by Unicode scalar, and only the differing middle becomes a
`delete` followed by an `insert`. This is O(text length), preserves the
surrounding operations, and does not run a Myers diff on every keystroke.

`NoteDocument::apply_edit` returns the computed `TextReplacement` so the UI can
log or measure it.

## Evidence

- `tests/text_binding.rs` applies 1,000 randomized whole-value edits to a buffer
  containing emoji, CJK and Markdown, asserting the Loro text equals the buffer
  after every edit.
- Targeted cases: typing at the end inserts one character; inserting a word in
  the middle keeps the suffix; replacing a selection removes only that range;
  an unchanged value is a no-op; emoji positions are scalar positions.
- Remote cursor stability: a cursor at the end of `hello` resolves to position 8
  after a remote replica inserts `oh ` at the start, so it still sits after the
  same character.
- `NoteDocument::from_snapshot` and update import preserve the converged text.

Not covered yet: Gboard, Windows IME composition events, and browser selection
restoration. The UI must ignore input events between `compositionstart` and
`compositionend`, then call `apply_edit` once with the final value. That guard
and a real-device test belong to Phase 4; this machine has no display or Android
device.

## Consequences

- The S5 pass condition is partially met: convergence and cursor stability are
  proven at the document layer; IME and Android keyboard behavior are not.
- The UI must use Unicode-safe positions and must not send intermediate
  composition values to Loro.
- No fallback is needed yet. If the composition guard fails on a device, fall
  back to sending edits on blur, as PLAN.md allows.
