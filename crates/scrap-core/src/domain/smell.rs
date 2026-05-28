//! `SmellCategory` enum and the `Smell` per-instance record.
//!
//! v0.1 ships the 5-detector slate from kickstart plan §3
//! (zero-assertion, tautological-assertion, no-op-io, surface-only-io,
//! large-example). The full Speclj 8-smell taxonomy lands across
//! v0.3–v0.5; `#[non_exhaustive]` lets new variants slot in without
//! breaking the wire envelope.

use crate::domain::classification::{Actionability, Severity};
use crate::domain::types::Span;
use serde::{Deserialize, Serialize};

/// Test smell taxonomy. Wire format mirrors envelope §6 in the kickstart
/// plan: `snake_case` strings on the wire, never integer codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SmellCategory {
    /// Test body invokes the system under test but never asserts on the
    /// result, after recognizing implicit-assertion sources (cucumber-rs,
    /// proptest, quickcheck, trybuild, insta, kani, `should_panic`).
    ///
    /// See the [`zero_assertion` fixture](https://github.com/breezy-bays-labs/scrap-rs/tree/main/crates/scrap-examples/examples/zero_assertion)
    /// for a curated bad-by-design example + the fix pattern + the
    /// emitted JSON envelope shape.
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
    /// `ruleId` fields, scorecard-row tags). Kept in lock-step with
    /// the per-variant `#[serde(rename = ...)]` annotations; the
    /// `enum_wire_strings_match_serde_output` test in the crate root
    /// pins the agreement.
    #[must_use]
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
/// `Smell::default_message`); detectors that want to supply context-
/// aware text can use `Smell::with_message`. v0.5 swaps in a richer
/// classifier without changing the wire shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Smell {
    /// Which smell category the detector matched.
    pub category: SmellCategory,
    /// Severity bucket for this instance.
    pub severity: Severity,
    /// Recommended follow-up class.
    pub actionability: Actionability,
    /// Human-readable follow-up suggestion. Defaults from
    /// `Smell::default_message`; override via `Smell::with_message`.
    pub ai_actionability_message: String,
    /// Score contribution from this smell. Sum across all smells on a
    /// `Finding` becomes the `scrap_score`.
    pub penalty: u32,
    /// Per-Smell line attribution within a `Finding`. Detectors emit
    /// `Some(span)` when they have per-assertion location (e.g. a
    /// `tautological-assertion` smell can point at the exact
    /// `assert_eq!(x, x)` line). `None` when the smell is whole-test
    /// (e.g. `zero-assertion`, where the whole body is the evidence).
    ///
    /// The enclosing `Finding::test.span` always covers the whole test
    /// body; `Smell::span` is strictly narrower or absent. Per ADR
    /// `adr-nested-json-envelope.md` D2 forward-compat rules, this
    /// field is additive and does not require a `schema_version` bump:
    /// `#[serde(skip_serializing_if = "Option::is_none")]` keeps the
    /// wire shape byte-identical for callers that don't supply a span.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span: Option<Span>,
}

impl Smell {
    /// Build a `Smell` with the v0.1 default actionability message for
    /// the given category. Detectors call this when the static
    /// template suffices. Pass `Some(span)` when the detector has
    /// per-assertion location (narrower than the enclosing test body);
    /// pass `None` when the smell is whole-test.
    #[must_use]
    pub fn new(
        category: SmellCategory,
        severity: Severity,
        actionability: Actionability,
        penalty: u32,
        span: Option<Span>,
    ) -> Self {
        Self::with_message(
            category,
            severity,
            actionability,
            penalty,
            span,
            Self::default_message(category),
        )
    }

    /// Build a `Smell` with a custom actionability message. Detectors
    /// emitting context-aware text (e.g., "split this 47-line test
    /// into 3 smaller examples") call this directly; the v0.5 5-class
    /// classifier will route through here exclusively.
    ///
    /// Parameter order mirrors `ParseDiagnostic::new(kind, span,
    /// message)` in `domain/parsed.rs` — `span` sits before `message`
    /// to keep the first five positional args identical to
    /// `Smell::new`, so detector call sites can be migrated to a
    /// richer message by appending one argument without reordering.
    pub fn with_message(
        category: SmellCategory,
        severity: Severity,
        actionability: Actionability,
        penalty: u32,
        span: Option<Span>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            category,
            severity,
            actionability,
            ai_actionability_message: message.into(),
            penalty,
            span,
        }
    }

    /// Static v0.1 follow-up template. Replaced by the v0.5 5-class
    /// classifier when richer context becomes available.
    #[must_use]
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
    use proptest::prelude::*;

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
            None,
        );
        assert_eq!(
            s.ai_actionability_message,
            Smell::default_message(SmellCategory::ZeroAssertion),
        );
        assert_eq!(s.penalty, 10);
    }

    #[test]
    fn smell_with_message_uses_supplied_text() {
        let s = Smell::with_message(
            SmellCategory::LargeExample,
            Severity::Moderate,
            Actionability::ManualSplit,
            4,
            None,
            "Split this 47-line test into three focused examples.",
        );
        assert_eq!(
            s.ai_actionability_message,
            "Split this 47-line test into three focused examples.",
        );
        assert_eq!(s.category, SmellCategory::LargeExample);
        assert_eq!(s.penalty, 4);
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

    #[test]
    fn smell_new_carries_none_span() {
        let s = Smell::new(
            SmellCategory::ZeroAssertion,
            Severity::High,
            Actionability::AutoRefactor,
            10,
            None,
        );
        assert!(s.span.is_none());
    }

    #[test]
    fn smell_new_carries_supplied_span() {
        let span = Span::new(7, 9, 1, 1);
        let s = Smell::new(
            SmellCategory::TautologicalAssertion,
            Severity::High,
            Actionability::AutoRefactor,
            10,
            Some(span),
        );
        assert_eq!(s.span, Some(span));
    }

    #[test]
    fn smell_with_message_carries_supplied_span() {
        let span = Span::new(42, 51, 1, 1);
        let s = Smell::with_message(
            SmellCategory::LargeExample,
            Severity::Moderate,
            Actionability::ManualSplit,
            4,
            Some(span),
            "Split this 47-line test into three focused examples.",
        );
        assert_eq!(s.span, Some(span));
        assert_eq!(
            s.ai_actionability_message,
            "Split this 47-line test into three focused examples.",
        );
    }

    #[test]
    fn smell_serde_round_trip_with_none_span_omits_field() {
        let s = Smell::new(
            SmellCategory::ZeroAssertion,
            Severity::High,
            Actionability::AutoRefactor,
            10,
            None,
        );
        let json = serde_json::to_value(&s).unwrap();
        // Confirms #[serde(skip_serializing_if = "Option::is_none")]:
        // wire snapshot stays byte-identical when no span is attached.
        assert!(
            json.get("span").is_none(),
            "span field must be omitted from JSON when None, got: {json}",
        );
        let back: Smell = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn smell_serde_round_trip_with_some_span_emits_field() {
        let span = Span::new(7, 9, 1, 1);
        let s = Smell::new(
            SmellCategory::ZeroAssertion,
            Severity::High,
            Actionability::AutoRefactor,
            10,
            Some(span),
        );
        let json = serde_json::to_value(&s).unwrap();
        assert_eq!(json["span"]["start_line"], 7);
        assert_eq!(json["span"]["end_line"], 9);
        let back: Smell = serde_json::from_value(json).unwrap();
        assert_eq!(back, s);
    }

    // Local strategy: ~half the time `None`, ~half the time `Some(Span)`
    // with well-ordered (start_line <= end_line) ranges. Reuses
    // `Span::try_new` so inverted ranges are impossible by construction.
    // Kept local rather than promoting to a global `proptest::Arbitrary`
    // impl on `Span` — premature abstraction; one consumer at v0.1
    // (CAO ADVISORY 2, 2026-05-26).
    prop_compose! {
        fn arb_optional_span()(
            none in proptest::bool::ANY,
            start in 1u32..1_000_000,
            len in 0u32..10_000,
        ) -> Option<Span> {
            if none {
                None
            } else {
                Some(Span::try_new(start, start + len, 1, 1).expect("well-ordered by construction"))
            }
        }
    }

    proptest! {
        #[test]
        fn smell_span_round_trip(span in arb_optional_span()) {
            let s = Smell::new(
                SmellCategory::ZeroAssertion,
                Severity::High,
                Actionability::AutoRefactor,
                10,
                span,
            );
            let json = serde_json::to_value(&s).unwrap();
            let back: Smell = serde_json::from_value(json).unwrap();
            prop_assert_eq!(back.span, span);
        }
    }
}
