//! `SmellCategory` enum and the `Smell` per-instance record.
//!
//! v0.1 ships the 5-detector slate from kickstart plan §3
//! (zero-assertion, tautological-assertion, no-op-io, surface-only-io,
//! large-example). The full Speclj 8-smell taxonomy lands across
//! v0.3–v0.5; `#[non_exhaustive]` lets new variants slot in without
//! breaking the wire envelope.

use crate::domain::finding::{Actionability, Severity};
use serde::{Deserialize, Serialize};

/// Test smell taxonomy. Wire format mirrors envelope §6 in the kickstart
/// plan: snake_case strings on the wire, never integer codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SmellCategory {
    /// Test body invokes the system under test but never asserts on the
    /// result, after recognizing implicit-assertion sources (cucumber-rs,
    /// proptest, quickcheck, trybuild, insta, kani, `should_panic`).
    #[serde(rename = "zero_assertion")]
    ZeroAssertion,
    /// `assert_eq!(x, x)`, `assert!(true)`, or any equivalent shape that
    /// cannot fail — the assertion exists but conveys nothing.
    #[serde(rename = "tautological_assertion")]
    TautologicalAssertion,
    /// Test performs I/O whose result is discarded — open/read/close
    /// without inspecting the data, HTTP request without inspecting the
    /// response, etc.
    #[serde(rename = "no_op_io")]
    NoOpIo,
    /// Test asserts only on surface-level metadata of an I/O operation
    /// (status code, file existence) without inspecting the substantive
    /// payload.
    #[serde(rename = "surface_only_io")]
    SurfaceOnlyIo,
    /// Test body exceeds the configured line threshold (default 30 for
    /// Rust per kickstart plan §3 — tuned higher than Uncle Bob's 20 to
    /// account for Rust's natural verbosity).
    #[serde(rename = "large_example")]
    LargeExample,
}

impl SmellCategory {
    /// Stable wire string for this smell category. Used by reporters
    /// that need the category outside a serde context (e.g. SARIF
    /// `ruleId` fields, scorecard-row tags).
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            Self::ZeroAssertion => "zero_assertion",
            Self::TautologicalAssertion => "tautological_assertion",
            Self::NoOpIo => "no_op_io",
            Self::SurfaceOnlyIo => "surface_only_io",
            Self::LargeExample => "large_example",
        }
    }
}

/// Per-smell instance attached to a `Finding`. Exactly one entry in
/// `Finding::smells` per detection; if a single test trips multiple
/// detectors, `Finding::smells.len() > 1`.
///
/// `ai_actionability_message` is the human-facing follow-up suggestion.
/// v0.1 defaults to a static template per category (see
/// `Smell::default_message`); v0.5 swaps in context-aware messages from
/// the actionability classifier.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Smell {
    pub category: SmellCategory,
    pub severity: Severity,
    pub actionability: Actionability,
    pub ai_actionability_message: String,
    pub penalty: u32,
}

impl Smell {
    /// Build a `Smell` with the v0.1 default actionability message for
    /// the given category. Detectors call this so messages stay
    /// consistent across the codebase until the v0.5 classifier ships.
    pub fn new(
        category: SmellCategory,
        severity: Severity,
        actionability: Actionability,
        penalty: u32,
    ) -> Self {
        Self {
            ai_actionability_message: Self::default_message(category).to_owned(),
            category,
            severity,
            actionability,
            penalty,
        }
    }

    /// Static v0.1 follow-up template. Replaced by the v0.5 5-class
    /// classifier when richer context becomes available.
    pub fn default_message(category: SmellCategory) -> &'static str {
        match category {
            SmellCategory::ZeroAssertion => "Add assertions for the function's observable effects.",
            SmellCategory::TautologicalAssertion => {
                "Replace the tautology with an assertion that can actually fail."
            }
            SmellCategory::NoOpIo => "Inspect or assert on the data returned by the I/O call.",
            SmellCategory::SurfaceOnlyIo => {
                "Assert on the substantive payload, not just the response status."
            }
            SmellCategory::LargeExample => {
                "Split this example into focused tests or extract setup helpers."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smell_category_serializes_snake_case() {
        let cases = [
            (SmellCategory::ZeroAssertion, "zero_assertion"),
            (
                SmellCategory::TautologicalAssertion,
                "tautological_assertion",
            ),
            (SmellCategory::NoOpIo, "no_op_io"),
            (SmellCategory::SurfaceOnlyIo, "surface_only_io"),
            (SmellCategory::LargeExample, "large_example"),
        ];
        for (cat, wire) in cases {
            assert_eq!(
                serde_json::to_value(cat).unwrap(),
                serde_json::Value::String(wire.into()),
            );
            assert_eq!(cat.as_wire_str(), wire);
        }
    }

    #[test]
    fn smell_new_picks_default_message_for_category() {
        let s = Smell::new(
            SmellCategory::ZeroAssertion,
            Severity::High,
            Actionability::AutoRefactor,
            10,
        );
        assert_eq!(
            s.ai_actionability_message,
            Smell::default_message(SmellCategory::ZeroAssertion),
        );
        assert_eq!(s.penalty, 10);
    }

    #[test]
    fn default_message_is_nonempty_for_every_category() {
        for cat in [
            SmellCategory::ZeroAssertion,
            SmellCategory::TautologicalAssertion,
            SmellCategory::NoOpIo,
            SmellCategory::SurfaceOnlyIo,
            SmellCategory::LargeExample,
        ] {
            assert!(!Smell::default_message(cat).is_empty());
        }
    }
}
