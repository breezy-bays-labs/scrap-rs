//! End-to-end test of the `no-op-io` detector against real Rust source
//! files via the syn parser. Each fixture parses through
//! [`SynTestParser`]; the resulting `ParsedTest`(s) feed
//! [`scrap_core::detectors::no_op_io::detect`].
//!
//! - **Positive**: `true_positives/no_op_io.rs` MUST trigger the
//!   detector (one finding emitted) — a `let _ = <call>;` discard with
//!   no positive check.
//! - **Negative**: every fixture under `runner_shells/` MUST NOT
//!   trigger (each carries an implicit-assertion source, a `.unwrap()`
//!   chain, or — for `no_op_io_unit_binding.rs` — a type-ascribed
//!   `let _: () =` that the parser must not project as a discard).
//! - **Negative**: the `behavioral_facts/` fixtures (`.unwrap()` /
//!   `.expect()` chains) MUST NOT trigger — `ResultAsserted` suppresses.

use scrap_core::detectors::no_op_io::detect;
use scrap_core::domain::config::DetectorConfig;
use scrap_core::domain::parsed::ParsedTest;
use scrap_core::domain::types::FilePath;
use scrap_core::ports::parser::TestParserPort;
use scrap4rs::parser::SynTestParser;

/// Read + parse one fixture. Returns `Err(String)` on read OR parse
/// failure so the negative-guard loops can accumulate the failure
/// alongside spurious-trigger violations and report ALL of them in one
/// run (per `feedback_pristine-test-output`) rather than aborting on the
/// first bad fixture. The single-fixture positive test `.expect()`s the
/// `Ok`.
fn parse_fixture(rel: &str) -> Result<Vec<ParsedTest>, String> {
    let abs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let source =
        std::fs::read_to_string(&abs).map_err(|e| format!("read {}: {e}", abs.display()))?;
    SynTestParser::new()
        .parse_test_source(&source, &FilePath::new(rel))
        .map(|f| f.tests)
        .map_err(|e| format!("parse {rel}: {e:?}"))
}

/// Run the detector across every fixture and accumulate violations: a
/// `parse: <detail>` entry per read/parse failure, and a
/// `trigger: <fixture>::<test>` entry per fixture that unexpectedly
/// fires the detector. Returns the (possibly empty) violations vec.
fn collect_unexpected_triggers(fixtures: &[&str]) -> Vec<String> {
    let mut violations: Vec<String> = Vec::new();
    for &f in fixtures {
        match parse_fixture(f) {
            Err(e) => violations.push(format!("parse: {e}")),
            Ok(tests) => {
                for parsed in tests {
                    if detect(&parsed, &DetectorConfig::default()).is_some() {
                        violations.push(format!(
                            "trigger: {f}::{name}",
                            name = parsed.identity.qualified_name.as_str(),
                        ));
                    }
                }
            }
        }
    }
    violations
}

#[test]
fn e2e_no_op_io_triggers_on_true_positive() {
    let tests = parse_fixture("tests/fixtures/true_positives/no_op_io.rs")
        .expect("true-positive fixture must parse cleanly");
    assert_eq!(
        tests.len(),
        1,
        "expected one test in the true-positive fixture"
    );
    let finding = detect(&tests[0], &DetectorConfig::default());
    assert!(
        finding.is_some(),
        "true_positives/no_op_io.rs MUST trigger the no-op-io detector",
    );
}

#[test]
fn e2e_no_op_io_does_not_trigger_on_runner_shells() {
    // Accumulate-and-assert (Gemini C2–C4 / `feedback_pristine-test-output`):
    // one run reports ALL offending fixtures — parse failures (`parse:`)
    // AND spurious detector triggers (`trigger:`) — instead of aborting
    // on the first.
    let fixtures = [
        "tests/fixtures/runner_shells/cucumber_shell.rs",
        "tests/fixtures/runner_shells/insta_shell.rs",
        "tests/fixtures/runner_shells/kani_shell.rs",
        "tests/fixtures/runner_shells/pretty_assertions_shell.rs",
        "tests/fixtures/runner_shells/proptest_macro_suffix.rs",
        "tests/fixtures/runner_shells/proptest_shell.rs",
        "tests/fixtures/runner_shells/quickcheck_shell.rs",
        "tests/fixtures/runner_shells/should_panic_shell.rs",
        "tests/fixtures/runner_shells/trybuild_shell.rs",
        // The scrap-rs#25 FP guard: `let _: () = ...;` must NOT project a
        // discard, so no-op-io must NOT fire here.
        "tests/fixtures/runner_shells/no_op_io_unit_binding.rs",
    ];

    let violations = collect_unexpected_triggers(&fixtures);
    assert!(
        violations.is_empty(),
        "no-op-io guard failed on runner shells (parse failures or spurious triggers):\n  - {}",
        violations.join("\n  - "),
    );
}

#[test]
fn e2e_no_op_io_does_not_trigger_on_result_asserted_fixtures() {
    // `.unwrap()`/`.expect()` chains project `ResultAsserted`, which is
    // positive-check evidence → no-op-io suppressed even though the line
    // is a `let _ = ...;` discard. Same accumulate-and-assert pattern.
    let fixtures = [
        "tests/fixtures/behavioral_facts/unwrap_chain.rs",
        "tests/fixtures/behavioral_facts/expect_chain.rs",
        "tests/fixtures/behavioral_facts/unwrap_err_chain.rs",
        "tests/fixtures/behavioral_facts/expect_err_chain.rs",
    ];

    let violations = collect_unexpected_triggers(&fixtures);
    assert!(
        violations.is_empty(),
        "no-op-io guard failed on ResultAsserted fixtures (parse failures or spurious triggers):\n  - {}",
        violations.join("\n  - "),
    );
}
