// Fixture: a #[test] fn using `trybuild::TestCases` (compile-fail /
// pass harness).
//
// The parser must surface `implicit_assertion_sources: [Trybuild]`
// via the `trybuild::TestCases::*` prefix match in recognise().
// trybuild's Drop impl runs the actual assertion on test fixture
// drop; the parser's job is just to flag the call so the
// zero-assertion detector doesn't false-positive.

#[test]
fn it() {
    trybuild::TestCases::new().compile_fail("tests/ui/*.rs");
}
