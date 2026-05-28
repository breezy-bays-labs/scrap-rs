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
use crate::domain::config::FileConfig;
use crate::domain::threshold::ThresholdMode;
use crate::ports::source::SourceError;

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
}
