//! Severity, Actionability, and the per-test `Finding` aggregate.
//!
//! A `Finding` is the per-test record that lands in the wire envelope's
//! `result.findings[]` array — test identity, all smells detected on
//! that test, total scrap score, threshold flag, and any opt-outs the
//! detector observed.

use crate::domain::smell::Smell;
use crate::domain::types::TestIdentity;
use serde::{Deserialize, Serialize};

/// Detector-emitted severity bucket. Scores layer on top via the
/// per-detector penalty table; severity is the human-facing classifier.
///
/// Ordered: `Low < Moderate < High`. Comparison is meaningful (used by
/// reporters that surface "highest severity per file").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Severity {
    #[serde(rename = "low")]
    Low,
    #[serde(rename = "moderate")]
    Moderate,
    #[serde(rename = "high")]
    High,
}

/// AI-actionability classification for a smell — what kind of follow-up
/// the finding suggests.
///
/// v0.1 ships the 3-class subset that's reachable from static analysis
/// alone. `LeaveAlone` and `AutoTableDrive` are reserved for the v0.5
/// 5-class classifier (see kickstart plan §3 phased roadmap) and are
/// intentionally absent until the duplication-aware pressure score
/// lands. `#[non_exhaustive]` keeps the enum forward-compatible.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Actionability {
    #[serde(rename = "auto_refactor")]
    AutoRefactor,
    #[serde(rename = "manual_split")]
    ManualSplit,
    #[serde(rename = "review_first")]
    ReviewFirst,
}

/// Per-test result — one entry in the wire envelope's
/// `result.findings[]` array.
///
/// `scrap_score` is the sum of `Smell::penalty` across `smells` for v0.1;
/// the saturating-curve score lands at v0.3 (kickstart plan §3). The
/// reporter compares `scrap_score` against the active `ThresholdMode`
/// cutoff to set `exceeds_threshold` — domain types do not consult
/// thresholds themselves.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Finding {
    pub test: TestIdentity,
    pub smells: Vec<Smell>,
    pub scrap_score: f64,
    pub exceeds_threshold: bool,
    /// `#[allow(scrap::*)]` attributes observed on or above the test —
    /// reported for visibility but suppress threshold contribution.
    pub opt_outs: Vec<String>,
}

impl Finding {
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
mod tests {
    use super::*;
    use crate::domain::smell::SmellCategory;
    use crate::domain::types::{FilePath, QualifiedName, Span};

    #[test]
    fn severity_ordering_low_to_high() {
        assert!(Severity::Low < Severity::Moderate);
        assert!(Severity::Moderate < Severity::High);
    }

    #[test]
    fn severity_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(Severity::Moderate).unwrap(),
            serde_json::Value::String("moderate".into()),
        );
    }

    #[test]
    fn actionability_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(Actionability::AutoRefactor).unwrap(),
            serde_json::Value::String("auto_refactor".into()),
        );
        assert_eq!(
            serde_json::to_value(Actionability::ManualSplit).unwrap(),
            serde_json::Value::String("manual_split".into()),
        );
        assert_eq!(
            serde_json::to_value(Actionability::ReviewFirst).unwrap(),
            serde_json::Value::String("review_first".into()),
        );
    }

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
}
