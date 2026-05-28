# zero-assertion

## Smell

A `#[test]` body invokes the system under test but never asserts
anything about the result. The test always passes — by virtue of
not raising a panic — but conveys no information about whether the
code under test behaves correctly. A regression that changes the
result of `1 + 1` would still leave this test green.

The detector recognises three clauses:

1. `parsed.assertions.is_empty()` — no explicit assertion macro
   (`assert!`/`assert_eq!`/`assert_ne!`/`assert_matches!`/`panic!`/
   `unimplemented!`/`todo!`).
2. `parsed.implicit_assertion_sources.is_empty()` — no runner shell
   (`proptest`/`quickcheck`/`kani`/`cucumber`/`trybuild`/`insta`/
   `pretty_assertions`) and no `#[should_panic]` attribute.
3. `!parsed.behavioral_facts.contains(&BehavioralFact::ResultAsserted)`
   — no `.unwrap()` / `.expect(...)` method-call chain (the
   explicit-panic-is-the-assertion pattern).

When all three clauses hold, the detector emits one
`SmellCategory::ZeroAssertion` finding at `Severity::High` with
`Actionability::AutoRefactor` and penalty 10.

## Fix

Add an assertion that observes the system-under-test's effect. The
`good.rs` example computes `1 + 1` and asserts the result equals
`2`. Clause 1 fails (`assertions` is non-empty), so the detector
returns `None` and emits no finding.

Equivalent fixes that also disarm the detector:

- `assert_ne!`, `assert_matches!`, `panic!("...")`, or any other
  assertion macro recognised by `assertion_sources.rs`.
- `let value: i32 = 1 + 1; let _: () = value.expect("...")` — wait,
  that doesn't typecheck; substitute any expression that produces a
  `Result` / `Option` and call `.unwrap()` / `.expect(...)`. The
  parser's body walker recognises method-call chains as a
  `BehavioralFact::ResultAsserted`; clause 3 then fires.
- `#[should_panic]` attribute on the test function combined with a
  body that panics. Clause 2 fails because the attribute populates
  `implicit_assertion_sources` with `AssertionSource::ShouldPanic`.

## Wire shape

See `expected.json` for the canonical envelope emitted by the
zero-assertion detector against `bad.rs`. The relevant fields:

- `result.files[0].findings[0].smells[0].category == "zero_assertion"`
- `severity == "high"`, `actionability == "auto_refactor"`,
  `penalty == 10`
- `span` (under both `test` and `smells[0]`) covers the whole test
  function from signature to closing brace
- `scrap_score == 10.0` (single smell with penalty 10)

The full v0.1 envelope shape is documented at
[`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md).
