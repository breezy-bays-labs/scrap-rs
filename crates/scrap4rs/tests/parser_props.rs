//! Property tests for `SynTestParser` invariants (scrap-rs#12 S3.1).
//!
//! Three invariants pinned:
//! 1. `proptest_no_panic_on_parse_file_able_source` — for any
//!    `syn::parse_file`-able source string, the parser MUST return
//!    `Ok(_)` and never panic.
//! 2. `proptest_idempotent_reparse` — parsing the same source twice
//!    must produce equal `ParsedTestFile` values.
//! 3. `proptest_span_monotonicity` — for every `ParsedTest` in the
//!    result, `span.start_line <= span.end_line` AND every nested
//!    assertion span obeys the same invariant. Catches the A9 span-
//!    swap bug class in `parse_error_from_syn_error` and elsewhere.
//!
//! Strategy: hand-rolled `prop_oneof!` of canonical test-source
//! shapes (bare `#[test]`, attribute variants, nested mods, body
//! assertions, opt-outs) — biases toward parse_file-able inputs so
//! the no-panic invariant is exercised on every case rather than
//! mostly rejected. Failures shrink via proptest's
//! `prop_assert_eq!` / `prop_assert!` machinery to actionable
//! minimal cases.

use proptest::prelude::*;
use scrap_core::domain::parsed::ParsedTestFile;
use scrap_core::domain::types::FilePath;
use scrap_core::ports::parser::TestParserPort;
use scrap4rs::parser::SynTestParser;

/// Strategy producing valid (parse_file-able) Rust test-source shapes.
///
/// Hand-rolled rather than random-bytes-with-rejection because the
/// random approach has a vanishingly small probability of hitting a
/// parse-file-able shape, making the invariant essentially untested.
/// The composed grammar here covers the structural variants the
/// parser cares about: bare #[test], various attribute combos,
/// nested mods, body shapes.
fn valid_test_source_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Bare #[test] fn — the simplest shape.
        any::<u8>().prop_map(|n| format!("#[test] fn t_{n}() {{}}")),
        // #[tokio::test] async fn.
        any::<u8>().prop_map(|n| format!("#[tokio::test] async fn t_{n}() {{}}")),
        // #[rstest] fn.
        any::<u8>().prop_map(|n| format!("#[rstest] fn t_{n}() {{}}")),
        // #[test] with #[should_panic].
        any::<u8>().prop_map(|n| format!("#[test] #[should_panic] fn t_{n}() {{}}")),
        // #[test] with body assertions.
        any::<u8>()
            .prop_map(|n| { format!("#[test] fn t_{n}() {{ assert!(true); assert_eq!(1, 1); }}") }),
        // #[test] with allow opt-out.
        any::<u8>()
            .prop_map(|n| { format!("#[test] #[allow(scrap::no_asserts)] fn t_{n}() {{}}") }),
        // Nested-mod test.
        any::<u8>()
            .prop_map(|n| { format!("mod outer {{ mod inner {{ #[test] fn t_{n}() {{}} }} }}") }),
        // Multiple tests in one file.
        any::<u8>().prop_map(|n| {
            format!("#[test] fn a_{n}() {{}} #[test] fn b_{n}() {{ assert!(true); }}")
        }),
        // Source with NO tests (still parse_file-able).
        any::<u8>()
            .prop_map(|n| { format!("pub struct S_{n};\nimpl S_{n} {{ fn helper(&self) {{}} }}") }),
        // Empty source.
        Just(String::new()),
    ]
}

fn parse(source: &str) -> ParsedTestFile {
    SynTestParser::new()
        .parse_test_source(source, &FilePath::new("propsrc.rs"))
        .expect("strategy produces parse_file-able sources")
}

proptest! {
    // Pin the case count explicitly so a future proptest default
    // change can't silently weaken the invariant strength.
    // 256 matches proptest's current default (and the count CHANGELOG
    // entry / S3.1 commit body claim).
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// For every parse_file-able source the strategy produces, the
    /// parser must return Ok(_) and never panic.
    #[test]
    fn proptest_no_panic_on_parse_file_able_source(
        source in valid_test_source_strategy(),
    ) {
        let result = SynTestParser::new()
            .parse_test_source(&source, &FilePath::new("propsrc.rs"));
        prop_assert!(
            result.is_ok(),
            "strategy source must produce Ok, got {:?}; source: {source:?}",
            result.as_ref().err(),
        );
    }

    /// Parsing the same source twice yields equal `ParsedTestFile`
    /// values. This pins the parser's determinism — important for
    /// snapshot tests, baseline diffs (v0.4+), and reproducible CI.
    #[test]
    fn proptest_idempotent_reparse(source in valid_test_source_strategy()) {
        let first = parse(&source);
        let second = parse(&source);
        prop_assert_eq!(first, second);
    }

    /// For every recovered `ParsedTest`, the identity span and
    /// every assertion span must obey `start_line <= end_line`.
    /// Catches the A9 bug class where `parse_error_from_syn_error`
    /// (or `span_from_spanned`) could emit an inverted span and trip
    /// the `Span::new` debug_assert.
    #[test]
    fn proptest_span_monotonicity(source in valid_test_source_strategy()) {
        let file = parse(&source);
        for (idx, test) in file.tests.iter().enumerate() {
            prop_assert!(
                test.identity.span.start_line <= test.identity.span.end_line,
                "test[{idx}] identity span inverted: {:?}",
                test.identity.span,
            );
            for (a_idx, assertion) in test.assertions.iter().enumerate() {
                prop_assert!(
                    assertion.span.start_line <= assertion.span.end_line,
                    "test[{idx}] assertion[{a_idx}] span inverted: {:?}",
                    assertion.span,
                );
            }
        }
    }
}
