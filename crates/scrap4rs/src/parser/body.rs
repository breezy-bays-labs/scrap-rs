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
}

impl BodyVisitor {
    pub(crate) fn new() -> Self {
        Self {
            assertions: Vec::new(),
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
        // `PrettyAssertions` source by S2.3's recognise() branch.
        if let Some(leaf) = path.rsplit("::").next()
            && ASSERTION_MACRO_NAMES.contains(&leaf)
        {
            let raw_args = stringify_tokens_opt(&mac.tokens);
            let span = span_from_spanned(mac);
            self.assertions
                .push(ParsedAssertion::new(leaf, raw_args, span));
        }

        // S2.3 will extend this fn with the implicit-source
        // `recognise(&path)` branch after the assertion-name match
        // block. No visit::visit_macro recursion — see v0.1 boundary
        // note above.
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
}
