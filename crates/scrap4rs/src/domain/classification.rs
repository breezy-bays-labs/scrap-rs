//! Classifying enums shared across `smell` and `finding`.
//!
//! Lives in its own module so the dependency direction matches the
//! semantic direction: `smell` and `finding` both import from
//! `classification`; `finding` imports from `smell`. No upward edges.
//!
//! The per-variant `#[serde(rename = "...")]` annotations are
//! deliberately belt-and-suspenders alongside `#[serde(rename_all =
//! "snake_case")]`: a refactor that renames a Rust variant cannot
//! accidentally change the wire shape, and a future serde major version
//! that changes its case-conversion algorithm cannot drift the wire
//! either. The redundancy is the wire-format insurance.

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
    /// Low severity — informational; never trips a strict gate.
    #[serde(rename = "low")]
    Low,
    /// Moderate severity — surfaces in default-mode reports but does
    /// not necessarily fail the gate.
    #[serde(rename = "moderate")]
    Moderate,
    /// High severity — top of the bucket; flagged prominently and
    /// usually contributes to gate failure.
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
    /// Mechanical refactor candidate — the smell shape suggests an
    /// automatable transformation (add an assertion, replace tautology).
    #[serde(rename = "auto_refactor")]
    AutoRefactor,
    /// Test boundary needs to move — split a multi-purpose example
    /// into focused tests.
    #[serde(rename = "manual_split")]
    ManualSplit,
    /// Human review needed before any change — context-dependent or
    /// ambiguous signal.
    #[serde(rename = "review_first")]
    ReviewFirst,
}

/// Confidence grade attached to a ranked recommendation. Reserved for
/// the v0.4 confidence-graded recommendations feature (Uncle Bob's
/// scrap surfaces top-N suggestions with `{LOW | MEDIUM | HIGH}`
/// labels). Pre-shaped now so the wire envelope decision lands once,
/// not after v0.4 work is in flight.
///
/// Not yet emitted by any detector or reporter — populated when the
/// v0.4 baseline-diff + ranked-recommendations work begins. Kept on
/// the wire shape's stable surface from v0.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Confidence {
    /// Low confidence — heuristic match, surface as suggestion only.
    #[serde(rename = "low")]
    Low,
    /// Medium confidence — most signals align; reasonable to act on.
    #[serde(rename = "medium")]
    Medium,
    /// High confidence — multiple signals agree; safe to act on.
    #[serde(rename = "high")]
    High,
}

/// Remediation mode classification. Reserved for the v0.5 5-class
/// actionability classifier — Bob's scrap distinguishes `STABLE` (no
/// change advised), `LOCAL` (refactor in place), and `SPLIT` (test
/// boundary needs to move). The v0.5 actionability decision tree maps
/// these onto the 5-class `Actionability` outcome.
///
/// Not yet emitted by any detector. Lives in the wire surface from v0.1
/// so v0.5 can populate it without bumping `schema_version`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RemediationMode {
    /// No structural change advised — leave the test as-is.
    #[serde(rename = "stable")]
    Stable,
    /// Refactor in place — change the test body without moving its
    /// boundary.
    #[serde(rename = "local")]
    Local,
    /// Split the example — the test boundary itself needs to move.
    #[serde(rename = "split")]
    Split,
}

/// Verdict of a baseline-vs-current comparison run. Reserved for the
/// v0.4 `--baseline` / `--compare` feature, which attaches one of these
/// to each delta entry so reporters can summarize a run as
/// "improved/worse/mixed/unchanged" without recomputing the diff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BaselineVerdict {
    /// Current run scores strictly better than baseline.
    #[serde(rename = "improved")]
    Improved,
    /// Current run scores strictly worse than baseline.
    #[serde(rename = "worse")]
    Worse,
    /// Some metrics improved, others regressed.
    #[serde(rename = "mixed")]
    Mixed,
    /// Current run is identical to baseline.
    #[serde(rename = "unchanged")]
    Unchanged,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_ordering_low_to_high() {
        assert!(Severity::Low < Severity::Moderate);
        assert!(Severity::Moderate < Severity::High);
    }

    #[test]
    fn severity_serializes_snake_case() {
        let cases = [
            (Severity::Low, "low"),
            (Severity::Moderate, "moderate"),
            (Severity::High, "high"),
        ];
        for (sev, wire) in cases {
            assert_eq!(
                serde_json::to_value(sev).unwrap(),
                serde_json::Value::String(wire.into()),
            );
        }
    }

    #[test]
    fn actionability_serializes_snake_case() {
        let cases = [
            (Actionability::AutoRefactor, "auto_refactor"),
            (Actionability::ManualSplit, "manual_split"),
            (Actionability::ReviewFirst, "review_first"),
        ];
        for (a, wire) in cases {
            assert_eq!(
                serde_json::to_value(a).unwrap(),
                serde_json::Value::String(wire.into()),
            );
        }
    }

    #[test]
    fn confidence_serializes_snake_case() {
        let cases = [
            (Confidence::Low, "low"),
            (Confidence::Medium, "medium"),
            (Confidence::High, "high"),
        ];
        for (c, wire) in cases {
            assert_eq!(
                serde_json::to_value(c).unwrap(),
                serde_json::Value::String(wire.into()),
            );
        }
    }

    #[test]
    fn remediation_mode_serializes_snake_case() {
        let cases = [
            (RemediationMode::Stable, "stable"),
            (RemediationMode::Local, "local"),
            (RemediationMode::Split, "split"),
        ];
        for (r, wire) in cases {
            assert_eq!(
                serde_json::to_value(r).unwrap(),
                serde_json::Value::String(wire.into()),
            );
        }
    }

    #[test]
    fn baseline_verdict_serializes_snake_case() {
        let cases = [
            (BaselineVerdict::Improved, "improved"),
            (BaselineVerdict::Worse, "worse"),
            (BaselineVerdict::Mixed, "mixed"),
            (BaselineVerdict::Unchanged, "unchanged"),
        ];
        for (v, wire) in cases {
            assert_eq!(
                serde_json::to_value(v).unwrap(),
                serde_json::Value::String(wire.into()),
            );
        }
    }
}
