// Fixture: a #[test] fn whose body asserts that a Result is Err via
// `.unwrap_err()`. Canonical Rust error-path "assert this failed" idiom.
//
// The parser must surface `behavioral_facts: [result_asserted]` via
// the v0.1 panic-chain method-ident set
// (`PANIC_CHAIN_METHOD_NAMES` in `body.rs`). No explicit assertion
// macro is present; no implicit_assertion_sources either.
//
// At v0.1 the zero-assertion detector (scrap-rs#30) reads this fact
// and suppresses emission — the `.unwrap_err()` panic IS the test's
// observable check that the Result is Err.

#[test]
fn it() {
    let x: Result<u32, ()> = Err(());
    let _ = x.unwrap_err();
}
