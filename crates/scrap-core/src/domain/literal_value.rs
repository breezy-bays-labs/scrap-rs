//! `LiteralValue` — typed literal-expression facts carried on
//! `ParsedAssertion::single_arg_value` (see
//! [`crate::domain::parsed::ParsedAssertion`]).
//!
//! The Rust parser adapter (`scrap4rs::parser::tautology::extract_tautology_facts`)
//! populates `Some(LiteralValue)` when an assertion macro's argument
//! tokens parse as a single literal expression. The
//! `tautological-assertion` detector reads the resulting field; the
//! current v0.1 rule flags `Some(LiteralValue::Bool(true))` only —
//! `Bool(false)` is NOT flagged (matches Uncle Bob's `unclebob/scrap`
//! convention; deliberate-failure assertions carry informational
//! value).
//!
//! ## Semantic Facts pattern
//!
//! Per [`adr-hexagonal-layout`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-hexagonal-layout.md)
//! and the [`feedback_semantic-facts-vs-statement-projection`
//! memory], the *adapter* answers "what is happening?" (the typed
//! fact: this assertion has a single-arg `Bool(true)` literal) and the
//! *core* detector answers "is this bad?" (the policy: emit a
//! `TautologicalAssertion` smell). `LiteralValue` is the cross-port
//! shape that lets a future `scrap4ts` adapter (`assert.equal(x, x)`
//! / `expect(true).toBe(true)`) project the same kind of fact onto
//! `ParsedAssertion` without re-litigating the detector contract.
//!
//! TODO(scrap-rs#73): when the planned
//! `adr-port-surface-and-domain-conventions` ADR lands, link its D8
//! (POD-only domain) and D10 (Semantic Facts constructor extension)
//! sections here.
//!
//! ## Wire shape
//!
//! `#[non_exhaustive]` per [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-nested-json-envelope.md)
//! enum discipline — consumers must use non-exhaustive matches so
//! future variants (e.g. `Float(...)` or `ByteStr(...)`) land without
//! breaking pattern-match sites in downstream code. Wire form uses
//! externally-tagged JSON via `tag = "kind"` / `content = "value"` for
//! deterministic round-trip; per-variant `#[serde(rename)]` is
//! belt-and-suspenders alongside `rename_all = "snake_case"` (mirrors
//! the convention in [`super::classification`]).
//!
//! ## Variant coverage notes
//!
//! - `Bool(bool)` — `assert!(true)` / `assert!(false)` shape; the
//!   detector's primary trigger surface.
//! - `Int(i128)` — covers every signed integer width up to `i128`,
//!   plus all unsigned widths up to `u64` (which fit in `i128`).
//!   Unsigned literals in `u128::MAX`-or-`usize::MAX`-on-64-bit
//!   territory overflow `base10_parse::<i128>()` and fall back to
//!   [`LiteralValue::Verbatim`]; the v0.1 detector never exercises
//!   that range (tests rarely use 128-bit unsigned literals in
//!   `assert!` positions).
//! - `Str(String)` — unescaped string literal value (`"hello"` → `Str("hello")`).
//! - `Char(char)` — character literal value.
//! - `Verbatim(String)` — escape hatch for literal kinds the v0.1
//!   adapter does not model (float literals, byte strings, byte chars,
//!   future syn `Lit` additions). Float literals (`f32`/`f64`) cannot
//!   derive `Eq`/`Hash` cleanly; storing them as `Verbatim` keeps the
//!   enum trivially derivable. v0.3+ may add a `Float` variant if a
//!   detector needs structured float comparison.

use serde::{Deserialize, Serialize};

/// Typed literal value carried on `ParsedAssertion::single_arg_value`
/// (see [`crate::domain::parsed::ParsedAssertion`]) when the
/// assertion's macro tokens parse as a single literal expression.
///
/// See the module-level docs for the Semantic Facts pattern, wire-shape
/// rationale (`#[non_exhaustive]`, externally-tagged serde), and
/// variant coverage notes (especially the `Int(i128)` widening and the
/// `Verbatim` escape hatch).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum LiteralValue {
    /// Boolean literal — `true` or `false`. The v0.1
    /// `tautological-assertion` detector flags `Some(Bool(true))` only;
    /// `Bool(false)` is intentionally NOT flagged (Uncle Bob convention).
    #[serde(rename = "bool")]
    Bool(bool),
    /// Integer literal normalized to `i128`. The adapter calls
    /// `syn::LitInt::base10_parse::<i128>()` so `0`, `0_u32`, `0i64`
    /// all collapse to `Int(0)`. Literals that overflow `i128` fall
    /// back to [`LiteralValue::Verbatim`].
    #[serde(rename = "int")]
    Int(i128),
    /// String literal unescaped — `"hello"` becomes `Str("hello")`.
    /// (NOT the token-stream form `"\"hello\""`; the unescaped value
    /// is what a detector would compare against.)
    #[serde(rename = "str")]
    Str(String),
    /// Character literal — `'a'` becomes `Char('a')`.
    #[serde(rename = "char")]
    Char(char),
    /// Escape hatch for literal kinds the v0.1 adapter does not model
    /// (float literals, byte strings, byte chars, future syn `Lit`
    /// additions). The wrapped string is `proc_macro2::Literal`'s
    /// `Display` form (whitespace-normalized per syn convention).
    #[serde(rename = "verbatim")]
    Verbatim(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ── Wire-shape: externally-tagged serde for round-trip stability ──

    #[test]
    fn literal_value_serializes_with_kind_content_tags() {
        let cases = [
            (
                LiteralValue::Bool(true),
                serde_json::json!({"kind": "bool", "value": true}),
            ),
            (
                LiteralValue::Bool(false),
                serde_json::json!({"kind": "bool", "value": false}),
            ),
            (
                LiteralValue::Int(0),
                serde_json::json!({"kind": "int", "value": 0}),
            ),
            (
                LiteralValue::Str("hello".into()),
                serde_json::json!({"kind": "str", "value": "hello"}),
            ),
            (
                LiteralValue::Char('a'),
                serde_json::json!({"kind": "char", "value": "a"}),
            ),
            (
                LiteralValue::Verbatim("3.14".into()),
                serde_json::json!({"kind": "verbatim", "value": "3.14"}),
            ),
        ];
        for (variant, expected_json) in cases {
            let actual = serde_json::to_value(&variant).unwrap();
            assert_eq!(actual, expected_json, "variant {variant:?}");
        }
    }

    #[test]
    fn literal_value_round_trips_via_serde_json() {
        let cases = [
            LiteralValue::Bool(true),
            LiteralValue::Bool(false),
            LiteralValue::Int(0),
            LiteralValue::Int(i128::MAX),
            LiteralValue::Int(i128::MIN),
            LiteralValue::Int(-1),
            LiteralValue::Str(String::new()),
            LiteralValue::Str("hello".into()),
            LiteralValue::Str("with \"escapes\" and \\backslashes".into()),
            LiteralValue::Char('a'),
            LiteralValue::Char('日'),
            LiteralValue::Verbatim("3.14".into()),
            LiteralValue::Verbatim(r#"b"bytes""#.into()),
        ];
        for variant in cases {
            let json = serde_json::to_string(&variant).unwrap();
            let back: LiteralValue = serde_json::from_str(&json).unwrap();
            assert_eq!(back, variant, "round-trip failed for {variant:?}");
        }
    }

    // ── Verbatim distinguished from Str by `kind` tag ──

    #[test]
    fn verbatim_and_str_are_distinguished_on_wire() {
        let str_form = LiteralValue::Str("3.14".into());
        let verbatim_form = LiteralValue::Verbatim("3.14".into());
        assert_ne!(str_form, verbatim_form);
        let str_json = serde_json::to_value(&str_form).unwrap();
        let verbatim_json = serde_json::to_value(&verbatim_form).unwrap();
        assert_eq!(str_json["kind"], "str");
        assert_eq!(verbatim_json["kind"], "verbatim");
        // `value` payload is identical; only `kind` distinguishes them.
        assert_eq!(str_json["value"], verbatim_json["value"]);
    }

    // ── Int width-coverage: documentation by example ──

    #[test]
    fn int_construction_is_width_agnostic_by_normalization() {
        // The adapter normalizes any `i8`..`u64` literal to `Int(i128)`.
        // This test documents the contract by demonstrating that values
        // from different widths construct to the same variant.
        let from_zero_literal = LiteralValue::Int(0);
        let from_zero_typed: LiteralValue = LiteralValue::Int(i128::from(0_u32));
        let from_zero_i64: LiteralValue = LiteralValue::Int(i128::from(0_i64));
        assert_eq!(from_zero_literal, from_zero_typed);
        assert_eq!(from_zero_literal, from_zero_i64);
    }

    // ── Hash / Eq derives compile (required for BTreeSet / HashSet membership) ──

    #[test]
    fn literal_value_supports_hash_set_membership() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(LiteralValue::Bool(true));
        set.insert(LiteralValue::Int(42));
        assert!(set.contains(&LiteralValue::Bool(true)));
        assert!(!set.contains(&LiteralValue::Bool(false)));
        assert_eq!(set.len(), 2);
    }

    // ── proptest: random Str values round-trip byte-identical ──

    proptest! {
        #[test]
        fn str_variant_round_trips_random_ascii(
            s in "[ -~]{0,64}",  // printable ASCII, 0..=64 chars
        ) {
            let v = LiteralValue::Str(s.clone());
            let json = serde_json::to_string(&v).unwrap();
            let back: LiteralValue = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(back, LiteralValue::Str(s));
        }

        #[test]
        fn int_variant_round_trips_random_i128(n in any::<i128>()) {
            let v = LiteralValue::Int(n);
            let json = serde_json::to_string(&v).unwrap();
            let back: LiteralValue = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(back, LiteralValue::Int(n));
        }
    }
}
