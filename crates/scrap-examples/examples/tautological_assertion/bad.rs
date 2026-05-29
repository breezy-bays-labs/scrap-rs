//! Tautological-assertion smell — minimal triggering example.
//!
//! The body asserts, but the assertions are shaped so they cannot
//! fail: `assert!(true)` is a constant-true literal, and
//! `assert_eq!(1, 1)` compares token-identical arguments. The test
//! always passes regardless of the system-under-test — it conveys no
//! information.
//!
//! Because the body DOES carry (tautological) assertions,
//! `parsed.assertions` is non-empty → `has_positive_check` is true, so
//! neither `zero-assertion` nor `no-op-io` co-fires. `detect_all`
//! emits ONLY the `tautological-assertion` smells: one per offending
//! assertion (penalty 10 each), aggregated into a single `Finding`
//! whose `scrap_score` is the stacked sum (2 × 10 = 20).
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn asserts_things_that_cannot_fail() {
    assert!(true);
    assert_eq!(1, 1);
}
