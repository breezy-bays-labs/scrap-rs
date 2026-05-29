//! No-op-io FIX — same shape as `bad.rs`, but inspects the discarded
//! `Result` instead of dropping it.
//!
//! `.expect(...)` on the write `Result` projects a
//! `BehavioralFact::ResultAsserted`, which is positive-check evidence:
//! the no-op-io detector's "no positive check" clause fails (so it
//! returns `None`), and the same fact disarms the zero-assertion
//! detector too. No finding emits.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn writes_a_file_and_checks_the_outcome() {
    std::fs::write("/tmp/scrap-no-op-io-example.txt", b"data").expect("write should succeed");
}
