//! `extract_tautology_facts` — token-level fact extraction for the
//! `tautological-assertion` detector (scrap-rs#24).
//!
//! The Rust parser adapter walks `#[test]` fn bodies via
//! `syn::visit::Visit` (see [`super::body::BodyVisitor`]). When the
//! visitor sees a known assertion macro
//! (`assert!`/`assert_eq!`/`assert_ne!`/...) it calls
//! [`extract_tautology_facts`] on the macro's token stream to derive
//! two typed predicates:
//!
//! - `arguments_identical: bool` — true iff the tokens parse as exactly
//!   two comma-separated expressions whose `ToTokens` stringifications
//!   are byte-equal. Fires the `assert_eq!(x, x)` / `assert_ne!(x, x)`
//!   shape.
//! - `single_arg_value: Option<LiteralValue>` — `Some(...)` iff the
//!   tokens parse as a single literal expression whose kind is modeled
//!   by [`scrap_core::domain::literal_value::LiteralValue`]. Fires the
//!   `assert!(true)` shape (when the literal is `Bool(true)`).
//!
//! Both predicates land on
//! [`scrap_core::domain::parsed::ParsedAssertion`]; the
//! `tautological-assertion` detector in `scrap-core::detectors` reads
//! them and emits a `Finding`. AST shape never crosses the port
//! boundary — this module is the Semantic Facts choke point.
//!
//! ## Locked v0.1 choices (see pipeline shape doc)
//!
//! - `arguments_identical` uses **token-string equality** (option (a))
//!   not structural syn equality (option (b)). `assert_eq!(x, x.clone())`
//!   is NOT identical (different token streams); `assert_eq!(0, 0_u32)`
//!   is NOT identical either (different suffix bytes). Safer direction
//!   at v0.1; v0.3+ may promote.
//! - `LiteralValue::Int(i128)` normalization: `syn::LitInt::base10_parse::<i128>()`
//!   collapses `0`, `0_u32`, `0i64` to `Int(0)`. Literals overflowing
//!   `i128` fall back to `Verbatim`.
//! - Negative-int literals are *not* detected as `Int(-N)` — `syn`
//!   parses `-5` as `Expr::Unary(Neg, Lit::Int(5))`, not `Lit::Int(-5)`.
//!   So `assert!(-5)` yields `Verbatim("- 5")` rather than `Int(-5)`.
//!   Filed as scrap-rs follow-up for v0.3+ enrichment.

use proc_macro2::TokenStream;
use quote::ToTokens;
use scrap_core::domain::literal_value::LiteralValue;
use syn::parse::Parser;
use syn::punctuated::Punctuated;
use syn::{Expr, ExprLit, Lit, Token, parse2};

/// Extract typed tautology facts from the argument tokens of a
/// recognized assertion macro. Returns
/// `(arguments_identical, single_arg_value)`.
///
/// - `arguments_identical = true` iff the tokens parse as exactly two
///   comma-separated [`syn::Expr`] elements whose
///   [`quote::ToTokens::to_token_stream`] output is byte-equal.
/// - `single_arg_value = Some(LiteralValue)` iff the tokens parse as a
///   single [`syn::Expr::Lit`] whose inner `syn::Lit` is recognised
///   by [`literal_to_value`]. (`syn::Lit` is left unlinked: rustdoc
///   refuses to disambiguate the bare name because `syn` exports both
///   a `Lit` enum and a `Lit` macro under the same path.)
///
/// Both predicates default to their "no signal" forms on any parse
/// failure or shape mismatch (`false` / `None` respectively). The
/// helper is total — it never panics, even on garbage tokens.
pub(crate) fn extract_tautology_facts(tokens: &TokenStream) -> (bool, Option<LiteralValue>) {
    // Single-literal shape (assert!(true), assert!(42), ...).
    // Attempt this BEFORE the two-arg shape because a single literal
    // also parses as a Punctuated<Expr, Comma> of length 1, which we
    // then reject for not having length 2 — but the single-literal
    // path gives us the LiteralValue payload while the two-arg path
    // would just return (false, None).
    if let Ok(Expr::Lit(ExprLit { lit, .. })) = parse2::<Expr>(tokens.clone()) {
        return (false, Some(literal_to_value(&lit)));
    }

    // Two-arg shape (assert_eq!(x, x), assert_ne!(0, 1), ...).
    // Length must be exactly 2 — three-arg `assert_eq!(a, b, "msg")` is
    // NOT detected as identical even if `a` and `b` were byte-equal.
    // `Punctuated<Expr, Comma>` doesn't impl `Parse` directly; use
    // `Punctuated::parse_terminated` via the `Parser` trait so trailing
    // commas + empty/single-element lists all parse cleanly.
    let parser = Punctuated::<Expr, Token![,]>::parse_terminated;
    if let Ok(args) = parser.parse2(tokens.clone())
        && args.len() == 2
    {
        let mut iter = args.iter();
        // SAFETY: args.len() == 2 guarantees both elements exist; the
        // assert pattern guards the proptest-style invariants.
        let a = iter.next().expect("len==2");
        let b = iter.next().expect("len==2");
        let a_s = a.to_token_stream().to_string();
        let b_s = b.to_token_stream().to_string();
        return (a_s == b_s, None);
    }

    (false, None)
}

/// Project a `syn::Lit` into a [`LiteralValue`] variant. Always
/// returns *some* variant — recognised kinds (`Bool`/`Int`/`Str`/`Char`)
/// map directly; everything else falls through the `Verbatim` escape
/// hatch. The caller (`extract_tautology_facts`) wraps the result in
/// `Some(...)` to thread through `ParsedAssertion::single_arg_value`.
fn literal_to_value(lit: &Lit) -> LiteralValue {
    match lit {
        Lit::Bool(b) => LiteralValue::Bool(b.value),
        Lit::Int(_) => lit_int_to_value(lit),
        Lit::Str(s) => LiteralValue::Str(s.value()),
        Lit::Char(c) => LiteralValue::Char(c.value()),
        _ => LiteralValue::Verbatim(lit.to_token_stream().to_string()),
    }
}

/// Normalize a `Lit::Int` to [`LiteralValue::Int`] via
/// [`syn::LitInt::base10_parse`] → `i128`. Falls back to
/// [`LiteralValue::Verbatim`] if the literal overflows `i128` (rare in
/// practice; tests don't typically use `u128::MAX`-range literals in
/// `assert!` positions).
fn lit_int_to_value(lit: &Lit) -> LiteralValue {
    if let Lit::Int(li) = lit
        && let Ok(n) = li.base10_parse::<i128>()
    {
        LiteralValue::Int(n)
    } else {
        LiteralValue::Verbatim(lit.to_token_stream().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    // ── single-arg literal shape ────────────────────────────────────

    #[test]
    fn single_arg_bool_true_returns_literal_bool_true() {
        let (ident, val) = extract_tautology_facts(&quote! { true });
        assert!(!ident);
        assert_eq!(val, Some(LiteralValue::Bool(true)));
    }

    #[test]
    fn single_arg_bool_false_returns_literal_bool_false() {
        let (ident, val) = extract_tautology_facts(&quote! { false });
        assert!(!ident);
        assert_eq!(val, Some(LiteralValue::Bool(false)));
    }

    #[test]
    fn single_arg_int_returns_literal_int() {
        let (ident, val) = extract_tautology_facts(&quote! { 42 });
        assert!(!ident);
        assert_eq!(val, Some(LiteralValue::Int(42)));
    }

    #[test]
    fn single_arg_int_with_unsigned_suffix_normalizes_to_i128() {
        // Documents the locked normalization: 0_u32 / 0i64 / 0 all
        // collapse to Int(0). assert!(0_u32) would be ill-typed in
        // Rust but the helper still handles the shape cleanly.
        let (ident, val) = extract_tautology_facts(&quote! { 0_u32 });
        assert!(!ident);
        assert_eq!(val, Some(LiteralValue::Int(0)));
    }

    #[test]
    fn single_arg_negative_int_falls_back_to_verbatim() {
        // Documents the v0.1 limitation: `syn` parses `-5` as
        // Expr::Unary(Neg, Lit::Int(5)), not Lit::Int(-5). The single
        // literal path here only matches Expr::Lit, so Expr::Unary
        // falls through to the two-arg path (which fails too), and we
        // return (false, None). NOT (false, Some(Verbatim)) — there
        // was no Lit to project. v0.3+ enrichment may detect this.
        let (ident, val) = extract_tautology_facts(&quote! { -5 });
        assert!(!ident);
        assert_eq!(val, None);
    }

    #[test]
    fn single_arg_str_returns_literal_str_unescaped() {
        let (ident, val) = extract_tautology_facts(&quote! { "hello" });
        assert!(!ident);
        assert_eq!(val, Some(LiteralValue::Str("hello".into())));
    }

    #[test]
    fn single_arg_char_returns_literal_char() {
        let (ident, val) = extract_tautology_facts(&quote! { 'a' });
        assert!(!ident);
        assert_eq!(val, Some(LiteralValue::Char('a')));
    }

    #[test]
    fn single_arg_float_falls_back_to_verbatim() {
        let (ident, val) = extract_tautology_facts(&quote! { 3.14 });
        assert!(!ident);
        // Float literal: not Bool/Int/Str/Char → Verbatim escape hatch.
        // Note: proc_macro2 may normalize the rendered form ("3.14" expected).
        match val {
            Some(LiteralValue::Verbatim(s)) => assert_eq!(s, "3.14"),
            other => panic!("expected Verbatim, got {other:?}"),
        }
    }

    #[test]
    fn single_arg_byte_string_falls_back_to_verbatim() {
        // Byte-string literals (`b"hello"`) are Lit::ByteStr — not
        // modeled by LiteralValue. Falls back to Verbatim with the
        // rendered token-stream form.
        let (ident, val) = extract_tautology_facts(&quote! { b"hello" });
        assert!(!ident);
        match val {
            Some(LiteralValue::Verbatim(s)) => assert!(s.starts_with("b\"")),
            other => panic!("expected Verbatim, got {other:?}"),
        }
    }

    // ── two-arg comma list shape ────────────────────────────────────

    #[test]
    fn two_args_identical_int_returns_true_none() {
        let (ident, val) = extract_tautology_facts(&quote! { 1, 1 });
        assert!(ident);
        assert_eq!(val, None);
    }

    #[test]
    fn two_args_distinct_int_returns_false_none() {
        let (ident, val) = extract_tautology_facts(&quote! { 1, 2 });
        assert!(!ident);
        assert_eq!(val, None);
    }

    #[test]
    fn two_args_identical_path_returns_true_none() {
        let (ident, val) = extract_tautology_facts(&quote! { x, x });
        assert!(ident);
        assert_eq!(val, None);
    }

    #[test]
    fn two_args_path_vs_call_returns_false_none() {
        // assert_eq!(x, x.clone()) — locked safer-direction: token
        // streams differ, NOT identical.
        let (ident, val) = extract_tautology_facts(&quote! { x, x.clone() });
        assert!(!ident);
        assert_eq!(val, None);
    }

    #[test]
    fn two_args_typed_literal_cross_suffix_returns_false_none() {
        // assert_eq!(0, 0_u32) — locked safer-direction: token suffixes
        // differ (`"0"` vs `"0_u32"`), NOT identical. v0.3+ may
        // normalize via base10_parse per side.
        let (ident, val) = extract_tautology_facts(&quote! { 0, 0_u32 });
        assert!(!ident);
        assert_eq!(val, None);
    }

    #[test]
    fn two_args_identical_negative_int_returns_true_none() {
        // assert_eq!(-5, -5) — both sides stringify to "- 5", which
        // is byte-equal. Documents the negative-literal handling at
        // the two-arg path even though the single-arg path can't
        // project Verbatim(-5).
        let (ident, val) = extract_tautology_facts(&quote! { -5, -5 });
        assert!(ident);
        assert_eq!(val, None);
    }

    #[test]
    fn three_args_with_message_returns_false_none() {
        // assert_eq!(a, b, "error msg") — three-arg form. Not detected
        // even if a and b were byte-equal. Intentional: the third arg
        // distinguishes intent.
        let (ident, val) = extract_tautology_facts(&quote! { a, b, "msg" });
        assert!(!ident);
        assert_eq!(val, None);
    }

    #[test]
    fn three_args_with_identical_first_two_returns_false_none() {
        // assert_eq!(x, x, "msg") — even though first two are
        // identical, the three-arg form rules out tautology detection.
        let (ident, val) = extract_tautology_facts(&quote! { x, x, "msg" });
        assert!(!ident);
        assert_eq!(val, None);
    }

    // ── degenerate / no-signal shapes ───────────────────────────────

    #[test]
    fn empty_tokens_returns_false_none() {
        // assert!() — empty token stream.
        let (ident, val) = extract_tautology_facts(&quote! {});
        assert!(!ident);
        assert_eq!(val, None);
    }

    #[test]
    fn non_literal_single_arg_returns_false_none() {
        // assert!(x) — single Expr::Path, not Expr::Lit.
        let (ident, val) = extract_tautology_facts(&quote! { x });
        assert!(!ident);
        assert_eq!(val, None);
    }

    #[test]
    fn non_literal_method_call_returns_false_none() {
        // assert!(some.method()) — single Expr but not a literal.
        let (ident, val) = extract_tautology_facts(&quote! { some.method() });
        assert!(!ident);
        assert_eq!(val, None);
    }

    // ── int width-coverage at the parse boundary ────────────────────

    #[test]
    fn int_max_i64_parses_to_int_variant() {
        let max_i64 = i128::from(i64::MAX);
        let tokens = quote! { 9223372036854775807 }; // i64::MAX
        let (ident, val) = extract_tautology_facts(&tokens);
        assert!(!ident);
        assert_eq!(val, Some(LiteralValue::Int(max_i64)));
    }

    #[test]
    fn int_overflowing_i128_falls_back_to_verbatim() {
        // 2^128 = 340282366920938463463374607431768211456 — 1 past
        // u128::MAX (2^128 - 1). Exceeds i128 too. base10_parse fails;
        // verbatim fallback engages.
        let tokens = quote! { 340282366920938463463374607431768211456 };
        let (ident, val) = extract_tautology_facts(&tokens);
        assert!(!ident);
        match val {
            Some(LiteralValue::Verbatim(s)) => {
                assert!(s.contains("340282366920938463463374607431768211456"));
            }
            other => panic!("expected Verbatim fallback for overflow, got {other:?}"),
        }
    }
}
