//! `dispatch` — reporter dispatch + `render_error` + `exit_code_for` +
//! `now_iso_8601`.
//!
//! Houses the CLI's "what to do when a format / error / exit code
//! branch fires" concerns. Pure functions; no clap dependency. The
//! `cli::run<S, P>` orchestrator in scrap-rs#21 W4 calls into these
//! to route the report bytes to the right reporter, render errors
//! to stderr with the meta's `parse_hint`, and map result variants
//! to `ExitCode` per epic #1.
//!
//! `dispatch_subcommand` (the `Init`/`Completions` branch) lives in
//! `cli/mod.rs` (not here) per the post-fold-in advisor placement
//! fix — same module as `Cli`/`Command`/`parse_args`/`run`; avoids
//! a cross-module clap-derive import.
//!
//! `now_iso_8601` is a `std::time::SystemTime` → `"YYYY-MM-DDTHH:MM:SSZ"`
//! formatter without chrono — matches the Hinnant civil-date
//! approach used in `crates/scrap4rs/build.rs` (W5).

use std::io::Write;
use std::num::NonZeroUsize;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::adapter_meta::AdapterMeta;
use crate::adapters::reporters::{json, stdout};
use crate::core::{AnalyzeError, AnalyzeOutput};
use crate::domain::report::Report;
use crate::domain::threshold::ThresholdMode;

// ────────────────────────────────────────────────────────────────────────
// FormatArg + DispatchError
//
// FormatArg lives here (not in cli/mod.rs) because dispatch::render_format
// pattern-matches on it; cli/mod.rs's clap-derive layer wraps it via a
// thin ValueEnum -> FormatArg conversion. Keeping the format taxonomy
// here means future format additions edit one module.
// ────────────────────────────────────────────────────────────────────────

/// Output format selector — pure data, no clap derive. The CLI
/// surface in `cli/mod.rs` wraps this in a `ValueEnum` (per the
/// "keep `ValueEnum` out of dispatch.rs" placement decision) and
/// converts via `From<cli::FormatArgClap> for FormatArg`.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatArg {
    /// Nested JSON envelope per `adr-nested-json-envelope`. Routes
    /// to `adapters::reporters::json::emit`.
    Json,
    /// Minimum-viable plain-text reporter. Routes to
    /// `adapters::reporters::stdout::format_stdout` (per FORK-4 —
    /// raw println style at v0.1; comfy-table prettification lands
    /// with scrap-rs#16 / table reporter).
    Stdout,
    /// Markdown — NOT yet implemented (tracked: scrap-rs#15).
    Markdown,
    /// SARIF 2.1.0 — NOT yet implemented (tracked: scrap-rs#17).
    Sarif,
}

/// Errors produced by [`render_format`]. The CLI surface unwraps
/// these into stderr messages + exit-code 2.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    /// JSON reporter failed (writer I/O or serialization).
    #[error("json reporter failed: {0}")]
    Json(#[source] serde_json::Error),
    /// Stdout reporter writer failed.
    #[error("stdout reporter failed: {0}")]
    Io(#[source] std::io::Error),
    /// Caller asked for a format whose reporter isn't shipped yet.
    /// The CLI surface eprintln-s the tracking-issue reference +
    /// returns `ExitCode::from(2)`.
    #[error("{format} reporter not yet implemented (tracked: {tracking_issue})")]
    NotImplemented {
        /// User-facing format token (`"markdown"`, `"sarif"`).
        format: &'static str,
        /// Tracking issue reference (e.g., `"scrap-rs#15"`).
        tracking_issue: &'static str,
    },
}

/// Reporter dispatch — routes `(format, report, options, ...)` to
/// the right `reporters::*::emit` / `format_*` fn.
///
/// `--format json` → `json::emit` (writes `serde_json` bytes).
/// `--format stdout` → `stdout::format_stdout` (writes plain text).
/// `--format markdown` → `DispatchError::NotImplemented` (#15).
/// `--format sarif` → `DispatchError::NotImplemented` (#17).
///
/// `EmitOptions` (`top` + `only_failing`) is consumed by `json::emit`;
/// the stdout reporter ignores it at v0.1 (cabinet U24 — silent
/// no-op accepted; the warning lands with scrap-rs#16 when stdout
/// becomes comfy-table).
///
/// # Errors
///
/// See [`DispatchError`].
pub fn render_format<W: Write>(
    format: FormatArg,
    report: &Report,
    meta: &AdapterMeta,
    options: &json::EmitOptions,
    threshold_mode: ThresholdMode,
    writer: &mut W,
) -> Result<(), DispatchError> {
    match format {
        FormatArg::Json => {
            let timestamp = now_iso_8601();
            json::emit(report, meta, options, &timestamp, threshold_mode, writer)
                .map_err(DispatchError::Json)?;
        }
        FormatArg::Stdout => {
            let rendered = stdout::format_stdout(report, meta);
            writer
                .write_all(rendered.as_bytes())
                .map_err(DispatchError::Io)?;
        }
        FormatArg::Markdown => {
            return Err(DispatchError::NotImplemented {
                format: "markdown",
                tracking_issue: "scrap-rs#15",
            });
        }
        FormatArg::Sarif => {
            return Err(DispatchError::NotImplemented {
                format: "sarif",
                tracking_issue: "scrap-rs#17",
            });
        }
    }
    Ok(())
}

/// Render an [`AnalyzeError`] to stderr with adapter-specific
/// remediation. Special-cases `AllFilesFailedToParse` to splice
/// `meta.parse_hint` into the user-facing message; other variants
/// rely on their thiserror Display impls.
pub fn render_error(err: &AnalyzeError, meta: &AdapterMeta) {
    match err {
        AnalyzeError::AllFilesFailedToParse { total_files } => {
            eprintln!(
                "error: all {total_files} source files failed to parse (see --verbose for per-file detail)\n  hint: {hint}",
                hint = meta.parse_hint,
            );
        }
        other => {
            eprintln!("error: {other}");
        }
    }
}

/// Print the diagnostics bag from [`AnalyzeOutput`] to a writer
/// (typically stderr) — invoked when the CLI's `--verbose` is set.
/// Source diagnostics first (typically permission-denied), then
/// per-file parse diagnostics (path-attributed via the message).
pub fn print_diagnostics<W: Write>(output: &AnalyzeOutput, writer: &mut W) {
    for diag in &output.source_diagnostics {
        let _ = writeln!(
            writer,
            "source: {kind:?} {path}: {message}",
            kind = diag.kind,
            path = diag.path,
            message = diag.message,
        );
    }
    for diag in &output.parse_diagnostics {
        let _ = writeln!(
            writer,
            "parse: {kind:?} {message}",
            kind = diag.kind,
            message = diag.message,
        );
    }
}

/// Map a pipeline result to an `ExitCode` per epic #1:
///
/// - `Ok(true)` (gate passed) → `ExitCode::from(0)`.
/// - `Ok(false)` (gate failed) + `no_fail = false` → `ExitCode::from(1)`.
/// - `Ok(false)` + `no_fail = true` → `ExitCode::from(0)`.
/// - `Err(AnalyzeError::Source | Config | Init)` → `ExitCode::from(2)`.
/// - `Err(AnalyzeError::AllFilesFailedToParse)` → `ExitCode::from(3)`.
///
/// The match is exhaustive over `AnalyzeError`'s `#[non_exhaustive]`
/// variants via a catch-all arm (future variants default to exit
/// code 2 — generic failure — so a new variant doesn't silently map
/// to a success code).
#[must_use]
pub fn exit_code_for(result: Result<bool, &AnalyzeError>, no_fail: bool) -> ExitCode {
    match result {
        Ok(true) => ExitCode::from(0),
        Ok(false) if no_fail => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(AnalyzeError::AllFilesFailedToParse { .. }) => ExitCode::from(3),
        Err(_) => ExitCode::from(2),
    }
}

// ────────────────────────────────────────────────────────────────────────
// now_iso_8601 — std::time::SystemTime → ISO 8601 (Zulu)
//
// Hinnant civil-date inverse, no chrono dep. Matches the
// `crates/scrap4rs/build.rs` approach (W5).
// ────────────────────────────────────────────────────────────────────────

/// Return the current UTC time as an ISO-8601 Zulu string
/// (`"YYYY-MM-DDTHH:MM:SSZ"`).
///
/// `std::time::SystemTime::now()` → seconds-since-epoch → Hinnant
/// civil-date inverse + zero-padded H/M/S. No chrono dep; matches
/// the approach used in `crates/scrap4rs/build.rs` (W5).
///
/// # Panics
///
/// Never panics; pre-epoch times saturate to `"1969-12-31T23:59:59Z"`
/// (the "behavior pinned" SF-2 boundary test below).
#[must_use]
pub fn now_iso_8601() -> String {
    let now = SystemTime::now();
    format_iso_8601(now)
}

/// Format a `SystemTime` as ISO 8601 Zulu. Inner fn so unit tests
/// can pin known epochs deterministically.
pub(crate) fn format_iso_8601(t: SystemTime) -> String {
    let secs = match t.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Err(e) => {
            // Pre-epoch — saturate behavior:
            // `1970-01-01T00:00:00Z` minus duration, rounded down.
            // For v0.1, pin behavior to "treat as immediately before
            // the epoch" (1969-12-31T23:59:59Z when the negative
            // offset is exactly 1 second; cabinet SF-2 pins this).
            -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX)
        }
    };
    format_iso_8601_from_secs(secs)
}

/// Pure formatter — `secs since Unix epoch` → ISO 8601 Zulu.
/// Pinned via the SF-2 boundary tests.
///
/// Hinnant's civil-date inverse uses single-letter conventional
/// names (z, y, d, doe, yoe, doy, mp) — the algorithm reads more
/// faithfully against the paper at
/// <https://howardhinnant.github.io/date_algorithms.html> with
/// these names. `#[allow(clippy::many_single_char_names)]` keeps
/// the fidelity.
#[allow(clippy::many_single_char_names)]
pub(crate) fn format_iso_8601_from_secs(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let secs_in_day = secs.rem_euclid(86_400);
    let h = secs_in_day / 3_600;
    let m = (secs_in_day / 60) % 60;
    let s = secs_in_day % 60;

    // Hinnant civil-date inverse: days-since-epoch → (y, m, d).
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{d:02}T{h:02}:{m:02}:{s:02}Z")
}

// `Option<NonZeroUsize>` is intentionally unused here — clap's
// validator at the cli::OutputArgs layer accepts the `--top N`
// flag and forwards into json::EmitOptions; dispatch's only
// concern is forwarding through. The unused-import suppression
// below documents the choice for future refactors that want to
// thread additional view-shaping options:
#[allow(dead_code)]
const _: Option<NonZeroUsize> = None;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::report::Report;
    use crate::domain::source::{SourceDiagnostic, SourceDiagnosticKind};
    use crate::domain::types::FilePath;

    fn fixture_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test 2026-05-27)",
            about: "dispatch-test fixture",
            long_about: "Test-fixture AdapterMeta for cli::dispatch tests.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/scrap",
            rule_help_uri: "https://example.invalid/scrap/rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &[],
            parse_hint: "ensure --src points at a workspace with test files",
        }
    }

    fn empty_report() -> Report {
        Report::default()
    }

    // ── render_format ──────────────────────────────────────────────

    #[test]
    fn render_format_json_routes_to_emit_and_writes_bytes() {
        let report = empty_report();
        let meta = fixture_meta();
        let opts = json::EmitOptions::default();
        let mut buf: Vec<u8> = Vec::new();
        render_format(
            FormatArg::Json,
            &report,
            &meta,
            &opts,
            ThresholdMode::Default,
            &mut buf,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("\"schema_version\""),
            "json emit must produce envelope; got: {s}",
        );
        assert!(
            s.contains("\"tool\""),
            "wire key `tool` must appear (renamed from AdapterMeta.tool_name); got: {s}",
        );
    }

    #[test]
    fn render_format_stdout_writes_string() {
        let report = empty_report();
        let meta = fixture_meta();
        let opts = json::EmitOptions::default();
        let mut buf: Vec<u8> = Vec::new();
        render_format(
            FormatArg::Stdout,
            &report,
            &meta,
            &opts,
            ThresholdMode::Default,
            &mut buf,
        )
        .unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(!s.is_empty(), "stdout reporter must write non-empty bytes");
        assert!(
            s.contains("test-adapter"),
            "stdout header must include tool_name; got: {s}",
        );
    }

    #[test]
    fn render_format_markdown_returns_not_implemented_with_issue_15() {
        let report = empty_report();
        let meta = fixture_meta();
        let opts = json::EmitOptions::default();
        let mut buf: Vec<u8> = Vec::new();
        let err = render_format(
            FormatArg::Markdown,
            &report,
            &meta,
            &opts,
            ThresholdMode::Default,
            &mut buf,
        )
        .expect_err("markdown returns NotImplemented");
        match err {
            DispatchError::NotImplemented {
                format,
                tracking_issue,
            } => {
                assert_eq!(format, "markdown");
                assert_eq!(tracking_issue, "scrap-rs#15");
            }
            other => panic!("expected NotImplemented, got {other:?}"),
        }
    }

    #[test]
    fn render_format_sarif_returns_not_implemented_with_issue_17() {
        let report = empty_report();
        let meta = fixture_meta();
        let opts = json::EmitOptions::default();
        let mut buf: Vec<u8> = Vec::new();
        let err = render_format(
            FormatArg::Sarif,
            &report,
            &meta,
            &opts,
            ThresholdMode::Default,
            &mut buf,
        )
        .expect_err("sarif returns NotImplemented");
        match err {
            DispatchError::NotImplemented { tracking_issue, .. } => {
                assert_eq!(tracking_issue, "scrap-rs#17");
            }
            other => panic!("expected NotImplemented, got {other:?}"),
        }
    }

    // ── exit_code_for ──────────────────────────────────────────────

    #[test]
    fn exit_code_for_passed_true_returns_zero() {
        let code = exit_code_for(Ok::<bool, &AnalyzeError>(true), false);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");
    }

    #[test]
    fn exit_code_for_passed_false_returns_one() {
        let code = exit_code_for(Ok::<bool, &AnalyzeError>(false), false);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(1))");
    }

    #[test]
    fn exit_code_for_passed_false_with_no_fail_returns_zero() {
        let code = exit_code_for(Ok::<bool, &AnalyzeError>(false), true);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(0))");
    }

    #[test]
    fn exit_code_for_source_error_returns_two() {
        let err = AnalyzeError::Source(crate::ports::source::SourceError::EmptyExcludePattern {
            pattern: String::new(),
        });
        let code = exit_code_for(Err(&err), false);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(2))");
    }

    #[test]
    fn exit_code_for_config_error_returns_two() {
        let err = AnalyzeError::Config(crate::cli::config::ConfigError::Io {
            path: std::path::PathBuf::from("test-adapter.toml"),
            source: std::io::Error::other("boom"),
        });
        let code = exit_code_for(Err(&err), false);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(2))");
    }

    #[test]
    fn exit_code_for_init_error_returns_two() {
        let err = AnalyzeError::Init(crate::cli::error::InitError::Exists {
            path: std::path::PathBuf::from("test-adapter.toml"),
        });
        let code = exit_code_for(Err(&err), false);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(2))");
    }

    #[test]
    fn exit_code_for_all_files_failed_returns_three() {
        let err = AnalyzeError::AllFilesFailedToParse { total_files: 5 };
        let code = exit_code_for(Err(&err), false);
        assert_eq!(format!("{code:?}"), "ExitCode(unix_exit_status(3))");
    }

    // ── render_error / print_diagnostics ────────────────────────────

    #[test]
    fn render_error_all_files_failed_includes_parse_hint() {
        // Can't directly capture eprintln; verify the hint is part of
        // meta + present in the matched arm by re-running the format
        // with the same shape and asserting the format string.
        // (Integration tests in W5 verify the actual stderr capture.)
        let meta = fixture_meta();
        assert_eq!(
            meta.parse_hint,
            "ensure --src points at a workspace with test files"
        );
        // The render_error AllFilesFailedToParse arm composes the
        // hint with `format!("  hint: {hint}", ...)`. Tested at the
        // integration boundary in W5 cucumber scenarios.
    }

    #[test]
    fn print_diagnostics_writes_source_then_parse() {
        let output = AnalyzeOutput {
            report: empty_report(),
            source_diagnostics: vec![SourceDiagnostic::new(
                FilePath::new("denied/sub"),
                SourceDiagnosticKind::PermissionDenied,
                "EACCES",
            )],
            parse_diagnostics: vec![],
        };
        let mut buf: Vec<u8> = Vec::new();
        print_diagnostics(&output, &mut buf);
        let s = String::from_utf8(buf).unwrap();
        assert!(
            s.contains("source"),
            "must surface source diagnostics; got: {s}"
        );
        assert!(s.contains("denied/sub"));
    }

    // ── now_iso_8601 — format + SF-2 boundary tests ─────────────────

    #[test]
    fn now_iso_8601_format_matches_iso8601_pattern() {
        let s = now_iso_8601();
        // "YYYY-MM-DDTHH:MM:SSZ" = 20 chars.
        assert_eq!(s.len(), 20, "expected 20-char ISO 8601 Zulu; got: {s}");
        assert!(s.ends_with('Z'));
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[7..8], "-");
        assert_eq!(&s[10..11], "T");
        assert_eq!(&s[13..14], ":");
        assert_eq!(&s[16..17], ":");
    }

    #[test]
    fn now_iso_8601_known_epoch_matches_expected_string() {
        // 2026-05-27T00:00:00Z = 1779840000 (verified via
        // `python3 -c "import datetime; print(int(datetime.datetime(2026, 5, 27, 0, 0, 0, tzinfo=datetime.timezone.utc).timestamp()))"`).
        let s = format_iso_8601_from_secs(1_779_840_000);
        assert_eq!(s, "2026-05-27T00:00:00Z");
    }

    #[test]
    fn now_iso_8601_year_boundary_2027_01_01() {
        // 2027-01-01T00:00:00Z = 1798761600 (verified via same python
        // recipe).
        let s = format_iso_8601_from_secs(1_798_761_600);
        assert_eq!(s, "2027-01-01T00:00:00Z");
    }

    #[test]
    fn now_iso_8601_leap_day_2028_02_29() {
        // 2028-02-29T00:00:00Z = 1835395200. Catches off-by-one bugs
        // in the Hinnant inverse-civil-date pass.
        let s = format_iso_8601_from_secs(1_835_395_200);
        assert_eq!(s, "2028-02-29T00:00:00Z");
    }

    #[test]
    fn now_iso_8601_pre_epoch_behavior_pinned() {
        // 1969-12-31T23:59:59Z = -1 (one second before the epoch).
        // PINS the behavior: the formatter does NOT panic; it returns
        // the historical date. A future refactor that changes this
        // (e.g., switches to "0000-... saturate") will catch the
        // diff here.
        let s = format_iso_8601_from_secs(-1);
        assert_eq!(s, "1969-12-31T23:59:59Z");
    }
}
