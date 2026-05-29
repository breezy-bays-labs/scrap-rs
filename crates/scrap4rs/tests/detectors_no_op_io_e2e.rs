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

use scrap_core::cli::config::DetectorConfig;
use scrap_core::detectors::no_op_io::detect;
use scrap_core::domain::parsed::ParsedTest;
use scrap_core::domain::types::FilePath;
use scrap_core::ports::parser::TestParserPort;
use scrap4rs::parser::SynTestParser;

fn parse_fixture(rel: &str) -> Vec<ParsedTest> {
    let abs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(rel);
    let source = std::fs::read_to_string(&abs)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", abs.display()));
    SynTestParser::new()
        .parse_test_source(&source, &FilePath::new(rel))
        .unwrap_or_else(|e| panic!("fixture {rel} must parse cleanly: {e:?}"))
        .tests
}

#[test]
fn e2e_no_op_io_triggers_on_true_positive() {
    let tests = parse_fixture("tests/fixtures/true_positives/no_op_io.rs");
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
    // Collect ALL failures before asserting (per
    // `feedback_pristine-test-output`) so one bad fixture doesn't hide
    // the others from downstream agentic loops.
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

    let mut unexpected_triggers: Vec<String> = Vec::new();
    for f in fixtures {
        for parsed in parse_fixture(f) {
            if detect(&parsed, &DetectorConfig::default()).is_some() {
                unexpected_triggers.push(format!(
                    "{f}::{name}",
                    name = parsed.identity.qualified_name.as_str(),
                ));
            }
        }
    }

    assert!(
        unexpected_triggers.is_empty(),
        "no-op-io fired on runner shells that should suppress it:\n  - {}",
        unexpected_triggers.join("\n  - "),
    );
}

#[test]
fn e2e_no_op_io_does_not_trigger_on_result_asserted_fixtures() {
    // `.unwrap()`/`.expect()` chains project `ResultAsserted`, which is
    // positive-check evidence → no-op-io suppressed even though the line
    // is a `let _ = ...;` discard.
    let fixtures = [
        "tests/fixtures/behavioral_facts/unwrap_chain.rs",
        "tests/fixtures/behavioral_facts/expect_chain.rs",
        "tests/fixtures/behavioral_facts/unwrap_err_chain.rs",
        "tests/fixtures/behavioral_facts/expect_err_chain.rs",
    ];

    let mut unexpected_triggers: Vec<String> = Vec::new();
    for f in fixtures {
        for parsed in parse_fixture(f) {
            if detect(&parsed, &DetectorConfig::default()).is_some() {
                unexpected_triggers.push(format!(
                    "{f}::{name}",
                    name = parsed.identity.qualified_name.as_str(),
                ));
            }
        }
    }

    assert!(
        unexpected_triggers.is_empty(),
        "no-op-io fired on ResultAsserted fixtures that should suppress it:\n  - {}",
        unexpected_triggers.join("\n  - "),
    );
}
