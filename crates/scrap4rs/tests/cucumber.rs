//! Cucumber-rs harness for `crates/scrap4rs/tests/features/parser.feature`.
//!
//! Mirrors `crates/scrap-core/tests/cucumber.rs` shape: `harness = false`
//! in `Cargo.toml`, async tokio current-thread runtime, World struct
//! carries per-scenario parser + result state. Step matchers follow
//! the Reusable Reference convention from impl-plan: exactly two
//! `When` matchers (`I parse the source: <docstring>` and
//! `I parse the fixture <path>`).
//!
//! S1.1 ships the harness scaffold + scenario 1 only ("Empty source
//! compiles to an empty test inventory"). Wave 2 sessions (S2.1 →
//! S2.4) extend the step impls as each .feature scenario becomes
//! reachable. Scenarios 2-9 will surface as "step undefined" at S1.1;
//! cucumber 0.23 reports them but does NOT fail the binary (exit 0).
//!
//! Step matchers use `regex = r"..."` mode rather than Cucumber
//! Expressions because the .feature scenarios embed brackets, quotes,
//! and backticks that the Expression parser would treat as special
//! tokens.

// Cucumber-rs step functions naturally take `String` params (the
// regex capture API yields owned data) and use `match { _ => panic!() }`
// blocks on `#[non_exhaustive]` enums where `let-else` would also
// suffice. Suppress the pedantic nits at file level for parity with
// the scrap-core file-walker harness (`crates/scrap-core/tests/cucumber.rs`),
// which uses the same allowlist + tracking edge.
//
// tracked: scrap-rs#50 — lift after parser PR; surfaced when workspace
// [lints] extended clippy::pedantic to integration tests. Same
// follow-up cleanup that owns the scrap-core/tests/cucumber.rs lift.
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::manual_let_else)]

use cucumber::{World as _, given, then, when};
use scrap_core::domain::parsed::ParsedTestFile;
use scrap_core::domain::types::FilePath;
use scrap_core::ports::parser::{ParseError, TestParserPort};
use scrap4rs::parser::SynTestParser;

// ─── World ─────────────────────────────────────────────────────────

/// Per-scenario state. Cucumber-rs constructs a fresh `World` for
/// each scenario via `Default`; the parser is seeded by the `Given`
/// step and the result is captured by the `When` step.
#[derive(Debug, cucumber::World, Default)]
struct World {
    parser: Option<SynTestParser>,
    result: Option<Result<ParsedTestFile, ParseError>>,
}

// ─── Given ─────────────────────────────────────────────────────────

#[given(regex = r"^a SynTestParser$")]
fn given_a_syn_test_parser(w: &mut World) {
    w.parser = Some(SynTestParser::new());
}

// ─── When ─────────────────────────────────────────────────────────
//
// Per the Reusable Reference: two `When` matchers ONLY in the final
// state. S1.1 ships ONLY the `I parse the source:` matcher (scenario
// 1 needs it); the `I parse the fixture <path>` matcher lands in
// S2.3 when the fixture files materialize. Until then, scenarios
// using the fixture matcher report "step undefined" and cucumber
// 0.23 exits 0 on those (no false-positive failures from missing
// fixtures during Wave 2 development).

#[when(regex = r"^I parse the source:$")]
fn when_i_parse_the_source(w: &mut World, step: &cucumber::gherkin::Step) {
    let parser = w.parser.as_ref().expect("Given step seeds the parser");
    // The docstring body is the inline source. cucumber-rs makes it
    // available via `step.docstring()`. Empty / absent docstring
    // collapses to empty source — exactly what scenario 1 needs.
    let source = step.docstring().cloned().unwrap_or_default();
    w.result = Some(parser.parse_test_source(&source, &FilePath::new("scenario.rs")));
}

// S2.3 — the deferred `When I parse the fixture <path>` matcher
// lands now that the runner-shell fixture corpus exists. Regex
// captures the path; file is read crate-relative via
// `CARGO_MANIFEST_DIR`. Per the Reusable Reference convention this
// is the SECOND and FINAL `When` matcher — Wave 2+ sessions extend
// the .feature rows, not the matcher surface.
#[when(regex = r"^I parse the fixture (.+)$")]
fn when_i_parse_the_fixture(w: &mut World, fixture_path: String) {
    let parser = w.parser.as_ref().expect("Given step seeds the parser");
    let abs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(fixture_path.trim());
    let source = std::fs::read_to_string(&abs)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", abs.display()));
    w.result = Some(parser.parse_test_source(&source, &FilePath::new(abs)));
}

// ─── Then (scenario 1 only — Wave 2 extends) ────────────────────────

#[then(regex = r"^parsing succeeds$")]
fn then_parsing_succeeds(w: &mut World) {
    let result = w.result.as_ref().expect("When step records the result");
    assert!(
        result.is_ok(),
        "expected Ok(ParsedTestFile), got {:?}",
        result.as_ref().err(),
    );
}

#[then(regex = r"^the parsed file contains (\d+) tests?$")]
fn then_parsed_file_contains_n_tests(w: &mut World, expected: usize) {
    let file = w
        .result
        .as_ref()
        .expect("When step recorded a result")
        .as_ref()
        .expect("parsing succeeded (Then 'parsing succeeds' is implicit prerequisite)");
    assert_eq!(file.tests.len(), expected, "tests count mismatch");
}

#[then(regex = r"^the parsed file contains (\d+) diagnostics$")]
fn then_parsed_file_contains_n_diagnostics(w: &mut World, expected: usize) {
    let file = w
        .result
        .as_ref()
        .expect("When step recorded a result")
        .as_ref()
        .expect("parsing succeeded");
    assert_eq!(
        file.diagnostics.len(),
        expected,
        "diagnostics count mismatch"
    );
}

#[then(regex = r"^parsing fails with a ParseError::Syntax$")]
fn then_parsing_fails_with_syntax(w: &mut World) {
    let result = w.result.as_ref().expect("When step recorded a result");
    let err = result.as_ref().expect_err("expected an Err, got Ok");
    match err {
        ParseError::Syntax { .. } => {}
        // `#[non_exhaustive]` future-compat: this branch may surface
        // if a future variant of ParseError lands (e.g. a TS-adapter
        // module resolution failure). For v0.1 / scrap4rs there's
        // only Syntax, so any other variant signals a bug.
        other => panic!("expected ParseError::Syntax, got {other:?}"),
    }
}

// ─── Per-test inspection helpers (S2.1+ scenarios) ──────────────────

/// Find the `ParsedTest` whose `qualified_name` matches `name`.
/// Panics with a list of available names if not found, so cucumber
/// failure messages stay actionable.
fn find_test<'a>(
    file: &'a scrap_core::domain::parsed::ParsedTestFile,
    name: &str,
) -> &'a scrap_core::domain::parsed::ParsedTest {
    file.tests
        .iter()
        .find(|t| t.identity.qualified_name.as_str() == name)
        .unwrap_or_else(|| {
            let available: Vec<&str> = file
                .tests
                .iter()
                .map(|t| t.identity.qualified_name.as_str())
                .collect();
            panic!("no test named {name:?}; have {available:?}")
        })
}

/// Wrapper for the success path that unwraps `w.result` into the
/// parsed file, with an actionable panic on Err.
fn assert_ok(w: &World) -> &scrap_core::domain::parsed::ParsedTestFile {
    w.result
        .as_ref()
        .expect("When step recorded a result")
        .as_ref()
        .expect("parsing succeeded (Then 'parsing succeeds' is implicit prerequisite)")
}

#[then(regex = r#"^test "([^"]+)" exists$"#)]
fn then_test_exists(w: &mut World, name: String) {
    let _ = find_test(assert_ok(w), &name);
}

#[then(regex = r#"^test "([^"]+)" has the attribute "([^"]+)"$"#)]
fn then_test_has_attribute(w: &mut World, test_name: String, attr_name: String) {
    let test = find_test(assert_ok(w), &test_name);
    let names: Vec<&str> = test.attributes.iter().map(|a| a.name.as_str()).collect();
    // Leaf-segment convention (pinned in S0.1): tokio::test → "test",
    // so the scenario's `#[tokio::test]` row expects "test" not
    // "tokio::test". But the Examples table in parser.feature uses
    // the full path string ("tokio::test") because that's what's
    // immediately readable. We accept either: leaf match OR full path
    // match against the user-provided attr.
    let attr_leaf = attr_name.rsplit("::").next().unwrap_or(&attr_name);
    assert!(
        names.contains(&attr_leaf),
        "test {test_name:?} attributes {names:?} missing {attr_name:?} \
         (leaf segment {attr_leaf:?})",
    );
}

#[then(regex = r#"^test "([^"]+)" has body_line_count (\d+)$"#)]
fn then_test_has_body_line_count(w: &mut World, name: String, expected: u32) {
    let test = find_test(assert_ok(w), &name);
    assert_eq!(
        test.body_line_count, expected,
        "test {name:?} body_line_count mismatch",
    );
}

#[then(regex = r#"^test "([^"]+)" has the opt-out (\w+)$"#)]
fn then_test_has_opt_out(w: &mut World, test_name: String, opt_out_name: String) {
    let test = find_test(assert_ok(w), &test_name);
    let opt_out = match opt_out_name.as_str() {
        "NoAsserts" => scrap_core::domain::opt_outs::OptOut::NoAsserts,
        "Tautology" => scrap_core::domain::opt_outs::OptOut::Tautology,
        "NoOp" => scrap_core::domain::opt_outs::OptOut::NoOp,
        _ => panic!("unknown OptOut variant in scenario: {opt_out_name:?}"),
    };
    assert!(
        test.opt_outs.contains(&opt_out),
        "test {test_name:?} opt_outs {:?} missing {opt_out:?}",
        test.opt_outs,
    );
}

#[then(regex = r#"^test "([^"]+)" has (\d+) opt-outs?$"#)]
fn then_test_has_n_opt_outs(w: &mut World, name: String, expected: usize) {
    let test = find_test(assert_ok(w), &name);
    assert_eq!(
        test.opt_outs.len(),
        expected,
        "test {name:?} opt-out count mismatch"
    );
}

// ─── S2.2 — explicit assertion + implicit-source counts ─────────────

#[then(regex = r#"^test "([^"]+)" has (\d+) explicit assertions?$"#)]
fn then_test_has_n_explicit_assertions(w: &mut World, name: String, expected: usize) {
    let test = find_test(assert_ok(w), &name);
    assert_eq!(
        test.assertions.len(),
        expected,
        "test {name:?} explicit assertion count mismatch",
    );
}

#[then(regex = r#"^test "([^"]+)" assertion (\d+) has name "([^"]+)"$"#)]
fn then_test_assertion_has_name(
    w: &mut World,
    test_name: String,
    index: usize,
    expected_name: String,
) {
    let test = find_test(assert_ok(w), &test_name);
    let assertion = test.assertions.get(index).unwrap_or_else(|| {
        panic!(
            "test {test_name:?} has only {} assertions; index {index} OOB",
            test.assertions.len()
        )
    });
    assert_eq!(
        assertion.name, expected_name,
        "test {test_name:?} assertion {index} name mismatch",
    );
}

#[then(regex = r#"^test "([^"]+)" has (\d+) implicit assertion sources?$"#)]
fn then_test_has_n_implicit_assertion_sources(w: &mut World, name: String, expected: usize) {
    let test = find_test(assert_ok(w), &name);
    assert_eq!(
        test.implicit_assertion_sources.len(),
        expected,
        "test {name:?} implicit_assertion_sources count mismatch",
    );
}

#[then(regex = r#"^test "([^"]+)" has the implicit assertion source (\w+)$"#)]
fn then_test_has_implicit_assertion_source(w: &mut World, test_name: String, variant_name: String) {
    use scrap_core::domain::assertion_sources::AssertionSource;
    let test = find_test(assert_ok(w), &test_name);
    let expected = match variant_name.as_str() {
        "Proptest" => AssertionSource::Proptest,
        "Quickcheck" => AssertionSource::Quickcheck,
        "Kani" => AssertionSource::Kani,
        "Cucumber" => AssertionSource::Cucumber,
        "Trybuild" => AssertionSource::Trybuild,
        "Insta" => AssertionSource::Insta,
        "PrettyAssertions" => AssertionSource::PrettyAssertions,
        "ShouldPanic" => AssertionSource::ShouldPanic,
        _ => panic!("unknown AssertionSource variant in scenario: {variant_name:?}"),
    };
    assert!(
        test.implicit_assertion_sources.contains(&expected),
        "test {test_name:?} implicit sources {:?} missing {expected:?}",
        test.implicit_assertion_sources,
    );
}

// ─── Main ─────────────────────────────────────────────────────────

#[tokio::main(flavor = "current_thread")]
async fn main() {
    // `filter_run_and_exit` ensures the exit code propagates correctly
    // (per the cucumber-rs standards/testing rule that lands with the
    // file-walker pipeline).
    //
    // S1.1 filter: skip scenarios tagged `@wip` — those need Wave 2
    // session support that hasn't landed yet. Each Wave 2 session
    // (S2.1 → S2.4) removes the `@wip` tag from the scenarios it
    // unlocks; the rest stay skipped. This keeps CI logs clean
    // during Wave 2 instead of showing N failing scenarios while the
    // walker grows incrementally. The `@wip` tag is removed entirely
    // (per scenario) as each session lands.
    World::cucumber()
        .filter_run_and_exit("tests/features", |_, _, sc| {
            !sc.tags.iter().any(|t| t == "wip")
        })
        .await;
}
