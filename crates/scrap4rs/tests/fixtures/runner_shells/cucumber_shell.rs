// Fixture: an #[tokio::test] async fn that runs cucumber-rs.
//
// The parser must surface `implicit_assertion_sources: [Cucumber]`
// via the synthetic `"cucumber::run"` key fabricated when
// visit_expr_await sees the `.await` on a `World::cucumber().run(...)`
// chain receiver. This is the highest-impact false-positive guard
// for the mokumo CI integration — mokumo uses cucumber-rs heavily,
// and the v0.2 zero-assertion detector would false-positive on
// every cucumber harness fn without this recognition.

#[tokio::test]
async fn it() {
    World::cucumber().run("tests/features").await;
}
