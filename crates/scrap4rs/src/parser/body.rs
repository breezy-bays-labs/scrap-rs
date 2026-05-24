//! `BodyVisitor<'ast>` — per-test body walker using `syn::visit::Visit`.
//!
//! Walks the body of one `#[test]`-attributed fn and accumulates
//! domain facts: explicit assertions (S2.2 — this session),
//! macro-form implicit-assertion sources (S2.3), non-macro implicit
//! sources + the cucumber `.await` chain (S2.4).
//!
//! Driven from `extract_parsed_test` in `parser/mod.rs` — fresh
//! visitor per test fn; `Vec`-based accumulators preserve emission
//! order (the breadboard's S3/S4 stores; useful for debugging).
//!
//! S2.2 ships: `BodyVisitor` + `visit_macro` (assertion-macro side
//! only — match against the v0.1 macro set + push `ParsedAssertion`).
//! S2.3 extends `visit_macro` with the implicit-source path via
//! `recognise()`. S2.4 adds `visit_expr_await` (cucumber chain) +
//! `visit_expr_call` (function-call implicit sources).

use scrap_core::domain::assertion_sources::{AssertionSource, recognise};
use scrap_core::domain::parsed::ParsedAssertion;
use syn::Block;
use syn::visit::Visit;

use super::assertions::compose_macro_path_string;
use super::spans::span_from_spanned;

/// Set of v0.1 assertion-macro leaf-segment names. Matched against
/// the LEAF segment of the macro path so `pretty_assertions::assert_eq`
/// matches `"assert_eq"` (and is also picked up as an implicit
/// `PrettyAssertions` source by S2.3's `recognise()` branch).
const ASSERTION_MACRO_NAMES: &[&str] = &[
    "assert",
    "assert_eq",
    "assert_ne",
    "assert_matches",
    "panic",
    "unimplemented",
    "todo",
];

/// Per-test body walker. Constructed fresh in `extract_parsed_test`,
/// drained via the field accessors after the body visit completes.
pub(crate) struct BodyVisitor {
    /// Explicit assertions found in the body. S2.2 populates via
    /// `visit_macro`.
    pub(crate) assertions: Vec<ParsedAssertion>,
    /// Implicit-assertion sources found in the body. S2.3 populates
    /// via `visit_macro` (macro-form sources: proptest, kani, insta,
    /// `pretty_assertions`, `*_proptest` suffix). S2.4 will extend with
    /// `visit_expr_await` (cucumber chain) and `visit_expr_call`
    /// (function-call sources). Emission order is preserved (`Vec`,
    /// not `BTreeSet`) — useful for debugging which body construct
    /// triggered recognition.
    pub(crate) implicit_assertion_sources: Vec<AssertionSource>,
}

impl BodyVisitor {
    pub(crate) fn new() -> Self {
        Self {
            assertions: Vec::new(),
            implicit_assertion_sources: Vec::new(),
        }
    }

    /// Drive the walk over a test fn's `&syn::Block`. Wrapper over
    /// `visit_block` so the caller doesn't have to import the Visit
    /// trait.
    pub(crate) fn drive(&mut self, block: &Block) {
        self.visit_block(block);
    }
}

impl<'ast> Visit<'ast> for BodyVisitor {
    /// Recognise explicit assertion macros (and S2.3+ implicit-source
    /// macros). Whitespace-sensitive path stringification via
    /// `compose_macro_path_string` (NOT `quote!`/`TokenStream`) so
    /// `recognise()`'s exact-string lookups stay accurate.
    ///
    /// **v0.1 boundary: do NOT call `visit::visit_macro(self, mac)` here.**
    /// The parser inspects the macro's immediate path identity only;
    /// token-stream descent is out of scope. Wrapped/custom macros
    /// (e.g. a hypothetical `my_proptest!` wrapping `proptest!`) are
    /// tracked under v0.3+ surface if real codebases push back at
    /// adoption time. (Per scrap-rs#12 S2.2 plan revision item 22.)
    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        let path = compose_macro_path_string(&mac.path);

        // S2.2 — explicit assertion macros: leaf-segment match
        // against the v0.1 set. The leaf (rightmost segment) is what
        // makes `pretty_assertions::assert_eq` match `"assert_eq"`
        // here while ALSO being recognised as an implicit
        // `PrettyAssertions` source by the S2.3 branch below.
        if let Some(leaf) = path.rsplit("::").next()
            && ASSERTION_MACRO_NAMES.contains(&leaf)
        {
            let raw_args = stringify_tokens_opt(&mac.tokens);
            let span = span_from_spanned(mac);
            self.assertions
                .push(ParsedAssertion::new(leaf, raw_args, span));
        }

        // S2.3 — implicit-assertion sources via the recognise()
        // contract (proptest!, kani::*, insta::assert_*!,
        // pretty_assertions::*, *_proptest suffix). Do NOT
        // short-circuit on the explicit-assertion branch above — a
        // macro can be BOTH (e.g. `pretty_assertions::assert_eq`
        // produces both a ParsedAssertion AND an AssertionSource).
        if let Some(src) = recognise(&path) {
            self.implicit_assertion_sources.push(src);
        }

        // v0.1 boundary: NO visit::visit_macro recursion. See the
        // doc-comment block above for the rationale (wrapped/custom
        // macros are out of scope at v0.1; v0.3+ surface follow-up).
    }
}

/// Convert a macro's `tokens` to `Option<String>` for `raw_args`.
///
/// Empty token streams (the `assert!()` form, vanishingly rare in
/// real test code) project to `None` for wire-format efficiency
/// (`raw_args` is `#[serde(skip_serializing_if = "Option::is_none")]`).
/// Non-empty streams stringify via `TokenStream::Display` — the
/// whitespace fidelity is documented as load-bearing for `raw_args`
/// (contrast with `compose_macro_path_string` where whitespace is
/// stripped).
fn stringify_tokens_opt(tokens: &proc_macro2::TokenStream) -> Option<String> {
    let s = tokens.to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::ItemFn;

    fn parse_test_fn(source: &str) -> ItemFn {
        let file: syn::File = syn::parse_str(source).expect("source parses");
        file.items
            .into_iter()
            .find_map(|i| {
                if let syn::Item::Fn(f) = i {
                    Some(f)
                } else {
                    None
                }
            })
            .expect("source contains an fn")
    }

    #[test]
    fn body_visitor_recovers_single_assert() {
        let item = parse_test_fn("fn t() { assert!(true); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert");
        assert_eq!(visitor.assertions[0].raw_args, Some("true".to_string()));
    }

    #[test]
    fn body_visitor_recovers_assert_eq_with_args() {
        let item = parse_test_fn("fn t() { assert_eq!(1, 1); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert_eq");
        // proc-macro2 `TokenStream::Display` stringifies with a
        // single space between comma-separated args.
        assert_eq!(visitor.assertions[0].raw_args, Some("1 , 1".to_string()));
    }

    #[test]
    fn body_visitor_recovers_multiple_assertions_in_order() {
        let item = parse_test_fn("fn t() { assert!(true); assert_eq!(1, 1); assert_ne!(2, 3); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        assert_eq!(visitor.assertions.len(), 3);
        let names: Vec<&str> = visitor.assertions.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["assert", "assert_eq", "assert_ne"]);
    }

    #[test]
    fn body_visitor_recognises_panic_unimplemented_todo() {
        let item = parse_test_fn("fn t() { panic!(); unimplemented!(); todo!(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        let names: Vec<&str> = visitor.assertions.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["panic", "unimplemented", "todo"]);
    }

    #[test]
    fn body_visitor_leaf_segment_match_for_namespaced_assert_eq() {
        // `pretty_assertions::assert_eq` matches `"assert_eq"` via
        // leaf-segment match. S2.3 will additionally recognise this
        // as `AssertionSource::PrettyAssertions` via recognise().
        let item = parse_test_fn("fn t() { pretty_assertions::assert_eq!(1, 1); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);

        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert_eq");
    }

    #[test]
    fn body_visitor_no_assertion_in_empty_body_produces_empty() {
        let item = parse_test_fn("fn t() {}");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.assertions.is_empty());
    }

    #[test]
    fn body_visitor_ignores_non_assertion_macros() {
        // `println!` and `vec!` are not in the v0.1 assertion set;
        // they must not produce ParsedAssertion entries.
        let item = parse_test_fn("fn t() { println!(\"hi\"); let _ = vec![1, 2]; }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.assertions.is_empty());
    }

    #[test]
    fn body_visitor_recovers_nested_assertion_in_if_branch() {
        // Visit's default recursion walks into Expr::If; the
        // assertion macro inside the arm still surfaces.
        let item = parse_test_fn("fn t() { if true { assert!(true); } }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert");
    }

    #[test]
    fn body_visitor_empty_args_macro_produces_none_raw_args() {
        let item = parse_test_fn("fn t() { panic!(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.assertions[0].name, "panic");
        assert_eq!(visitor.assertions[0].raw_args, None);
    }

    // ─── S2.3: implicit-source recognition ──────────────────────────

    #[test]
    fn body_visitor_recognises_proptest_macro() {
        let item = parse_test_fn("fn t() { proptest! { |(x in any::<u32>())| { let _ = x; } } }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Proptest]
        );
        // proptest! is not in the explicit set; assertions stays empty.
        assert!(visitor.assertions.is_empty());
    }

    #[test]
    fn body_visitor_recognises_kani_macro() {
        let item = parse_test_fn("fn t() { let x: u32 = kani::any!(); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Kani]
        );
    }

    #[test]
    fn body_visitor_recognises_insta_assert_snapshot() {
        let item = parse_test_fn("fn t() { insta::assert_snapshot!(\"rendered\"); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Insta]
        );
    }

    #[test]
    fn body_visitor_recognises_pretty_assertions_as_both() {
        // The load-bearing dual-recognition case: `pretty_assertions::assert_eq`
        // matches the leaf-segment explicit assertion branch (→
        // ParsedAssertion("assert_eq")) AND the recognise() prefix
        // branch (→ AssertionSource::PrettyAssertions). Both must
        // populate — they're two different facts the detector layer
        // consumes independently.
        let item = parse_test_fn("fn t() { pretty_assertions::assert_eq!(1, 1); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(visitor.assertions.len(), 1);
        assert_eq!(visitor.assertions[0].name, "assert_eq");
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::PrettyAssertions]
        );
    }

    #[test]
    fn body_visitor_recognises_suffix_proptest() {
        // The `*_proptest` suffix rule from recognise() — custom
        // proptest-derived macros (e.g. `my_proptest!`) project to
        // `Proptest`.
        let item = parse_test_fn("fn t() { my_proptest! { x in 0..10 } }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert_eq!(
            visitor.implicit_assertion_sources,
            vec![AssertionSource::Proptest]
        );
    }

    #[test]
    fn body_visitor_skips_non_implicit_macros() {
        // `vec!` and `println!` are neither explicit assertions nor
        // implicit-source macros. Both vecs stay empty.
        let item = parse_test_fn("fn t() { let _ = vec![1, 2]; println!(\"hi\"); }");
        let mut visitor = BodyVisitor::new();
        visitor.drive(&item.block);
        assert!(visitor.assertions.is_empty());
        assert!(visitor.implicit_assertion_sources.is_empty());
    }
}
