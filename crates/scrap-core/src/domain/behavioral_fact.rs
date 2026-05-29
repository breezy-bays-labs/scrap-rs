//! `BehavioralFact` enum — typed projection of body-call-shape facts
//! the adapter parser recognises and detectors consume.
//!
//! Lands with scrap-rs#30 (introduces the `ResultAsserted` variant +
//! `ParsedTest.behavioral_facts` field + the parser visitor). scrap-rs#25
//! adds the `ResultDiscarded { kind }` variant + the [`ResultDiscardKind`]
//! shape taxonomy that drives the `no-op-io` detector.
//!
//! `AsyncEscape` (a future-built-but-never-awaited signal) was originally
//! sketched for scrap-rs#25 but is intentionally NOT modeled here: it is
//! **test-darkness** (did the assertion's code path actually run?), a
//! separate detector pillar with its own discriminator, tracked by the
//! darkness-detection epic. The scrap-rs#25 surface is `no-op-io` only.
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
//! ## Wire shape note (heterogeneous array as of scrap-rs#25)
//!
//! `ParsedTest::behavioral_facts` serializes as a JSON array. Before
//! scrap-rs#25 every variant was unit-only, so the array was `string[]`
//! (`["result_asserted"]`). `ResultDiscarded` is the **first
//! data-carrying variant**, so the array is now heterogeneous —
//! `(string | object)[]`, e.g.
//! `["result_asserted", {"result_discarded": {"kind": "call"}}]`. The
//! mokumo scorecard + the future napi-rs FFI consumer must handle both
//! the bare-string and externally-tagged-object forms.
//!
//! TODO(scrap-rs#73): once `adr-port-surface-and-domain-conventions`
//! lands, link to it for the dumb-parser/smart-detector boundary (D10)
//! rationale.

use serde::{Deserialize, Serialize};

/// Heuristic shape of a discarded (`let _ = <expr>;`) initializer, as
/// recognised by the adapter parser. **No type inference** — the parser
/// classifies the syntactic form only, so `Call` fires on any discarded
/// function/method call regardless of its real return type.
///
/// Language-agnostic by design (per the Semantic-Facts cross-port rule):
/// the variant names describe *expression shapes*, not Rust-specific
/// types, so a future scrap4ts adapter can populate the same kinds for
/// TypeScript discards without inventing a faithful TS AST.
///
/// No catch-all `Other` variant: `#[non_exhaustive]` already provides
/// the forward-compat hatch (mirrors
/// [`crate::domain::parsed::ParseDiagnosticKind`]'s discipline). The
/// parser's classifier returns `None` (do-not-project) for every shape
/// outside this set — literals, paths, macros, tuples, control-flow
/// exprs, references, and panic-chain-terminated chains (which project
/// [`BehavioralFact::ResultAsserted`] instead).
///
/// Wire format is `snake_case`; per-variant `#[serde(rename = "...")]`
/// is belt-and-suspenders against future `rename_all` drift.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultDiscardKind {
    /// `let _ = some_call(...);` / `let _ = x.method(...);` — a
    /// function or method call whose result is dropped.
    #[serde(rename = "call")]
    Call,
    /// `let _ = Ok(...);` / `let _ = Err(...);` — an explicit
    /// `Result`-constructor call whose value is dropped.
    #[serde(rename = "result_ctor")]
    ResultCtor,
    /// `let _ = x.ok();` / `let _ = x.err();` — the
    /// `Result`↔`Option` conversion adapters, dropped.
    #[serde(rename = "result_adapter")]
    ResultAdapter,
}

/// Body-shape behavioral facts the adapter parser recognises.
///
/// `#[non_exhaustive]` per [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md)'s
/// enum discipline; new variants land additively as detector PRs introduce
/// new language-shape facts. The wire format is `snake_case`; per-variant
/// `#[serde(rename = "...")]` is belt-and-suspenders against future
/// `rename_all` drift (matches sibling [`crate::domain::assertion_sources::AssertionSource`]
/// + [`crate::domain::opt_outs::OptOut`] discipline).
///
/// Storage: `Vec<BehavioralFact>` on `ParsedTest` (migrated from
/// `BTreeSet` at scrap-rs#112). Two reasons drove the switch, both
/// looking ahead to the located, correlation-carrying fact variants
/// arriving at scrap-rs#26:
/// 1. **Correlation facts must not dedup-collapse.** A `BTreeSet`
///    silently merges two facts that compare equal; the #26 located
///    variants carry distinct `String` path-keys + `Span`s that are
///    semantically separate observations and must each survive on the
///    wire. The "≥1 of shape X" presence-fact dedup the two existing
///    variants relied on now happens at **projection** in the parser
///    adapter (`scrap4rs::parser::body::BodyVisitor`), not via
///    set-admission.
/// 2. **`Span` must not be forced into an `Ord` wire-ordering.**
///    `BTreeSet` admission demands `Ord`; a `Span`-carrying variant
///    would force a total order on source coordinates with no
///    meaningful semantics — the same reason [`crate::domain::types::FilePath`]
///    refuses to derive `Ord` for the wire contract. A `Vec` preserves
///    the parser's natural emission order instead.
///
/// The `Copy`/`PartialOrd`/`Ord` derives on the enum below stay valid
/// for the two existing unit/`Copy`-data variants; they become
/// unused-but-harmless under `Vec` storage and are removed at
/// scrap-rs#26 when the `String`/`Span`-carrying variants land (they
/// cannot derive `Copy`/`Ord`). Keeping them now is minimal scope.
///
/// **No per-instance line field** (still true): the `no-op-io` finding
/// span is whole-test, and no v0.1 consumer reads a per-discard line.
/// Located per-fact spans are scrap-rs#26's surface, not a v0.1 add.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehavioralFact {
    /// Body contains a `.unwrap()` / `.expect(...)` (or the `*_err`
    /// error-path siblings) method-call chain anywhere in the test fn's
    /// body.
    ///
    /// Recognised syntactically by the adapter parser
    /// (`scrap4rs::parser::body::BodyVisitor::visit_expr_method_call`
    /// against the method ident); no type inference is performed —
    /// `.unwrap()` on any value type fires the recognition. Detector-side
    /// consumption (zero-assertion + no-op-io suppression) is the
    /// detector's concern; this variant only encodes the syntactic shape.
    #[serde(rename = "result_asserted")]
    ResultAsserted,
    /// Body contains a `let _ = <Result-shaped expr>;` discard — a bare
    /// wildcard binding (NOT `let _: T = ...;` type-ascribed) whose
    /// initializer is one of the [`ResultDiscardKind`] shapes.
    ///
    /// Recognised by `BodyVisitor::visit_local` delegating to
    /// `classify_discard_init`; drives the `no-op-io` detector
    /// (scrap-rs#25). `kind` records the heuristic shape; see
    /// [`ResultDiscardKind`] for the do-NOT-project boundary.
    #[serde(rename = "result_discarded")]
    ResultDiscarded {
        /// The heuristic shape of the discarded initializer.
        kind: ResultDiscardKind,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Wire-key pin: every variant round-trips its documented form ──

    #[test]
    fn behavioral_fact_result_asserted_serializes_bare_string() {
        // Unit variant → bare snake_case string (the pre-scrap-rs#25 form
        // the mokumo/FFI consumer compiled against; must stay stable).
        let variant = BehavioralFact::ResultAsserted;
        let json = serde_json::to_value(variant).unwrap();
        assert_eq!(json, serde_json::Value::String("result_asserted".into()));
        let back: BehavioralFact = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant);
    }

    #[test]
    fn behavioral_fact_result_discarded_serializes_externally_tagged_object() {
        // Data-carrying variant → externally-tagged object:
        // {"result_discarded": {"kind": "call"}}. Pins the heterogeneous
        // (string | object)[] wire shape the consumer must handle.
        let variant = BehavioralFact::ResultDiscarded {
            kind: ResultDiscardKind::Call,
        };
        let json = serde_json::to_value(variant).unwrap();
        assert_eq!(
            json,
            serde_json::json!({"result_discarded": {"kind": "call"}})
        );
        let back: BehavioralFact = serde_json::from_value(json).unwrap();
        assert_eq!(back, variant);
    }

    #[test]
    fn result_discard_kind_serializes_snake_case() {
        for (kind, wire) in [
            (ResultDiscardKind::Call, "call"),
            (ResultDiscardKind::ResultCtor, "result_ctor"),
            (ResultDiscardKind::ResultAdapter, "result_adapter"),
        ] {
            let json = serde_json::to_value(kind).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: ResultDiscardKind = serde_json::from_value(json).unwrap();
            assert_eq!(back, kind);
        }
    }

    // ── Vec emission-order discipline (post scrap-rs#112 storage) ──

    #[test]
    fn behavioral_fact_vec_serializes_in_emission_order() {
        // Storage is now `Vec<BehavioralFact>` (scrap-rs#112): the wire
        // array reflects **emission order**, NOT `Ord`-sorted order. A
        // `ResultDiscarded`-then-`ResultAsserted` emission serializes in
        // exactly that order — the reverse of the BTreeSet's old
        // `Ord`-sorted "ResultAsserted-first" contract — proving order
        // now tracks emission rather than declaration order. The
        // per-fact wire shape (heterogeneous `(string | object)[]`) is
        // unchanged.
        let facts = vec![
            BehavioralFact::ResultDiscarded {
                kind: ResultDiscardKind::ResultCtor,
            },
            BehavioralFact::ResultAsserted,
        ];
        assert_eq!(
            serde_json::to_value(&facts).unwrap(),
            serde_json::json!([{"result_discarded": {"kind": "result_ctor"}}, "result_asserted"]),
        );
    }
}
