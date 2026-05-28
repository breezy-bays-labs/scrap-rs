//! Zero-assertion FIX — same shape as `bad.rs`, but adds an
//! assertion that observes the system-under-test's effect.
//!
//! The detector's three-clause rule fails the first clause
//! (`parsed.assertions.is_empty()` is false), so no finding emits.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn it_does_a_thing() {
    let value = 1 + 1;
    assert_eq!(value, 2);
}
