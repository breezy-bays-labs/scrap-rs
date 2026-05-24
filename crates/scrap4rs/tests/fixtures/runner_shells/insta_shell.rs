// Fixture: a #[test] fn whose body is an insta snapshot assertion.
//
// The parser must surface `implicit_assertion_sources: [Insta]` via
// the `insta::assert_*` prefix-plus-leaf rule in recognise(). The
// `assert_snapshot` leaf is in the explicit assertion set too —
// neither path triggers (leaf is `assert_snapshot`, not the v0.1
// explicit set `assert`/`assert_eq`/etc.), so this fixture
// surfaces ONLY the implicit Insta source (no ParsedAssertion entry).

#[test]
fn it() {
    insta::assert_snapshot!("rendered output");
}
