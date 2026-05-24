// Fixture: a #[test] fn using `pretty_assertions::assert_eq`.
//
// The dual-recognition case: the parser must surface BOTH a
// `ParsedAssertion("assert_eq")` (the leaf-segment match in the
// v0.1 explicit set) AND `implicit_assertion_sources:
// [PrettyAssertions]` (the recognise() prefix match). Both facts are
// load-bearing — the explicit-assertion count rules out the
// zero-assertion detector independently; the implicit-source entry
// makes the test reasonable to mark with `#[allow(scrap::tautology)]`
// even though the leaf looks like a stdlib assert.

#[test]
fn it() {
    pretty_assertions::assert_eq!(1, 1);
}
