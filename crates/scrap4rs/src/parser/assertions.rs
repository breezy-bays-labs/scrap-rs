//! Assertion / implicit-source path stringification helpers — the
//! "what does this macro path look like as a key for `recognise()`?"
//! glue between `syn::Path` and `domain::assertion_sources::recognise`.
//!
//! Lives in the parser module tree (depends on `syn`); the domain
//! crate stays AST-pure.
//!
//! S2.2 ships `compose_macro_path_string`. S2.4 will add
//! `compose_expr_path_string` (the symmetric helper for
//! `Expr::Path` in `visit_expr_call`).

use syn::Path;

/// Stringify a `syn::Path` as a `::`-joined identifier sequence with
/// no whitespace.
///
/// **CRITICAL — do NOT use `quote!(#path).to_string()` or
/// `path.to_token_stream().to_string()`** for this purpose. Both inject
/// whitespace around `::` (`"pretty_assertions :: assert_eq"`) which
/// silently breaks the exact-string lookups in
/// `scrap_core::domain::assertion_sources::recognise()`. The
/// hand-rolled `iter().map().join("::")` shape preserved here is the
/// load-bearing convention (per scrap-rs#12 S2.2 plan revision
/// item 12).
///
/// Used by both:
/// - `visit_macro` (S2.2 + S2.3) — `mac.path` projection
/// - (S2.4) `compose_expr_path_string` analog for function calls
pub(crate) fn compose_macro_path_string(path: &Path) -> String {
    path.segments
        .iter()
        .map(|seg| seg.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_path(source: &str) -> Path {
        syn::parse_str(source).expect("path parses")
    }

    #[test]
    fn compose_single_segment_returns_leaf() {
        assert_eq!(
            compose_macro_path_string(&parse_path("assert_eq")),
            "assert_eq"
        );
    }

    #[test]
    fn compose_namespaced_joins_with_colons() {
        assert_eq!(
            compose_macro_path_string(&parse_path("pretty_assertions::assert_eq")),
            "pretty_assertions::assert_eq"
        );
    }

    #[test]
    fn compose_deeper_namespace() {
        assert_eq!(
            compose_macro_path_string(&parse_path("trybuild::TestCases::new")),
            "trybuild::TestCases::new"
        );
    }

    #[test]
    fn compose_no_whitespace_injection() {
        // The load-bearing invariant: the output is byte-equal to
        // what `recognise()` matches against, with NO spaces around
        // `::`. This test fails if a future refactor swaps in
        // `quote!`-based stringification.
        let s = compose_macro_path_string(&parse_path("insta::assert_snapshot"));
        assert!(
            !s.contains(' '),
            "no whitespace injection allowed; got {s:?}"
        );
    }
}
