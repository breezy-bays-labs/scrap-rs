// Fixture: a #[test] fn with no assertions of any kind.
//
// The parser must surface `ParsedTest::assertions: []` (no
// explicit assertion macros) AND `implicit_assertion_sources: []`
// (no runner shell). At v0.2+ the `zero-assertion` detector (#30)
// will flag this; v0.1 ships the parser only — this fixture is the
// parser-side baseline that detector tests will build on.

#[test]
fn no_assertions_at_all() {
    let x = 1 + 1;
    let _ = x;
}
