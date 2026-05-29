# tautological-assertion

## Smell

A `#[test]` body asserts, but the assertions are shaped so they can
never fail — so they carry no information about the system under test:

- `assert!(true)` — a single-arg constant-true literal.
- `assert_eq!(x, x)` / `assert_ne!(x, x)` — token-identical two-argument
  shape.
- Literal-vs-literal compare (`assert_eq!(1, 1)`) — same
  token-identical mechanism.

`assert!(false)` is deliberately NOT flagged — Uncle Bob's convention
([`unclebob/scrap`](https://github.com/unclebob/scrap)) treats
deliberate-failure assertions as informational, not smell.

The detector emits a finding when, for one or more assertions on the
test, the parser-supplied facts indicate tautology:

- `arguments_identical == true` (the `assert_eq!(x, x)` shape), OR
- `single_arg_value == Some(LiteralValue::Bool(true))` (the
  `assert!(true)` shape).

Each offending assertion produces one `SmellCategory::TautologicalAssertion`
smell at `Severity::High` with `Actionability::AutoRefactor` and penalty
10. N tautological assertions on one test produce ONE `Finding` with N
`Smell`s; `Finding::scrap_score` aggregates (10 × N). Each smell carries
the offending assertion's own `span` (per-assertion line attribution),
which is narrower than the whole-test `test.span`.

### No co-fire with zero-assertion / no-op-io

A tautological assertion is still a *recorded* assertion, so
`parsed.assertions` is non-empty and
`detectors::has_positive_check(parsed)` is `true`. Both `zero-assertion`
and `no-op-io` suppress on that predicate, so a tautology body trips
ONLY `tautological-assertion` — no stacked co-fire. (Contrast the
`no_op_io` fixture, whose all-discard body has NO assertions and so
co-fires `zero-assertion`.)

## Fix

Replace the tautology with an assertion that observes a real value the
test computed, so the assertion can actually fail on a regression. The
`good.rs` example replaces `assert!(true)` / `assert_eq!(1, 1)` with
`assert_eq!(result, 4)` (runtime value vs. expected constant — not a
token-identical pair) and `assert!(result > 0)` (a non-constant
predicate). Neither trips the tautology rule, and the presence of real
assertions also keeps `zero-assertion` silent. No finding emits.

## Wire shape

See `expected.json` for the canonical envelope emitted against
`bad.rs`. The relevant fields:

- `result.files[0].findings[0].smells` contains TWO
  `tautological_assertion` smells (`penalty == 10`, `severity ==
  "high"`), one per offending assertion.
- `scrap_score == 20.0` (the stacked sum, 2 × 10).
- Each `smells[*].span` covers the offending assertion's line; the
  enclosing `test.span` covers the whole test function from signature
  to closing brace.

The full v0.1 envelope shape is documented at
[`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md).
