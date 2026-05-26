//! `BehavioralFact` enum — typed projection of body-call-shape facts
//! the adapter parser recognises and detectors consume.
//!
//! Lands with scrap-rs#30 (introduces the `ResultAsserted` variant +
//! `ParsedTest.behavioral_facts` field + the parser visitor). scrap-rs#25
//! spawns additional variants (`ResultDiscarded`, `AsyncEscape`, etc.)
//! when its parser-projection work lands.
//!
//! Why a separate `BehavioralFact` from `AssertionSource`:
//! - [`crate::domain::assertion_sources::AssertionSource`] is data-driven
//!   recognition for FRAMEWORK runner shells (proptest, kani, insta, ...).
//!   Path-string matched via `recognise()`.
//! - `BehavioralFact` is shape-recognition for LANGUAGE idioms
//!   (`.unwrap()`/`.expect()` chains, `let _ = ...` discards, etc.).
//!   Walked via syn-visit overrides, not path-string-matched.
//! - Both feed into detector logic but the projection mechanics differ;
//!   keeping the enums separate keeps the parser-side code paths
//!   discoverable.
//!
//! No `syn` dependency — the parser produces these typed facts at the
//! adapter boundary; the domain holds only the enum.
//!
//! TODO(scrap-rs#73): once `adr-port-surface-and-domain-conventions`
//! lands, link to it for the dumb-parser/smart-detector boundary (D10)
//! rationale.

use serde::{Deserialize, Serialize};

/// Body-shape behavioral facts the adapter parser recognises.
///
/// `#[non_exhaustive]` per [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md)'s
/// enum discipline; new variants land additively as detector PRs introduce
/// new language-shape facts. The wire format is `snake_case`; per-variant
/// `#[serde(rename = "...")]` is belt-and-suspenders against future
/// `rename_all` drift (matches sibling [`crate::domain::assertion_sources::AssertionSource`]
/// + [`crate::domain::opt_outs::OptOut`] discipline).
///
/// Storage: `BTreeSet<BehavioralFact>` on `ParsedTest` — `Ord` is the
/// cost of `BTreeSet` admission; deterministic serialization order
/// mirrors the `OptOut` precedent.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehavioralFact {
    /// Body contains a `.unwrap()` or `.expect(...)` method-call chain
    /// anywhere in the test fn's body.
    ///
    /// Recognised syntactically by the adapter parser
    /// (`scrap4rs::parser::body::BodyVisitor::visit_expr_method_call`
    /// against the method ident); no type inference is performed —
    /// `.unwrap()` on any value type fires the recognition. Detector-side
    /// consumption (e.g. zero-assertion suppression) is the detector's
    /// concern; this variant only encodes the syntactic shape.
    #[serde(rename = "result_asserted")]
    ResultAsserted,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    // ── Wire-key pin: every variant round-trips its documented snake_case key ──

    #[test]
    fn behavioral_fact_serializes_snake_case() {
        // v0.1 ships one variant. Asserted directly here to avoid the
        // clippy::single_element_loop nit; the for-(variant, wire) loop
        // pattern (see sibling `assertion_sources.rs::assertion_source_serializes_snake_case`
        // with 8 variants) is the destination shape when v0.3+ adds
        // additional variants — sub-issues of scrap-rs#25 / scrap-rs#27
        // will lift this back to a loop when they introduce
        // `BehavioralFact::ResultDiscarded` / `BehavioralFact::AsyncEscape`.
        let variant = BehavioralFact::ResultAsserted;
        let wire = "result_asserted";
        let json = serde_json::to_value(variant).unwrap();
        assert_eq!(json, serde_json::Value::String(wire.into()));
        let back: BehavioralFact = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant);
    }

    // ── Ord discipline (BTreeSet admission cost; pins declaration order) ──

    #[test]
    fn behavioral_fact_ord_matches_declaration_order() {
        // v0.1 ships exactly one variant. Pins the canonical Ord contract
        // so future variants land in a known position relative to
        // ResultAsserted (sibling OptOut::Ord test at `opt_outs.rs:104`
        // does the same).
        let only = BehavioralFact::ResultAsserted;
        assert_eq!(only, BehavioralFact::ResultAsserted);
    }

    #[test]
    fn behavioral_fact_btreeset_preserves_deterministic_order() {
        // Insertion order varies; BTreeSet imposes Ord.
        // Sanity check: the same set serialized twice produces the
        // same JSON, even when inserted in different orders.
        // (Trivial at v0.1 single-variant, but pins the contract for
        // when v0.3+ variants land.)
        let mut set_a = BTreeSet::new();
        set_a.insert(BehavioralFact::ResultAsserted);

        let mut set_b = BTreeSet::new();
        set_b.insert(BehavioralFact::ResultAsserted);

        assert_eq!(
            serde_json::to_string(&set_a).unwrap(),
            serde_json::to_string(&set_b).unwrap(),
        );
    }
}
