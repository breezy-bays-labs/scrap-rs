//! GitHub Actions inline-annotations reporter (peer format).
//!
//! Emits `::warning` workflow-command lines so test-smell findings
//! render inline on the PR "Files changed" tab — universal, free, no
//! GHAS / Code Scanning dependency. The GitHub Actions runner
//! intercepts the `::warning file=…,line=…,col=…,title=…::message`
//! shape from stdout and renders an inline annotation at the named
//! location, with `title=` as the annotation header (crap-rs parity).
//!
//! Free function `emit()` per the reporter design (see
//! [`crate::adapters::reporters`] module docstring). A PEER format to
//! SARIF (scrap-rs#17 D1, mirroring crap-rs#276's multi-format-with-
//! per-sink model), NOT a flag on the SARIF reporter — canonical usage
//! is `--format sarif:results.sarif,github-annotations` (SARIF to a
//! file, annotations to stdout for the runner).
//!
//! Like SARIF, this is a *gate translation*, not a display: it iterates
//! every smell across the report regardless of view-shaping flags
//! (`--top`, `--only-failing`). It sorts by penalty DESC itself, then
//! truncates at `annotation_limit`.
//!
//! Granularity is per-Smell (scrap-rs#17 D2) — one `::warning` per
//! smell, consistent with the SARIF reporter.
//!
//! GitHub Actions silently drops annotations past a per-step cap (10
//! warning + 10 error + 10 notice per step). The configurable
//! `annotation_limit` plus a trailing `::notice` summary line are the
//! user-visible mitigation; the runner cap is the underlying constraint.
//!
//! Spec: <https://docs.github.com/en/actions/using-workflows/workflow-commands-for-github-actions>

use std::fmt::Write as _;
use std::path::Path;

use crate::adapter_meta::AdapterMeta;
use crate::domain::report::Report;
use crate::domain::types::Span;

/// One annotation candidate, projected from a smell + its enclosing
/// finding. Carries the sort keys (penalty, file, line, column) and the
/// pre-projected display fields.
struct Annotation {
    penalty: u32,
    file: String,
    line: u32,
    column: u32,
    title: String,
    message: String,
}

/// Emit GitHub Actions `::warning` workflow commands for every smell in
/// the report, sorted by penalty DESC, truncated at `annotation_limit`.
///
/// One `::warning file=X,line=Y,col=Z,title=T::message` per smell
/// (scrap-rs#17 D2 per-Smell granularity), where `T` is the smell's
/// wire string + penalty. Location is `smell.span.unwrap_or(test
/// span)`. When the eligible set exceeds `annotation_limit`, the top-N
/// are emitted and a single trailing `::notice` line names the dropped
/// count so reviewers know findings were withheld.
///
/// `meta` is accepted for signature parity with the other reporters
/// (the adapter binary threads it via `AdapterMeta`); workflow commands
/// have no driver/version slot so it is not embedded in the lines.
///
/// # Errors
///
/// Returns [`std::io::Error`] on writer failure.
pub fn emit<W: std::io::Write>(
    report: &Report,
    meta: &AdapterMeta,
    annotation_limit: usize,
    writer: &mut W,
) -> Result<(), std::io::Error> {
    let _ = meta; // parity arg — no driver/version slot in workflow cmds
    let cwd = std::env::current_dir().ok();
    let rendered = render(report, annotation_limit, cwd.as_deref());
    writer.write_all(rendered.as_bytes())
}

/// Pure projection: report → workflow-command string. `cwd` is
/// parameterized so tests can pin the relativization prefix without
/// chdir'ing the process; `emit` threads
/// `std::env::current_dir().ok().as_deref()`.
fn render(report: &Report, annotation_limit: usize, cwd: Option<&Path>) -> String {
    let mut eligible: Vec<Annotation> = report
        .files
        .iter()
        .flat_map(|file| file.findings.iter())
        .flat_map(|finding| {
            let file_path = finding.test.file_path.to_string();
            let test_span = finding.test.span;
            finding.smells.iter().map(move |smell| {
                let span: Span = smell.span.unwrap_or(test_span);
                Annotation {
                    penalty: smell.penalty,
                    file: file_path.clone(),
                    line: span.start_line,
                    column: span.start_column,
                    // Smell wire string + penalty — the per-smell analog
                    // of crap-rs's `title=CRAP {score}` (cross-tool
                    // annotation-header parity), and the same identity
                    // SARIF uses for `ruleId`.
                    title: format!(
                        "{} (penalty {})",
                        smell.category.as_wire_str(),
                        smell.penalty
                    ),
                    message: smell.ai_actionability_message.clone(),
                }
            })
        })
        .collect();

    // Penalty DESC primary; tie-break on (file, line, column) ASC so
    // equal-penalty runs are deterministic across walker orderings.
    eligible.sort_by(|a, b| {
        b.penalty
            .cmp(&a.penalty)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.column.cmp(&b.column))
    });

    let total = eligible.len();
    let take = total.min(annotation_limit);

    let mut out = String::new();
    for ann in eligible.iter().take(take) {
        // Two escape contexts per the GH Actions workflow-command spec:
        //   * property values (file=) escape %, CR, LF, plus the
        //     delimiters `:` and `,` (POSIX paths legally contain both)
        //   * message data (after `::`) escapes only %, CR, LF
        // `line` / `col` are integers (no escape needed).
        let file = gha_escape_property(&relativize_path(&ann.file, cwd));
        let title = gha_escape_property(&ann.title);
        let message = gha_escape(&ann.message);
        // `writeln!` into a String is infallible (the `fmt::Write` impl
        // for String never errors); the `let _` discards the Result.
        let _ = writeln!(
            out,
            "::warning file={file},line={line},col={col},title={title}::{message}",
            file = file,
            line = ann.line,
            col = ann.column,
            title = title,
            message = message,
        );
    }

    let dropped = total.saturating_sub(take);
    if dropped > 0 {
        let _ = writeln!(
            out,
            "::notice::{dropped} more test smells detected; see the full report for the complete list",
        );
    }

    out
}

/// Percent-encode the three characters that would terminate or corrupt
/// a workflow-command message: `%`, `\r`, `\n` (the message-data escape,
/// applied to text after the final `::`). `%` is escaped first so the
/// `%25` from the CR/LF substitutions is not re-escaped.
fn gha_escape(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

/// Percent-encode all five characters the GH Actions spec requires in
/// property-value positions: `%`, `\r`, `\n`, plus the property-list
/// delimiters `:` and `,`. The runner parses each annotation as
/// `name=value,name=value,…::message`, so an unescaped `:` or `,` in a
/// dynamic value (most realistically a `file=` path on POSIX) would
/// split the property list and corrupt the annotation.
fn gha_escape_property(s: &str) -> String {
    gha_escape(s).replace(':', "%3A").replace(',', "%2C")
}

/// Strip a CWD prefix from `file_path` so PR annotations reference files
/// by repo-relative path (which GitHub renders inline on the diff).
/// Returns the original path unchanged when it is already relative, no
/// CWD is available, or it does not live under CWD.
fn relativize_path(file_path: &str, cwd: Option<&Path>) -> String {
    let p = Path::new(file_path);
    if !p.is_absolute() {
        return file_path.to_string();
    }
    match cwd.and_then(|c| p.strip_prefix(c).ok()) {
        Some(rel) => rel.to_string_lossy().into_owned(),
        None => file_path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::finding::Finding;
    use crate::domain::report::{FileReport, Report};
    use crate::domain::smell::{Smell, SmellCategory};
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

    fn meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test)",
            about: "annotations test fixture",
            long_about: "Test-fixture AdapterMeta for the github-annotations reporter.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/test-adapter",
            rule_help_uri: "https://example.invalid/test-adapter#rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &[],
            parse_hint: "hint",
        }
    }

    fn finding_with(path: &str, name: &str, test_span: Span, smells: Vec<Smell>) -> Finding {
        let test = TestIdentity::new(FilePath::new(path), QualifiedName::new(name), test_span);
        Finding::new(test, smells)
    }

    fn report_with(findings: Vec<Finding>) -> Report {
        use std::collections::BTreeMap;
        let mut by_path: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
        for f in findings {
            let key = f.test.file_path.as_path().to_string_lossy().into_owned();
            by_path.entry(key).or_default().push(f);
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

    fn smell_with(
        category: SmellCategory,
        severity: Severity,
        penalty: u32,
        span: Option<Span>,
        message: &str,
    ) -> Smell {
        Smell::with_message(
            category,
            severity,
            Actionability::AutoRefactor,
            penalty,
            span,
            message,
        )
    }

    /// Render with no CWD so absolute paths pass through unchanged
    /// (keeps tests independent of the process working directory).
    fn render_no_cwd(report: &Report, limit: usize) -> String {
        render(report, limit, None)
    }

    #[test]
    fn empty_report_produces_empty_output() {
        let report = report_with(vec![]);
        assert_eq!(render_no_cwd(&report, usize::MAX), "");
    }

    #[test]
    fn single_smell_emits_one_warning_with_file_line_col() {
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(9, 13, 5, 2),
            vec![smell_with(
                SmellCategory::ZeroAssertion,
                Severity::High,
                10,
                None,
                "Add assertions.",
            )],
        );
        let report = report_with(vec![finding]);
        let out = render_no_cwd(&report, usize::MAX);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 1, "one warning line, got {lines:?}");
        let line = lines[0];
        assert!(line.starts_with("::warning "), "wrong prefix: {line}");
        assert!(line.contains("file=src/lib.rs"), "missing file: {line}");
        assert!(line.contains("line=9"), "uses test span start_line: {line}");
        assert!(
            line.contains("col=5"),
            "uses test span start_column: {line}"
        );
        assert!(
            line.contains("title=zero_assertion (penalty 10)"),
            "title names the smell + penalty (crap-rs parity): {line}"
        );
        assert!(line.ends_with("::Add assertions."), "message tail: {line}");
    }

    #[test]
    fn title_property_is_escaped_and_precedes_message_delimiter() {
        // The title is built from wire strings + integers (no specials
        // today), but it sits in property position — pin that it goes
        // through the property escape and lands before the `::` message
        // delimiter so a future dynamic title can't corrupt the line.
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(3, 4, 1, 2),
            vec![smell_with(
                SmellCategory::LargeExample,
                Severity::Low,
                4,
                None,
                "shrink",
            )],
        );
        let report = report_with(vec![finding]);
        let out = render_no_cwd(&report, usize::MAX);
        let line = out.lines().next().expect("one line");
        let (props, message) = line
            .rsplit_once("::")
            .expect("workflow-command delimiter present");
        assert!(
            props.contains("title=large_example (penalty 4)"),
            "title in property list: {props}"
        );
        assert_eq!(message, "shrink");
    }

    #[test]
    fn one_warning_per_smell_not_per_finding() {
        // Per-Smell granularity (D2): a 2-smell finding → 2 warnings.
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(1, 10, 1, 1),
            vec![
                smell_with(SmellCategory::ZeroAssertion, Severity::High, 10, None, "m1"),
                smell_with(SmellCategory::LargeExample, Severity::Low, 4, None, "m2"),
            ],
        );
        let report = report_with(vec![finding]);
        let warnings = render_no_cwd(&report, usize::MAX)
            .lines()
            .filter(|l| l.starts_with("::warning "))
            .count();
        assert_eq!(warnings, 2);
    }

    #[test]
    fn warning_uses_smell_span_when_present() {
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(1, 20, 1, 1),
            vec![smell_with(
                SmellCategory::TautologicalAssertion,
                Severity::High,
                10,
                Some(Span::new(7, 7, 12, 30)),
                "fix",
            )],
        );
        let report = report_with(vec![finding]);
        let line = render_no_cwd(&report, usize::MAX);
        assert!(line.contains("line=7"), "smell span line: {line}");
        assert!(line.contains("col=12"), "smell span column: {line}");
    }

    #[test]
    fn zero_smell_finding_emits_nothing() {
        let finding = finding_with("src/lib.rs", "tests::it", Span::new(1, 5, 1, 1), vec![]);
        let report = report_with(vec![finding]);
        assert_eq!(render_no_cwd(&report, usize::MAX), "");
    }

    #[test]
    fn output_sorted_by_penalty_desc() {
        let report = report_with(vec![
            finding_with(
                "a.rs",
                "a::low",
                Span::new(1, 2, 1, 1),
                vec![smell_with(
                    SmellCategory::LargeExample,
                    Severity::Low,
                    4,
                    None,
                    "low",
                )],
            ),
            finding_with(
                "b.rs",
                "b::high",
                Span::new(1, 2, 1, 1),
                vec![smell_with(
                    SmellCategory::ZeroAssertion,
                    Severity::High,
                    10,
                    None,
                    "high",
                )],
            ),
            finding_with(
                "c.rs",
                "c::mid",
                Span::new(1, 2, 1, 1),
                vec![smell_with(
                    SmellCategory::SurfaceOnlyIo,
                    Severity::Moderate,
                    6,
                    None,
                    "mid",
                )],
            ),
        ]);
        let out = render_no_cwd(&report, usize::MAX);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].contains("high"), "penalty 10 first: {out}");
        assert!(lines[1].contains("mid"), "penalty 6 second: {out}");
        assert!(lines[2].contains("low"), "penalty 4 third: {out}");
    }

    #[test]
    fn equal_penalty_ties_break_by_file_then_line_then_col() {
        let report = report_with(vec![
            finding_with(
                "z.rs",
                "z::t",
                Span::new(1, 2, 1, 1),
                vec![smell_with(
                    SmellCategory::ZeroAssertion,
                    Severity::High,
                    10,
                    None,
                    "z",
                )],
            ),
            finding_with(
                "a.rs",
                "a::late",
                Span::new(10, 11, 1, 1),
                vec![smell_with(
                    SmellCategory::ZeroAssertion,
                    Severity::High,
                    10,
                    None,
                    "a_late",
                )],
            ),
            finding_with(
                "a.rs",
                "a::early",
                Span::new(5, 6, 1, 1),
                vec![smell_with(
                    SmellCategory::ZeroAssertion,
                    Severity::High,
                    10,
                    None,
                    "a_early",
                )],
            ),
        ]);
        let out = render_no_cwd(&report, usize::MAX);
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[0].contains("a_early"), "a.rs:5 first: {out}");
        assert!(lines[1].contains("a_late"), "a.rs:10 second: {out}");
        assert!(lines[2].contains('z'), "z.rs last: {out}");
    }

    #[test]
    fn truncation_emits_top_n_and_dropped_notice() {
        let report = report_with(vec![
            finding_with(
                "a.rs",
                "a::worst",
                Span::new(1, 2, 1, 1),
                vec![smell_with(
                    SmellCategory::ZeroAssertion,
                    Severity::High,
                    10,
                    None,
                    "worst",
                )],
            ),
            finding_with(
                "b.rs",
                "b::bad",
                Span::new(1, 2, 1, 1),
                vec![smell_with(
                    SmellCategory::SurfaceOnlyIo,
                    Severity::Moderate,
                    6,
                    None,
                    "bad",
                )],
            ),
            finding_with(
                "c.rs",
                "c::least",
                Span::new(1, 2, 1, 1),
                vec![smell_with(
                    SmellCategory::LargeExample,
                    Severity::Low,
                    4,
                    None,
                    "least",
                )],
            ),
        ]);
        let out = render_no_cwd(&report, 1);
        let warnings: Vec<&str> = out
            .lines()
            .filter(|l| l.starts_with("::warning "))
            .collect();
        assert_eq!(warnings.len(), 1, "limit=1 keeps top 1: {out}");
        assert!(warnings[0].contains("worst"));
        let notices: Vec<&str> = out.lines().filter(|l| l.starts_with("::notice")).collect();
        assert_eq!(notices.len(), 1, "one trailing notice: {out}");
        assert_eq!(
            notices[0],
            "::notice::2 more test smells detected; see the full report for the complete list",
        );
    }

    #[test]
    fn no_notice_when_limit_not_exceeded() {
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(1, 2, 1, 1),
            vec![smell_with(
                SmellCategory::ZeroAssertion,
                Severity::High,
                10,
                None,
                "m",
            )],
        );
        let report = report_with(vec![finding]);
        let out = render_no_cwd(&report, 10);
        assert_eq!(out.lines().filter(|l| l.starts_with("::notice")).count(), 0);
        assert_eq!(
            out.lines().filter(|l| l.starts_with("::warning ")).count(),
            1
        );
    }

    // ── Escaping ───────────────────────────────────────────────────

    #[test]
    fn message_escapes_percent_cr_lf() {
        let raw = "weird%name\rwith\nbreaks";
        assert_eq!(gha_escape(raw), "weird%25name%0Dwith%0Abreaks");
    }

    #[test]
    fn message_leaves_colons_and_commas_alone() {
        assert_eq!(
            gha_escape("module::submodule, function"),
            "module::submodule, function",
            "`:` and `,` are legal in message data",
        );
    }

    #[test]
    fn property_escape_covers_colon_and_comma() {
        assert_eq!(
            gha_escape_property("src:weird,file.rs"),
            "src%3Aweird%2Cfile.rs"
        );
        assert_eq!(gha_escape_property("f%o.rs"), "f%25o.rs");
        assert_eq!(gha_escape_property("src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn file_property_escapes_delimiters_in_path() {
        let finding = finding_with(
            "src/a:b,c.rs",
            "tests::it",
            Span::new(1, 2, 1, 1),
            vec![smell_with(
                SmellCategory::ZeroAssertion,
                Severity::High,
                10,
                None,
                "m",
            )],
        );
        let report = report_with(vec![finding]);
        let out = render_no_cwd(&report, usize::MAX);
        let line = out.lines().next().expect("one warning line");
        assert!(
            line.contains("file=src/a%3Ab%2Cc.rs"),
            "escaped path: {line}"
        );
        // Exactly three `,` separators between four properties
        // (file/line/col/title), then the `::` data marker. The title's
        // own content is property-escaped, so a literal `,` can never
        // smuggle in a fourth separator.
        let props = line.split("::").nth(1).expect("`::` present");
        assert_eq!(
            props.matches(',').count(),
            3,
            "file/line/col/title props: {props}"
        );
    }

    // ── relativize_path ────────────────────────────────────────────

    #[test]
    fn relativize_strips_cwd_prefix() {
        let cwd = Path::new("/home/user/repo");
        assert_eq!(
            relativize_path("/home/user/repo/src/lib.rs", Some(cwd)),
            "src/lib.rs"
        );
    }

    #[test]
    fn relativize_falls_back_when_outside_cwd() {
        let cwd = Path::new("/home/user/repo");
        assert_eq!(
            relativize_path("/elsewhere/file.rs", Some(cwd)),
            "/elsewhere/file.rs"
        );
    }

    #[test]
    fn relativize_passes_through_relative_paths() {
        let cwd = Path::new("/home/user/repo");
        assert_eq!(relativize_path("src/lib.rs", Some(cwd)), "src/lib.rs");
    }

    #[test]
    fn relativize_handles_no_cwd() {
        assert_eq!(relativize_path("/abs/file.rs", None), "/abs/file.rs");
    }

    // ── emit (writer) smoke ────────────────────────────────────────

    #[test]
    fn emit_writes_to_writer() {
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(1, 2, 1, 1),
            vec![smell_with(
                SmellCategory::ZeroAssertion,
                Severity::High,
                10,
                None,
                "m",
            )],
        );
        let report = report_with(vec![finding]);
        let mut buf: Vec<u8> = Vec::new();
        emit(&report, &meta(), 10, &mut buf).expect("emit succeeds");
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.starts_with("::warning "),
            "emit writes warning lines: {s}"
        );
    }
}
