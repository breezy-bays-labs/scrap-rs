//! Step defs for `tests/features/json_reporter.feature` (scrap-rs#14).
//!
//! Sibling to `cucumber_steps/config.rs`. Cucumber-rs registers
//! `#[given]`/`#[when]`/`#[then]` step fns globally within the test
//! binary; mod-block split per scrap-rs#18 W5.1 SHOULD-FIX #5.
//!
//! These fixtures use NEUTRAL adapter names (`adapter-a` / `adapter-b`)
//! rather than the real `scrap4rs` / `scrap4ts` names. Per scrap-rs#37
//! the adapter-name literal purity CI gate scans `crates/scrap-core/`
//! source AND tests: scrap-core is adapter-name-pure, so its test
//! fixtures must be too. A neutral fixture is also COVERAGE-SUPERIOR
//! here — `emit()` threads `meta.tool_name` into the wire WITHOUT
//! branching on its value, so a fixture asserting `tool == "scrap4rs"`
//! would still PASS if `emit` hardcoded the string; a neutral
//! `tool == "adapter-a"` assertion catches that name-hardcoding bug.
//! Real-name emission verification lives in the adapter crate that
//! owns the identity (`crates/scrap4rs/tests/wire_real_name.rs`).

#![allow(clippy::needless_pass_by_value)] // cucumber step-fn convention

use cucumber::{given, then, when};
use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::adapters::reporters::json::{EmitOptions, emit};
use scrap_core::domain::classification::{Actionability, Severity};
use scrap_core::domain::finding::Finding;
use scrap_core::domain::report::{FileReport, Report};
use scrap_core::domain::smell::{Smell, SmellCategory};
use scrap_core::domain::threshold::ThresholdMode;
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
use std::num::NonZeroUsize;

use crate::World;

const FIXED_TIMESTAMP: &str = "2026-05-26T00:00:00Z";

fn build_finding(path: &str, name: &str, penalty: u32) -> Finding {
    let test = TestIdentity::new(
        FilePath::new(path),
        QualifiedName::new(name),
        Span::new(1, 5, 1, 1),
    );
    if penalty == 0 {
        Finding::new(test, vec![])
    } else {
        Finding::new(
            test,
            vec![Smell::new(
                SmellCategory::ZeroAssertion,
                Severity::High,
                Actionability::AutoRefactor,
                penalty,
                None,
            )],
        )
    }
}

fn default_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: "adapter-a",
        language: "rust",
        tool_version: "0.1.0",
        long_version: "0.1.0 (test 2026-05-27)",
        about: "adapter-a (cucumber-test fixture)",
        long_about: "Cucumber-step fixture AdapterMeta for the json reporter scenarios.",
        after_help: "",
        extensions: &["rs"],
        tool_info_uri: "https://example.invalid/scrap",
        rule_help_uri: "https://example.invalid/scrap/rules",
        config_file_name: "adapter-a.toml",
        default_excludes: &["tests/**", "benches/**", "examples/**"],
        parse_hint: "ensure --src points at a Cargo workspace with test files",
    }
}

fn alt_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: "adapter-b",
        language: "typescript",
        tool_version: "0.1.0",
        long_version: "0.1.0 (test 2026-05-27)",
        about: "adapter-b (cucumber-test fixture)",
        long_about: "Cucumber-step fixture AdapterMeta for the json reporter scenarios.",
        after_help: "",
        extensions: &["ts", "tsx"],
        tool_info_uri: "https://example.invalid/scrap",
        rule_help_uri: "https://example.invalid/scrap/rules",
        config_file_name: "adapter-b.toml",
        default_excludes: &["node_modules/**", "dist/**"],
        parse_hint: "ensure --src points at a TypeScript project with test files",
    }
}

/// Group accumulated `World.envelope_findings` into a `Report`
/// (one `FileReport` per distinct `file_path`).
fn report_from_world(w: &World) -> Report {
    use std::collections::BTreeMap;
    let mut by_path: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
    for finding in &w.envelope_findings {
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

// ── Given ────────────────────────────────────────────────────────────

#[given(regex = r"^a fixture Report with one finding scoring (\d+) in `(.+?)`$")]
fn fixture_finding(w: &mut World, penalty_str: String, path: String) {
    let penalty: u32 = penalty_str.parse().expect("penalty parses as u32");
    let finding = build_finding(&path, &format!("{path}::tests::t1"), penalty);
    w.envelope_findings.push(finding);
}

#[given(regex = r"^an additional finding scoring (\d+) in `(.+?)`$")]
fn additional_finding(w: &mut World, penalty_str: String, path: String) {
    let penalty: u32 = penalty_str.parse().expect("penalty parses as u32");
    let finding = build_finding(&path, &format!("{path}::tests::t_extra"), penalty);
    w.envelope_findings.push(finding);
}

#[given(regex = r"^an additional zero-score finding in `(.+?)`$")]
fn additional_zero_finding(w: &mut World, path: String) {
    let finding = build_finding(&path, &format!("{path}::tests::t_zero"), 0);
    w.envelope_findings.push(finding);
}

// ── When ─────────────────────────────────────────────────────────────

#[when(regex = r"^the caller invokes `emit\(\)` with default options$")]
fn emit_default(w: &mut World) {
    let report = report_from_world(w);
    let meta = default_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .expect("emit succeeds");
    w.envelope_output = Some(buf);
}

#[when(regex = r"^the caller invokes `emit\(\)` with `top = (\d+)`$")]
fn emit_with_top(w: &mut World, top_str: String) {
    let report = report_from_world(w);
    let meta = default_meta();
    let top = NonZeroUsize::new(top_str.parse().expect("top parses")).expect("top > 0");
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions {
            top: Some(top),
            only_failing: false,
        },
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .expect("emit succeeds");
    w.envelope_output = Some(buf);
}

#[when(regex = r"^the caller invokes `emit\(\)` with `only_failing = true`$")]
fn emit_only_failing(w: &mut World) {
    let report = report_from_world(w);
    let meta = default_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions {
            top: None,
            only_failing: true,
        },
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .expect("emit succeeds");
    w.envelope_output = Some(buf);
}

#[when(regex = r"^the caller invokes `emit\(\)` with adapter meta tool=`(.+?)` language=`(.+?)`$")]
fn emit_with_meta(w: &mut World, tool: String, language: String) {
    let report = report_from_world(w);
    let meta = if tool == "adapter-b" {
        alt_meta()
    } else if tool == "adapter-a" {
        default_meta()
    } else {
        // Fallback — unit-test extensibility. Construct via static refs
        // since AdapterMeta fields are `&'static str`. Today the test
        // only exercises adapter-a / adapter-b so this branch shouldn't
        // hit; keeping the panic explicit catches future scenario typos.
        panic!("unrecognized adapter tool `{tool}` (language=`{language}`)");
    };
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .expect("emit succeeds");
    if tool == "adapter-a" {
        w.envelope_output = Some(buf);
    } else {
        w.envelope_output_alt = Some(buf);
    }
}

// ── Then ─────────────────────────────────────────────────────────────

#[then(
    regex = r"^the envelope wire shape contains the top-level keys schema_version, tool, tool_version, language, timestamp, threshold_mode, result, view$"
)]
fn assert_top_level_keys(w: &mut World) {
    let buf = w.envelope_output.as_ref().expect("envelope emitted");
    let value: serde_json::Value = serde_json::from_slice(buf).expect("envelope is valid JSON");
    for key in [
        "schema_version",
        "tool",
        "tool_version",
        "language",
        "timestamp",
        "threshold_mode",
        "result",
        "view",
    ] {
        assert!(
            value.get(key).is_some(),
            "expected top-level key `{key}` present, envelope:\n{value:#}",
        );
    }
}

#[then(regex = r"^`result\.passed` equals `(true|false)`$")]
fn assert_result_passed(w: &mut World, expected: String) {
    let buf = w.envelope_output.as_ref().expect("envelope emitted");
    let value: serde_json::Value = serde_json::from_slice(buf).unwrap();
    let want = expected == "true";
    assert_eq!(
        value["result"]["passed"].as_bool(),
        Some(want),
        "result.passed mismatch",
    );
}

#[then(regex = r"^`view\.shown` length equals (\d+)$")]
fn assert_view_shown_len(w: &mut World, expected_str: String) {
    let buf = w.envelope_output.as_ref().expect("envelope emitted");
    let value: serde_json::Value = serde_json::from_slice(buf).unwrap();
    let expected: usize = expected_str.parse().unwrap();
    assert_eq!(
        value["view"]["shown"].as_array().unwrap().len(),
        expected,
        "view.shown length mismatch",
    );
}

#[then(regex = r"^`view\.truncated` is (true|false)$")]
fn assert_view_truncated(w: &mut World, expected: String) {
    let buf = w.envelope_output.as_ref().expect("envelope emitted");
    let value: serde_json::Value = serde_json::from_slice(buf).unwrap();
    let want = expected == "true";
    assert_eq!(value["view"]["truncated"].as_bool(), Some(want));
}

#[then(regex = r"^`view\.eligible_count` equals (\d+)$")]
fn assert_view_eligible(w: &mut World, expected_str: String) {
    let buf = w.envelope_output.as_ref().expect("envelope emitted");
    let value: serde_json::Value = serde_json::from_slice(buf).unwrap();
    let expected: u64 = expected_str.parse().unwrap();
    assert_eq!(value["view"]["eligible_count"].as_u64(), Some(expected));
}

#[then(regex = r"^`result\.files` total findings count equals (\d+)$")]
fn assert_result_files_total(w: &mut World, expected_str: String) {
    let buf = w.envelope_output.as_ref().expect("envelope emitted");
    let value: serde_json::Value = serde_json::from_slice(buf).unwrap();
    let expected: usize = expected_str.parse().unwrap();
    let total: usize = value["result"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["findings"].as_array().unwrap().len())
        .sum();
    assert_eq!(total, expected, "result.files findings total mismatch");
}

#[then(regex = r"^the envelope wire shape does NOT contain the top-level key `(.+?)`$")]
fn assert_top_level_key_absent(w: &mut World, key: String) {
    let buf = w.envelope_output.as_ref().expect("envelope emitted");
    let value: serde_json::Value = serde_json::from_slice(buf).unwrap();
    assert!(
        value.get(&key).is_none(),
        "expected top-level key `{key}` absent (skip_serializing_if), but found: {}",
        value[&key],
    );
}

#[then(regex = r"^the envelope's `schema_version` equals the integer (\d+)$")]
fn assert_schema_version(w: &mut World, expected_str: String) {
    let buf = w.envelope_output.as_ref().expect("envelope emitted");
    let value: serde_json::Value = serde_json::from_slice(buf).unwrap();
    let expected: u64 = expected_str.parse().unwrap();
    assert_eq!(value["schema_version"].as_u64(), Some(expected));
}

#[then(regex = r"^both envelopes are byte-identical except for the `tool` and `language` fields$")]
fn assert_envelopes_identical_modulo_tool_language(w: &mut World) {
    let rust_buf = w.envelope_output.as_ref().expect("rust envelope emitted");
    let ts_buf = w.envelope_output_alt.as_ref().expect("ts envelope emitted");
    let mut rust_value: serde_json::Value = serde_json::from_slice(rust_buf).unwrap();
    let mut ts_value: serde_json::Value = serde_json::from_slice(ts_buf).unwrap();

    // Strip the two fields that are expected to differ.
    let rust_obj = rust_value.as_object_mut().unwrap();
    let ts_obj = ts_value.as_object_mut().unwrap();

    let rust_tool = rust_obj.remove("tool").unwrap();
    let rust_lang = rust_obj.remove("language").unwrap();
    let ts_tool = ts_obj.remove("tool").unwrap();
    let ts_lang = ts_obj.remove("language").unwrap();

    // Distinctness property: two DIFFERENT metas produce DIFFERENT wire
    // `tool`/`language` strings (neutral names per scrap-rs#37 — the
    // distinctness claim is fully preserved without the real names).
    assert_eq!(rust_tool, serde_json::json!("adapter-a"));
    assert_eq!(ts_tool, serde_json::json!("adapter-b"));
    assert_ne!(
        rust_tool, ts_tool,
        "tool fields must differ across adapters"
    );
    assert_eq!(rust_lang, serde_json::json!("rust"));
    assert_eq!(ts_lang, serde_json::json!("typescript"));
    assert_ne!(
        rust_lang, ts_lang,
        "language fields must differ across adapters",
    );

    // The remaining envelope must be identical.
    assert_eq!(
        rust_value, ts_value,
        "envelopes differ in fields other than tool/language",
    );
}
