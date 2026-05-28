//! Step defs for `tests/features/table_reporter.feature` (scrap-rs#16).
//!
//! Sibling to `cucumber_steps/json_reporter.rs`. Cucumber-rs registers
//! `#[given]`/`#[when]`/`#[then]` step fns globally within the test
//! binary; mod-block split per scrap-rs#18 W5.1 SHOULD-FIX #5.
//!
//! The literal `"scrap4rs"` / `"scrap4ts"` / `"test-adapter"` strings
//! here are OK because the source-only adapter-name literal purity CI
//! gate (scrap-rs#37 / scrap-rs#52) scopes to
//! `crates/scrap-core/src/`, NOT `tests/`. Test fixtures use the
//! concrete adapter names for realism.

#![allow(clippy::needless_pass_by_value)] // cucumber step-fn convention

use cucumber::{given, then, when};
use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::adapters::reporters::table::{RowGrouping, TableOptions, emit};
use scrap_core::domain::classification::{Actionability, Severity};
use scrap_core::domain::finding::Finding;
use scrap_core::domain::report::{FileReport, Report};
use scrap_core::domain::smell::{Smell, SmellCategory};
use scrap_core::domain::threshold::ThresholdMode;
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
use std::num::NonZeroUsize;

use crate::World;

fn build_finding(path: &str, name: &str, penalty: u32) -> Finding {
    let test = TestIdentity::new(
        FilePath::new(path),
        QualifiedName::new(name),
        Span::new(1, 5),
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
        tool_name: "test-adapter",
        language: "rust",
        tool_version: "0.1.0",
        long_version: "0.1.0 (cucumber 2026-05-27)",
        about: "Static test smell detector",
        long_about: "Cucumber-test fixture AdapterMeta for table_reporter.feature.",
        after_help: "",
        extensions: &["rs"],
        tool_info_uri: "https://example.invalid/scrap",
        rule_help_uri: "https://example.invalid/scrap/rules",
        config_file_name: "test-adapter.toml",
        default_excludes: &["tests/**"],
        parse_hint: "ensure --src points at a workspace with test files",
    }
}

/// Group accumulated `World.table_findings` into a `Report`
/// (one `FileReport` per distinct `file_path`). Mirrors the
/// `report_from_world` helper in `json_reporter` step defs.
fn report_from_world(w: &World) -> Report {
    use std::collections::BTreeMap;
    let mut by_path: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
    for finding in &w.table_findings {
        let key = finding
            .test
            .file_path
            .as_path()
            .to_string_lossy()
            .into_owned();
        by_path.entry(key).or_default().push(finding.clone());
    }
    let files: Vec<FileReport> = by_path
        .into_iter()
        .map(|(path, fs)| FileReport::new(FilePath::new(&path), fs))
        .collect();

    // Populate summary so the header line reflects the truth (header
    // reads from report.summary.* per D-HEADER-1).
    let total_tests = u32::try_from(w.table_findings.len()).unwrap_or(u32::MAX);
    let exceeding_threshold = u32::try_from(
        w.table_findings
            .iter()
            .filter(|f| f.exceeds_threshold)
            .count(),
    )
    .unwrap_or(u32::MAX);

    let mut report = Report {
        files,
        ..Report::default()
    };
    report.summary.total_tests = total_tests;
    report.summary.exceeding_threshold = exceeding_threshold;
    report.passed = w.table_report_passed;
    report
}

fn emit_to_buf(
    w: &World,
    options: &TableOptions,
    mode: ThresholdMode,
    meta: &AdapterMeta,
) -> Vec<u8> {
    let report = report_from_world(w);
    let mut buf = Vec::new();
    emit(&report, meta, options, mode, &mut buf).expect("emit succeeds");
    buf
}

// ── Given ────────────────────────────────────────────────────────────

#[given(
    regex = r"^a fixture Report for the table reporter with one finding scoring (\d+) in `(.+?)`$"
)]
fn fixture_finding_table(w: &mut World, penalty_str: String, path: String) {
    let penalty: u32 = penalty_str.parse().expect("penalty parses as u32");
    let mut finding = build_finding(&path, &format!("{path}::tests::t1"), penalty);
    // Mark as exceeding threshold so the default "FAILED" verdict
    // path is exercised by the first scenario.
    if penalty > 0 {
        finding.exceeds_threshold = true;
    }
    w.table_findings.push(finding);
    w.table_report_passed = false;
}

#[given(regex = r"^an additional table-fixture finding scoring (\d+) in `(.+?)`$")]
fn additional_table_finding(w: &mut World, penalty_str: String, path: String) {
    let penalty: u32 = penalty_str.parse().expect("penalty parses as u32");
    let finding = build_finding(&path, &format!("{path}::tests::t_extra"), penalty);
    w.table_findings.push(finding);
}

#[given(regex = r"^an additional table-fixture zero-score finding in `(.+?)`$")]
fn additional_table_zero_finding(w: &mut World, path: String) {
    let finding = build_finding(&path, &format!("{path}::tests::t_zero"), 0);
    w.table_findings.push(finding);
}

#[given(
    regex = r"^the table-fixture Report has `passed = (true|false)` and zero findings exceeding threshold$"
)]
fn set_report_passed(w: &mut World, passed: String) {
    w.table_report_passed = passed == "true";
    // Drop the fixture finding so `0 of 0 tests` reads correctly in
    // the PASSED footer.
    w.table_findings.clear();
}

// ── When ─────────────────────────────────────────────────────────────

#[when(regex = r"^the caller invokes table `emit\(\)` with default options$")]
fn emit_table_default(w: &mut World) {
    let opts = TableOptions::default();
    let buf = emit_to_buf(w, &opts, ThresholdMode::Default, &default_meta());
    w.table_output = Some(buf);
}

#[when(regex = r"^the caller invokes table `emit\(\)` with `top = (\d+)`$")]
fn emit_table_with_top(w: &mut World, top_str: String) {
    let top: NonZeroUsize =
        NonZeroUsize::new(top_str.parse().expect("top parses")).expect("top > 0");
    let opts = TableOptions {
        top: Some(top),
        ..TableOptions::default()
    };
    let buf = emit_to_buf(w, &opts, ThresholdMode::Default, &default_meta());
    w.table_output = Some(buf);
}

#[when(regex = r"^the caller invokes table `emit\(\)` with `only_failing = true`$")]
fn emit_table_only_failing(w: &mut World) {
    let opts = TableOptions {
        only_failing: true,
        ..TableOptions::default()
    };
    let buf = emit_to_buf(w, &opts, ThresholdMode::Default, &default_meta());
    w.table_output = Some(buf);
}

#[when(regex = r"^the caller invokes table `emit\(\)` with `grouping = (smell|finding)`$")]
fn emit_table_with_grouping(w: &mut World, grouping_str: String) {
    let grouping = match grouping_str.as_str() {
        "smell" => RowGrouping::Smell,
        "finding" => RowGrouping::Finding,
        other => panic!("unknown grouping `{other}`"),
    };
    let opts = TableOptions {
        grouping,
        ..TableOptions::default()
    };
    let buf = emit_to_buf(w, &opts, ThresholdMode::Default, &default_meta());
    w.table_output = Some(buf);
}

#[when(regex = r"^the caller invokes table `emit\(\)` with `use_color = (true|false)`$")]
fn emit_table_with_use_color(w: &mut World, use_color_str: String) {
    let use_color = use_color_str == "true";
    let opts = TableOptions {
        use_color,
        ..TableOptions::default()
    };
    let buf = emit_to_buf(w, &opts, ThresholdMode::Default, &default_meta());
    w.table_output = Some(buf);
}

#[when(
    regex = r"^the caller invokes table `emit\(\)` with default options and threshold mode `(strict|default|lenient)`$"
)]
fn emit_table_with_threshold_mode(w: &mut World, mode_str: String) {
    let mode = match mode_str.as_str() {
        "strict" => ThresholdMode::Strict,
        "default" => ThresholdMode::Default,
        "lenient" => ThresholdMode::Lenient,
        other => panic!("unknown threshold mode `{other}`"),
    };
    let opts = TableOptions::default();
    let buf = emit_to_buf(w, &opts, mode, &default_meta());
    w.table_output = Some(buf);
}

#[when(
    regex = r"^the caller invokes table `emit\(\)` with adapter meta tool=`(.+?)` and tool_version=`(.+?)`$"
)]
fn emit_table_with_meta(w: &mut World, tool: String, tool_version: String) {
    // Build a meta with the requested tool + tool_version; other
    // fields stay at default. Use match against a small set of
    // expected literals so the &'static str lifetime matches
    // AdapterMeta's field type.
    let meta = match (tool.as_str(), tool_version.as_str()) {
        ("test-adapter", "0.1.0") => default_meta(),
        // Fallback path for future scenarios that add new combos.
        other => panic!(
            "unsupported adapter meta combo: tool=`{}` tool_version=`{}` (extend test fixture)",
            other.0, other.1,
        ),
    };
    let opts = TableOptions::default();
    let buf = emit_to_buf(w, &opts, ThresholdMode::Default, &meta);
    w.table_output = Some(buf);
}

// ── Then ─────────────────────────────────────────────────────────────

fn table_output_str(w: &World) -> String {
    let buf = w.table_output.as_ref().expect("table output emitted");
    String::from_utf8(buf.clone()).expect("table output is UTF-8")
}

#[then(regex = r"^the table output contains the column header `(.+?)`$")]
fn assert_column_header_present(w: &mut World, header: String) {
    let output = table_output_str(w);
    assert!(
        output.contains(&header),
        "expected column header `{header}` present in output:\n{output}",
    );
}

#[then(regex = r"^the table output does NOT contain the column header `(.+?)`$")]
fn assert_column_header_absent(w: &mut World, header: String) {
    let output = table_output_str(w);
    assert!(
        !output.contains(&header),
        "expected column header `{header}` absent from output:\n{output}",
    );
}

#[then(regex = r"^the footer line contains `(.+?)`$")]
fn assert_footer_contains(w: &mut World, fragment: String) {
    let output = table_output_str(w);
    let footer = output.lines().last().expect("footer present");
    assert!(
        footer.contains(&fragment),
        "expected footer to contain `{fragment}`, got: {footer}",
    );
}

#[then(regex = r"^the header line contains `(.+?)`$")]
fn assert_header_contains(w: &mut World, fragment: String) {
    let output = table_output_str(w);
    let header = output.lines().next().expect("header present");
    assert!(
        header.contains(&fragment),
        "expected header to contain `{fragment}`, got: {header}",
    );
}

#[then(regex = r"^the table output contains exactly (\d+) data rows? under the `(.+?)` header$")]
fn assert_n_data_rows(w: &mut World, n_str: String, header: String) {
    let n: usize = n_str.parse().expect("data-row count parses");
    let output = table_output_str(w);
    // Data rows = lines with `│` (outer border) that DON'T contain
    // the column-header text. Mirrors `count_data_rows` from the
    // table.rs test helper.
    let count = output
        .lines()
        .filter(|l| l.contains('│'))
        .filter(|l| !l.contains(&header))
        .count();
    assert_eq!(
        count, n,
        "expected exactly {n} data rows under `{header}` header, got {count}; output:\n{output}",
    );
}

#[then(regex = r"^the table output does NOT contain ANSI escape sequences$")]
fn assert_no_ansi(w: &mut World) {
    let output = table_output_str(w);
    assert!(
        !output.contains("\x1b["),
        "expected no ANSI escape sequences (use_color=false), got:\n{output}",
    );
}

#[then(regex = r"^the table output contains an ANSI escape sequence$")]
fn assert_has_ansi(w: &mut World) {
    let output = table_output_str(w);
    assert!(
        output.contains("\x1b["),
        "expected ANSI escape sequence present (use_color=true), got:\n{output}",
    );
}
