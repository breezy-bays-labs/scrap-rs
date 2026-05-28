//! Minimum-viable stdout reporter — raw println-style text.
//!
//! Per FORK-4, v0.1 ships a plain-text reporter with zero formatting
//! ceremony — every detector finding renders on its own line under a
//! per-file header. Comfy-table prettification + view-shaping
//! (`--top` / `--only-failing`) consumption land with the table
//! reporter PR (scrap-rs#16); the silent no-op of those flags here is
//! a documented limitation per breadboard U24.
//!
//! Free function per the reporter convention in
//! `crates/scrap-core/src/adapters/reporters/mod.rs` — takes
//! `&Report` + `&AdapterMeta` and returns `String`. The CLI writes
//! the string to stdout via `dispatch::render_format`.

use std::fmt::Write as _;

use crate::adapter_meta::AdapterMeta;
use crate::domain::report::Report;

/// Render a [`Report`] as a minimum-viable plain-text summary.
///
/// Layout:
///
/// ```text
/// <tool> v<version>
/// files: N    tests: M    findings: K
///
/// <file path>:
///   <qualified_name> (lines <start>-<end>)  scrap_score=<score>
///     - <smell category> (<severity>)
///
/// (empty report)
/// <tool> v<version>
/// files: 0    tests: 0    findings: 0
/// no findings.
/// ```
///
/// View-shaping flags (`--top`, `--only-failing`) are intentionally
/// not consumed at v0.1 (cabinet U24). When scrap-rs#16 lands the
/// comfy-table reporter, this function gets a sibling with full
/// view-shaping; the plain-text version stays as a fallback.
#[must_use]
pub fn format_stdout(report: &Report, meta: &AdapterMeta) -> String {
    let mut out = String::new();
    out.push_str(meta.tool_name);
    out.push_str(" v");
    out.push_str(meta.tool_version);
    out.push('\n');

    let _ = writeln!(
        out,
        "files: {files}    tests: {tests}    findings: {findings}",
        files = report.summary.total_files,
        tests = report.summary.total_tests,
        findings = report.summary.distribution.total(),
    );

    if report.files.is_empty() {
        out.push_str("no findings.\n");
        return out;
    }

    for fr in &report.files {
        out.push('\n');
        let _ = writeln!(out, "{}:", fr.file_path.as_path().display());
        for finding in &fr.findings {
            let _ = writeln!(
                out,
                "  {qn} (lines {start}-{end})  scrap_score={score:.1}",
                qn = finding.test.qualified_name.as_str(),
                start = finding.test.span.start_line,
                end = finding.test.span.end_line,
                score = finding.scrap_score,
            );
            for smell in &finding.smells {
                // Severity has no Display impl; serde rename produces
                // the wire string. Skip the serde round-trip and use
                // `Debug` lowercase as a v0.1 pragma — the wire string
                // matches Debug-lowercase for all three Low/Moderate/
                // High variants. (When scrap-rs#16's table reporter
                // lands, a shared `as_wire_str` helper for Severity
                // is the right place to consolidate.)
                let severity = format!("{:?}", smell.severity).to_lowercase();
                let _ = writeln!(
                    out,
                    "    - {category} ({severity})",
                    category = smell.category.as_wire_str(),
                );
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::finding::Finding;
    use crate::domain::report::{FileReport, Report, Summary};
    use crate::domain::smell::{Smell, SmellCategory};
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

    fn fixture_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test 2026-05-27)",
            about: "stdout-test fixture",
            long_about: "Test-fixture AdapterMeta for stdout reporter tests.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/scrap",
            rule_help_uri: "https://example.invalid/scrap/rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &[],
            parse_hint: "ensure --src points at a workspace with test files",
        }
    }

    fn one_finding_report() -> Report {
        let test = TestIdentity::new(
            FilePath::new("crates/foo/src/bar.rs"),
            QualifiedName::new("foo::tests::it_smells"),
            Span::new(42, 51),
        );
        let smell = Smell::new(
            SmellCategory::ZeroAssertion,
            Severity::High,
            Actionability::AutoRefactor,
            10,
            None,
        );
        let finding = Finding::new(test, vec![smell]);
        let mut summary = Summary::default();
        summary
            .distribution
            .record(SmellCategory::ZeroAssertion, Severity::High);
        summary.total_files = 1;
        summary.total_tests = 1;
        Report {
            files: vec![FileReport::new(
                FilePath::new("crates/foo/src/bar.rs"),
                vec![finding],
            )],
            summary,
            passed: false,
        }
    }

    #[test]
    fn format_stdout_empty_report_shows_no_findings() {
        let meta = fixture_meta();
        let out = format_stdout(&Report::default(), &meta);
        assert!(out.contains("no findings."), "empty report; got: {out}");
        assert!(out.contains("files: 0"));
        assert!(out.contains("tests: 0"));
    }

    #[test]
    fn format_stdout_one_finding_shows_qualified_name() {
        let meta = fixture_meta();
        let out = format_stdout(&one_finding_report(), &meta);
        assert!(
            out.contains("foo::tests::it_smells"),
            "qualified name must appear; got: {out}",
        );
        assert!(
            out.contains("crates/foo/src/bar.rs"),
            "file path must appear; got: {out}",
        );
        assert!(out.contains("lines 42-51"), "span must appear; got: {out}");
        assert!(
            out.contains("zero_assertion"),
            "smell category wire string must appear; got: {out}",
        );
    }

    #[test]
    fn format_stdout_includes_tool_name_and_version() {
        let meta = fixture_meta();
        let out = format_stdout(&Report::default(), &meta);
        assert!(out.starts_with("test-adapter v0.1.0\n"));
    }

    #[test]
    fn format_stdout_summary_line_reflects_distribution_total() {
        let meta = fixture_meta();
        let out = format_stdout(&one_finding_report(), &meta);
        assert!(
            out.contains("findings: 1"),
            "summary findings count must match distribution.total(); got: {out}",
        );
    }
}
