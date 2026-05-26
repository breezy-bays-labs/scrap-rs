// Fixture: a #[test] fn whose body recovers a Result via `.unwrap()`.
//
// The parser must surface `behavioral_facts: [result_asserted]` via
// N30 (`BodyVisitor::visit_expr_method_call`). No explicit assertion
// macro is present; no implicit_assertion_sources either.
//
// At v0.1 the zero-assertion detector (scrap-rs#30) reads this fact
// and suppresses emission — the .unwrap() panic IS the test's
// observable check on the Result.

#[test]
fn it() {
    let x: Result<u32, ()> = Ok(1);
    let _ = x.unwrap();
}
