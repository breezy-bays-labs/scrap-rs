//! Large-example smell — minimal triggering example.
//!
//! The body is a single `#[test]` fn whose interior runs well past the
//! default 30-line threshold: it stitches together unrelated setup,
//! many sequential operations, and a scattering of assertions into one
//! sprawling example. Each assertion compares DISTINCT operands (so this
//! is neither a tautology nor a zero-assertion body) and discards no
//! `Result` — so the ONLY smell that fires is `large-example` (penalty
//! 4, `scrap_score: 4.0`). That isolation is deliberate: the fixture
//! demonstrates the structural smell on its own, not stacked with the
//! assertion-quality smells.
//!
//! The fix is to split this into focused examples (or extract the setup
//! into a helper) so each test reads as one idea.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn one_test_does_far_too_much() {
    // A grab-bag of setup, mutation, and checks all crammed into a
    // single example — the classic "large example" shape.
    let mut totals: Vec<i64> = Vec::new();
    let mut running = 0i64;
    for step in 1..=8 {
        running += step;
        totals.push(running);
    }
    assert_eq!(totals.len(), 8);
    assert_eq!(totals[0], 1);
    assert_eq!(totals[1], 3);
    assert_eq!(totals[2], 6);
    assert_eq!(totals[7], 36);

    let mut labels: Vec<String> = Vec::new();
    for (idx, total) in totals.iter().enumerate() {
        labels.push(format!("step-{idx}={total}"));
    }
    assert_eq!(labels.len(), 8);
    assert_eq!(labels[0], "step-0=1");
    assert_eq!(labels[7], "step-7=36");

    let sum: i64 = totals.iter().sum();
    assert_eq!(sum, 120);

    let max = totals.iter().copied().max();
    assert_eq!(max, Some(36));

    let min = totals.iter().copied().min();
    assert_eq!(min, Some(1));

    let evens: Vec<i64> = totals.iter().copied().filter(|n| n % 2 == 0).collect();
    assert_eq!(evens, vec![6, 10, 28, 36]);

    let doubled: Vec<i64> = totals.iter().map(|n| n * 2).collect();
    assert_eq!(doubled[0], 2);
    assert_eq!(doubled[7], 72);
}
