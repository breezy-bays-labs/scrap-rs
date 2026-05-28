//! Step defs for `tests/features/sarif_reporter.feature` (scrap-rs#17).
//!
//! Sibling to `cucumber_steps/json_reporter.rs`. Cucumber-rs registers
//! `#[given]`/`#[when]`/`#[then]` step fns globally within the test
//! binary.
//!
//! Uses a NEUTRAL adapter name ("test-adapter") so the SARIF behavioral
//! contract stays adapter-name-agnostic (forward-compatible across
//! `scrap4rs` / `scrap4ts`). The existing `json_reporter` steps keep
//! their real adapter names (sibling scope, scrap-rs#37).

#![allow(clippy::needless_pass_by_value)] // cucumber step-fn convention

use cucumber::{given, then, when};
use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::adapters::reporters::sarif::{SarifLog, emit};
use scrap_core::domain::classification::{Actionability, Severity};
use scrap_core::domain::finding::Finding;
use scrap_core::domain::report::{FileReport, Report};
use scrap_core::domain::smell::{Smell, SmellCategory};
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

use crate::World;

/// Neutral adapter name asserted by the "driver name comes from meta"
/// scenario.
const TEST_TOOL_NAME: &str = "test-adapter";

const SCHEMA_URI: &str = "https://json.schemastore.org/sarif-2.1.0.json";

fn neutral_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: TEST_TOOL_NAME,
        language: "rust",
        tool_version: "0.1.0",
        long_version: "0.1.0 (cucumber 2026-05-27)",
        about: "sarif cucumber fixture",
        long_about: "Cucumber-step fixture AdapterMeta for the SARIF reporter scenarios.",
        after_help: "",
        extensions: &["rs"],
        tool_info_uri: "https://example.invalid/test-adapter",
        rule_help_uri: "https://example.invalid/test-adapter#rules",
        config_file_name: "test-adapter.toml",
        default_excludes: &[],
        parse_hint: "ensure --src points at a workspace with test files",
    }
}

fn report_from_world(w: &World) -> Report {
    use std::collections::BTreeMap;
    let mut by_path: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
    for finding in &w.sarif_findings {
        let key = finding
            .test
            .file_path
            .as_path()
            .to_string_lossy()
            .into_owned();
        by_path.entry(key).or_default().push(finding.clone());
    }
    let files = by_path
        .into_iter()
        .map(|(path, fs)| FileReport::new(FilePath::new(&path), fs))
        .collect();
    Report {
        files,
        ..Report::default()
    }
}

fn emit_world(w: &mut World) {
    let report = report_from_world(w);
    let meta = neutral_meta();
    let mut buf = Vec::new();
    emit(&report, &meta, &mut buf).expect("SARIF emit succeeds");
    w.sarif_output = Some(buf);
}

fn output_value(w: &World) -> serde_json::Value {
    let buf = w.sarif_output.as_ref().expect("SARIF emitted");
    serde_json::from_slice(buf).expect("SARIF is valid JSON")
}

// ── Given ────────────────────────────────────────────────────────────

#[given(regex = r"^a SARIF fixture finding with 2 smells in `(.+?)`$")]
fn fixture_two_smells(w: &mut World, path: String) {
    let test = TestIdentity::new(
        FilePath::new(&path),
        QualifiedName::new(format!("{path}::tests::it")),
        Span::new(1, 10, 1, 1),
    );
    let finding = Finding::new(
        test,
        vec![
            Smell::new(
                SmellCategory::ZeroAssertion,
                Severity::High,
                Actionability::AutoRefactor,
                10,
                None,
            ),
            Smell::new(
                SmellCategory::LargeExample,
                Severity::Low,
                Actionability::ManualSplit,
                4,
                None,
            ),
        ],
    );
    w.sarif_findings.push(finding);
}

#[given(regex = r"^a SARIF fixture finding with a high-severity smell in `(.+?)`$")]
fn fixture_high_severity(w: &mut World, path: String) {
    let test = TestIdentity::new(
        FilePath::new(&path),
        QualifiedName::new(format!("{path}::tests::it")),
        Span::new(1, 10, 1, 1),
    );
    let finding = Finding::new(
        test,
        vec![Smell::new(
            SmellCategory::ZeroAssertion,
            Severity::High,
            Actionability::AutoRefactor,
            10,
            None,
        )],
    );
    w.sarif_findings.push(finding);
}

// ── When ─────────────────────────────────────────────────────────────

#[when(regex = r"^the caller invokes SARIF `emit\(\)` with no findings$")]
fn emit_empty(w: &mut World) {
    emit_world(w);
}

#[when(regex = r"^the caller invokes SARIF `emit\(\)` over the fixture findings$")]
fn emit_fixtures(w: &mut World) {
    emit_world(w);
}

// ── Then ─────────────────────────────────────────────────────────────

#[then(regex = r"^the SARIF `version` equals `2\.1\.0`$")]
fn assert_version(w: &mut World) {
    assert_eq!(output_value(w)["version"], "2.1.0");
}

#[then(regex = r"^the SARIF `\$schema` is the sarif-2\.1\.0 schema URL$")]
fn assert_schema(w: &mut World) {
    assert_eq!(output_value(w)["$schema"], SCHEMA_URI);
}

#[then(regex = r"^the SARIF run has zero results$")]
fn assert_zero_results(w: &mut World) {
    let v = output_value(w);
    assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 0);
}

#[then(regex = r"^the SARIF `runs\[0\]\.tool\.driver\.name` equals the adapter tool name$")]
fn assert_driver_name(w: &mut World) {
    let v = output_value(w);
    assert_eq!(v["runs"][0]["tool"]["driver"]["name"], TEST_TOOL_NAME);
}

#[then(regex = r"^the SARIF run defines (\d+) rules$")]
fn assert_rule_count(w: &mut World, expected: String) {
    let v = output_value(w);
    let expected: usize = expected.parse().unwrap();
    assert_eq!(
        v["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .unwrap()
            .len(),
        expected,
    );
}

#[then(regex = r"^the SARIF run has (\d+) results$")]
fn assert_result_count(w: &mut World, expected: String) {
    let v = output_value(w);
    let expected: usize = expected.parse().unwrap();
    assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), expected);
}

#[then(regex = r"^the first SARIF result has `level` equal to `(.+?)`$")]
fn assert_first_level(w: &mut World, expected: String) {
    let v = output_value(w);
    assert_eq!(v["runs"][0]["results"][0]["level"], expected);
}

#[then(regex = r"^the emitted SARIF round-trips into a `SarifLog`$")]
fn assert_round_trip(w: &mut World) {
    let buf = w.sarif_output.as_ref().expect("SARIF emitted");
    let log: SarifLog =
        serde_json::from_slice(buf).expect("emitted SARIF parses back into SarifLog");
    assert_eq!(log.version, "2.1.0");
    assert_eq!(log.runs.len(), 1);
}
