# no-op-io

## Smell

A `#[test]` body performs work whose `Result` is discarded — `let _ =
some_call();` — and never inspects the outcome. The test always passes
(no panic is raised), but it conveys nothing about whether the work
succeeded: a regression that makes the call fail would leave this test
green.

The detector emits a finding when ALL of these hold:

1. `cfg.enabled != Some(false)` — the detector is enabled.
2. The body carries at least one `BehavioralFact::ResultDiscarded`
   fact — a bare-wildcard `let _ = <expr>;` whose initializer is a
   `Result`-shaped call (`ResultDiscardKind::{Call, ResultCtor,
   ResultAdapter}`). A type-ascribed `let _: T = ...;` is an
   intentional must-use silencer and does NOT project.
3. `detectors::has_positive_check(parsed)` is `false` — no explicit
   assertion, no implicit-assertion source, and no `.unwrap()` /
   `.expect(...)` (`BehavioralFact::ResultAsserted`) chain.

When all hold, the detector emits one `SmellCategory::NoOpIo` finding
at `Severity::Moderate` with `Actionability::AutoRefactor` and penalty
8.

### v0.1 over-fire note

`ResultDiscardKind::Call` matches ANY discarded call, not just I/O, so
`no-op-io` is broader than its name in v0.1 (`let _ = pure_fn();`
projects too). This is tolerable: it only fires when there is zero
positive-check evidence, so it never falsely accuses a test that
actually asserts. An I/O-narrowing refinement is a v0.3+ follow-up.

### Stacking with zero-assertion

`no-op-io` is a strict subset of `zero-assertion`: an all-discard body
also has no assertions, so BOTH detectors fire. In v0.1 their penalties
**stack** into one `Finding` (`scrap_score == 18.0` — 10 + 8). Whether
the more-specific smell should supersede the general one is a
scoring-layer policy deferred to the scrap-rs#32 `score_example`
aggregator. The `bad.rs` envelope below captures the stacked shape.

## Fix

Inspect the discarded `Result` instead of dropping it. The `good.rs`
example replaces `let _ = std::fs::write(...);` with
`std::fs::write(...).expect("write should succeed")` — a bare
expression statement (not a `let _` binding), so no
`ResultDiscarded` is projected, and the `.expect(...)` chain projects
`ResultAsserted`, which disarms both detectors. No finding emits.

Equivalent fixes:

- Bind the result and assert on it (`let r = ...; assert!(r.is_ok());`).
- `.unwrap()` / `.expect(...)` the call (the panic IS the check).
- For a `let _ =` you truly want to keep, ascribe the unit type
  (`let _: () = ...;`) — recognised as an intentional must-use
  silencer, not a discard.

## Wire shape

See `expected.json` for the canonical envelope emitted against
`bad.rs`. The relevant fields:

- `result.files[0].findings[0].smells` contains BOTH a `no_op_io`
  smell (`penalty == 8`, `severity == "moderate"`) and a
  `zero_assertion` smell (`penalty == 10`, `severity == "high"`).
- `scrap_score == 18.0` (the stacked sum).
- `span` (under `test` and each `smells[*]`) covers the whole test
  function from signature to closing brace.

The full v0.1 envelope shape is documented at
[`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md).
