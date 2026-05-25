//! Cucumber-rs harness for the file-walker behavioral contract at
//! `tests/features/file_walker.feature`.
//!
//! Step matchers use `regex = r"..."` mode rather than Cucumber
//! Expressions because most steps embed backticks, brackets, and
//! quoted strings that the Expression parser treats as special-syntax
//! tokens; regex mode sidesteps that for every step in this harness.

// TODO(scrap-rs#12 S1.1 follow-up): the workspace lints lift in S1.1
// (`[workspace.lints.clippy]` block in root Cargo.toml replacing the
// per-crate `#![warn(clippy::pedantic, clippy::cargo)]` headers)
// surfaced 17 pre-existing pedantic nits in this file
// (needless_pass_by_value × 14, manual_let_else × 2,
// needless_raw_string_hashes × 1). They were latent under the
// per-crate setup because lib-root `#![warn(...)]` doesn't propagate
// to integration tests; the workspace `[lints]` block does. The
// suppressions below keep S1.1 scoped — the cleanup (changing
// cucumber step-fn `String` params to `&str`, rewriting
// `match { _ => panic!() }` blocks as `let-else`) lands as a focused
// follow-up chore commit so this PR stays the parser PR, not the
// file-walker-harness-cleanup PR.
//
// tracked: scrap-rs#50 — lift after parser PR; surfaced when workspace
// [lints] extended clippy::pedantic to integration tests.
#![allow(clippy::needless_pass_by_value)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::needless_raw_string_hashes)]

use cucumber::{World as _, gherkin, given, then, when};
use scrap_core::adapters::source::fs::FsWalker;
use scrap_core::adapters::source::memory::MemorySource;
use scrap_core::cli::config::{ConfigError, FileConfig};
use scrap_core::domain::config::AnalysisConfig;
use scrap_core::domain::source::{DiscoveryOutcome, SourceDiagnostic, SourceDiagnosticKind};
use scrap_core::domain::types::{FilePath, SourceRoot};
use scrap_core::ports::source::{SourceError, SourcePort};
use std::path::{Path, PathBuf};

// ─── Sibling step-def modules (W5.1 mod-block split per SHOULD-FIX #5) ─
//
// Cucumber-rs registers `#[given]/#[when]/#[then]` step fns globally
// within the test binary; sub-modules referenced via `#[path]` work
// identically. World stays in this entry file so cucumber::World's
// derive sees the single canonical struct.
//
// scrap-rs#67 stays open as a fallback if implementation surfaces
// unexpected cucumber-rs quirks; W0.1 spike found mod-blocks viable.

#[path = "cucumber_steps/config.rs"]
mod config_steps;

// ─── World ──────────────────────────────────────────────────────────

/// Per-scenario state. Cucumber-rs constructs a fresh `World` for each
/// scenario via `Default`; all fields default to `None` / empty.
#[derive(Debug, cucumber::World, Default)]
pub struct World {
    pub tempdir: Option<tempfile::TempDir>,
    pub config: Option<AnalysisConfig>,
    pub walker: Option<FsWalker>,
    pub walker_construction_result: Option<Result<FsWalker, SourceError>>,
    pub outcome: Option<Result<DiscoveryOutcome, SourceError>>,
    pub memory_source: Option<MemorySource>,
    /// Tracks the explicit pre-flight root path supplied by the
    /// `missing-root` and `file-root` scenarios, so the corresponding
    /// `Then` step can assert that `SourceError::Io.path` equals it.
    pub expected_io_path: Option<PathBuf>,
    /// W5.1 config-loader fields — populated by `cucumber_steps::config`
    /// step defs.
    pub config_fixture_path: Option<PathBuf>,
    pub config_load_result: Option<Result<FileConfig, ConfigError>>,
    pub discover_result: Option<Result<Option<PathBuf>, ConfigError>>,
}

// ─── Background ─────────────────────────────────────────────────────

#[given(regex = r"^a fresh test World$")]
fn fresh_world(_w: &mut World) {
    // World::default() already gives empty Option fields; nothing to do.
}

// ─── Tempdir / fixture builders ─────────────────────────────────────

fn ensure_tempdir(w: &mut World) {
    if w.tempdir.is_none() {
        w.tempdir = Some(tempfile::tempdir().expect("tempdir creation"));
    }
}

fn tempdir_path(w: &World) -> &Path {
    w.tempdir.as_ref().expect("tempdir given").path()
}

fn touch(root: &Path, rel: &str) -> PathBuf {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, "").unwrap();
    path
}

#[given(regex = r"^a temporary directory containing no files$")]
fn empty_tempdir(w: &mut World) {
    ensure_tempdir(w);
}

#[given(regex = r"^a temporary directory containing exactly `(.+?)`$")]
fn tempdir_with_one_file(w: &mut World, file: String) {
    ensure_tempdir(w);
    touch(tempdir_path(w), &file);
}

#[given(regex = r"^a temporary directory containing `(.+?)` and `(.+?)`$")]
fn tempdir_with_two_files(w: &mut World, a: String, b: String) {
    ensure_tempdir(w);
    let root = tempdir_path(w).to_path_buf();
    touch(&root, &a);
    touch(&root, &b);
}

#[given(regex = r"^a temporary directory containing `(.+?)`, `(.+?)`, and `(.+?)`$")]
fn tempdir_with_three_files(w: &mut World, a: String, b: String, c: String) {
    ensure_tempdir(w);
    let root = tempdir_path(w).to_path_buf();
    touch(&root, &a);
    touch(&root, &b);
    touch(&root, &c);
}

#[given(
    regex = r"^a temporary directory containing `(.+?)`, `(.+?)`, and a `\.gitignore` listing `(.+?)`$"
)]
fn tempdir_with_gitignore(w: &mut World, a: String, b: String, ignored: String) {
    ensure_tempdir(w);
    let root = tempdir_path(w).to_path_buf();
    touch(&root, &a);
    touch(&root, &b);
    std::fs::write(root.join(".gitignore"), format!("{ignored}\n")).unwrap();
}

#[given(regex = r"^a temporary directory with the following structure:$")]
fn tempdir_with_structure(w: &mut World, step: &gherkin::Step) {
    ensure_tempdir(w);
    let root = tempdir_path(w).to_path_buf();
    let table = step.table.as_ref().expect("data table given");
    for row in table.rows.iter().skip(1) {
        let rel = row.first().expect("path cell").trim();
        if !rel.is_empty() {
            touch(&root, rel);
        }
    }
}

#[cfg(unix)]
#[given(regex = r"^a temporary directory containing `(.+?)` and a symlink `(.+?)` pointing at it$")]
fn tempdir_with_symlink(w: &mut World, target: String, link: String) {
    ensure_tempdir(w);
    let root = tempdir_path(w).to_path_buf();
    touch(&root, &target);
    std::os::unix::fs::symlink(root.join(&target), root.join(&link)).expect("symlink creation");
}

// ─── AnalysisConfig builders ────────────────────────────────────────

#[given(
    regex = r#"^an `AnalysisConfig` with `extensions = \["rs"\]` and `respect_gitignore = (true|false)`$"#
)]
fn config_rs_gitignore_param(w: &mut World, respect: String) {
    let src = SourceRoot::new(tempdir_path(w));
    let respect_bool = respect == "true";
    w.config = Some(AnalysisConfig::new(
        src,
        vec![],
        vec!["rs".into()],
        respect_bool,
    ));
}

#[given(regex = r#"^an `AnalysisConfig` with `extensions = \["rs"\]`$"#)]
fn config_rs(w: &mut World) {
    let src = SourceRoot::new(tempdir_path(w));
    w.config = Some(AnalysisConfig::new(src, vec![], vec!["rs".into()], true));
}

#[given(regex = r#"^an `AnalysisConfig` with `extensions = \[\]`$"#)]
fn config_empty_extensions(w: &mut World) {
    let src = SourceRoot::new(tempdir_path(w));
    w.config = Some(AnalysisConfig::new(src, vec![], vec![], true));
}

#[given(
    regex = r#"^an `AnalysisConfig` with `exclude = \["vendored/\*\*"\]` and `extensions = \["rs"\]`$"#
)]
fn config_with_exclude(w: &mut World) {
    let src = SourceRoot::new(tempdir_path(w));
    w.config = Some(AnalysisConfig::new(
        src,
        vec!["vendored/**".into()],
        vec!["rs".into()],
        true,
    ));
}

#[given(regex = r#"^an `AnalysisConfig` with `exclude = \["\[unclosed"\]`$"#)]
fn config_with_invalid_glob(w: &mut World) {
    // No tempdir needed for the pre-walk-fatal scenario; build a
    // throwaway SourceRoot just to satisfy AnalysisConfig::new.
    ensure_tempdir(w);
    let src = SourceRoot::new(tempdir_path(w));
    w.config = Some(AnalysisConfig::new(
        src,
        vec!["[unclosed".into()],
        vec!["rs".into()],
        true,
    ));
}

#[given(regex = r"^an `FsWalker` constructed from that config$")]
fn fswalker_from_config(w: &mut World) {
    let config = w.config.clone().expect("config given");
    w.walker = Some(FsWalker::try_new(config).expect("walker construction"));
}

#[given(
    regex = r#"^an `FsWalker` constructed from a valid `AnalysisConfig`(?:| with `extensions = \["rs"\]`)$"#
)]
fn fswalker_with_default_config(w: &mut World) {
    ensure_tempdir(w);
    let src = SourceRoot::new(tempdir_path(w));
    let cfg = AnalysisConfig::new(src, vec![], vec!["rs".into()], false);
    w.config = Some(cfg.clone());
    w.walker = Some(FsWalker::try_new(cfg).expect("walker construction"));
}

// ─── Pre-flight root fixtures (missing / file) ──────────────────────
//
// After the trait revision dropped the `root` parameter, the pre-flight
// scenarios wire the unusual root into `AnalysisConfig::src` directly.
// `expected_io_path` records the path the assertion will compare
// against `SourceError::Io.path`.

#[given(
    regex = r"^an `AnalysisConfig` with `src` pointing at a non-existent path under the test temp directory$"
)]
fn config_src_missing(w: &mut World) {
    ensure_tempdir(w);
    let missing = tempdir_path(w).join("does/not/exist");
    let cfg = AnalysisConfig::new(SourceRoot::new(&missing), vec![], vec!["rs".into()], false);
    w.expected_io_path = Some(missing);
    w.config = Some(cfg);
}

#[given(
    regex = r"^an `AnalysisConfig` with `src` pointing at a regular file under the test temp directory$"
)]
fn config_src_is_file(w: &mut World) {
    ensure_tempdir(w);
    let file = tempdir_path(w).join("regular_file.rs");
    std::fs::write(&file, "").unwrap();
    let cfg = AnalysisConfig::new(SourceRoot::new(&file), vec![], vec!["rs".into()], false);
    w.expected_io_path = Some(file);
    w.config = Some(cfg);
}

// ─── Permission-denied fixture (chmod scope-guard via World drop) ───

#[cfg(unix)]
#[given(regex = r"^`denied` has been chmod'd to 0o000$")]
fn chmod_denied_dir(w: &mut World) {
    use std::os::unix::fs::PermissionsExt;
    let denied = tempdir_path(w).join("denied");
    // chmod the subdir; the World::Drop hook below restores 0o755 in
    // World's Drop impl so TempDir cleanup stays stderr-clean.
    std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o000))
        .expect("chmod 0o000 on denied/");
}

// ─── MemorySource builders ──────────────────────────────────────────

fn rows_to_filepaths(table: &gherkin::Table) -> Vec<FilePath> {
    table
        .rows
        .iter()
        .skip(1)
        .map(|row| FilePath::new(row.first().expect("path cell").trim()))
        .collect()
}

#[given(regex = r"^a `MemorySource` constructed via `MemorySource::with_files` with the files:$")]
fn memory_source_with_files(w: &mut World, step: &gherkin::Step) {
    let files = rows_to_filepaths(step.table.as_ref().expect("data table"));
    w.memory_source = Some(MemorySource::with_files(files));
}

#[given(regex = r"^a `MemorySource` constructed via `MemorySource::new` with the files:$")]
fn memory_source_new_files(w: &mut World, step: &gherkin::Step) {
    // Stash files; diagnostics arrive in the next Given step.
    let files = rows_to_filepaths(step.table.as_ref().expect("data table"));
    w.memory_source = Some(MemorySource::with_files(files));
}

fn parse_diagnostic_kind(s: &str) -> SourceDiagnosticKind {
    match s {
        "PermissionDenied" => SourceDiagnosticKind::PermissionDenied,
        "MidwalkIo" => SourceDiagnosticKind::MidwalkIo,
        "Other" => SourceDiagnosticKind::Other,
        other => panic!("unrecognized diagnostic kind: {other}"),
    }
}

fn rows_to_diagnostics(table: &gherkin::Table) -> Vec<SourceDiagnostic> {
    table
        .rows
        .iter()
        .skip(1)
        .map(|row| {
            let kind = parse_diagnostic_kind(row[0].trim());
            let path = FilePath::new(row[1].trim());
            let message = row[2].trim().to_string();
            SourceDiagnostic::new(path, kind, message)
        })
        .collect()
}

#[given(regex = r"^the diagnostics:$")]
fn memory_source_diagnostics(w: &mut World, step: &gherkin::Step) {
    let diagnostics = rows_to_diagnostics(step.table.as_ref().expect("data table"));
    let existing = w.memory_source.take().expect("memory source already set");
    let files = existing.files().to_vec();
    w.memory_source = Some(MemorySource::new(files, diagnostics));
}

// ─── When ───────────────────────────────────────────────────────────

/// Build the walker on demand from the stored config. Many scenarios
/// in the .feature go directly from `Given AnalysisConfig` to
/// `When discover_test_files()` without an explicit
/// `Given an FsWalker constructed from that config` step; this
/// implicit construction satisfies them.
fn ensure_walker(w: &mut World) {
    if w.walker.is_none() {
        let cfg = w.config.clone().expect("config given");
        w.walker = Some(FsWalker::try_new(cfg).expect("walker construction"));
    }
}

#[when(regex = r"^the caller invokes `discover_test_files\(\)`$")]
fn invoke_discover(w: &mut World) {
    ensure_walker(w);
    let walker = w.walker.as_ref().expect("walker");
    w.outcome = Some(walker.discover_test_files());
}

#[when(regex = r"^the caller invokes `discover_test_files\(\)` twice$")]
fn invoke_discover_twice(w: &mut World) {
    ensure_walker(w);
    let walker = w.walker.as_ref().expect("walker");
    let _ = walker.discover_test_files().expect("first invocation");
    w.outcome = Some(walker.discover_test_files());
}

#[when(regex = r"^the caller constructs `FsWalker::try_new\(config\)`$")]
fn caller_constructs_walker(w: &mut World) {
    let cfg = w.config.clone().expect("config given");
    let result = FsWalker::try_new(cfg);
    if let Ok(ref walker) = result {
        w.walker = Some(walker.clone());
    }
    w.walker_construction_result = Some(result);
}

#[when(regex = r"^the caller invokes `discover_test_files\(\)` on the `MemorySource`$")]
fn invoke_memory_source(w: &mut World) {
    let src = w.memory_source.as_ref().expect("memory source given");
    w.outcome = Some(src.discover_test_files());
}

// ─── Then helpers ───────────────────────────────────────────────────

fn outcome_paths(w: &World) -> Vec<String> {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    outcome
        .files
        .iter()
        .map(|fp| fp.as_path().to_string_lossy().into_owned())
        .collect()
}

fn parse_path_table(table: &gherkin::Table) -> Vec<String> {
    table
        .rows
        .iter()
        .skip(1)
        .map(|row| row.first().expect("path cell").trim().to_string())
        .collect()
}

/// Robust split for the Scenario Outline `;`-delimited cell. Trims
/// whitespace and drops empty pieces so trailing/leading delimiters
/// from Markdown table cells don't manifest as empty paths.
fn split_semi(s: &str) -> Vec<String> {
    s.split(';')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(String::from)
        .collect()
}

// ─── Then — Ok with files (in order, exact) ─────────────────────────

#[then(regex = r"^the result is `Ok` and `files` is empty$")]
fn assert_files_empty(w: &mut World) {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    assert!(
        outcome.files.is_empty(),
        "expected empty, got {:?}",
        outcome.files
    );
}

#[then(regex = r"^`diagnostics` is empty$")]
fn assert_diagnostics_empty(w: &mut World) {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    assert!(
        outcome.diagnostics.is_empty(),
        "expected empty, got {:?}",
        outcome.diagnostics,
    );
}

#[then(regex = r"^the result is `Ok` and `files` equals \(in order\):$")]
fn assert_files_equals_in_order(w: &mut World, step: &gherkin::Step) {
    let expected = parse_path_table(step.table.as_ref().expect("data table"));
    let actual = outcome_paths(w);
    assert_eq!(actual, expected);
}

#[then(regex = r"^the result is `Ok` and `files` contains exactly:$")]
fn assert_files_contains_exactly(w: &mut World, step: &gherkin::Step) {
    let expected_raw = parse_path_table(step.table.as_ref().expect("data table"));
    // Each cell may itself be `;`-delimited (Scenario Outline reuses
    // the same step body, substituting `keep.rs;skip.rs` etc.).
    let mut expected: Vec<String> = expected_raw.iter().flat_map(|c| split_semi(c)).collect();
    let mut actual = outcome_paths(w);
    expected.sort();
    actual.sort();
    assert_eq!(actual, expected);
}

#[then(regex = r"^both invocations return `Ok` with the exact same `files` \(in order\):$")]
fn assert_both_invocations_match_files(w: &mut World, step: &gherkin::Step) {
    let expected = parse_path_table(step.table.as_ref().expect("data table"));
    let actual = outcome_paths(w);
    assert_eq!(actual, expected);
}

#[then(regex = r"^`files` does NOT contain `(.+?)`$")]
fn assert_files_does_not_contain(w: &mut World, missing: String) {
    let actual = outcome_paths(w);
    assert!(
        !actual.contains(&missing),
        "expected {missing} absent, but found in {actual:?}",
    );
}

// ─── Then — Err(SourceError::InvalidGlob) ──────────────────────────

#[then(regex = r#"^the result is `Err\(SourceError::InvalidGlob\)` with `pattern = "(.+?)"`$"#)]
fn assert_err_invalid_glob(w: &mut World, expected_pattern: String) {
    let result = w
        .walker_construction_result
        .as_ref()
        .expect("walker_construction_result");
    match result {
        Err(SourceError::InvalidGlob { pattern, .. }) => {
            assert_eq!(pattern, &expected_pattern);
        }
        other => panic!("expected SourceError::InvalidGlob, got {other:?}"),
    }
}

#[then(regex = r"^the underlying `source` is a `globset::Error`$")]
fn assert_underlying_globset(w: &mut World) {
    use std::error::Error;
    let result = w
        .walker_construction_result
        .as_ref()
        .expect("walker_construction_result");
    let err = match result {
        Err(e) => e,
        Ok(_) => panic!("expected Err, got Ok"),
    };
    let source = err.source().expect("source chain");
    // Best-effort downcast via type_id — globset::Error is the
    // documented source of SourceError::InvalidGlob.
    assert!(
        source.downcast_ref::<globset::Error>().is_some(),
        "expected globset::Error, got {source:?}",
    );
}

#[then(regex = r"^no walk has begun$")]
fn assert_no_walk_began(w: &mut World) {
    assert!(
        w.outcome.is_none(),
        "outcome should be None — walk did not run"
    );
}

// ─── Then — Err(SourceError::Io) (pre-flight scenarios) ─────────────

#[then(
    regex = r"^the result is `Err\(SourceError::Io\)` with `path` equal to the configured-root `FilePath`$"
)]
fn assert_err_io_with_configured_path(w: &mut World) {
    let outcome = w.outcome.as_ref().expect("outcome");
    let expected = w.expected_io_path.as_ref().expect("expected_io_path");
    match outcome {
        Err(SourceError::Io { path, .. }) => {
            assert_eq!(path, &FilePath::new(expected));
        }
        other => panic!("expected SourceError::Io, got {other:?}"),
    }
}

#[then(regex = r"^the underlying `source` is a `std::io::Error`$")]
fn assert_underlying_io(w: &mut World) {
    use std::error::Error;
    let outcome = w.outcome.as_ref().expect("outcome");
    let err = match outcome {
        Err(e) => e,
        Ok(_) => panic!("expected Err, got Ok"),
    };
    let source = err.source().expect("source chain");
    assert!(
        source.downcast_ref::<std::io::Error>().is_some(),
        "expected std::io::Error, got {source:?}",
    );
}

// ─── Then — Permission-denied diagnostic ────────────────────────────

#[then(regex = r"^`diagnostics` contains exactly one `SourceDiagnostic`$")]
fn assert_one_diagnostic(w: &mut World) {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    assert_eq!(
        outcome.diagnostics.len(),
        1,
        "expected exactly one diagnostic, got {:?}",
        outcome.diagnostics
    );
}

#[then(regex = r"^that diagnostic has `kind = PermissionDenied`$")]
fn assert_diagnostic_kind_permission_denied(w: &mut World) {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    assert_eq!(
        outcome.diagnostics[0].kind,
        SourceDiagnosticKind::PermissionDenied,
    );
}

#[then(regex = r"^that diagnostic's `path` includes the `denied` subdirectory$")]
fn assert_diagnostic_path_contains_denied(w: &mut World) {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    let diag_path = outcome.diagnostics[0].path.as_path().display().to_string();
    assert!(
        diag_path.contains("denied"),
        "expected 'denied' in path, got {diag_path}",
    );
}

// ─── Then — Symlink diagnostic ──────────────────────────────────────

#[then(regex = r"^that diagnostic has `kind = Other`$")]
fn assert_diagnostic_kind_other(w: &mut World) {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    assert_eq!(outcome.diagnostics[0].kind, SourceDiagnosticKind::Other);
}

#[then(regex = r"^that diagnostic's `message` mentions `symlink`$")]
fn assert_diagnostic_message_mentions_symlink(w: &mut World) {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    assert!(
        outcome.diagnostics[0].message.contains("symlink"),
        "expected 'symlink' in message, got {:?}",
        outcome.diagnostics[0].message,
    );
}

// ─── Then — MemorySource diagnostics carry-through ──────────────────

#[then(regex = r"^`diagnostics` equals \(in order\):$")]
fn assert_diagnostics_equal_in_order(w: &mut World, step: &gherkin::Step) {
    let outcome = w.outcome.as_ref().expect("outcome").as_ref().expect("Ok");
    let expected = rows_to_diagnostics(step.table.as_ref().expect("data table"));
    assert_eq!(outcome.diagnostics, expected);
}

// ─── Cleanup hook — restore chmod 0o755 before TempDir drops ────────
//
// World's Drop runs first as the outer impl, so the chmod-restore
// happens before the TempDir field's destructor unlinks the tree.
// Without it, TempDir's rm -rf on a 0o000 subdir would emit stderr
// noise that downstream agentic loops misread as a real test failure.

impl Drop for World {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(tmp) = self.tempdir.as_ref() {
            use std::os::unix::fs::PermissionsExt;
            let denied = tmp.path().join("denied");
            if denied.exists() {
                let _ = std::fs::set_permissions(&denied, std::fs::Permissions::from_mode(0o755));
            }
        }
    }
}

// ─── Harness ────────────────────────────────────────────────────────

#[tokio::main(flavor = "current_thread")]
async fn main() {
    World::cucumber()
        .filter_run_and_exit("tests/features", |_feature, _rule, scenario| {
            // @unix-tagged scenarios run only on Unix; everything else
            // runs everywhere. Expressed as `not-tagged || unix` so
            // clippy doesn't flag the branch as always-true on Unix.
            !scenario.tags.iter().any(|t| t == "unix") || cfg!(unix)
        })
        .await;
}
