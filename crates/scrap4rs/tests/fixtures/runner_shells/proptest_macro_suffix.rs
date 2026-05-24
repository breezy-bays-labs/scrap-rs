// Fixture: a #[test] fn using a custom `*_proptest!` macro.
//
// The parser must surface `implicit_assertion_sources: [Proptest]`
// via the `*_proptest` suffix rule in recognise(). Custom
// proptest-derived macros (e.g. project-local DSLs) inherit
// proptest's implicit-assertion semantics, so the suffix rule
// catches them.

#[test]
fn it() {
    my_proptest! { x in 0u32..100 }
}
