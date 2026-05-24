// Fixture: a #[test] #[should_panic] fn with no explicit body
// assertion — the canonical attribute-sourced implicit assertion.
//
// The parser must surface `implicit_assertion_sources: [ShouldPanic]`
// via N24 (`implicit_sources_from_attributes`) — the attribute path,
// NOT the body-walker path. `should_panic` is the only v0.1
// AssertionSource that ships through the attribute channel; future
// v0.3+ attribute-sourced variants extend N24 additively.

#[test]
#[should_panic]
fn it() {
    let x: u32 = 1;
    let _ = x.checked_sub(2).unwrap();
}
