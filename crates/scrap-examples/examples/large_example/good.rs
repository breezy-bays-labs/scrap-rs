//! Large-example FIX — the sprawling example from `bad.rs` is split
//! into a focused test with a small body, well under the default
//! 30-line threshold.
//!
//! One idea per test: this checks a single observable behavior with one
//! real assertion on DISTINCT operands. `body_line_count` is a handful
//! of lines, so `large-example` does not fire — and because it asserts
//! properly on distinct values, neither does any other v0.1 detector.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn running_total_after_three_steps_is_six() {
    let mut running = 0i64;
    for step in 1..=3 {
        running += step;
    }
    assert_eq!(running, 6);
}
