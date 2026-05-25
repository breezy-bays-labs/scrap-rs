// Fixture: a #[test] fn that delegates to `proptest!`.
//
// The parser must surface `implicit_assertion_sources: [Proptest]` so
// the v0.2 zero-assertion detector (#30) doesn't false-positive on
// the test (proptest!'s body contains the real assertions).
//
// This fixture is parsed by:
//   - The cucumber scenario "Implicit-assertion sources are
//     recognized from runner shells" (S2.3 row).
//   - The insta snapshot at `parser_snapshots.rs::snapshot_proptest_shell`.

#[test]
fn it() {
    proptest! { |(x in 0u32..100)| {
        let _ = x;
    } }
}
