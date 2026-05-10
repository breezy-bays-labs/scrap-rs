//! Cucumber-rs harness for the file-walker behavioral contract at
//! `tests/features/file_walker.feature`.
//!
//! V8a wires the harness skeleton + the empty-directory scenario only;
//! V8b adds the remaining 12 scenarios + the Scenario Outline.
//!
//! Step matching uses `regex = r"..."` mode rather than Cucumber
//! Expressions because most steps embed backticks, brackets, and
//! quoted strings that the Expression parser treats as special. The
//! 2026-03-25 Kit lesson against cucumber 0.21 still applies on 0.23
//! for any step with embedded `\[...\]` or quoted-string literals.
//! Trivial Background steps could use plain text; regex throughout
//! keeps the file uniform.

use cucumber::{World as _, given, then, when};
use scrap_core::adapters::source::fs::FsWalker;
use scrap_core::adapters::source::memory::MemorySource;
use scrap_core::domain::config::AnalysisConfig;
use scrap_core::domain::source::DiscoveryOutcome;
use scrap_core::domain::types::SourceRoot;
use scrap_core::ports::source::{SourceError, SourcePort};

/// Per-scenario state. Cucumber-rs constructs a fresh `World` for each
/// scenario via `Default`, so step definitions can rely on every field
/// starting as `None` / empty.
///
/// `walker_construction_result` and `memory_source` are V8b-only — V8a
/// only exercises the empty-directory happy path through `walker` /
/// `outcome`. The `#[allow(dead_code)]` on the struct prevents the
/// dead-field lint from blocking V8a; V8b's added scenarios exercise
/// every field.
#[allow(dead_code)]
#[derive(Debug, cucumber::World, Default)]
struct World {
    tempdir: Option<tempfile::TempDir>,
    config: Option<AnalysisConfig>,
    walker: Option<FsWalker>,
    walker_construction_result: Option<Result<FsWalker, SourceError>>,
    outcome: Option<Result<DiscoveryOutcome, SourceError>>,
    memory_source: Option<MemorySource>,
}

// ─── Background ─────────────────────────────────────────────────────

#[given(regex = r"^a fresh test World$")]
fn fresh_world(_w: &mut World) {
    // World::default() already gives empty Option fields; nothing to do.
}

// ─── Empty-directory scenario steps ─────────────────────────────────

#[given(regex = r"^a temporary directory containing no files$")]
fn empty_tempdir(w: &mut World) {
    w.tempdir = Some(tempfile::tempdir().expect("tempdir creation"));
}

#[given(
    regex = r#"^an `AnalysisConfig` with `extensions = \["rs"\]` and `respect_gitignore = true`$"#
)]
fn config_rs_gitignore_true(w: &mut World) {
    let src = SourceRoot::new(w.tempdir.as_ref().expect("tempdir given").path());
    w.config = Some(AnalysisConfig::new(src, vec![], vec!["rs".into()], true));
}

#[given(regex = r"^an `FsWalker` constructed from that config$")]
fn fswalker_from_config(w: &mut World) {
    let config = w.config.clone().expect("config given");
    w.walker = Some(FsWalker::try_new(config).expect("walker construction"));
}

#[when(
    regex = r"^the caller invokes `discover_test_files\(root\)` against the temporary directory$"
)]
fn invoke_discover(w: &mut World) {
    let walker = w.walker.as_ref().expect("walker given");
    let root = SourceRoot::new(w.tempdir.as_ref().expect("tempdir given").path());
    w.outcome = Some(walker.discover_test_files(&root));
}

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

// ─── Harness ────────────────────────────────────────────────────────

#[tokio::main(flavor = "current_thread")]
async fn main() {
    World::cucumber()
        .filter_run_and_exit("tests/features", |_feature, _rule, scenario| {
            // @unix-tagged scenarios run only on Unix; everything else
            // runs everywhere. Permission-denied scenarios are gated
            // here per the .feature file's @unix tag. Expressed as
            // `not-tagged || unix` so clippy doesn't flag the branch
            // as always-true on Unix builds.
            !scenario.tags.iter().any(|t| t == "unix") || cfg!(unix)
        })
        .await;
}
