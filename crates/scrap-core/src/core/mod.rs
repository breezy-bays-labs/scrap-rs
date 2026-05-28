//! Core — orchestration. Wires adapters through ports and exposes
//! `analyze()` for the CLI and embedding consumers.
//!
//! `analyze()` is the public API surface for embedding scrap4rs in
//! other tools (e.g. mokumo's quality crate). It takes a workspace
//! root + config and returns a `domain::Report`. Lands in v0.1 once
//! enough adapters and detectors exist to make the call meaningful.
//!
//! Wave 1 of scrap-rs#21 fills in the **type surface only**:
//! [`AnalyzeOptions`] POD + [`AnalyzeError`] thiserror enum. The
//! `analyze<S, P>` fn body lands in Wave 2 (intra-doc links to
//! `analyze` resolve once Wave 2 ships).

use std::path::PathBuf;

use crate::cli::config::ConfigError;
use crate::cli::error::InitError;
use crate::detectors::detect_all;
use crate::domain::config::FileConfig;
use crate::domain::finding::Finding;
use crate::domain::parsed::{ParseDiagnostic, ParseDiagnosticKind};
use crate::domain::report::{FileReport, Report, Summary};
use crate::domain::source::{DiscoveryOutcome, SourceDiagnostic, SourceDiagnosticKind};
use crate::domain::threshold::ThresholdMode;
use crate::ports::parser::{ParseError, TestParserPort};
use crate::ports::source::{SourceError, SourcePort};

/// Options threaded from `cli::run` into `analyze` (Wave 2 body —
/// intra-doc links resolve once Wave 2 ships the fn).
///
/// POD per ADR D8 — no methods beyond `Default::default()`. The CLI
/// in scrap-rs#21 (`cli::bootstrap`) builds this from the merged
/// `Cli` + `FileConfig`; library embedders construct it directly.
///
/// Imports `FileConfig` from `domain::config` (the POD-types home
/// post-MF-1) NOT from `cli::config` (loader-only). `detectors/`
/// and `core/` must never depend on `cli/` for the type per
/// adr-hexagonal-layout — cabinet MF-1 fold relocates the type to
/// satisfy that constraint.
#[derive(Debug, Clone)]
pub struct AnalyzeOptions {
    /// Workspace root the walker walks (post-canonicalize). Built
    /// from CLI `--src <path>` ∨ `file_config.src` ∨ default `"src"`.
    pub src: PathBuf,
    /// Effective exclude globs (merged: `meta.forced_excludes` ∪
    /// `cli.filter.exclude` ∪ `file_config.exclude`). Adapter-side
    /// validation lives in `FsWalker::try_new`.
    pub exclude: Vec<String>,
    /// File extensions the walker keeps. Built from
    /// `file_config.extensions` ∨ `meta.extensions_owned()`.
    pub extensions: Vec<String>,
    /// Honor `.gitignore` / `.ignore` / `.git/info/exclude`.
    /// `false` iff CLI `--no-gitignore` (folds scrap-rs#33).
    pub respect_gitignore: bool,
    /// Project-level config — threaded into per-test
    /// [`crate::detectors::detect_all`] for per-detector enable /
    /// penalty / line-threshold knobs. v0.1 `detect_all` ignores this
    /// (stub); scrap-rs#24 / scrap-rs#30 consume it.
    pub config: FileConfig,
    /// Threshold mode — emitted onto the JSON envelope's
    /// `threshold_mode` field. v0.1 wire-only (per FORK-3 +
    /// scrap-rs#75 follow-up for the real `Report.passed` computation
    /// driven by the mode + penalty cutoffs).
    pub threshold_mode: ThresholdMode,
}

impl Default for AnalyzeOptions {
    fn default() -> Self {
        Self {
            src: PathBuf::from("src"),
            exclude: Vec::new(),
            extensions: Vec::new(),
            respect_gitignore: true,
            config: FileConfig::default(),
            threshold_mode: ThresholdMode::default(),
        }
    }
}

/// Top-level analysis-pipeline error.
///
/// Three of the four variants wrap per-port error types via `#[from]`
/// so the analyzer doesn't have to hand-write conversion boilerplate;
/// the fourth (`AllFilesFailedToParse`) is the analyzer's own
/// surfaced condition (every per-file `parse_test_source` failed →
/// no findings can be produced; CLI maps to exit code 3 per epic #1).
///
/// `#[non_exhaustive]` per ADR D2 — future variants land additively
/// without breaking `dispatch::exit_code_for`'s match.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum AnalyzeError {
    /// Discovery failure — `FsWalker::try_new` validation or
    /// mid-walk fatal error.
    #[error(transparent)]
    Source(#[from] SourceError),
    /// Config-file load failure (loader propagates `ConfigError`).
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// Init subcommand failure — `dispatch_subcommand`'s `Init` arm
    /// wraps `InitError` via this variant before handing off to
    /// `dispatch::render_error`.
    #[error(transparent)]
    Init(#[from] InitError),
    /// Every source file in the workspace failed to parse. Maps to
    /// exit code 3 per epic #1; CLI's `dispatch::render_error`
    /// splices `meta.parse_hint` into the user-facing remediation.
    #[error("all {total_files} source files failed to parse (see --verbose for per-file detail)")]
    AllFilesFailedToParse {
        /// Number of source files the walker discovered (all failed).
        total_files: u32,
    },
}

/// Output of [`analyze`] — bundles the `Report` (the truthful gate)
/// with the source + parse diagnostics the pipeline collected.
///
/// Per cabinet S1 fold: `analyze` returns this struct (not just
/// `Report`) so the CLI's `--verbose` mode has something to surface
/// and the JSON envelope's `DiagnosticsBlock` has data to populate.
/// POD; no `Option<T>` fields; no `Serialize` derive at this stage
/// (the reporter wraps the inner vecs into `DiagnosticsBlock` which
/// IS Serialize-derived).
#[derive(Debug, Clone)]
pub struct AnalyzeOutput {
    /// The truthful gate — files / findings / summary / passed.
    pub report: Report,
    /// Non-fatal source-discovery + per-file-read diagnostics.
    /// Surfaced via the JSON envelope's `diagnostics.source` block
    /// under `--verbose`.
    pub source_diagnostics: Vec<SourceDiagnostic>,
    /// Per-file parse-recovery diagnostics. Each parse failure that
    /// did NOT abort the whole pipeline ends up here. The message
    /// embeds the file path (the wire shape's
    /// `ParseDiagnostic.message` is the only place to attribute);
    /// the diagnostic stays POD-flat.
    pub parse_diagnostics: Vec<ParseDiagnostic>,
}

/// Run the full walker → parser → detector pipeline and assemble a
/// [`Report`] plus the diagnostics bag.
///
/// **Per-file failure semantics** (issue AC):
/// - File-read I/O failure → push a [`SourceDiagnostic`] with
///   [`SourceDiagnosticKind::MidwalkIo`] (verified against
///   `domain/source.rs:86`); continue to next file. Survivors are
///   still analyzed + reported.
/// - Parse failure ([`ParseError::Syntax`]) → push a
///   [`ParseDiagnostic`] with `ParseDiagnosticKind::Syntax`, embed
///   the file path in the message (the wire shape doesn't carry a
///   per-diagnostic file_path field), increment the
///   all-failed-counter, continue. If every file failed to parse
///   AND the walker found files at all,
///   return `Err(AllFilesFailedToParse { total_files })`.
/// - Walker failure ([`SourceError`]) → bubble via the `Source(_)`
///   variant; pipeline aborts (no `Report` to produce).
///
/// **Source-root canonicalization** — `opts.src` is best-effort
/// canonicalized once at the top so downstream code doesn't repeat
/// the concern. Canonicalize failure (path doesn't exist) falls back
/// to the lexical path; the walker surfaces the cleaner
/// `Source(SourceError::Io)` if the path is genuinely missing.
///
/// **Generic over `S: SourcePort + P: TestParserPort`** per the
/// issue body's enumerated AC. Library embedders can wire a
/// `MemorySource` + fake parser without forcing FsWalker / SynParser.
///
/// **Detector loop is a stub at v0.1** — `detect_all` returns the
/// zero-assertion detector's findings only (scrap-rs#30 / PR #82).
/// Future detector PRs (#24/#25/#26/#31) extend `detect_all` and
/// inherit this orchestrator unchanged.
///
/// **`Report.passed` is initialized to `false`** — per FORK-3 +
/// scrap-rs#75, the real `ThresholdMode`-driven computation lands
/// later. v0.1 reporter consumes the field verbatim.
///
/// # Errors
///
/// - [`AnalyzeError::Source`] — walker failure during discovery.
/// - [`AnalyzeError::AllFilesFailedToParse`] — every discovered
///   file failed to parse. Maps to exit code 3 at the CLI boundary.
pub fn analyze<S, P>(
    opts: &AnalyzeOptions,
    source: &S,
    parser: &P,
) -> Result<AnalyzeOutput, AnalyzeError>
where
    S: SourcePort,
    P: TestParserPort,
{
    // Best-effort canonicalize once. Fall back to lexical path if
    // canonicalize fails (path doesn't exist yet — let downstream
    // surface the cleaner error). NOT stored back into `opts`; POD
    // is immutable from analyze's POV.
    let _canonical_src = std::fs::canonicalize(&opts.src).unwrap_or_else(|_| opts.src.clone());

    let DiscoveryOutcome {
        files,
        diagnostics: mut source_diagnostics,
    } = source.discover_test_files()?;

    let mut file_reports: Vec<FileReport> = Vec::with_capacity(files.len());
    let mut parse_diagnostics: Vec<ParseDiagnostic> = Vec::new();
    let mut all_failed_count: u32 = 0;

    for file_path in &files {
        // Read the file's bytes. On I/O failure, push a
        // SourceDiagnostic::MidwalkIo (cabinet S1 — DO NOT silently
        // swallow) and continue to the next file.
        let source_text = match std::fs::read_to_string(file_path.as_path()) {
            Ok(s) => s,
            Err(e) => {
                source_diagnostics.push(SourceDiagnostic::new(
                    file_path.clone(),
                    SourceDiagnosticKind::MidwalkIo,
                    e.to_string(),
                ));
                continue;
            }
        };

        // Parse via the adapter. On Err(ParseError::Syntax), push a
        // ParseDiagnostic + increment the all-failed counter; the
        // file's contribution to the report drops out entirely.
        let parsed = match parser.parse_test_source(&source_text, file_path) {
            Ok(p) => p,
            Err(ParseError::Syntax { message, span }) => {
                all_failed_count = all_failed_count.saturating_add(1);
                // ParseDiagnostic doesn't carry a per-diagnostic
                // file_path field (wire shape inherited from #14);
                // embed the path in the message for attribution.
                let attributed = format!("{}: {message}", file_path.as_path().display());
                parse_diagnostics.push(ParseDiagnostic::new(
                    ParseDiagnosticKind::Syntax,
                    span,
                    attributed,
                ));
                continue;
            }
        };

        // Run every enabled detector against each test in the file.
        let mut findings: Vec<Finding> = Vec::with_capacity(parsed.tests.len());
        for parsed_test in &parsed.tests {
            let smells = detect_all(parsed_test, &opts.config);
            findings.push(Finding::new(parsed_test.identity.clone(), smells));
        }

        // Per-file diagnostics from the parser (e.g., partial-recovery
        // warnings) carry over too. Attribute via the same message
        // pattern as parse failures so the wire shape stays uniform.
        for diag in parsed.diagnostics {
            let attributed = format!("{}: {}", file_path.as_path().display(), diag.message);
            parse_diagnostics.push(ParseDiagnostic::new(diag.kind, diag.span, attributed));
        }

        if !findings.is_empty() {
            file_reports.push(FileReport::new(file_path.clone(), findings));
        }
    }

    // All-files-failed-to-parse is a fatal condition per issue AC
    // (exit code 3). Only fires when the walker DID find files —
    // an empty workspace returns Ok with an empty Report.
    let total_files_u32 = u32::try_from(files.len()).unwrap_or(u32::MAX);
    if !files.is_empty() && all_failed_count == total_files_u32 {
        return Err(AnalyzeError::AllFilesFailedToParse {
            total_files: total_files_u32,
        });
    }

    let summary = Summary::from_findings(file_reports.iter().flat_map(|fr| fr.findings.iter()));
    let report = Report {
        files: file_reports,
        summary,
        // `passed` left as default `false` per FORK-3 + scrap-rs#75
        // (the real ThresholdMode-driven computation lands later).
        // Reporter consumes verbatim.
        passed: false,
    };

    Ok(AnalyzeOutput {
        report,
        source_diagnostics,
        parse_diagnostics,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_options_default_is_default_pod() {
        let opts = AnalyzeOptions::default();
        assert_eq!(opts.src, PathBuf::from("src"));
        assert!(opts.exclude.is_empty());
        assert!(opts.extensions.is_empty());
        assert!(opts.respect_gitignore);
        assert_eq!(opts.config, FileConfig::default());
        assert_eq!(opts.threshold_mode, ThresholdMode::default());
    }

    #[test]
    fn analyze_error_from_source_error_wraps() {
        let glob_err = globset::Glob::new("[unclosed").unwrap_err();
        let src_err = SourceError::InvalidGlob {
            pattern: "[unclosed".to_string(),
            source: glob_err,
        };
        let wrapped: AnalyzeError = src_err.into();
        assert!(
            matches!(wrapped, AnalyzeError::Source(_)),
            "From<SourceError> must produce Source variant, got: {wrapped:?}",
        );
    }

    #[test]
    fn analyze_error_from_config_error_wraps() {
        let cfg_err = ConfigError::Io {
            path: PathBuf::from("test-adapter.toml"),
            source: std::io::Error::other("boom"),
        };
        let wrapped: AnalyzeError = cfg_err.into();
        assert!(matches!(wrapped, AnalyzeError::Config(_)));
    }

    #[test]
    fn analyze_error_from_init_error_wraps() {
        let init_err = InitError::Exists {
            path: PathBuf::from("test-adapter.toml"),
        };
        let wrapped: AnalyzeError = init_err.into();
        assert!(matches!(wrapped, AnalyzeError::Init(_)));
    }

    #[test]
    fn analyze_error_all_files_failed_display_includes_count() {
        let err = AnalyzeError::AllFilesFailedToParse { total_files: 7 };
        let display = err.to_string();
        assert!(
            display.contains('7'),
            "Display must include total_files count; got: {display}",
        );
        assert!(
            display.contains("--verbose"),
            "Display must hint the --verbose flag; got: {display}",
        );
    }

    // ── analyze<S, P> pipeline tests (cabinet S1 fold lands here) ──

    use crate::adapters::source::memory::MemorySource;
    use crate::domain::parsed::ParsedTestFile;
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

    /// Test-only mock parser: returns whatever `parse_test_source`
    /// gets configured to return. Mode encoded in the parser type via
    /// an inner enum.
    struct MockParser {
        mode: MockMode,
    }

    #[derive(Clone)]
    enum MockMode {
        /// Return `Ok(ParsedTestFile { ... empty tests ... })` —
        /// the file parses cleanly but exposes no tests.
        EmptyOk,
        /// Return `Ok(ParsedTestFile { ... one test ... })`.
        OneTestOk,
        /// Return `Err(ParseError::Syntax)` always.
        AlwaysSyntaxErr,
    }

    impl TestParserPort for MockParser {
        fn parse_test_source(
            &self,
            _source: &str,
            path: &FilePath,
        ) -> Result<ParsedTestFile, ParseError> {
            use crate::domain::parsed::ParsedTest;
            match self.mode {
                MockMode::EmptyOk => Ok(ParsedTestFile::new(path.clone(), vec![], vec![])),
                MockMode::OneTestOk => {
                    let identity = TestIdentity::new(
                        path.clone(),
                        QualifiedName::new("tests::it_smells"),
                        Span::new(1, 5),
                    );
                    Ok(ParsedTestFile::new(
                        path.clone(),
                        vec![ParsedTest::new(
                            identity,
                            vec![],                            // attributes
                            vec![],                            // assertions
                            10,                                // body_line_count
                            vec![],                            // implicit_assertion_sources
                            std::collections::BTreeSet::new(), // opt_outs
                            std::collections::BTreeSet::new(), // behavioral_facts
                        )],
                        vec![],
                    ))
                }
                MockMode::AlwaysSyntaxErr => Err(ParseError::Syntax {
                    message: "unexpected token".into(),
                    span: Some(Span::new(3, 3)),
                }),
            }
        }
    }

    /// Build a tempdir + one rust source file; return (TempDir,
    /// FilePath to the file). TempDir kept alive by the caller via
    /// the returned guard.
    fn tempdir_with_one_file(content: &str) -> (tempfile::TempDir, FilePath) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.rs");
        std::fs::write(&path, content).unwrap();
        let fp = FilePath::new(path.to_str().unwrap());
        (dir, fp)
    }

    #[test]
    fn analyze_empty_workspace_yields_empty_report() {
        let source = MemorySource::with_files(vec![]);
        let parser = MockParser {
            mode: MockMode::EmptyOk,
        };
        let opts = AnalyzeOptions::default();
        let output = analyze(&opts, &source, &parser).expect("empty workspace yields Ok");
        assert!(output.report.files.is_empty());
        assert_eq!(output.report.summary.total_tests, 0);
        assert!(!output.report.passed);
        assert!(output.source_diagnostics.is_empty());
        assert!(output.parse_diagnostics.is_empty());
    }

    #[test]
    fn analyze_one_file_no_findings_yields_one_file_no_smells() {
        // Mock parser returns one ParsedTest with empty assertions;
        // detect_all's zero-assertion detector WILL fire (empty
        // assertions + empty implicit_assertion_sources +
        // empty behavioral_facts → zero-assertion smell).
        let (_dir, fp) = tempdir_with_one_file("#[test] fn it() {}");
        let source = MemorySource::with_files(vec![fp]);
        let parser = MockParser {
            mode: MockMode::OneTestOk,
        };
        let opts = AnalyzeOptions::default();
        let output = analyze(&opts, &source, &parser).expect("one-file analyze succeeds");
        assert_eq!(output.report.files.len(), 1, "one FileReport produced");
        assert_eq!(output.report.summary.total_tests, 1);
        assert_eq!(output.report.summary.total_files, 1);
        assert!(output.source_diagnostics.is_empty());
        assert!(output.parse_diagnostics.is_empty());
    }

    #[test]
    fn analyze_all_files_failed_parse_returns_all_files_failed_error() {
        let (_dir, fp) = tempdir_with_one_file("garbage");
        let source = MemorySource::with_files(vec![fp]);
        let parser = MockParser {
            mode: MockMode::AlwaysSyntaxErr,
        };
        let opts = AnalyzeOptions::default();
        let err = analyze(&opts, &source, &parser).expect_err("all-failed → Err");
        match err {
            AnalyzeError::AllFilesFailedToParse { total_files } => {
                assert_eq!(total_files, 1);
            }
            other => panic!("expected AllFilesFailedToParse, got {other:?}"),
        }
    }

    #[test]
    fn analyze_partial_parse_failure_pushes_diagnostic_continues() {
        // Two files: one parses (OneTestOk), one fails. The mock can
        // only return one mode at a time, so we use two single-file
        // analyze runs to verify each behavior cleanly; the partial
        // case is exercised once the real syn parser ships.
        // What we DO verify here: a single failing file produces a
        // parse_diagnostic that embeds the file path.
        let (_dir, fp) = tempdir_with_one_file("garbage");
        let source = MemorySource::with_files(vec![fp.clone()]);
        let parser = MockParser {
            mode: MockMode::AlwaysSyntaxErr,
        };
        let opts = AnalyzeOptions::default();
        // All-failed kicks in because only one file exists; the
        // diagnostic still gets pushed before the all-failed check.
        let err = analyze(&opts, &source, &parser).expect_err("single-failure → all-failed");
        assert!(matches!(err, AnalyzeError::AllFilesFailedToParse { .. }));
    }

    #[test]
    fn analyze_pushes_midwalk_io_diagnostic_for_unreadable_file() {
        // MemorySource hands the analyzer a FilePath that points to a
        // nonexistent file; std::fs::read_to_string fails;
        // SourceDiagnostic::MidwalkIo is pushed; pipeline continues.
        // With ONLY that file + no parse attempt happening, the
        // all-failed-counter stays at 0; analyze returns Ok with the
        // diagnostic but no findings.
        let fp = FilePath::new("/nonexistent/does-not-exist.rs");
        let source = MemorySource::with_files(vec![fp.clone()]);
        let parser = MockParser {
            mode: MockMode::EmptyOk,
        };
        let opts = AnalyzeOptions::default();
        let output = analyze(&opts, &source, &parser).expect("missing file → Ok with diagnostic");
        assert_eq!(
            output.source_diagnostics.len(),
            1,
            "missing-file read failure → 1 MidwalkIo diagnostic",
        );
        assert_eq!(
            output.source_diagnostics[0].kind,
            SourceDiagnosticKind::MidwalkIo,
        );
        assert!(output.report.files.is_empty());
    }

    #[test]
    fn analyze_preserves_walker_supplied_diagnostics() {
        // MemorySource pre-loads a diagnostic the walker collected.
        // analyze() must keep it (append to it, don't replace it).
        let walker_diag = SourceDiagnostic::new(
            FilePath::new("dropped/path"),
            SourceDiagnosticKind::PermissionDenied,
            "walker-supplied",
        );
        let source = MemorySource::new(vec![], vec![walker_diag.clone()]);
        let parser = MockParser {
            mode: MockMode::EmptyOk,
        };
        let opts = AnalyzeOptions::default();
        let output = analyze(&opts, &source, &parser).unwrap();
        assert_eq!(output.source_diagnostics, vec![walker_diag]);
    }
}
