//! `OptOut` enum — the agent-immutable allowlist of per-test detector suppressions.
//!
//! Lands with scrap-rs#12 (folds in #4 surface). The parser
//! ([`crate::ports::parser::TestParserPort`] / `scrap4rs::parser::SynTestParser`)
//! populates `ParsedTest::opt_outs` by scanning `#[allow(scrap::*)]`
//! attributes on the test fn. Detectors in `scrap-core::detectors/` read the
//! populated field and skip emission when the matching variant is present.
//!
//! ## Wire shape
//!
//! `#[non_exhaustive]` per [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md)
//! — consumers must use non-exhaustive matches. Wire keys are
//! `snake_case` (`no_asserts`, `tautology`, `no_op`); per-variant
//! `#[serde(rename = "...")]` is belt-and-suspenders against future
//! `rename_all` drift.
//!
//! ## Why `PartialOrd + Ord`
//!
//! `ParsedTest::opt_outs` is `BTreeSet<OptOut>` for deterministic
//! serialization order. `Ord` is the cost of admission to that
//! container. `AssertionSource` (sibling enum) does NOT derive `Ord`
//! because it lives in `Vec<AssertionSource>` (parser emission order
//! preserved).

use serde::{Deserialize, Serialize};

/// Per-test detector-suppression markers, projected from
/// `#[allow(scrap::*)]` attributes on the test fn.
///
/// New variants land additively as new detectors join the v0.1 set
/// (e.g. v0.3+ smell expansion will add `LargeExample`,
/// `LowAssertionDensity`, etc.). The `#[non_exhaustive]` attribute
/// requires every consumer to pattern-match with a fallback arm.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptOut {
    /// `#[allow(scrap::no_asserts)]` — suppress the `zero-assertion`
    /// detector (scrap-rs#30) for this test. Use sparingly: the
    /// detector's intent is to catch tests that ship without observable
    /// effect; opt-outs should carry an inline comment justifying
    /// the suppression.
    #[serde(rename = "no_asserts")]
    NoAsserts,
    /// `#[allow(scrap::tautology)]` — suppress the
    /// `tautological-assertion` detector (scrap-rs#24) for this test.
    /// Useful for sanity-check fixtures (e.g. `assert_eq!(1, 1)` in a
    /// table-driven test where one row legitimately collapses to a
    /// trivial comparison).
    #[serde(rename = "tautology")]
    Tautology,
    /// `#[allow(scrap::no_op)]` — suppress the `no-op-io` detector
    /// (scrap-rs#25) for this test. Useful when the test deliberately
    /// exercises a `let _ = call()` form to assert non-panic without
    /// caring about the return value.
    #[serde(rename = "no_op")]
    NoOp,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn opt_out_serializes_snake_case() {
        for (variant, wire) in [
            (OptOut::NoAsserts, "no_asserts"),
            (OptOut::Tautology, "tautology"),
            (OptOut::NoOp, "no_op"),
        ] {
            let json = serde_json::to_value(variant).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: OptOut = serde_json::from_value(json).unwrap();
            assert_eq!(back, variant);
        }
    }

    #[test]
    fn opt_out_btreeset_preserves_deterministic_order() {
        // Insertion order varies; BTreeSet imposes Ord.
        // Sanity check: the same set serialized twice produces the
        // same JSON, even when inserted in different orders.
        let mut set_a = BTreeSet::new();
        set_a.insert(OptOut::NoOp);
        set_a.insert(OptOut::NoAsserts);
        set_a.insert(OptOut::Tautology);

        let mut set_b = BTreeSet::new();
        set_b.insert(OptOut::Tautology);
        set_b.insert(OptOut::NoAsserts);
        set_b.insert(OptOut::NoOp);

        assert_eq!(
            serde_json::to_string(&set_a).unwrap(),
            serde_json::to_string(&set_b).unwrap(),
        );
    }

    #[test]
    fn opt_out_ord_matches_declaration_order() {
        // The variant declaration order (NoAsserts < Tautology < NoOp)
        // is the contract `BTreeSet<OptOut>` serializes by. Pin it.
        assert!(OptOut::NoAsserts < OptOut::Tautology);
        assert!(OptOut::Tautology < OptOut::NoOp);
    }
}
