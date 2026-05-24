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

// TODO(S2.3): add the `I parse the fixture <path>` matcher when the
// fixture corpus lands (proptest_shell.rs, quickcheck_shell.rs, etc.).
// Defining it earlier would panic on missing fixtures across the
// Scenario Outline rows.

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
