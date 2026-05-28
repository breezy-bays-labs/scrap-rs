// Fixture: a #[test] fn with assert!(true) inside a proptest! body.
//
// The parser MUST NOT see the inner assert!(true) — the
// `visit_macro` boundary in `crates/scrap4rs/src/parser/body.rs`
// explicitly does NOT recurse into macro token streams (the v0.1
// "do NOT call visit::visit_macro(self, mac)" rule). This fixture
// LOCKS that invariant: if a future PR re-enables recursion, the
// snapshot for this fixture would gain a phantom `ParsedAssertion`
// entry and CI would fail.
//
// Expected parsed shape:
//   assertions: []  (empty — the inner assert!(true) is opaque tokens)
//   implicit_assertion_sources: [Proptest]  (recognised via the
//     proptest! macro path)
//
// Cucumber scenario coverage: `crates/scrap4rs/tests/features/parser.feature`
//   "Inner assert!(true) inside proptest! is NOT visible to parser".

#[test]
fn it() {
    proptest! { |(x in 0u32..100)| {
        assert!(true);  // MUST NOT trigger the tautological-assertion detector
        let _ = x;
    } }
}
