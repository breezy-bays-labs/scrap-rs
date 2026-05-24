// Fixture: a #[test] fn that delegates to `quickcheck::quickcheck`
// (function call form).
//
// The parser must surface `implicit_assertion_sources: [Quickcheck]`
// via the `quickcheck::quickcheck` exact-key match in recognise().
// (The `quickcheck!` macro form would surface via visit_macro; this
// fixture exercises the visit_expr_call function-call path
// specifically — same variant, different recognition channel.)

#[test]
fn it() {
    quickcheck::quickcheck(prop_fn as fn(u32) -> bool);
}
