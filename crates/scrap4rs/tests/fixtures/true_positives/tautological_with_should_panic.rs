// Fixture: #[should_panic] fn with a tautological assert!(true).
//
// Per Christopher's SHAPE-Q1=(ii) lock at the pipeline shape gate,
// the v0.1 tautological-assertion detector EMITS for this fixture —
// the pipeline driver (scrap-rs#72) is responsible for handling the
// user's intent via `[opt_outs]` config or `#[allow(scrap::tautology)]`
// on the test. The detector contract is "emit unconditionally when
// the facts indicate tautology"; policy lives in the pipeline layer.
//
// Expected parsed shape:
//   assertions: [ParsedAssertion {
//     name: "assert",
//     single_arg_value: Some(LiteralValue::Bool(true)),
//     arguments_identical: false,
//     ...
//   }]
//   implicit_assertion_sources: [ShouldPanic]
// Detector verdict: TautologicalAssertion smell, penalty 10.
// Policy at scrap-rs#72: may suppress or demote based on
// implicit_assertion_sources or per-test #[allow(scrap::tautology)].

#[test]
#[should_panic]
fn assert_true_with_should_panic_is_still_tautological() {
    assert!(true);
}
