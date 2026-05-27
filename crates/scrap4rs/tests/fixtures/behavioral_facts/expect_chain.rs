// Fixture: a #[test] fn whose body recovers an Option via `.expect(...)`.
//
// The parser must surface `behavioral_facts: [result_asserted]` via
// N30 (`BodyVisitor::visit_expr_method_call`). No explicit assertion
// macro is present; no implicit_assertion_sources either.
//
// At v0.1 the zero-assertion detector (scrap-rs#30) reads this fact
// and suppresses emission — the .expect(...) panic IS the test's
// observable check on the Option.

#[test]
fn it() {
    let x: Option<u32> = Some(1);
    let _ = x.expect("expected Some");
}
