//! Markdown reporter (scrap-rs#15) — human-readable GitHub-Flavored
//! Markdown via an askama compile-time template, following the
//! cross-tool recipe (crap-rs#260, dry-rs#17): template file under
//! `crates/scrap-core/templates/`, `escape = "none"`, pre-projected
//! row structs, and a trailing-newline guard.
//!
//! ## result vs view
//!
//! The summary block renders RESULT-side truth (totals, distribution,
//! gate verdict — never reshaped by display flags); the findings table
//! follows the same view semantics as the JSON envelope's `view.*`
//! block: `--only-failing` filter, then `--top N` truncation, in
//! source order (filter → truncate, mirroring `json::build_view`).
//! When truncation drops rows the heading says "showing X of Y
//! eligible" so a reader can't mistake a shaped view for the whole.
//!
//! ## No ANSI, no terminal assumptions
//!
//! Markdown is consumed by GitHub PR comments, dashboards, and
//! copy-paste — never a terminal. No color codes; plain GFM tables.
//!
//! ## Escaping policy
//!
//! askama renders with `escape = "none"` (HTML-escaping would mangle
//! markdown). Dynamic table-cell text goes through `md_cell`, which
//! neutralizes the two characters that can break a GFM table row:
//! `|` (cell delimiter) and newlines. Paths/test names additionally
//! render inside backtick spans for readability, not as an escaping
//! mechanism.

use askama::Template;
use std::fmt::Write as _;

use crate::adapter_meta::AdapterMeta;
use crate::adapters::reporters::json::EmitOptions;
use crate::domain::classification::Severity;
use crate::domain::finding::Finding;
use crate::domain::report::Report;
use crate::domain::types::Span;

/// Emit the markdown report to `writer`. Free function per the
/// reporter design (see [`crate::adapters::reporters`] module doc) —
/// the `OutputPort`-trait shape from the original issue AC predates
/// the #17 free-function reporter decision.
///
/// # Errors
///
/// Returns [`std::io::Error`] on writer failure.
pub fn emit<W: std::io::Write>(
    report: &Report,
    meta: &AdapterMeta,
    options: &EmitOptions,
    writer: &mut W,
) -> Result<(), std::io::Error> {
    let rendered = render(report, meta, options);
    writer.write_all(rendered.as_bytes())
}

/// Pure projection: report → markdown string. Split from `emit` so
/// snapshot tests pin the exact bytes without a writer.
fn render(report: &Report, meta: &AdapterMeta, options: &EmitOptions) -> String {
    // View shaping — same order of operations as `json::build_view`
    // (filter → truncate, source order).
    let all: Vec<&Finding> = report
        .files
        .iter()
        .flat_map(|f| f.findings.iter())
        .collect();
    let mut filtered: Vec<&Finding> = if options.only_failing {
        all.into_iter().filter(|f| f.scrap_score > 0.0).collect()
    } else {
        all
    };
    let eligible_count = filtered.len();
    if let Some(top) = options.top {
        filtered.truncate(top.get());
    }
    let truncated = filtered.len() < eligible_count;

    // One row per SMELL (same granularity as SARIF + github-annotations,
    // #17 D2) so the severity/penalty columns stay scalar.
    let rows: Vec<FindingRow> = filtered
        .iter()
        .flat_map(|finding| {
            let file = finding.test.file_path.to_string();
            let test_name = finding.test.qualified_name.to_string();
            let test_span = finding.test.span;
            finding.smells.iter().map(move |smell| {
                let span: Span = smell.span.unwrap_or(test_span);
                FindingRow {
                    location: md_cell(&format!("{file}:{line}", line = span.start_line)),
                    test_name: md_cell(&test_name),
                    smell: smell.category.as_wire_str(),
                    severity: severity_label(smell.severity),
                    penalty: smell.penalty,
                    suggestion: md_cell(&smell.ai_actionability_message),
                }
            })
        })
        .collect();

    // RESULT-side distribution (truthful, view-independent).
    let smell_rows: Vec<CountRow> = report
        .summary
        .distribution
        .by_smell
        .iter()
        .map(|(category, count)| CountRow {
            name: category.as_wire_str(),
            count: *count,
        })
        .collect();
    let severity_rows: Vec<CountRow> = report
        .summary
        .distribution
        .by_severity
        .iter()
        .map(|(severity, count)| CountRow {
            name: severity_label(*severity),
            count: *count,
        })
        .collect();

    let mut out = MarkdownReport {
        tool_name: meta.tool_name,
        tool_version: meta.tool_version,
        pass_fail: if report.passed { "PASS" } else { "FAIL" },
        total_tests: report.summary.total_tests,
        total_files: report.summary.total_files,
        exceeding_threshold: report.summary.exceeding_threshold,
        max_score: format_score(report.summary.max_scrap_score),
        avg_score: format_score(report.summary.average_scrap_score),
        has_distribution: !smell_rows.is_empty(),
        smell_rows,
        severity_rows,
        has_rows: !rows.is_empty(),
        truncated,
        shown_count: rows.len(),
        eligible_count,
        rows,
    }
    .render()
    .expect("markdown template render is total — all fields owned");

    // POSIX text files end with `\n`; askama ws-control can strip it.
    // Downstream heredoc/`$GITHUB_OUTPUT` consumers rely on it (same
    // contract note as crap-rs's markdown reporter).
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[derive(Template)]
#[template(path = "markdown_report.txt", escape = "none")]
struct MarkdownReport<'a> {
    tool_name: &'a str,
    tool_version: &'a str,
    pass_fail: &'static str,
    total_tests: u32,
    total_files: u32,
    exceeding_threshold: u32,
    max_score: String,
    avg_score: String,
    has_distribution: bool,
    smell_rows: Vec<CountRow>,
    severity_rows: Vec<CountRow>,
    has_rows: bool,
    truncated: bool,
    shown_count: usize,
    eligible_count: usize,
    rows: Vec<FindingRow>,
}

/// One `| name | count |` distribution row.
struct CountRow {
    name: &'static str,
    count: u32,
}

/// One `| location | test | smell | severity | penalty | suggestion |`
/// findings-table row. All display text pre-projected (and cell-escaped
/// where dynamic) in `render`.
struct FindingRow {
    location: String,
    test_name: String,
    smell: &'static str,
    severity: &'static str,
    penalty: u32,
    suggestion: String,
}

/// Human label for a severity bucket (capitalized, vs the
/// `snake_case` wire strings).
fn severity_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Low => "Low",
        Severity::Moderate => "Moderate",
        Severity::High => "High",
    }
}

/// Format a scrap score with one decimal place (pre-rendered in Rust —
/// askama interpolation has no format specifiers).
fn format_score(score: f64) -> String {
    let mut s = String::new();
    let _ = write!(s, "{score:.1}");
    s
}

/// Neutralize the characters that break a GFM table row: `|` becomes
/// `\|`, newlines collapse to a single space. Applied to every dynamic
/// cell (paths, test names, suggestion text).
fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::report::{FileReport, Report, Summary};
    use crate::domain::smell::{Smell, SmellCategory};
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
    use std::num::NonZeroUsize;

    fn meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test)",
            about: "markdown test fixture",
            long_about: "Test-fixture AdapterMeta for the markdown reporter.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/test-adapter",
            rule_help_uri: "https://example.invalid/test-adapter#rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &[],
            parse_hint: "hint",
        }
    }

    fn smell(category: SmellCategory, severity: Severity, penalty: u32, msg: &str) -> Smell {
        Smell::with_message(
            category,
            severity,
            Actionability::AutoRefactor,
            penalty,
            Some(Span::new(7, 9, 5, 2)),
            msg,
        )
    }

    fn finding(path: &str, name: &str, smells: Vec<Smell>) -> Finding {
        let test = TestIdentity::new(
            FilePath::new(path),
            QualifiedName::new(name),
            Span::new(3, 20, 1, 2),
        );
        Finding::new(test, smells)
    }

    fn report(findings_by_file: Vec<(&str, Vec<Finding>)>) -> Report {
        let files: Vec<FileReport> = findings_by_file
            .into_iter()
            .map(|(path, fs)| FileReport::new(FilePath::new(path), fs))
            .collect();
        let summary = Summary::from_findings(files.iter().flat_map(|f| f.findings.iter()));
        Report {
            files,
            summary,
            passed: false,
        }
    }

    fn representative_report() -> Report {
        report(vec![
            (
                "src/a.rs",
                vec![
                    finding(
                        "src/a.rs",
                        "tests::no_asserts",
                        vec![smell(
                            SmellCategory::ZeroAssertion,
                            Severity::High,
                            10,
                            "Add assertions on observable behavior.",
                        )],
                    ),
                    finding(
                        "src/a.rs",
                        "tests::pipe|name",
                        vec![smell(
                            SmellCategory::LargeExample,
                            Severity::Low,
                            4,
                            "Split the example | extract helpers.",
                        )],
                    ),
                ],
            ),
            (
                "src/b.rs",
                vec![finding(
                    "src/b.rs",
                    "tests::clean",
                    // Clean test: finding entry with no smells (the
                    // envelope emits these; the row projection must
                    // skip them naturally — zero rows).
                    vec![],
                )],
            ),
        ])
    }

    #[test]
    fn snapshot_full_report() {
        let out = render(&representative_report(), &meta(), &EmitOptions::default());
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_empty_report() {
        let out = render(&report(vec![]), &meta(), &EmitOptions::default());
        insta::assert_snapshot!(out);
    }

    #[test]
    fn snapshot_view_shaped_top_one_only_failing() {
        let opts = EmitOptions {
            top: Some(NonZeroUsize::new(1).expect("nonzero")),
            only_failing: true,
        };
        let out = render(&representative_report(), &meta(), &opts);
        insta::assert_snapshot!(out);
    }

    #[test]
    fn output_ends_with_single_trailing_newline() {
        let out = render(&representative_report(), &meta(), &EmitOptions::default());
        assert!(out.ends_with('\n'), "trailing newline contract");
        assert!(!out.ends_with("\n\n"), "exactly one trailing newline");
    }

    #[test]
    fn no_ansi_escape_codes() {
        let out = render(&representative_report(), &meta(), &EmitOptions::default());
        assert!(!out.contains('\u{1b}'), "markdown must carry no ANSI");
    }

    #[test]
    fn pipe_in_dynamic_cells_is_escaped() {
        let out = render(&representative_report(), &meta(), &EmitOptions::default());
        assert!(
            out.contains("tests::pipe\\|name"),
            "test-name pipe escaped: {out}"
        );
        assert!(
            out.contains("Split the example \\| extract helpers."),
            "suggestion pipe escaped: {out}"
        );
    }

    #[test]
    fn truncation_discloses_shown_of_eligible() {
        let opts = EmitOptions {
            top: Some(NonZeroUsize::new(1).expect("nonzero")),
            only_failing: false,
        };
        let out = render(&representative_report(), &meta(), &opts);
        assert!(
            out.contains("showing 1 of 3 eligible"),
            "shaped view must disclose truncation: {out}"
        );
    }

    #[test]
    fn summary_block_is_result_side_even_when_view_truncates() {
        // RESULT-side distribution stays complete under --top 1.
        let opts = EmitOptions {
            top: Some(NonZeroUsize::new(1).expect("nonzero")),
            only_failing: false,
        };
        let out = render(&representative_report(), &meta(), &opts);
        assert!(out.contains("`zero_assertion` | 1"), "by-smell: {out}");
        assert!(out.contains("`large_example` | 1"), "by-smell: {out}");
    }
}
