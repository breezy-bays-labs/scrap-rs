# large-example

## Smell

A `#[test]` body grows past a configured line threshold — it stitches
unrelated setup, many sequential operations, and a scattering of
assertions into one sprawling example. A test that long is hard to
read as a single idea, hard to name accurately, and tends to fail for
several unrelated reasons at once.

The detector emits a finding when BOTH of these hold:

1. `cfg.enabled != Some(false)` — the detector is enabled.
2. `parsed.body_line_count > cfg.line_threshold.unwrap_or(30)` — the
   body block interior is strictly longer than the threshold. A body
   exactly AT the threshold is NOT flagged (`>`, not `>=`); a zero-line
   body never fires.

When both hold, the detector emits one `SmellCategory::LargeExample`
finding at `Severity::Low` with `Actionability::ManualSplit` and
penalty 4.

### Default threshold — 30, Rust-tuned

The default of 30 lines is tuned higher than Uncle Bob's Clojure
`scrap` default of 20: Rust test bodies are syntactically more verbose
(type annotations, explicit `let` bindings, builder calls, closing
braces), so the same conceptual test reads as more physical lines. The
knob is per-project tunable:

```toml
[detectors.large_example]
line_threshold = 40
```

### Orthogonal to the assertion-based smells

`large-example` is purely structural — it reads only `body_line_count`,
never the assertion / behavioral-fact bag. It therefore neither
suppresses nor is suppressed by `zero-assertion` / `no-op-io` /
`tautological-assertion`, and can co-fire with all of them: a long body
that ALSO fails to assert stacks `large-example` (4) + `zero-assertion`
(10) into one `Finding`. This fixture's `bad.rs` deliberately asserts
on DISTINCT operands and discards no `Result`, so it isolates the smell
— the only entry in `smells` is `large_example` and `scrap_score` is
exactly `4.0`.

## Fix

Split the oversized test into focused examples (one observable behavior
per test), or extract the shared setup into a helper. The `good.rs`
example keeps a single focused test with a small body well under the
threshold and one real `assert_eq!` on distinct operands — so
`large-example` does not fire, and neither does any other v0.1
detector.

## Wire shape

See `expected.json` for the canonical envelope emitted against
`bad.rs`. The relevant fields:

- `result.files[0].findings[0].smells` contains exactly ONE
  `large_example` smell (`penalty == 4`, `severity == "low"`,
  `actionability == "manual_split"`).
- `scrap_score == 4.0`.
- `span` (under `test` and the `smells[0]`) covers the whole test
  function from signature to closing brace.

The full v0.1 envelope shape is documented at
[`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md).
