//! Per-test `Finding` aggregate.
//!
//! A `Finding` is the per-test record that lands in the wire envelope's
//! `result.findings[]` array — test identity, all smells detected on
//! that test, total scrap score, threshold flag, and any opt-outs the
//! detector observed. Severity, Actionability, and the smell-instance
//! type all live in sibling modules so this file owns only the
//! aggregate concern.

use crate::domain::smell::Smell;
use crate::domain::types::TestIdentity;
use serde::{Deserialize, Serialize};

/// Per-test result — one entry in the wire envelope's
/// `result.findings[]` array.
///
/// `scrap_score` is `f64` even in v0.1 (where it equals the sum of
/// `Smell::penalty` across `smells`) so the v0.3 saturating-curve score
/// (kickstart plan §3) lands as a value computation behind the same
/// field — no envelope migration required. `Eq`/`Hash` are
/// intentionally absent: float comparison would need bit-pattern
/// hashing to be sound.
///
/// The reporter compares `scrap_score` against the active
/// `ThresholdMode` cutoff to set `exceeds_threshold`; domain types do
/// not consult thresholds themselves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Identity of the test this finding applies to.
    pub test: TestIdentity,
    /// All smells the detector pipeline emitted for this test.
    pub smells: Vec<Smell>,
    /// Aggregate score for this test, computed by `Finding::new`. See
    /// the struct-level doc for the score-formula migration plan.
    pub scrap_score: f64,
    /// True when `scrap_score` exceeds the active `ThresholdMode`
    /// cutoff. Set by the reporter, not by domain construction.
    pub exceeds_threshold: bool,
    /// `#[allow(scrap::*)]` attributes observed on or above the test —
    /// reported for visibility but suppress threshold contribution.
    pub opt_outs: Vec<String>,
}

impl Finding {
    /// Build a `Finding` from a test identity and detector-emitted
    /// smells. Computes `scrap_score` from `smells` (current formula:
    /// sum of penalties); leaves `exceeds_threshold` false (the reporter
    /// sets it) and `opt_outs` empty (the parser populates it
    /// post-construction).
    #[must_use]
    pub fn new(test: TestIdentity, smells: Vec<Smell>) -> Self {
        let scrap_score = smells.iter().map(|s| f64::from(s.penalty)).sum();
        Self {
            test,
            smells,
            scrap_score,
            exceeds_threshold: false,
            opt_outs: Vec::new(),
        }
    }
}

#[cfg(test)]
// `Finding::scrap_score` is `f64` but the v0.1 sum is computed from
// `u32` penalties — every assertion in this module compares against an
// exact integer-derived value, so direct equality is intentional.
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::smell::SmellCategory;
    use crate::domain::types::{FilePath, QualifiedName, Span};

    #[test]
    fn finding_scrap_score_sums_penalties() {
        let test = TestIdentity::new(
            FilePath::new("a.rs"),
            QualifiedName::new("a::tests::t"),
            Span::new(10, 20),
        );
        let smells = vec![
            Smell::new(
                SmellCategory::ZeroAssertion,
                Severity::High,
                Actionability::AutoRefactor,
                10,
            ),
            Smell::new(
                SmellCategory::LargeExample,
                Severity::Low,
                Actionability::ManualSplit,
                4,
            ),
        ];
        let f = Finding::new(test, smells);
        assert_eq!(f.scrap_score, 14.0);
        assert!(!f.exceeds_threshold);
        assert!(f.opt_outs.is_empty());
    }

    #[test]
    fn finding_with_no_smells_has_zero_score() {
        let test = TestIdentity::new(
            FilePath::new("a.rs"),
            QualifiedName::new("a::tests::t"),
            Span::new(1, 1),
        );
        let f = Finding::new(test, vec![]);
        assert_eq!(f.scrap_score, 0.0);
    }
}
