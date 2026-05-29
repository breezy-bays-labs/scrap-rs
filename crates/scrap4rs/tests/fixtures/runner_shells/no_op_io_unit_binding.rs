// FP-guard fixture: `let _: () = ...;` is an INTENTIONAL unit binding
// to silence the `#[must_use]` lint on a Result-returning call whose
// outcome the author has deliberately chosen not to inspect via the
// type ascription. This is NOT the no-op-io smell.
//
// The parser must NOT project `BehavioralFact::ResultDiscarded` for a
// type-ascribed wildcard (`Pat::Type`), only for a bare `Pat::Wild`
// (`let _ = ...;`). So the no-op-io detector MUST NOT fire here.
//
// `should_panic` is the implicit-assertion source that keeps this a
// legitimate test (it asserts the call panics), and the `let _: () =`
// is the must-use silencer for the non-panicking setup line.

#[test]
#[should_panic]
fn intentional_unit_binding_does_not_smell() {
    // Type-ascribed wildcard — deliberate must-use silencer, NOT a discard.
    let _: () = setup_returning_unit();
    // The actual behavior under test: this panics, asserted by #[should_panic].
    trigger_panic();
}

fn setup_returning_unit() {}

fn trigger_panic() {
    panic!("the behavior under test");
}
