//! v0.1 comfy-table terminal reporter — free function `emit()` that
//! renders a `Report` as a human-readable table for the default
//! `--format table` CLI dispatch (scrap-rs#16).
//!
//! Sibling to [`crate::adapters::reporters::json`] — same free-function
//! pattern per
//! [`crap4rs/adr-free-functions-over-reporter-trait`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/crap4rs/adr-free-functions-over-reporter-trait.md)
//! (D1) and codified in `crates/scrap-core/src/ports/mod.rs:8-13`.
//!
//! ## Public surface (v0.1)
//!
//! - `emit` — free function rendering a `Report` to any
//!   `std::io::Write`. Header (from [`crate::adapter_meta::AdapterMeta`]
//!   identity + summary counts) → table body (dispatched on
//!   [`RowGrouping`]) → footer (`PASSED`/`FAILED` + threshold-mode).
//!   Lands in W5; the public function definition is the entry point
//!   the CLI scrap-rs#21 dispatch calls.
//! - [`TableOptions`] — display-shaping options (`top`, `only_failing`,
//!   `use_color`, `grouping`). Mirrors `json::EmitOptions` field shape
//!   for `top` + `only_failing` so the CLI #21 boundary maps one
//!   `clap::Args` block to both reporter options.
//! - [`RowGrouping`] — non-exhaustive enum dispatching renderer
//!   choice. Default `Smell` (one row per `Smell`); `Finding`
//!   collapses multi-smell tests into one row. Serializable for
//!   config-file override (`[table] grouping = "smell|finding"` in
//!   `scrap.toml`).
//!
//! ## tracked
//!
//! - scrap-rs#73 — `adr-port-surface-and-domain-conventions` ADR
//!   not yet authored; references existing `ports/mod.rs:8-13`
//!   docstring + `adr-free-functions-over-reporter-trait` as
//!   load-bearing.
//! - SF-1 from /plan close cabinet — 1-LOC duplication of
//!   `if options.only_failing && finding.scrap_score == 0.0 { continue; }`
//!   between `render_smell_rows` and `render_finding_rows`. Kept
//!   inline (cabinet CAO SF-1 verdict: NO LIFT). Re-evaluate if a
//!   third reporter recurs the same shape.

use crate::adapter_meta::AdapterMeta;
use crate::domain::classification::Severity;
use crate::domain::report::Report;
use crate::domain::threshold::ThresholdMode;
use comfy_table::{Cell, Color, ContentArrangement, Table};
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

// ────────────────────────────────────────────────────────────────────
// Public input types
// ────────────────────────────────────────────────────────────────────

/// Row grouping strategy for the table reporter.
///
/// Locked at scrap-rs#16 D-GROUPING-1. The default ([`Self::Smell`])
/// matches the issue body contract; [`Self::Finding`] collapses
/// multi-Smell tests into one row at the cost of per-rule
/// attribution detail. Future variants (`File`, `Severity`,
/// `Detector`, etc.) slot in without breaking external
/// pattern-matchers via `#[non_exhaustive]`.
///
/// Config-file override flows through serde (`[table] grouping =
/// "smell|finding"` in scrap.toml deserializes via kebab-case).
/// CLI override is added by scrap-rs#21 (clap `ValueEnum` derive
/// lands in that PR — not here, because scrap-core does not yet
/// list `clap` in its `[dependencies]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RowGrouping {
    /// One row per Smell — `file:line / smell / severity / penalty / score`.
    #[default]
    Smell,
    /// One row per Finding (test) — `File / Test / Smells / Score / Pass-Fail`.
    Finding,
}

/// Display-shaping options for the table reporter.
///
/// Mirrors the json reporter's `EmitOptions` field shape (`top` +
/// `only_failing`) and adds two table-specific fields (`use_color` +
/// `grouping`). Separate struct (not a shared base) because reporters
/// will evolve their presentation needs independently (scrap-rs#16
/// D-OPT-1).
///
/// `Default::default()` produces "no filter, no truncation, no color,
/// per-Smell grouping" — the default invocation behavior.
///
/// CLI scrap-rs#21 owns the `Args → TableOptions` adapter; consumers
/// of `scrap-core` who bypass the CLI surface (library embedders)
/// construct `TableOptions` directly.
#[derive(Debug, Clone, Default)]
pub struct TableOptions {
    /// `--top N`: truncate the rendered rows to N (post-filter,
    /// counted as **rows** — not Findings — so a multi-Smell Finding
    /// under [`RowGrouping::Smell`] may be partially shown). `None` =
    /// no truncation. `NonZeroUsize` rules out the `--top 0` footgun
    /// at the type level (mirrors `json::EmitOptions::top`).
    pub top: Option<NonZeroUsize>,
    /// `--only-failing`: drop Findings whose `scrap_score == 0.0`
    /// before grouping. Same semantics as
    /// `json::EmitOptions::only_failing`.
    pub only_failing: bool,
    /// Color output toggle. CLI binary boundary resolves `--color
    /// auto|always|never` to a concrete bool via
    /// `std::io::IsTerminal` (stable Rust 1.70; MSRV 1.93 — no crate
    /// dep needed). Reporter takes the resolved value. `false` =
    /// plain ASCII output, no ANSI escapes.
    pub use_color: bool,
    /// Row grouping strategy. See [`RowGrouping`].
    pub grouping: RowGrouping,
}

// ────────────────────────────────────────────────────────────────────
// Internal renderers — per-Smell row layout (W3)
// ────────────────────────────────────────────────────────────────────

/// Render the table contents for [`RowGrouping::Smell`] into `writer`.
///
/// Iterates `report.files → findings → smells` in source order;
/// optionally filters out zero-score Findings (`options.only_failing`)
/// and truncates the rendered row count (`options.top`).
///
/// `threshold_mode` is intentionally NOT a parameter — the header
/// (W5 `write_header`) and footer (W5 `write_footer`) consume it; the
/// row-rendering path doesn't need it. Per cabinet CAO SF-2 fold-in
/// 2026-05-27 — keeps the internal contract tight.
///
/// tracked: SF-1 from /plan close cabinet — `if options.only_failing
/// && finding.scrap_score == 0.0 { continue; }` is 1 LOC of
/// duplication with `render_finding_rows`; kept inline. Lift to
/// `Report::filter_view()` only if a 3rd reporter recurs the same
/// shape.
fn render_smell_rows<W: std::io::Write>(
    report: &Report,
    options: &TableOptions,
    writer: &mut W,
) -> std::io::Result<()> {
    let mut table = Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(120)
        .set_header(vec!["file:line", "Smell", "Severity", "Penalty", "Score"]);

    let top_limit = options.top.map(NonZeroUsize::get);
    for row in iter_smell_rows(report, options).take(top_limit.unwrap_or(usize::MAX)) {
        table.add_row(row);
    }

    writeln!(writer, "{table}")
}

/// Flatten `report.files → findings → smells` into per-row `Vec<Cell>`
/// values, skipping zero-score findings when `options.only_failing`.
///
/// Extracted from `render_smell_rows` to keep the parent under the
/// crap4rs cognitive-complexity threshold (15) — the triple-nested
/// `for/for/for` + the labelled `break 'outer` is the entire CC cost.
/// Returning an `Iterator<Item = Vec<Cell>>` lets the parent express
/// `--top` as `.take(n)` and the body collapses to a single `for row`
/// loop.
///
/// tracked: SF-1 from /plan close cabinet — `if options.only_failing
/// && finding.scrap_score == 0.0 { continue; }` is 1 LOC of
/// duplication with `iter_finding_rows`; kept inline. Lift to
/// `Report::filter_view()` only if a 3rd reporter recurs the same
/// shape.
fn iter_smell_rows<'a>(
    report: &'a Report,
    options: &'a TableOptions,
) -> impl Iterator<Item = Vec<Cell>> + 'a {
    report
        .files
        .iter()
        .flat_map(|file| file.findings.iter())
        .filter(|finding| !(options.only_failing && finding.scrap_score == 0.0))
        .flat_map(move |finding| {
            finding
                .smells
                .iter()
                .map(move |smell| build_smell_row(finding, smell, options.use_color))
        })
}

/// Build the five `Cell`s for one row under `RowGrouping::Smell`.
///
/// Extracted from `render_smell_rows` so the nesting-heavy outer
/// function (file → finding → smell loops + `--top` short-circuit)
/// stays under the crap4rs cognitive-complexity threshold (15). The
/// per-row cell construction is straight-line: no branches except the
/// `map_or` line-fallback closure.
///
/// Column layout per D-COL-SMELL-1: `file:line`, `Smell`, `Severity`,
/// `Penalty`, `Score`.
fn build_smell_row(
    finding: &crate::domain::finding::Finding,
    smell: &crate::domain::smell::Smell,
    use_color: bool,
) -> Vec<Cell> {
    let line = smell
        .span
        .map_or(finding.test.span.start_line, |s| s.start_line);
    let file_line = format!("{}:{}", finding.test.file_path, line);
    vec![
        Cell::new(file_line),
        Cell::new(smell.category.as_wire_str()),
        severity_cell(smell.severity, use_color),
        Cell::new(smell.penalty.to_string()),
        Cell::new(format_score(finding.scrap_score)),
    ]
}

/// Format a `scrap_score` for the Score column.
///
/// Normalizes IEEE-754 negative-zero (which `Vec<f64>::iter().sum()`
/// produces on empty iterators — see `Finding::new` with no smells)
/// to positive zero so the rendered cell reads "0" not "-0". Matches
/// the json reporter's behavior (serde emits `0.0` not `-0.0` for
/// `scrap_score`).
fn format_score(score: f64) -> String {
    // `+ 0.0` flips -0.0 to +0.0 per IEEE-754 (-0.0 + 0.0 == +0.0).
    format!("{:.0}", score + 0.0)
}

/// Style a severity-text cell. When `use_color`, sets the foreground
/// per [`Severity`]; otherwise returns the plain cell. comfy-table's
/// `Cell::fg` only renders inside a `Table` — that's why this helper
/// is here (the footer needs different handling — see W5
/// `write_footer`).
///
/// Takes `Severity` directly so the cell text comes from a static
/// `&'static str` table — no `format!("{:?}", ..)` heap allocation per
/// rendered row. Per Gemini Code Assist review on PR #88
/// (`PRRT_kwDOSTlgZs6FRRMD` / `PRRT_kwDOSTlgZs6FRRL9`, 2026-05-27):
/// equivalent text bytes ("High"/"Moderate"/"Low" — same as `Debug`),
/// fewer allocations.
fn severity_cell(severity: Severity, use_color: bool) -> Cell {
    let text = match severity {
        Severity::High => "High",
        Severity::Moderate => "Moderate",
        Severity::Low => "Low",
    };
    let cell = Cell::new(text);
    if !use_color {
        return cell;
    }
    let color = match severity {
        Severity::High => Color::Red,
        Severity::Moderate => Color::Yellow,
        Severity::Low => Color::DarkGrey,
    };
    cell.fg(color)
}

// ────────────────────────────────────────────────────────────────────
// Internal renderers — per-Finding row layout (W4)
// ────────────────────────────────────────────────────────────────────

/// Render the table contents for [`RowGrouping::Finding`] into `writer`.
///
/// `threshold_mode` is intentionally NOT a parameter — see
/// `render_smell_rows` docstring for the cabinet CAO SF-2 rationale.
fn render_finding_rows<W: std::io::Write>(
    report: &Report,
    options: &TableOptions,
    writer: &mut W,
) -> std::io::Result<()> {
    let mut table = Table::new();
    table
        .load_preset(comfy_table::presets::UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(120)
        .set_header(vec!["File", "Test", "Smells", "Score", "Pass/Fail"]);

    let top_limit = options.top.map(NonZeroUsize::get);
    for row in iter_finding_rows(report, options).take(top_limit.unwrap_or(usize::MAX)) {
        table.add_row(row);
    }

    writeln!(writer, "{table}")
}

/// Flatten `report.files → findings` into per-row `Vec<Cell>` values,
/// skipping zero-score findings when `options.only_failing`.
///
/// Sibling to `iter_smell_rows`; both extracted to keep the parent
/// render fns under the crap4rs cognitive-complexity threshold (15).
/// `options` is in the signature for parity with `iter_smell_rows`
/// (both reporters consume `only_failing`); the unused `use_color`
/// path stays in the parent's `--top` plumbing layer.
///
/// tracked: SF-1 — duplicated `if options.only_failing && ...` shape
/// with `iter_smell_rows`; kept inline per cabinet CAO SF-1 verdict.
fn iter_finding_rows<'a>(
    report: &'a Report,
    options: &'a TableOptions,
) -> impl Iterator<Item = Vec<Cell>> + 'a {
    report
        .files
        .iter()
        .flat_map(|file| file.findings.iter())
        .filter(|finding| !(options.only_failing && finding.scrap_score == 0.0))
        .map(build_finding_row)
}

/// Build the five `Cell`s for one row under `RowGrouping::Finding`.
///
/// Extracted from `render_finding_rows` so the outer function (file →
/// finding nesting + `--top` short-circuit) stays under the crap4rs
/// cognitive-complexity threshold (15). The smells-text join is
/// constructed via `fold` (single allocation, per Gemini Code Assist
/// review `PRRT_kwDOSTlgZs6FRRME` on PR #88, 2026-05-27) instead of
/// `.collect::<Vec<_>>().join(", ")`; output bytes are identical.
///
/// Column layout per D-COL-FINDING-1: `File`, `Test`, `Smells`,
/// `Score`, `Pass/Fail`.
fn build_finding_row(finding: &crate::domain::finding::Finding) -> Vec<Cell> {
    let smells_text = finding
        .smells
        .iter()
        .map(|s| s.category.as_wire_str())
        .fold(String::new(), |mut acc, s| {
            if !acc.is_empty() {
                acc.push_str(", ");
            }
            acc.push_str(s);
            acc
        });
    let pass_fail = if finding.exceeds_threshold {
        "FAIL"
    } else {
        "PASS"
    };
    vec![
        Cell::new(finding.test.file_path.to_string()),
        Cell::new(finding.test.qualified_name.as_str()),
        Cell::new(smells_text),
        Cell::new(format_score(finding.scrap_score)),
        Cell::new(pass_fail),
    ]
}

// ────────────────────────────────────────────────────────────────────
// Public emit() function — dispatch + header + footer (W5)
// ────────────────────────────────────────────────────────────────────

/// Render a [`Report`] as a human-readable table to `writer`.
///
/// Default human-facing output for `scrap4rs` / `scrap4ts` CLI dispatch
/// (`--format table`). Returns [`std::io::Result`] because writer I/O
/// is the only failure mode (no serde; no custom error type).
///
/// `meta` provides the header's tool identity (`tool_name` +
/// `tool_version`). `options` controls view shaping (top truncation,
/// only-failing filter, color, row grouping). `threshold_mode` is
/// echoed verbatim into the footer.
///
/// # Errors
///
/// Returns [`std::io::Error`] when `writer.write_all` fails.
pub fn emit<W: std::io::Write>(
    report: &Report,
    meta: &AdapterMeta,
    options: &TableOptions,
    threshold_mode: ThresholdMode,
    writer: &mut W,
) -> std::io::Result<()> {
    write_header(report, meta, threshold_mode, writer)?;
    match options.grouping {
        RowGrouping::Smell => render_smell_rows(report, options, writer)?,
        RowGrouping::Finding => render_finding_rows(report, options, writer)?,
    }
    write_footer(report, threshold_mode, options.use_color, writer)?;
    Ok(())
}

/// Write the single-line identification header per D-HEADER-1.
///
/// Format: `{tool_name} {tool_version} — {total_tests} tests inspected,
/// {total_findings} findings, {exceeding_threshold} exceeding
/// '{threshold_mode}' threshold`.
fn write_header<W: std::io::Write>(
    report: &Report,
    meta: &AdapterMeta,
    threshold_mode: ThresholdMode,
    writer: &mut W,
) -> std::io::Result<()> {
    let total_findings: usize = report.files.iter().map(|f| f.findings.len()).sum();
    writeln!(
        writer,
        "{} {} — {} tests inspected, {} findings, {} exceeding '{}' threshold",
        meta.tool_name,
        meta.tool_version,
        report.summary.total_tests,
        total_findings,
        report.summary.exceeding_threshold,
        threshold_mode.as_wire_str(),
    )
}

/// Write the verdict footer per D-FOOTER-1.
///
/// Format: `{PASSED|FAILED} — {exceeding_threshold} of {total_tests}
/// tests exceed threshold under '{threshold_mode}' mode`. ANSI
/// colored (Green/PASSED, Red/FAILED) only when `use_color`. Per
/// cabinet `CEng` S1 fold-in 2026-05-27, `color_code()` returns
/// `Option<u8>`; unmapped colors emit plain text (no silent ANSI
/// reset).
fn write_footer<W: std::io::Write>(
    report: &Report,
    threshold_mode: ThresholdMode,
    use_color: bool,
    writer: &mut W,
) -> std::io::Result<()> {
    let (verdict, color) = if report.passed {
        ("PASSED", Color::Green)
    } else {
        ("FAILED", Color::Red)
    };
    let line = format!(
        "{verdict} — {} of {} tests exceed threshold under '{}' mode",
        report.summary.exceeding_threshold,
        report.summary.total_tests,
        threshold_mode.as_wire_str(),
    );
    if use_color {
        match color_code(color) {
            Some(code) => writeln!(writer, "\x1b[{code}m{line}\x1b[0m"),
            None => writeln!(writer, "{line}"),
        }
    } else {
        writeln!(writer, "{line}")
    }
}

/// Map a `comfy_table::Color` to its ANSI escape numeric code for
/// the footer.
///
/// Returns `Option<u8>` rather than a wildcard fallback (which would
/// silently emit `0`, the ANSI RESET code, for unmapped variants — a
/// real bug class for any future caller passing `Color::Yellow` or
/// similar). Per cabinet `CEng` S1 fold-in 2026-05-27.
///
/// Today only `Color::Green` (PASSED) and `Color::Red` (FAILED) are
/// ever passed by `write_footer`; the unmapped path is
/// impossible-by-construction. The `Option<u8>` API surfaces "no
/// color code for this variant" honestly to `write_footer`, which
/// then emits plain text rather than a malformed escape.
fn color_code(c: Color) -> Option<u8> {
    match c {
        Color::Green => Some(32),
        Color::Red => Some(31),
        _ => None,
    }
}

// ────────────────────────────────────────────────────────────────────
// Unit tests — W2 (types) + W3 (render_smell_rows) + W4
// (render_finding_rows) + W5 (emit + header + footer dispatch).
// W6 ANSI verification + W7 insta snapshots land in subsequent commits.
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::float_cmp)] // exact-integer-derived scores in fixtures (mirrors json.rs)
mod tests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::finding::Finding;
    use crate::domain::report::{FileReport, Report};
    use crate::domain::smell::{Smell, SmellCategory};
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

    // ── Fixture helpers ────────────────────────────────────────────

    /// Build a `Finding` for `path` with one Smell at `penalty`.
    /// `penalty = 0` produces a zero-score finding (no smells).
    fn finding_at(path: &str, name: &str, penalty: u32) -> Finding {
        let test = TestIdentity::new(
            FilePath::new(path),
            QualifiedName::new(name),
            Span::new(5, 15, 1, 1),
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

    /// Build a `Finding` with N smells at `penalty` each (constant
    /// severity High, no per-Smell span).
    fn finding_with_n_smells(path: &str, name: &str, penalty: u32, n: usize) -> Finding {
        let test = TestIdentity::new(
            FilePath::new(path),
            QualifiedName::new(name),
            Span::new(5, 15, 1, 1),
        );
        let smells = (0..n)
            .map(|_| {
                Smell::new(
                    SmellCategory::ZeroAssertion,
                    Severity::High,
                    Actionability::AutoRefactor,
                    penalty,
                    None,
                )
            })
            .collect();
        Finding::new(test, smells)
    }

    /// Build a `Finding` with one Smell carrying the given `Some(Span)`
    /// for `Smell.span` (per-Smell line attribution).
    fn finding_with_smell_span(path: &str, name: &str, penalty: u32, smell_span: Span) -> Finding {
        let test = TestIdentity::new(
            FilePath::new(path),
            QualifiedName::new(name),
            Span::new(5, 15, 1, 1),
        );
        Finding::new(
            test,
            vec![Smell::new(
                SmellCategory::TautologicalAssertion,
                Severity::High,
                Actionability::AutoRefactor,
                penalty,
                Some(smell_span),
            )],
        )
    }

    /// Wrap a list of (path, Findings) into a `Report`.
    fn report_from(files: Vec<(&str, Vec<Finding>)>) -> Report {
        let file_reports: Vec<FileReport> = files
            .into_iter()
            .map(|(path, findings)| FileReport::new(FilePath::new(path), findings))
            .collect();
        Report {
            files: file_reports,
            ..Report::default()
        }
    }

    /// Render `render_smell_rows` to a UTF-8 string.
    fn render_smell(report: &Report, options: &TableOptions) -> String {
        let mut buf: Vec<u8> = Vec::new();
        render_smell_rows(report, options, &mut buf).expect("render_smell_rows writes");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    /// Render `render_finding_rows` to a UTF-8 string.
    fn render_finding(report: &Report, options: &TableOptions) -> String {
        let mut buf: Vec<u8> = Vec::new();
        render_finding_rows(report, options, &mut buf).expect("render_finding_rows writes");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    /// Build a Finding with `exceeds_threshold = true` for Pass/Fail
    /// column testing.
    fn finding_exceeding(path: &str, name: &str, penalty: u32) -> Finding {
        let mut f = finding_at(path, name, penalty);
        f.exceeds_threshold = true;
        f
    }

    /// Test-fixture [`AdapterMeta`]. Uses `test-adapter` placeholder per
    /// scrap-rs#18 source-only adapter-name-purity gate (which scopes
    /// to `crates/scrap-core/src/`; tests/ uses concrete names — but
    /// this fixture lives in `src/` under `#[cfg(test)]` so it MUST
    /// use the placeholder). 13 fields per scrap-rs#21 rename.
    fn fixture_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test 2026-05-27)",
            about: "table-test fixture",
            long_about: "Test-fixture AdapterMeta for table reporter tests.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/scrap",
            rule_help_uri: "https://example.invalid/scrap/rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &["tests/**"],
            parse_hint: "ensure --src points at a workspace with test files",
        }
    }

    /// Render `emit()` to a UTF-8 string with the given options +
    /// threshold mode.
    fn render_emit(report: &Report, options: &TableOptions, mode: ThresholdMode) -> String {
        let meta = fixture_meta();
        let mut buf: Vec<u8> = Vec::new();
        emit(report, &meta, options, mode, &mut buf).expect("emit writes");
        String::from_utf8(buf).expect("output is UTF-8")
    }

    /// Count data rows in a rendered table by scanning for `│` cell
    /// dividers and dropping the header row. Robust against
    /// comfy-table cosmetic changes; the snapshot tests pin exact
    /// borders.
    ///
    /// `header_marker` is the first column's header text (`file:line`
    /// for `RowGrouping::Smell`, `File` for `RowGrouping::Finding`).
    fn count_data_rows(output: &str, header_marker: &str) -> usize {
        output
            .lines()
            .filter(|l| l.contains('│'))
            .filter(|l| !l.contains(header_marker))
            .count()
    }

    // ── RowGrouping ────────────────────────────────────────────────

    #[test]
    fn row_grouping_default_is_smell() {
        assert_eq!(RowGrouping::default(), RowGrouping::Smell);
    }

    #[test]
    fn row_grouping_serializes_kebab_case() {
        let cases = [
            (RowGrouping::Smell, "smell"),
            (RowGrouping::Finding, "finding"),
        ];
        for (grouping, wire) in cases {
            assert_eq!(
                serde_json::to_value(grouping).unwrap(),
                serde_json::Value::String(wire.into()),
            );
        }
    }

    #[test]
    fn row_grouping_deserializes_kebab_case() {
        let smell: RowGrouping =
            serde_json::from_value(serde_json::Value::String("smell".into())).unwrap();
        assert_eq!(smell, RowGrouping::Smell);

        let finding: RowGrouping =
            serde_json::from_value(serde_json::Value::String("finding".into())).unwrap();
        assert_eq!(finding, RowGrouping::Finding);
    }

    #[test]
    fn row_grouping_round_trips_through_toml() {
        // Exercises the config-file override path:
        // `[table] grouping = "finding"` in scrap.toml.
        #[derive(serde::Deserialize)]
        struct Section {
            grouping: RowGrouping,
        }
        let parsed: Section = toml::from_str("grouping = \"finding\"\n").unwrap();
        assert_eq!(parsed.grouping, RowGrouping::Finding);
    }

    // ── TableOptions ───────────────────────────────────────────────

    #[test]
    fn table_options_default_no_filter_no_truncate_no_color_smell_grouping() {
        let opts = TableOptions::default();
        assert!(opts.top.is_none(), "default top = None");
        assert!(!opts.only_failing, "default only_failing = false");
        assert!(!opts.use_color, "default use_color = false");
        assert_eq!(opts.grouping, RowGrouping::Smell);
    }

    // ── format_score helper (negative-zero normalization) ─────────

    #[test]
    fn format_score_normalizes_negative_zero_to_positive() {
        // Vec<f64>::iter().sum() on empty produces -0.0 (Rust stdlib
        // quirk; documented IEEE-754 behavior). The format_score
        // helper normalizes via `+ 0.0` so the rendered Score column
        // reads "0" not "-0".
        let neg_zero: f64 = Vec::<f64>::new().iter().sum();
        assert!(neg_zero.is_sign_negative(), "sanity: empty sum is -0.0");
        assert_eq!(format_score(neg_zero), "0", "format_score normalizes -0");
        assert_eq!(format_score(0.0), "0", "positive zero stays 0");
        assert_eq!(format_score(10.0), "10", "positive scores unchanged");
        assert_eq!(format_score(4.0), "4", "small scores unchanged");
    }

    // ── render_smell_rows ─────────────────────────────────────────

    #[test]
    fn render_smell_rows_one_finding_one_smell_renders_one_row() {
        let report = report_from(vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])]);
        let output = render_smell(&report, &TableOptions::default());
        assert_eq!(
            count_data_rows(&output, "file:line"),
            1,
            "one finding → one row"
        );
        // Spot-check all 5 columns surfaced: file:line, smell wire str,
        // severity, penalty, score.
        assert!(output.contains("a.rs:5"), "file:line populated");
        assert!(output.contains("zero_assertion"), "smell column");
        assert!(output.contains("High"), "severity column");
        assert!(output.contains(" 10 "), "penalty column");
    }

    #[test]
    fn render_smell_rows_one_finding_two_smells_renders_two_rows() {
        let report = report_from(vec![(
            "a.rs",
            vec![finding_with_n_smells("a.rs", "a::tests::t", 5, 2)],
        )]);
        let output = render_smell(&report, &TableOptions::default());
        assert_eq!(
            count_data_rows(&output, "file:line"),
            2,
            "two smells → two rows"
        );
        // Score column shows per-Finding total (5 + 5 = 10) on each row
        // — duplication is expected per D-COL-SMELL-1.
        // Count "10" occurrences in score-column cells (look for " 10 "
        // padding); should appear twice.
        let score_cell_count = output.matches(" 10 ").count();
        assert!(
            score_cell_count >= 2,
            "score column shows per-Finding total on each row (got {score_cell_count})",
        );
    }

    #[test]
    fn render_smell_rows_uses_smell_span_when_present() {
        let report = report_from(vec![(
            "a.rs",
            vec![finding_with_smell_span(
                "a.rs",
                "a::tests::t",
                10,
                Span::new(42, 42, 1, 1),
            )],
        )]);
        let output = render_smell(&report, &TableOptions::default());
        assert!(
            output.contains("a.rs:42"),
            "file:line uses Smell.span.start_line (42), not test span (5)\n{output}",
        );
        assert!(
            !output.contains("a.rs:5"),
            "test-span fallback NOT used when Smell.span is Some",
        );
    }

    #[test]
    fn render_smell_rows_falls_back_to_test_span_when_smell_span_none() {
        // finding_at creates a Smell with span = None.
        let report = report_from(vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])]);
        let output = render_smell(&report, &TableOptions::default());
        assert!(
            output.contains("a.rs:5"),
            "file:line falls back to test span start (5) when Smell.span is None\n{output}",
        );
    }

    #[test]
    fn render_smell_rows_source_order_preserved() {
        let report = report_from(vec![
            ("a.rs", vec![finding_at("a.rs", "a::tests::t1", 10)]),
            ("b.rs", vec![finding_at("b.rs", "b::tests::t2", 5)]),
            ("c.rs", vec![finding_at("c.rs", "c::tests::t3", 7)]),
        ]);
        let output = render_smell(&report, &TableOptions::default());
        let a_pos = output.find("a.rs:5").expect("a.rs row present");
        let b_pos = output.find("b.rs:5").expect("b.rs row present");
        let c_pos = output.find("c.rs:5").expect("c.rs row present");
        assert!(a_pos < b_pos, "a.rs before b.rs (source order)");
        assert!(b_pos < c_pos, "b.rs before c.rs (source order)");
    }

    #[test]
    fn render_smell_rows_only_failing_filters_zero_score() {
        let report = report_from(vec![
            ("a.rs", vec![finding_at("a.rs", "a::tests::t1", 10)]),
            ("b.rs", vec![finding_at("b.rs", "b::tests::t2", 0)]),
            ("c.rs", vec![finding_at("c.rs", "c::tests::t3", 4)]),
        ]);
        let opts = TableOptions {
            only_failing: true,
            ..TableOptions::default()
        };
        let output = render_smell(&report, &opts);
        assert_eq!(
            count_data_rows(&output, "file:line"),
            2,
            "zero-score Finding filtered → 2 rows",
        );
        assert!(!output.contains("b.rs:5"), "b.rs (zero-score) absent");
    }

    #[test]
    fn render_smell_rows_top_truncates_rows_not_findings() {
        // Single Finding with 3 Smells; top=2 → 2 rows (mid-Finding break).
        let report = report_from(vec![(
            "a.rs",
            vec![finding_with_n_smells("a.rs", "a::tests::t", 5, 3)],
        )]);
        let opts = TableOptions {
            top: Some(NonZeroUsize::new(2).expect("non-zero")),
            ..TableOptions::default()
        };
        let output = render_smell(&report, &opts);
        assert_eq!(
            count_data_rows(&output, "file:line"),
            2,
            "top=2 truncates to 2 rows (mid-Finding break acceptable)",
        );
    }

    #[test]
    fn render_smell_rows_empty_report_renders_zero_rows() {
        let report = Report::default();
        let output = render_smell(&report, &TableOptions::default());
        assert_eq!(
            count_data_rows(&output, "file:line"),
            0,
            "no findings → no rows"
        );
        // Header still present (column headers in border-rendered table).
        assert!(
            output.contains("file:line"),
            "header row still rendered on empty report",
        );
    }

    #[test]
    fn render_smell_rows_only_failing_with_all_zero_findings_renders_zero_rows() {
        let report = report_from(vec![
            ("a.rs", vec![finding_at("a.rs", "a::tests::t1", 0)]),
            ("b.rs", vec![finding_at("b.rs", "b::tests::t2", 0)]),
        ]);
        let opts = TableOptions {
            only_failing: true,
            ..TableOptions::default()
        };
        let output = render_smell(&report, &opts);
        assert_eq!(
            count_data_rows(&output, "file:line"),
            0,
            "all-zero + only_failing → 0 rows",
        );
    }

    // ── Cabinet fold-in tests (CQO A1 + A2 absorbed at /plan close) ─

    #[test]
    fn render_smell_rows_zero_smell_finding_only_failing_false_renders_zero_rows() {
        // Cabinet CQO A1 fold-in 2026-05-27: Finding with no Smells
        // (zero-score) + only_failing = false → zero rows rendered
        // (no inner loop iteration because Finding.smells is empty).
        // Distinct from `..._all_zero_findings_...` because
        // only_failing = false; the zero-smells-vs-zero-score
        // boundary is structural, not filter-driven.
        let report = report_from(vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 0)])]);
        let opts = TableOptions::default(); // only_failing = false
        let output = render_smell(&report, &opts);
        assert_eq!(
            count_data_rows(&output, "file:line"),
            0,
            "zero-smell Finding renders 0 rows under Smell grouping (no inner loop iteration)",
        );
    }

    #[test]
    fn render_smell_rows_top_no_truncate_when_n_gte_total_rows() {
        // Cabinet CQO A2 fold-in 2026-05-27, mirror of json.rs
        // `view_top_no_truncate_when_n_gte_eligible`: 3 rows + top=5
        // → all 3 rows present; no truncation triggered.
        let report = report_from(vec![
            ("a.rs", vec![finding_at("a.rs", "a::tests::t1", 10)]),
            ("b.rs", vec![finding_at("b.rs", "b::tests::t2", 10)]),
            ("c.rs", vec![finding_at("c.rs", "c::tests::t3", 10)]),
        ]);
        let opts = TableOptions {
            top: Some(NonZeroUsize::new(5).expect("non-zero")),
            ..TableOptions::default()
        };
        let output = render_smell(&report, &opts);
        assert_eq!(
            count_data_rows(&output, "file:line"),
            3,
            "top=5 vs 3 total rows → all 3 present (no truncation)",
        );
    }

    // ── render_finding_rows (W4) ──────────────────────────────────

    #[test]
    fn render_finding_rows_one_finding_renders_one_row() {
        let report = report_from(vec![(
            "a.rs",
            vec![finding_with_n_smells("a.rs", "a::tests::t", 5, 2)],
        )]);
        let output = render_finding(&report, &TableOptions::default());
        assert_eq!(
            count_data_rows(&output, "File"),
            1,
            "single Finding → one row",
        );
        // Smells cell lists categories comma-separated (both ZeroAssertion).
        assert!(
            output.contains("zero_assertion, zero_assertion"),
            "Smells column lists categories comma-separated\n{output}",
        );
        // Score column shows per-Finding total (5 + 5 = 10).
        assert!(output.contains(" 10 "), "Score column shows Finding total");
    }

    #[test]
    fn render_finding_rows_zero_smell_finding_renders_empty_smells_cell() {
        let report = report_from(vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 0)])]);
        let output = render_finding(&report, &TableOptions::default());
        assert_eq!(count_data_rows(&output, "File"), 1, "row present");
        // Pass/Fail = PASS (exceeds_threshold defaults to false).
        assert!(
            output.contains("PASS"),
            "Pass/Fail = PASS for zero-score Finding without exceeds_threshold flag\n{output}",
        );
        // Score column shows "0" (not "-0") thanks to format_score
        // helper that normalizes IEEE-754 negative-zero (from empty
        // Vec<f64>::iter().sum() in Finding::new).
        let data_row = output
            .lines()
            .find(|l| l.contains('│') && l.contains("a::tests::t"))
            .expect("data row present");
        assert!(
            data_row.contains(" 0 "),
            "data row contains '0' score cell (no negative-zero): {data_row}",
        );
        assert!(
            !data_row.contains("-0"),
            "score must not render as -0: {data_row}",
        );
    }

    #[test]
    fn render_finding_rows_pass_fail_reflects_exceeds_threshold() {
        let report = report_from(vec![
            (
                "a.rs",
                vec![finding_exceeding("a.rs", "a::tests::failing", 10)],
            ),
            ("b.rs", vec![finding_at("b.rs", "b::tests::passing", 3)]),
        ]);
        let output = render_finding(&report, &TableOptions::default());
        // FAIL row for a.rs::failing (exceeds_threshold = true).
        // PASS row for b.rs::passing (exceeds_threshold = false).
        assert!(output.contains("FAIL"), "FAIL row present for exceeding");
        assert!(
            output.contains("PASS"),
            "PASS row present for non-exceeding"
        );
        // Spot-check both qualified names surfaced.
        assert!(output.contains("a::tests::failing"));
        assert!(output.contains("b::tests::passing"));
    }

    #[test]
    fn render_finding_rows_source_order_preserved() {
        let report = report_from(vec![
            ("a.rs", vec![finding_at("a.rs", "a::tests::t1", 10)]),
            ("b.rs", vec![finding_at("b.rs", "b::tests::t2", 5)]),
        ]);
        let output = render_finding(&report, &TableOptions::default());
        let a_pos = output.find("a::tests::t1").expect("a row present");
        let b_pos = output.find("b::tests::t2").expect("b row present");
        assert!(a_pos < b_pos, "source order preserved");
    }

    #[test]
    fn render_finding_rows_only_failing_filters_zero_score() {
        let report = report_from(vec![
            ("a.rs", vec![finding_at("a.rs", "a::tests::t1", 10)]),
            ("b.rs", vec![finding_at("b.rs", "b::tests::t2", 0)]),
        ]);
        let opts = TableOptions {
            only_failing: true,
            ..TableOptions::default()
        };
        let output = render_finding(&report, &opts);
        assert_eq!(
            count_data_rows(&output, "File"),
            1,
            "only_failing drops zero-score Finding",
        );
        assert!(
            !output.contains("a::tests::t2"),
            "b.rs Finding filtered out"
        );
    }

    #[test]
    fn render_finding_rows_top_truncates() {
        let report = report_from(vec![
            ("a.rs", vec![finding_at("a.rs", "a::tests::t1", 10)]),
            ("b.rs", vec![finding_at("b.rs", "b::tests::t2", 5)]),
            ("c.rs", vec![finding_at("c.rs", "c::tests::t3", 7)]),
        ]);
        let opts = TableOptions {
            top: Some(NonZeroUsize::new(2).expect("non-zero")),
            ..TableOptions::default()
        };
        let output = render_finding(&report, &opts);
        assert_eq!(
            count_data_rows(&output, "File"),
            2,
            "top=2 truncates to 2 rows",
        );
    }

    #[test]
    fn render_finding_rows_empty_report_renders_zero_rows() {
        let report = Report::default();
        let output = render_finding(&report, &TableOptions::default());
        assert_eq!(count_data_rows(&output, "File"), 0, "no findings → no rows");
        // Header still present.
        assert!(output.contains("File"), "header still rendered");
    }

    // ── emit dispatch + header + footer (W5) ──────────────────────

    /// Build a Report with summary counts pre-populated for the
    /// header line tests (header reads from `report.summary.*`).
    fn report_with_summary(
        files: Vec<(&str, Vec<Finding>)>,
        total_tests: u32,
        exceeding_threshold: u32,
        passed: bool,
    ) -> Report {
        let mut report = report_from(files);
        report.summary.total_tests = total_tests;
        report.summary.exceeding_threshold = exceeding_threshold;
        report.passed = passed;
        report
    }

    #[test]
    fn emit_writes_header_line_with_tool_and_summary() {
        let report = report_with_summary(
            vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])],
            1,
            1,
            false,
        );
        let output = render_emit(&report, &TableOptions::default(), ThresholdMode::Default);
        // Header is the first line.
        let header = output.lines().next().expect("header present");
        assert!(header.contains("test-adapter"), "tool name in header");
        assert!(header.contains("0.1.0"), "tool_version in header");
        assert!(
            header.contains("1 tests inspected"),
            "total_tests in header: {header}",
        );
        assert!(
            header.contains("1 findings"),
            "total findings in header: {header}",
        );
        assert!(
            header.contains("1 exceeding 'default' threshold"),
            "exceeding + threshold-mode in header: {header}",
        );
    }

    #[test]
    fn emit_writes_footer_passed_when_report_passed_true() {
        // Empty report — no findings, no exceedances; report.passed = true.
        let report = report_with_summary(vec![], 0, 0, true);
        let output = render_emit(&report, &TableOptions::default(), ThresholdMode::Default);
        let footer = output.lines().last().expect("footer present");
        assert!(
            footer.contains("PASSED"),
            "PASSED verdict for report.passed=true: {footer}",
        );
        assert!(
            footer.contains("'default' mode"),
            "threshold mode in footer: {footer}",
        );
        assert!(
            !footer.contains("FAILED"),
            "no FAILED in PASSED footer: {footer}",
        );
    }

    #[test]
    fn emit_writes_footer_failed_when_report_passed_false() {
        let report = report_with_summary(
            vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])],
            1,
            1,
            false,
        );
        let output = render_emit(&report, &TableOptions::default(), ThresholdMode::Default);
        let footer = output.lines().last().expect("footer present");
        assert!(
            footer.contains("FAILED"),
            "FAILED verdict for report.passed=false: {footer}",
        );
        assert!(
            footer.contains("1 of 1 tests exceed threshold"),
            "exceeding-of-total in footer: {footer}",
        );
        assert!(
            footer.contains("'default' mode"),
            "threshold mode in footer: {footer}",
        );
    }

    #[test]
    fn emit_dispatches_to_smell_renderer_by_default() {
        let report = report_with_summary(
            vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])],
            1,
            1,
            false,
        );
        let output = render_emit(&report, &TableOptions::default(), ThresholdMode::Default);
        // Smell layout has `file:line` + `Penalty` columns.
        assert!(
            output.contains("file:line"),
            "Smell layout header signature (file:line column) present\n{output}",
        );
        assert!(
            output.contains("Penalty"),
            "Smell layout header signature (Penalty column) present",
        );
    }

    #[test]
    fn emit_dispatches_to_finding_renderer_when_grouping_finding() {
        let report = report_with_summary(
            vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])],
            1,
            1,
            false,
        );
        let opts = TableOptions {
            grouping: RowGrouping::Finding,
            ..TableOptions::default()
        };
        let output = render_emit(&report, &opts, ThresholdMode::Default);
        // Finding layout has `Test`, `Smells`, `Pass/Fail` columns.
        assert!(
            output.contains("Test"),
            "Finding layout (Test column) present\n{output}",
        );
        assert!(
            output.contains("Smells"),
            "Finding layout (Smells column) present",
        );
        assert!(
            output.contains("Pass/Fail"),
            "Finding layout (Pass/Fail column) present",
        );
        // Smell-layout signature columns absent.
        assert!(
            !output.contains("file:line"),
            "no Smell-layout file:line column under Finding grouping",
        );
        assert!(
            !output.contains("Penalty"),
            "no Smell-layout Penalty column under Finding grouping",
        );
    }

    #[test]
    fn emit_threshold_mode_strict_appears_in_footer() {
        let report = report_with_summary(vec![], 0, 0, true);
        let output = render_emit(&report, &TableOptions::default(), ThresholdMode::Strict);
        let footer = output.lines().last().expect("footer present");
        assert!(
            footer.contains("'strict' mode"),
            "threshold mode 'strict' in footer: {footer}",
        );
        // And the header echoes it too.
        let header = output.lines().next().expect("header present");
        assert!(
            header.contains("'strict' threshold"),
            "threshold mode 'strict' in header: {header}",
        );
    }

    // ── ANSI escape verification (W6) ─────────────────────────────

    #[test]
    fn emit_use_color_true_emits_ansi_escapes_in_output() {
        // Fixture with a High-severity smell → severity column gets
        // Red (ANSI code 31) when use_color = true.
        let report = report_with_summary(
            vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])],
            1,
            1,
            false,
        );
        let opts = TableOptions {
            use_color: true,
            ..TableOptions::default()
        };
        let output = render_emit(&report, &opts, ThresholdMode::Default);
        assert!(
            output.contains("\x1b["),
            "use_color=true emits ANSI escape sequences",
        );
        // Severity High → Red foreground in cell. comfy-table renders
        // Red as `\x1b[38;5;9m` (the bright red 256-color form) or
        // `\x1b[31m` (basic 8-color form). Don't pin which encoding;
        // just verify SOME escape sequence is present.
    }

    #[test]
    fn emit_use_color_false_emits_no_ansi_escapes() {
        let report = report_with_summary(
            vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])],
            1,
            1,
            false,
        );
        let opts = TableOptions {
            use_color: false,
            ..TableOptions::default()
        };
        let output = render_emit(&report, &opts, ThresholdMode::Default);
        assert!(
            !output.contains("\x1b["),
            "use_color=false emits no ANSI escape sequences",
        );
    }

    #[test]
    fn emit_footer_passed_use_color_true_contains_green_escape() {
        // report.passed = true → footer verdict colored green
        // (color_code(Color::Green) = Some(32)).
        let report = report_with_summary(vec![], 0, 0, true);
        let opts = TableOptions {
            use_color: true,
            ..TableOptions::default()
        };
        let output = render_emit(&report, &opts, ThresholdMode::Default);
        let footer = output.lines().last().expect("footer present");
        assert!(
            footer.contains("\x1b[32m"),
            "PASSED footer wrapped in ANSI green (32): {footer}",
        );
        assert!(
            footer.contains("\x1b[0m"),
            "PASSED footer terminated by ANSI reset: {footer}",
        );
    }

    #[test]
    fn emit_footer_failed_use_color_true_contains_red_escape() {
        let report = report_with_summary(
            vec![("a.rs", vec![finding_at("a.rs", "a::tests::t", 10)])],
            1,
            1,
            false,
        );
        let opts = TableOptions {
            use_color: true,
            ..TableOptions::default()
        };
        let output = render_emit(&report, &opts, ThresholdMode::Default);
        let footer = output.lines().last().expect("footer present");
        assert!(
            footer.contains("\x1b[31m"),
            "FAILED footer wrapped in ANSI red (31): {footer}",
        );
        assert!(
            footer.contains("\x1b[0m"),
            "FAILED footer terminated by ANSI reset: {footer}",
        );
    }

    // ── color_code helper (CEng S1 fold-in coverage) ──────────────

    #[test]
    fn color_code_returns_some_for_green_and_red() {
        assert_eq!(color_code(Color::Green), Some(32));
        assert_eq!(color_code(Color::Red), Some(31));
    }

    #[test]
    fn color_code_returns_none_for_unmapped_variants() {
        // The impossible-by-construction path — write_footer only ever
        // calls with Green or Red — but the Option<u8> API surfaces
        // the unmapped case rather than silently returning the ANSI
        // reset code (cabinet CEng S1 fold-in 2026-05-27).
        assert_eq!(
            color_code(Color::Yellow),
            None,
            "Yellow is not mapped (Some(33) would conflict with future detector severity colors)",
        );
        assert_eq!(color_code(Color::DarkGrey), None);
        assert_eq!(color_code(Color::Reset), None);
    }

    // ── Insta snapshots — no-color path only (W7, D-SNAP-1) ───────
    //
    // Per cabinet CQO S2 fold-in 2026-05-27: first-time snapshot
    // bless requires eyeball against D-COL-SMELL-1 / D-COL-FINDING-1
    // / D-HEADER-1 / D-FOOTER-1 expected layout BEFORE
    // `cargo insta accept --unreviewed`. Subsequent re-bless on
    // comfy-table version bumps: `cargo insta review` interactive
    // — NEVER blind accept.

    /// Snapshot fixture: 2 files, 3 Findings, mixed severity, mixed
    /// scores. Stays small for reviewer eyeball.
    /// - `a.rs::tests::t_high` — High severity, `ZeroAssertion`, penalty 10
    /// - `b.rs::tests::t_low`  — Low severity, `LargeExample`, penalty 4
    /// - `b.rs::tests::t_zero` — zero smells (zero score, PASS)
    fn snapshot_fixture() -> Report {
        let t_high = {
            let test = TestIdentity::new(
                FilePath::new("a.rs"),
                QualifiedName::new("a::tests::t_high"),
                Span::new(10, 18, 1, 1),
            );
            let mut f = Finding::new(
                test,
                vec![Smell::new(
                    SmellCategory::ZeroAssertion,
                    Severity::High,
                    Actionability::AutoRefactor,
                    10,
                    None,
                )],
            );
            f.exceeds_threshold = true;
            f
        };
        let t_low = {
            let test = TestIdentity::new(
                FilePath::new("b.rs"),
                QualifiedName::new("b::tests::t_low"),
                Span::new(5, 9, 1, 1),
            );
            Finding::new(
                test,
                vec![Smell::new(
                    SmellCategory::LargeExample,
                    Severity::Low,
                    Actionability::ManualSplit,
                    4,
                    None,
                )],
            )
        };
        let t_zero = {
            let test = TestIdentity::new(
                FilePath::new("b.rs"),
                QualifiedName::new("b::tests::t_zero"),
                Span::new(12, 14, 1, 1),
            );
            Finding::new(test, vec![])
        };

        let files = vec![
            FileReport::new(FilePath::new("a.rs"), vec![t_high]),
            FileReport::new(FilePath::new("b.rs"), vec![t_low, t_zero]),
        ];
        let mut report = Report {
            files,
            ..Report::default()
        };
        report.summary.total_tests = 3;
        report.summary.total_files = 2;
        report.summary.exceeding_threshold = 1;
        report.passed = false;
        report
    }

    #[test]
    fn snapshot_smell_grouping_default_options_no_color() {
        let report = snapshot_fixture();
        let opts = TableOptions::default(); // grouping = Smell, no color
        let output = render_emit(&report, &opts, ThresholdMode::Default);
        insta::assert_snapshot!(output);
    }

    #[test]
    fn snapshot_finding_grouping_default_options_no_color() {
        let report = snapshot_fixture();
        let opts = TableOptions {
            grouping: RowGrouping::Finding,
            ..TableOptions::default()
        };
        let output = render_emit(&report, &opts, ThresholdMode::Default);
        insta::assert_snapshot!(output);
    }
}
