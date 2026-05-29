//! Tautological-assertion FIX — same shape as `bad.rs`, but the
//! assertions observe a real value that the test actually computed,
//! so they can fail if the system-under-test regresses.
//!
//! `assert_eq!(result, 4)` compares a runtime value against an
//! expected constant (not a token-identical pair), and
//! `assert!(result > 0)` is a non-constant predicate. Neither trips
//! the tautology rule (`arguments_identical` is false; no
//! `single_arg_value == Bool(true)`), so no finding emits. The
//! presence of real assertions also keeps `zero-assertion` silent.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn asserts_a_value_that_can_fail() {
    let result = 2 + 2;
    assert_eq!(result, 4);
    assert!(result > 0);
}
