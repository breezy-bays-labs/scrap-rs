//! `AssertionSource` enum + `recognise()` lookup — data-driven recognition
//! of implicit-assertion sources (cucumber-rs, proptest, quickcheck, kani,
//! trybuild, insta, `pretty_assertions`, `#[should_panic]`).
//!
//! Lands with scrap-rs#12 (folds in scrap-rs#4). The parser
//! (`scrap4rs::parser::SynTestParser`) calls `recognise()` while walking
//! each test body and populates `ParsedTest::implicit_assertion_sources`.
//! Detectors in `scrap-core::detectors/` (lands at #19/#30) read the
//! populated field and skip `zero-assertion` emission when non-empty.
//!
//! No `syn` dependency — string-keyed lookup. The parser composes the
//! path string at the adapter boundary.

use serde::{Deserialize, Serialize};

/// Frameworks whose test bodies count as having an assertion even when
/// no explicit `assert*!` macro is present.
///
/// New variants land additively as new frameworks gain adoption.
/// `#[non_exhaustive]` requires every consumer (parser, detector,
/// reporter) to pattern-match with a fallback arm.
///
/// Stored in `Vec<AssertionSource>` on `ParsedTest` — emission order
/// is the parser's natural body-walk order, useful for debugging.
/// Deliberately does NOT derive `PartialOrd + Ord` (sibling `OptOut`
/// does, because it lives in `BTreeSet`; this enum does not).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssertionSource {
    /// `proptest!` macro (also matched via `*_proptest!` suffix for
    /// derived/wrapper names) — body assertions are inside the macro
    /// block.
    #[serde(rename = "proptest")]
    Proptest,
    /// `quickcheck!` macro OR `quickcheck::quickcheck(prop_fn)`
    /// function call — failure surfaces via panic on `false` return.
    #[serde(rename = "quickcheck")]
    Quickcheck,
    /// Any path under `kani::*` — formal-verification harness;
    /// failures surface via the prover, not via assertions.
    #[serde(rename = "kani")]
    Kani,
    /// Cucumber chain terminal `.await` on `World::cucumber().run(...)`
    /// or `cucumber::Cucumber::run(...)`. Parser fabricates the
    /// synthetic key `"cucumber::run"` when it detects the chain;
    /// `recognise("cucumber::run")` returns this variant.
    #[serde(rename = "cucumber")]
    Cucumber,
    /// `trybuild::TestCases::new()` / `compile_fail` / `pass` — drop
    /// impl runs the assertions when the test fn returns.
    #[serde(rename = "trybuild")]
    Trybuild,
    /// `insta::assert_*!` family — every variant is itself an
    /// assertion; the parser recognizes the path family.
    #[serde(rename = "insta")]
    Insta,
    /// Any path under `pretty_assertions::*` — drop-in `assert!`/
    /// `assert_eq!` replacements with prettier diff output.
    #[serde(rename = "pretty_assertions")]
    PrettyAssertions,
    /// `#[should_panic]` attribute on the enclosing fn. The parser
    /// extracts this via `implicit_sources_from_attributes` (scrap4rs
    /// `parser::attributes` module, lands in scrap-rs#12 S2.4), NOT
    /// via the body-walker `recognise()` path — the attribute is on
    /// the fn signature, not in the body.
    #[serde(rename = "should_panic")]
    ShouldPanic,
}

/// Recognize an implicit-assertion source from a composed path string.
///
/// Returns `Some(AssertionSource)` when `macro_path` matches a known
/// framework idiom; returns `None` otherwise.
///
/// # Precedence (first-match-wins; tested in order)
///
/// 1. **Exact-key matches**:
///    - `"proptest"`                  → `Proptest`
///    - `"quickcheck"`                → `Quickcheck`
///    - `"quickcheck::quickcheck"`    → `Quickcheck`
///    - `"cucumber::run"`             → `Cucumber`  *(synthetic key; parser fabricates from `.await` chain)*
///    - `"cucumber::Cucumber::run"`   → `Cucumber`  *(alt form of the chain terminal)*
///
/// 2. **Prefix matches**:
///    - `"trybuild::TestCases::*"`    → `Trybuild`  *(any `*` suffix — `new`, `compile_fail`, `pass`, etc.)*
///    - `"pretty_assertions::*"`      → `PrettyAssertions`  *(any path under the namespace)*
///    - `"kani::*"`                   → `Kani`              *(any path under the namespace)*
///    - `"insta::assert_*"`           → `Insta`             *(prefix `"insta::"` AND leaf segment starts with `"assert_"`)*
///
/// 3. **Suffix matches**:
///    - `"*_proptest"`                → `Proptest`  *(custom proptest-derived macros like `my_proptest`)*
///
/// This precedence is the **single source of truth**. Downstream
/// sessions (parser body-walker; detectors) consume the contract; they
/// do not redefine precedence. If a macro matches multiple patterns,
/// the order above determines which variant emits.
///
/// `ShouldPanic` is NOT reachable through this function — it's an
/// attribute, not a macro/call path. The parser emits the variant
/// directly via `implicit_sources_from_attributes` when it sees
/// `#[should_panic]` on a test fn.
///
/// No `syn` dependency — the parser composes the path string at the
/// adapter boundary and passes a `&str` here.
#[must_use]
pub fn recognise(macro_path: &str) -> Option<AssertionSource> {
    // Band 1: exact-key matches.
    match macro_path {
        "proptest" => return Some(AssertionSource::Proptest),
        "quickcheck" | "quickcheck::quickcheck" => return Some(AssertionSource::Quickcheck),
        "cucumber::run" | "cucumber::Cucumber::run" => return Some(AssertionSource::Cucumber),
        _ => {}
    }

    // Band 2: prefix matches.
    if macro_path.starts_with("trybuild::TestCases::") {
        return Some(AssertionSource::Trybuild);
    }
    if macro_path.starts_with("pretty_assertions::") {
        return Some(AssertionSource::PrettyAssertions);
    }
    if macro_path.starts_with("kani::") {
        return Some(AssertionSource::Kani);
    }
    // insta::assert_* — prefix `"insta::"` AND leaf segment starts with `"assert_"`.
    // Express as two checks against the parsed segment list (NOT a regex):
    // the path must be under the `insta::` namespace, and the final
    // segment after the last `::` must begin with `assert_`.
    if let Some(leaf) = macro_path.strip_prefix("insta::")
        && leaf.starts_with("assert_")
    {
        return Some(AssertionSource::Insta);
    }

    // Band 3: suffix matches.
    if macro_path.ends_with("_proptest") {
        return Some(AssertionSource::Proptest);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ─── Wire-key pin: each variant round-trips via the documented snake_case key ───

    #[test]
    fn assertion_source_serializes_snake_case() {
        for (variant, wire) in [
            (AssertionSource::Proptest, "proptest"),
            (AssertionSource::Quickcheck, "quickcheck"),
            (AssertionSource::Kani, "kani"),
            (AssertionSource::Cucumber, "cucumber"),
            (AssertionSource::Trybuild, "trybuild"),
            (AssertionSource::Insta, "insta"),
            (AssertionSource::PrettyAssertions, "pretty_assertions"),
            (AssertionSource::ShouldPanic, "should_panic"),
        ] {
            let json = serde_json::to_value(variant).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.into()));
            let back: AssertionSource = serde_json::from_value(json).unwrap();
            assert_eq!(back, variant);
        }
    }

    // ─── recognise() exact-key matches (Band 1) ───

    #[test]
    fn recognise_exact_proptest() {
        assert_eq!(recognise("proptest"), Some(AssertionSource::Proptest));
    }

    #[test]
    fn recognise_exact_quickcheck_macro() {
        assert_eq!(recognise("quickcheck"), Some(AssertionSource::Quickcheck));
    }

    #[test]
    fn recognise_exact_quickcheck_fn() {
        assert_eq!(
            recognise("quickcheck::quickcheck"),
            Some(AssertionSource::Quickcheck)
        );
    }

    #[test]
    fn recognise_exact_cucumber_synthetic_key() {
        assert_eq!(recognise("cucumber::run"), Some(AssertionSource::Cucumber));
    }

    #[test]
    fn recognise_exact_cucumber_alt_form() {
        assert_eq!(
            recognise("cucumber::Cucumber::run"),
            Some(AssertionSource::Cucumber)
        );
    }

    // ─── recognise() prefix matches (Band 2) ───

    #[test]
    fn recognise_prefix_trybuild_new() {
        assert_eq!(
            recognise("trybuild::TestCases::new"),
            Some(AssertionSource::Trybuild)
        );
    }

    #[test]
    fn recognise_prefix_trybuild_compile_fail() {
        assert_eq!(
            recognise("trybuild::TestCases::compile_fail"),
            Some(AssertionSource::Trybuild)
        );
    }

    #[test]
    fn recognise_prefix_pretty_assertions_assert_eq() {
        assert_eq!(
            recognise("pretty_assertions::assert_eq"),
            Some(AssertionSource::PrettyAssertions)
        );
    }

    #[test]
    fn recognise_prefix_kani_any() {
        assert_eq!(recognise("kani::any"), Some(AssertionSource::Kani));
    }

    #[test]
    fn recognise_prefix_kani_assume() {
        assert_eq!(recognise("kani::assume"), Some(AssertionSource::Kani));
    }

    #[test]
    fn recognise_prefix_insta_assert_snapshot() {
        assert_eq!(
            recognise("insta::assert_snapshot"),
            Some(AssertionSource::Insta)
        );
    }

    #[test]
    fn recognise_prefix_insta_assert_json_snapshot() {
        assert_eq!(
            recognise("insta::assert_json_snapshot"),
            Some(AssertionSource::Insta)
        );
    }

    // ─── recognise() leaf-discrimination for insta::* ───

    #[test]
    fn recognise_insta_without_assert_prefix_is_none() {
        // `insta::Settings::new` is NOT an assertion macro; the leaf
        // discriminator (`assert_*`) rejects it.
        assert_eq!(recognise("insta::Settings::new"), None);
    }

    // ─── recognise() suffix matches (Band 3) ───

    #[test]
    fn recognise_suffix_custom_proptest() {
        assert_eq!(recognise("my_proptest"), Some(AssertionSource::Proptest));
    }

    #[test]
    fn recognise_suffix_namespaced_custom_proptest() {
        // A namespaced custom proptest macro also matches the suffix.
        assert_eq!(
            recognise("my_crate::my_proptest"),
            Some(AssertionSource::Proptest)
        );
    }

    // ─── recognise() None paths ───

    #[test]
    fn recognise_empty_string_is_none() {
        assert_eq!(recognise(""), None);
    }

    #[test]
    fn recognise_plain_assert_macros_are_none() {
        // `assert!`, `assert_eq!`, etc. are EXPLICIT assertions
        // (handled directly by the parser's assertion-macro match),
        // not implicit sources. They must NOT round-trip through
        // recognise().
        assert_eq!(recognise("assert"), None);
        assert_eq!(recognise("assert_eq"), None);
        assert_eq!(recognise("assert_ne"), None);
        assert_eq!(recognise("panic"), None);
    }

    #[test]
    fn recognise_random_unrelated_paths_are_none() {
        assert_eq!(recognise("std::println"), None);
        assert_eq!(recognise("println"), None);
        assert_eq!(recognise("vec"), None);
        assert_eq!(recognise("std::vec::Vec::new"), None);
    }

    // ─── recognise() never panics on arbitrary input ───

    proptest! {
        #[test]
        fn recognise_never_panics(s in "[\\p{Any}]{0,64}") {
            // Body of the proptest cell — recognise() must terminate
            // and return either None or Some(known variant) for any
            // string we throw at it. We don't assert on the result
            // value here; the panic-freedom IS the invariant.
            let _ = recognise(&s);
        }

        #[test]
        fn recognise_round_trips_known_proptest_suffixes(
            prefix in "[a-z][a-z_]{0,15}",
        ) {
            // Any `*_proptest` suffix recognises as Proptest.
            let path = format!("{prefix}_proptest");
            prop_assert_eq!(recognise(&path), Some(AssertionSource::Proptest));
        }
    }
}
