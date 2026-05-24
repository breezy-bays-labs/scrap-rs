//! Attribute recognition helpers — convert `syn::Attribute` lists
//! into domain `ParsedAttribute` / `OptOut` / `AssertionSource`
//! projections.
//!
//! Lives in the parser module tree (depends on `syn`); the domain
//! crate stays AST-pure.
//!
//! S2.1 ships: `is_test_fn`, `extract_attributes`,
//! `parsed_attribute_from_syn`, `extract_opt_outs`, `match_opt_out_key`.
//! S2.4 adds `implicit_sources_from_attributes` for the
//! `#[should_panic]` → `AssertionSource::ShouldPanic` projection.

use proc_macro2::TokenStream;
use scrap_core::domain::assertion_sources::AssertionSource;
use scrap_core::domain::opt_outs::OptOut;
use scrap_core::domain::parsed::ParsedAttribute;
use std::collections::BTreeSet;
use syn::{Attribute, ItemFn, Meta};

/// Set of attribute leaf-segment names that mark a fn as a `#[test]`.
/// Match order: leaf segment of `attr.path()` against this slice.
/// `#[test]`, `#[tokio::test]`, `#[rstest]` all share the leaf
/// segment `"test"` for the first two and `"rstest"` for the third.
const TEST_ATTR_LEAF_NAMES: &[&str] = &["test", "rstest"];

/// Set of attribute leaf-segment names recognised when projecting a
/// test fn's attributes into the `ParsedAttribute` vec.
///
/// This is the v0.1 whitelist (per scrap-rs#12 AC R1): bare `#[test]`,
/// `#[tokio::test]`, `#[rstest]`, `#[should_panic]`, `#[ignore]`.
/// Other attributes (e.g. `#[allow(...)]`, `#[doc(...)]`) are NOT
/// projected here — opt-outs go through `extract_opt_outs` instead;
/// general `#[allow(...)]` is purely a compiler hint and out of scope.
const PROJECTED_ATTR_LEAF_NAMES: &[&str] = &["test", "rstest", "should_panic", "ignore"];

/// True iff one of the attributes marks the fn as a `#[test]`-like.
///
/// Matches on the leaf segment of `attr.path()` (last segment after
/// any `::` qualifiers), so `#[tokio::test]` and bare `#[test]` both
/// match `"test"`. `#[rstest]` matches `"rstest"`. Anything else
/// returns `false`.
pub(crate) fn is_test_fn(attrs: &[Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().segments.last().is_some_and(|seg| {
            let ident = seg.ident.to_string();
            TEST_ATTR_LEAF_NAMES.contains(&ident.as_str())
        })
    })
}

/// Project an `ItemFn`'s attributes into the `ParsedAttribute` vec
/// the domain expects on `ParsedTest::attributes`.
///
/// Filters by the v0.1 whitelist (see `PROJECTED_ATTR_LEAF_NAMES`);
/// each surviving attribute is mapped via `parsed_attribute_from_syn`.
/// Order preserved from the source.
pub(crate) fn extract_attributes(item: &ItemFn) -> Vec<ParsedAttribute> {
    item.attrs
        .iter()
        .filter(|attr| {
            attr.path().segments.last().is_some_and(|seg| {
                let ident = seg.ident.to_string();
                PROJECTED_ATTR_LEAF_NAMES.contains(&ident.as_str())
            })
        })
        .map(parsed_attribute_from_syn)
        .collect()
}

/// Convert one `syn::Attribute` into a `ParsedAttribute { name, raw }`.
///
/// - `name` = leaf segment of `attr.path()` (e.g. `"test"` for both
///   `#[test]` and `#[tokio::test]`), per the leaf-segment convention
///   pinned in S0.1 for `ParsedAssertion::name` consistency.
/// - `raw` = the argument text as a string:
///   - `Meta::Path` (bare attribute, e.g. `#[test]`) → `None`
///   - `Meta::List(list)` (e.g. `#[ignore(slow)]` or `#[allow(scrap::tautology, scrap::no_op)]`) → `Some(list.tokens.to_string())`
///   - `Meta::NameValue(nv)` (e.g. `#[ignore = "flaky"]`) → `Some(nv.value.to_token_stream().to_string())`
///
/// `TokenStream::to_string()` is acceptable HERE (raw arg text) because
/// `raw` is documented to preserve verbatim source bytes; any
/// whitespace injection in the proc-macro2 stringification is
/// load-bearing fidelity, not a bug. (Contrast with `recognise()`
/// lookup paths where whitespace injection silently breaks the
/// exact-key match — that's why `compose_macro_path_string` /
/// `compose_expr_path_string` hand-roll the path join.)
pub(crate) fn parsed_attribute_from_syn(attr: &Attribute) -> ParsedAttribute {
    use quote::ToTokens;

    let name = attr
        .path()
        .segments
        .last()
        .map(|seg| seg.ident.to_string())
        .unwrap_or_default();

    let raw = match &attr.meta {
        Meta::Path(_) => None,
        Meta::List(list) => Some(stringify_tokens(&list.tokens)),
        Meta::NameValue(nv) => Some(nv.value.to_token_stream().to_string()),
    };

    ParsedAttribute::new(name, raw)
}

/// Stringify a token stream for the `raw` field of `ParsedAttribute`.
///
/// Pure passthrough wrapper around `TokenStream::to_string()`. Exists
/// as a single helper so the contrast with `compose_macro_path_string`
/// (which deliberately hand-rolls the join to avoid whitespace
/// injection) is explicit in the call graph.
fn stringify_tokens(tokens: &TokenStream) -> String {
    tokens.to_string()
}

/// Scan a fn's attributes for `#[allow(scrap::*)]` markers and project
/// them into the domain `OptOut` `BTreeSet`.
///
/// Walks `#[allow(...)]` attribute lists, parses each inner `Meta`
/// item as a path, and tries `match_opt_out_key` on the composed
/// path string. Unmatched paths (e.g. `#[allow(dead_code)]`) are
/// silently ignored — that's a compiler hint, not an opt-out.
///
/// `BTreeSet` keeps insertion order deterministic across runs and
/// across fixture authoring orders; `OptOut` derives `Ord` so the
/// final serialized order is the canonical variant declaration order.
pub(crate) fn extract_opt_outs(item: &ItemFn) -> BTreeSet<OptOut> {
    let mut opt_outs = BTreeSet::new();

    for attr in &item.attrs {
        // Only inspect `#[allow(...)]` — opt-outs ride that channel
        // because the compiler treats them as no-op lints (the scrap
        // detectors are not registered as Rust lints, just shapes the
        // parser projects).
        if !attr.path().is_ident("allow") {
            continue;
        }

        // `#[allow(scrap::no_asserts, scrap::tautology)]` parses as a
        // Meta::List whose `parse_args_with` interprets the tokens as
        // a comma-separated list of `syn::Path` items.
        let Ok(paths) = attr.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        ) else {
            // Malformed `#[allow(...)]` content — skip. The Rust
            // compiler would have caught this on the user's side
            // already (the source has to be `syn::parse_file`-able
            // for us to reach this branch in the first place).
            continue;
        };

        for path in paths {
            let path_str = path
                .segments
                .iter()
                .map(|seg| seg.ident.to_string())
                .collect::<Vec<_>>()
                .join("::");
            if let Some(opt_out) = match_opt_out_key(&path_str) {
                opt_outs.insert(opt_out);
            }
        }
    }

    opt_outs
}

/// S2.4 — N24 — implicit-assertion sources sourced from the fn's
/// attribute list (not its body).
///
/// At v0.1 the only attribute-sourced `AssertionSource` is `ShouldPanic`
/// (from `#[should_panic]` on the test fn). The function is shaped
/// for additive extension: if v0.3+ adds a new attribute-sourced
/// variant (e.g. a hypothetical `#[no_fail]`), it lands here.
///
/// Called by `extract_parsed_test` alongside `extract_attributes` and
/// `extract_opt_outs`; the result merges into the body-walker's S4
/// collection before `ParsedTest::new`.
pub(crate) fn implicit_sources_from_attributes(item: &ItemFn) -> Vec<AssertionSource> {
    let mut sources = Vec::new();
    for attr in &item.attrs {
        if attr.path().is_ident("should_panic") {
            sources.push(AssertionSource::ShouldPanic);
        }
    }
    sources
}

/// Match a composed `scrap::*` path string against the v0.1 `OptOut`
/// variants.
///
/// Exact-string match (no prefix/suffix logic — the opt-out namespace
/// is closed at v0.1 to three keys). Returns `None` for unrelated
/// paths (e.g. `dead_code`, `clippy::pedantic`).
pub(crate) fn match_opt_out_key(path_str: &str) -> Option<OptOut> {
    match path_str {
        "scrap::no_asserts" => Some(OptOut::NoAsserts),
        "scrap::tautology" => Some(OptOut::Tautology),
        "scrap::no_op" => Some(OptOut::NoOp),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_first_fn(source: &str) -> ItemFn {
        let file: syn::File = syn::parse_str(source).expect("test source parses");
        file.items
            .into_iter()
            .find_map(|item| match item {
                syn::Item::Fn(f) => Some(f),
                _ => None,
            })
            .expect("fixture contains at least one fn")
    }

    // ─── is_test_fn ───

    #[test]
    fn is_test_fn_matches_bare_test() {
        let item = parse_first_fn("#[test] fn it() {}");
        assert!(is_test_fn(&item.attrs));
    }

    #[test]
    fn is_test_fn_matches_tokio_test_via_leaf_segment() {
        let item = parse_first_fn("#[tokio::test] async fn it() {}");
        assert!(is_test_fn(&item.attrs));
    }

    #[test]
    fn is_test_fn_matches_rstest() {
        let item = parse_first_fn("#[rstest] fn it() {}");
        assert!(is_test_fn(&item.attrs));
    }

    #[test]
    fn is_test_fn_rejects_unrelated_attributes() {
        let item = parse_first_fn("#[derive(Debug)] fn it() {}");
        assert!(!is_test_fn(&item.attrs));
    }

    #[test]
    fn is_test_fn_rejects_no_attributes() {
        let item = parse_first_fn("fn it() {}");
        assert!(!is_test_fn(&item.attrs));
    }

    // ─── extract_attributes ───

    #[test]
    fn extract_attributes_filters_to_whitelist() {
        let item = parse_first_fn(
            "#[test] #[should_panic] #[allow(dead_code)] #[doc = \"hello\"] fn it() {}",
        );
        let attrs = extract_attributes(&item);
        let names: Vec<&str> = attrs.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["test", "should_panic"]);
    }

    #[test]
    fn extract_attributes_preserves_source_order() {
        let item = parse_first_fn("#[should_panic] #[test] #[ignore] fn it() {}");
        let attrs = extract_attributes(&item);
        let names: Vec<&str> = attrs.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, vec!["should_panic", "test", "ignore"]);
    }

    // ─── parsed_attribute_from_syn ───

    #[test]
    fn parsed_attribute_bare_has_no_raw() {
        let item = parse_first_fn("#[test] fn it() {}");
        let pa = parsed_attribute_from_syn(&item.attrs[0]);
        assert_eq!(pa.name, "test");
        assert_eq!(pa.raw, None);
    }

    #[test]
    fn parsed_attribute_name_value_has_quoted_raw() {
        let item = parse_first_fn("#[ignore = \"flaky\"] fn it() {}");
        let pa = parsed_attribute_from_syn(&item.attrs[0]);
        assert_eq!(pa.name, "ignore");
        // The NameValue arm strips the `=`, captures the value
        // expression's token stream — for a string literal that's
        // the literal itself including quotes.
        assert_eq!(pa.raw, Some("\"flaky\"".to_string()));
    }

    #[test]
    fn parsed_attribute_list_captures_inner_tokens() {
        let item = parse_first_fn("#[ignore(slow)] fn it() {}");
        let pa = parsed_attribute_from_syn(&item.attrs[0]);
        assert_eq!(pa.name, "ignore");
        // Inner tokens stringify to whatever proc-macro2's
        // TokenStream::Display produces. Whitespace fidelity is
        // documented as load-bearing for `raw`.
        assert_eq!(pa.raw, Some("slow".to_string()));
    }

    #[test]
    fn parsed_attribute_qualified_path_uses_leaf_segment() {
        let item = parse_first_fn("#[tokio::test] async fn it() {}");
        let pa = parsed_attribute_from_syn(&item.attrs[0]);
        // Leaf-segment convention: `tokio::test` → `name == "test"`.
        assert_eq!(pa.name, "test");
    }

    // ─── extract_opt_outs ───

    #[test]
    fn extract_opt_outs_recovers_single_key() {
        let item = parse_first_fn("#[allow(scrap::no_asserts)] fn it() {}");
        let outs = extract_opt_outs(&item);
        assert_eq!(outs.len(), 1);
        assert!(outs.contains(&OptOut::NoAsserts));
    }

    #[test]
    fn extract_opt_outs_recovers_multi_key_allow() {
        let item = parse_first_fn("#[allow(scrap::tautology, scrap::no_op)] fn it() {}");
        let outs = extract_opt_outs(&item);
        assert_eq!(outs.len(), 2);
        assert!(outs.contains(&OptOut::Tautology));
        assert!(outs.contains(&OptOut::NoOp));
    }

    #[test]
    fn extract_opt_outs_recovers_keys_from_separate_allow_attrs() {
        let item = parse_first_fn("#[allow(scrap::no_asserts)] #[allow(scrap::no_op)] fn it() {}");
        let outs = extract_opt_outs(&item);
        assert_eq!(outs.len(), 2);
        assert!(outs.contains(&OptOut::NoAsserts));
        assert!(outs.contains(&OptOut::NoOp));
    }

    #[test]
    fn extract_opt_outs_ignores_unrelated_allow() {
        let item = parse_first_fn("#[allow(dead_code)] fn it() {}");
        let outs = extract_opt_outs(&item);
        assert!(outs.is_empty());
    }

    #[test]
    fn extract_opt_outs_ignores_non_allow_attributes() {
        let item = parse_first_fn("#[test] fn it() {}");
        let outs = extract_opt_outs(&item);
        assert!(outs.is_empty());
    }

    #[test]
    fn extract_opt_outs_btreeset_ordering_is_canonical() {
        // Insertion order is reversed in source vs declaration order;
        // BTreeSet imposes the canonical Ord (NoAsserts < Tautology < NoOp).
        let item = parse_first_fn(
            "#[allow(scrap::no_op, scrap::tautology, scrap::no_asserts)] fn it() {}",
        );
        let outs: Vec<OptOut> = extract_opt_outs(&item).into_iter().collect();
        assert_eq!(
            outs,
            vec![OptOut::NoAsserts, OptOut::Tautology, OptOut::NoOp]
        );
    }

    // ─── match_opt_out_key ───

    #[test]
    fn match_opt_out_key_recognises_all_three_v01_keys() {
        assert_eq!(
            match_opt_out_key("scrap::no_asserts"),
            Some(OptOut::NoAsserts)
        );
        assert_eq!(
            match_opt_out_key("scrap::tautology"),
            Some(OptOut::Tautology)
        );
        assert_eq!(match_opt_out_key("scrap::no_op"), Some(OptOut::NoOp));
    }

    #[test]
    fn match_opt_out_key_rejects_unrelated() {
        assert_eq!(match_opt_out_key("dead_code"), None);
        assert_eq!(match_opt_out_key("clippy::pedantic"), None);
        assert_eq!(match_opt_out_key("scrap::not_a_real_key"), None);
        assert_eq!(match_opt_out_key(""), None);
    }

    // ─── implicit_sources_from_attributes (S2.4 / N24) ───

    #[test]
    fn implicit_sources_from_attributes_recognises_should_panic() {
        let item = parse_first_fn("#[test] #[should_panic] fn it() {}");
        let sources = implicit_sources_from_attributes(&item);
        assert_eq!(sources, vec![AssertionSource::ShouldPanic]);
    }

    #[test]
    fn implicit_sources_from_attributes_ignores_other_attributes() {
        let item = parse_first_fn("#[test] #[allow(dead_code)] #[ignore] fn it() {}");
        let sources = implicit_sources_from_attributes(&item);
        assert!(sources.is_empty());
    }

    #[test]
    fn implicit_sources_from_attributes_empty_when_no_attrs() {
        let item = parse_first_fn("fn it() {}");
        let sources = implicit_sources_from_attributes(&item);
        assert!(sources.is_empty());
    }
}
