//! `FsWalker` — disk-backed `SourcePort` impl built on `ignore::WalkBuilder`.

use crate::domain::config::AnalysisConfig;
use crate::domain::source::{DiscoveryOutcome, SourceDiagnostic, SourceDiagnosticKind};
use crate::domain::types::FilePath;
use crate::ports::source::{SourceError, SourcePort};
use ignore::overrides::{Override, OverrideBuilder};
use std::path::Path;

/// Disk-backed `SourcePort` implementation.
///
/// Construction (`FsWalker::try_new`) eagerly compiles every user
/// exclude glob via `globset` and assembles an `ignore::overrides::Override`
/// matcher. Failures surface as `SourceError::InvalidGlob` (per-pattern
/// compile error), `SourceError::EmptyExcludePattern` (empty/whitespace
/// pattern that `globset` would silently accept and rewrite into a
/// global whitelist), or `SourceError::Ignore` (the rare
/// `OverrideBuilder::build()` rejection — see the variant docstring).
///
/// `discover_test_files` runs lazily per call: pre-flights the
/// adapter-configured root, builds an `ignore::WalkBuilder` honouring
/// `AnalysisConfig::respect_gitignore`, iterates entries, applies a
/// post-iteration extension filter, sorts the collected paths
/// byte-wise, and returns a [`DiscoveryOutcome`] with non-fatal mid-walk
/// diagnostics attached. Emitted file paths are relative to
/// `AnalysisConfig::src` so reports and snapshots are stable across
/// machines.
#[derive(Debug, Clone)]
pub struct FsWalker {
    /// Caller-supplied configuration. Stored verbatim — the walker
    /// re-reads `extensions`, `respect_gitignore`, `src`, etc. per call.
    config: AnalysisConfig,
    /// Pre-compiled negative-override matcher built from
    /// `config.exclude` at `try_new` time. `Override` is internally
    /// `Arc`-backed, so cloning the walker is cheap.
    override_matcher: Override,
}

impl FsWalker {
    /// Construct a walker, eagerly validating every entry in
    /// `config.exclude`.
    ///
    /// # Errors
    ///
    /// - [`SourceError::EmptyExcludePattern`] when an exclude pattern
    ///   is empty or whitespace-only. `globset` accepts these silently
    ///   and the override builder rewrites them into a global whitelist
    ///   — the opposite of caller intent. We reject eagerly.
    /// - [`SourceError::InvalidGlob`] when an exclude pattern fails
    ///   `globset::Glob::new`. The variant's `pattern` field carries
    ///   the offending raw pattern.
    /// - [`SourceError::Ignore`] when `OverrideBuilder::build()`
    ///   rejects the assembled matcher despite each individual
    ///   `.add()` call having succeeded. Forward-compat hatch — see
    ///   the variant docstring.
    pub fn try_new(config: AnalysisConfig) -> Result<Self, SourceError> {
        // Validate each user exclude pattern. Empty/whitespace-only
        // patterns must be rejected eagerly: globset::Glob::new("")
        // succeeds and OverrideBuilder::add("!") rewrites the result
        // into a "**/" whitelist that nullifies ALL exclude semantics
        // — silent data deletion the caller didn't ask for.
        for pattern in &config.exclude {
            if pattern.trim().is_empty() {
                return Err(SourceError::EmptyExcludePattern {
                    pattern: pattern.clone(),
                });
            }
            globset::Glob::new(pattern).map_err(|source| SourceError::InvalidGlob {
                pattern: pattern.clone(),
                source,
            })?;
        }

        let mut builder = OverrideBuilder::new(config.src.as_path());
        for pattern in &config.exclude {
            // Negative override (leading `!`) excludes matching paths.
            // Per-call .add() failure is treated the same as
            // OverrideBuilder::build() failure — forward-compat hatch.
            builder
                .add(&format!("!{pattern}"))
                .map_err(|source| SourceError::Ignore { source })?;
        }

        let override_matcher = builder
            .build()
            .map_err(|source| SourceError::Ignore { source })?;

        Ok(Self {
            config,
            override_matcher,
        })
    }
}

impl SourcePort for FsWalker {
    fn discover_test_files(&self) -> Result<DiscoveryOutcome, SourceError> {
        let path = self.config.src.as_path();
        preflight_root(path)?;

        let builder = self.build_walker(path);
        let allowed_extensions: &[String] = &self.config.extensions;

        let mut files: Vec<FilePath> = Vec::new();
        let mut diagnostics: Vec<SourceDiagnostic> = Vec::new();

        for entry in builder.build() {
            match entry {
                Ok(entry) => match classify_entry(&entry, path, allowed_extensions) {
                    Decision::Include(fp) => files.push(fp),
                    Decision::Skip => {}
                    Decision::Diagnostic(d) => diagnostics.push(d),
                },
                Err(err) => diagnostics.push(classify_walk_error(&err, path)),
            }
        }

        // Post-collect byte-wise sort on the underlying OsStr (E1 from
        // shaping). NOT `files.sort()` — `PathBuf`'s natural Ord is
        // component-wise (sorts `a/b.rs` before `a.rs` because the
        // first components compare `"a"` < `"a.rs"`). We need byte-wise
        // comparison of the full path string so the .feature data
        // table's `a.rs` before `a/b.rs` ordering holds (`.` byte 0x2E
        // < `/` byte 0x2F). FilePath deliberately does not derive Ord;
        // the explicit sort_by below is the only canonical iteration
        // order.
        files.sort_by(|a, b| a.as_path().as_os_str().cmp(b.as_path().as_os_str()));

        Ok(DiscoveryOutcome::new(files, diagnostics))
    }
}

impl FsWalker {
    /// Build the `ignore::WalkBuilder` configured from this walker's
    /// `AnalysisConfig`. Per-entry extension filtering and root-skip
    /// happen later in [`classify_entry`]; this method only sets up
    /// the VCS-honouring and user-override layers.
    fn build_walker(&self, path: &Path) -> ignore::WalkBuilder {
        let mut builder = ignore::WalkBuilder::new(path);
        builder
            .git_ignore(self.config.respect_gitignore)
            .git_exclude(self.config.respect_gitignore)
            .git_global(self.config.respect_gitignore)
            .ignore(self.config.respect_gitignore)
            // Honour .gitignore files even outside a git repository
            // (the ignore crate's default is to require .git/). User
            // expectation is "if there's a .gitignore here, respect
            // it"; this matches rg/fd behaviour with --no-require-git.
            .require_git(false)
            .overrides(self.override_matcher.clone());
        builder
    }
}

/// Per-entry classification outcome returned by [`classify_entry`].
///
/// The walk-loop body in [`FsWalker::discover_test_files`] is a flat
/// `match` on this enum: `Include` appends to the file collection,
/// `Skip` is a no-op, `Diagnostic` appends to the non-fatal diagnostic
/// stream. Lifts the per-entry decision tree out of the hot loop so the
/// loop reads as a 4-line dispatch and the classification logic can be
/// unit-tested through `classify_entry` independently of the walker.
enum Decision {
    Include(FilePath),
    Skip,
    Diagnostic(SourceDiagnostic),
}

/// Pre-flight the walk root: surface missing/non-directory roots as
/// fatal [`SourceError::Io`] *before* the walker silently yields an
/// empty iterator (which is indistinguishable from an empty directory).
fn preflight_root(path: &Path) -> Result<(), SourceError> {
    let metadata = std::fs::metadata(path).map_err(|source| SourceError::Io {
        path: FilePath::new(path),
        source,
    })?;
    if !metadata.is_dir() {
        return Err(SourceError::Io {
            path: FilePath::new(path),
            source: std::io::Error::other("source root is not a directory"),
        });
    }
    Ok(())
}

/// Classify one walker-yielded entry into a [`Decision`] that the walk
/// loop can dispatch on. Flat early-return chain mirrors the original
/// branch order:
///
/// 1. Depth-0 root entry → `Skip`. Documented `ignore` crate predicate
///    for the walk root; avoids a path-component comparison per entry.
/// 2. `file_type()` returns `None` → `Diagnostic(MidwalkIo)`. Typically
///    a stat failure on a dangling symlink; surface so the silent skip
///    is observable.
/// 3. Symlink → `Diagnostic(Other)`. Walker default is
///    `follow_links(false)`; without a diagnostic, symlinked source
///    files would be silently dropped (silent-failure class).
/// 4. Non-file (directory, fifo, etc.) → `Skip`.
/// 5. `allowed_extensions` empty → `Include` (no extension filtering).
/// 6. Extension matches one of `allowed_extensions` case-insensitively
///    → `Include`; otherwise `Skip`.
///
/// Per-entry extension matching uses `eq_ignore_ascii_case` against the
/// configured extensions verbatim (E2 from shaping). This avoids both a
/// `Vec<String>` pre-allocation and a per-entry `to_ascii_lowercase()`
/// heap allocation; the case-insensitive comparison happens in-place.
fn classify_entry(
    entry: &ignore::DirEntry,
    walked_root: &Path,
    allowed_extensions: &[String],
) -> Decision {
    if entry.depth() == 0 {
        return Decision::Skip;
    }
    let entry_path = entry.path();
    let Some(ft) = entry.file_type() else {
        return Decision::Diagnostic(SourceDiagnostic::new(
            relative_filepath(entry_path, walked_root),
            SourceDiagnosticKind::MidwalkIo,
            format!("could not determine file type for {}", entry_path.display()),
        ));
    };
    if ft.is_symlink() {
        return Decision::Diagnostic(SourceDiagnostic::new(
            relative_filepath(entry_path, walked_root),
            SourceDiagnosticKind::Other,
            format!("symlink not followed: {}", entry_path.display()),
        ));
    }
    if !ft.is_file() {
        return Decision::Skip;
    }
    if allowed_extensions.is_empty() {
        return Decision::Include(relative_filepath(entry_path, walked_root));
    }
    if let Some(ext) = entry_path.extension().and_then(std::ffi::OsStr::to_str)
        && allowed_extensions
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(ext))
    {
        return Decision::Include(relative_filepath(entry_path, walked_root));
    }
    Decision::Skip
}

/// Strip `walked_root` from `entry_path` and wrap the result as a
/// `FilePath`.
///
/// Two callers, two fallback semantics:
/// - File-collection path (`SourcePort::discover_test_files` walk loop):
///   the walker only yields entries under its base, so `strip_prefix`
///   is expected to succeed. The fallback prevents a surprise panic
///   if canonicalisation ever diverges.
/// - Diagnostic-attribution path (`classify_walk_error`): the
///   `WithPath` wrapper can carry paths from outside the walked tree
///   (e.g. `~/.config/git/ignore` when `git_global(true)` parses a
///   broken global gitignore). The fallback emits the raw absolute
///   path verbatim — a deliberate signal that the diagnostic
///   originated outside the project tree. `FilePath` documents this
///   exception to its "relative-when-emitted-by-FsWalker" convention.
fn relative_filepath(entry_path: &Path, walked_root: &Path) -> FilePath {
    entry_path
        .strip_prefix(walked_root)
        .map_or_else(|_| FilePath::new(entry_path), FilePath::new)
}

/// Classify a non-fatal mid-walk `ignore::Error` into a
/// [`SourceDiagnostic`]. Wrapper variants (`WithPath`, `WithDepth`,
/// `WithLineNumber`) are peeled to find the underlying classification;
/// `WithPath` also supplies path attribution. Errors that lack a
/// `WithPath` wrapper fall back to `fallback_root` (typically the walk
/// root). Branch coverage is unit-tested below against
/// hand-constructed `ignore::Error` values (per shaping A7b).
///
/// `Partial(Vec<Error>)` is intentionally NOT peeled: it surfaces only
/// when an ignore file partially loads, the inner `Vec` is rarely
/// non-empty in practice, and recursing would force the `path`
/// attribution to choose between conflicting inner wrappers. The
/// `Other` classification + `to_string()` message preserves the diag
/// signal without committing the adapter to a peel rule that would
/// shift across `ignore` minor versions.
fn classify_walk_error(err: &ignore::Error, fallback_root: &Path) -> SourceDiagnostic {
    let attributed_path = walk_error_attributed_path(err).unwrap_or(fallback_root);
    let attributed = relative_filepath(attributed_path, fallback_root);
    let kind = walk_error_kind(err);
    SourceDiagnostic::new(attributed, kind, err.to_string())
}

/// Peel `ignore::Error` wrappers to find the first `WithPath`-supplied
/// path, if any.
fn walk_error_attributed_path(err: &ignore::Error) -> Option<&Path> {
    let mut cursor = err;
    loop {
        match cursor {
            ignore::Error::WithPath { path, .. } => return Some(path.as_path()),
            ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
                cursor = err.as_ref();
            }
            _ => return None,
        }
    }
}

/// Peel `ignore::Error` wrappers to classify the underlying failure.
fn walk_error_kind(err: &ignore::Error) -> SourceDiagnosticKind {
    let mut cursor = err;
    loop {
        match cursor {
            ignore::Error::WithPath { err, .. }
            | ignore::Error::WithDepth { err, .. }
            | ignore::Error::WithLineNumber { err, .. } => cursor = err.as_ref(),
            ignore::Error::Io(io_err) => {
                return if io_err.kind() == std::io::ErrorKind::PermissionDenied {
                    SourceDiagnosticKind::PermissionDenied
                } else {
                    SourceDiagnosticKind::MidwalkIo
                };
            }
            _ => return SourceDiagnosticKind::Other,
        }
    }
}

#[cfg(test)]
static_assertions::assert_impl_all!(FsWalker: Send, Sync);

#[cfg(unix)]
#[cfg(test)]
struct PermissionGuard {
    path: std::path::PathBuf,
    restore_mode: u32,
}

#[cfg(unix)]
#[cfg(test)]
impl Drop for PermissionGuard {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        // Best-effort restore; ignore failure so panics in the test
        // body propagate cleanly. The whole point of the guard is to
        // chmod back to 0o755 BEFORE TempDir drops, so its rm -rf
        // does not leave stderr noise that downstream agentic loops
        // misread as a test failure.
        let _ = std::fs::set_permissions(
            &self.path,
            std::fs::Permissions::from_mode(self.restore_mode),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::config::AnalysisConfig;
    use crate::domain::types::SourceRoot;
    use std::fs;
    use std::path::PathBuf;

    fn cfg(exclude: Vec<String>) -> AnalysisConfig {
        AnalysisConfig::new(SourceRoot::new("."), exclude, vec!["rs".into()], true)
    }

    fn cfg_for(
        root: &Path,
        exclude: Vec<String>,
        extensions: Vec<String>,
        respect_gitignore: bool,
    ) -> AnalysisConfig {
        AnalysisConfig::new(
            SourceRoot::new(root),
            exclude,
            extensions,
            respect_gitignore,
        )
    }

    fn touch(root: &Path, rel: &str) -> PathBuf {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, "").unwrap();
        path
    }

    fn paths(outcome: &DiscoveryOutcome) -> Vec<String> {
        outcome
            .files
            .iter()
            .map(|fp| fp.as_path().to_string_lossy().into_owned())
            .collect()
    }

    // ─── try_new tests ───────────────────────────────────────────────

    #[test]
    fn try_new_with_no_excludes_returns_ok() {
        let walker = FsWalker::try_new(cfg(vec![]));
        assert!(walker.is_ok(), "expected Ok, got {:?}", walker.err());
    }

    #[test]
    fn try_new_with_valid_excludes_returns_ok() {
        let walker = FsWalker::try_new(cfg(vec!["vendored/**".into(), "target/**".into()]));
        assert!(walker.is_ok(), "expected Ok, got {:?}", walker.err());
    }

    #[test]
    fn try_new_with_invalid_glob_returns_invalid_glob_error() {
        let walker = FsWalker::try_new(cfg(vec!["[unclosed".into()]));
        match walker {
            Err(SourceError::InvalidGlob { pattern, .. }) => {
                assert_eq!(pattern, "[unclosed");
            }
            other => panic!("expected SourceError::InvalidGlob, got {other:?}"),
        }
    }

    #[test]
    fn try_new_with_empty_exclude_pattern_returns_empty_exclude_error() {
        let walker = FsWalker::try_new(cfg(vec![String::new()]));
        match walker {
            Err(SourceError::EmptyExcludePattern { pattern }) => {
                assert_eq!(pattern, "");
            }
            other => panic!("expected SourceError::EmptyExcludePattern, got {other:?}"),
        }
    }

    #[test]
    fn try_new_with_whitespace_exclude_pattern_returns_empty_exclude_error() {
        let walker = FsWalker::try_new(cfg(vec!["   ".into()]));
        match walker {
            Err(SourceError::EmptyExcludePattern { pattern }) => {
                assert_eq!(pattern, "   ");
            }
            other => panic!("expected SourceError::EmptyExcludePattern, got {other:?}"),
        }
    }

    // ─── Pre-flight failure tests ───────────────────────────────────

    #[test]
    fn missing_root_returns_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does/not/exist");
        let cfg = cfg_for(&missing, vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files();
        match outcome {
            Err(SourceError::Io { path, .. }) => {
                assert_eq!(path, FilePath::new(&missing));
            }
            other => panic!("expected SourceError::Io, got {other:?}"),
        }
    }

    #[test]
    fn root_is_file_returns_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("f.rs");
        fs::write(&file, "").unwrap();
        let cfg = cfg_for(&file, vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files();
        match outcome {
            Err(SourceError::Io { path, .. }) => {
                assert_eq!(path, FilePath::new(&file));
            }
            other => panic!("expected SourceError::Io, got {other:?}"),
        }
    }

    // ─── Walk happy-path tests ───────────────────────────────────────

    #[test]
    fn empty_directory_yields_empty_outcome() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        assert!(outcome.files.is_empty(), "{:?}", outcome.files);
        assert!(outcome.diagnostics.is_empty(), "{:?}", outcome.diagnostics);
    }

    #[test]
    fn single_file_is_returned() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.rs");
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        assert_eq!(paths(&outcome), vec!["a.rs".to_string()]);
    }

    #[test]
    fn nested_tree_returns_all_matching_files_in_deterministic_order() {
        let tmp = tempfile::tempdir().unwrap();
        for rel in ["a.rs", "a/b.rs", "a/sub/c.rs", "a/sub/d.rs", "b.rs"] {
            touch(tmp.path(), rel);
        }
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome_a = walker.discover_test_files().unwrap();
        let outcome_b = walker.discover_test_files().unwrap();
        let expected = vec![
            "a.rs".to_string(),
            "a/b.rs".to_string(),
            "a/sub/c.rs".to_string(),
            "a/sub/d.rs".to_string(),
            "b.rs".to_string(),
        ];
        assert_eq!(paths(&outcome_a), expected);
        assert_eq!(paths(&outcome_b), expected);
    }

    // ─── Filter tests ────────────────────────────────────────────────

    #[test]
    fn extension_filter_is_case_insensitive_and_bare() {
        let tmp = tempfile::tempdir().unwrap();
        for rel in ["a.rs", "b.RS", "c.txt"] {
            touch(tmp.path(), rel);
        }
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        let mut paths = paths(&outcome);
        paths.sort();
        assert_eq!(paths, vec!["a.rs".to_string(), "b.RS".to_string()]);
    }

    #[test]
    fn empty_extensions_includes_all_files() {
        let tmp = tempfile::tempdir().unwrap();
        for rel in ["a.rs", "b.txt", "c.md"] {
            touch(tmp.path(), rel);
        }
        let cfg = cfg_for(tmp.path(), vec![], vec![], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        let mut paths = paths(&outcome);
        paths.sort();
        assert_eq!(
            paths,
            vec!["a.rs".to_string(), "b.txt".to_string(), "c.md".to_string()]
        );
    }

    #[test]
    fn hidden_files_are_skipped_with_respect_gitignore_true() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "visible.rs");
        touch(tmp.path(), ".hidden.rs");
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        assert_eq!(paths(&outcome), vec!["visible.rs".to_string()]);
    }

    #[test]
    fn hidden_files_are_also_skipped_with_respect_gitignore_false() {
        // Pins the contract: hidden-file skipping comes from
        // WalkBuilder::hidden(true) (always-on default), not from the
        // gitignore toggle. A future refactor that conflates the two
        // flags would silently start including dotfiles when callers
        // disable gitignore — surprising behaviour the suite must catch.
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "visible.rs");
        touch(tmp.path(), ".hidden.rs");
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        assert_eq!(paths(&outcome), vec!["visible.rs".to_string()]);
    }

    // ─── VCS ignore tests ────────────────────────────────────────────

    #[test]
    fn respect_gitignore_true_skips_listed_files() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "keep.rs");
        touch(tmp.path(), "skip.rs");
        fs::write(tmp.path().join(".gitignore"), "skip.rs\n").unwrap();
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        assert_eq!(paths(&outcome), vec!["keep.rs".to_string()]);
    }

    #[test]
    fn respect_gitignore_false_includes_listed_files() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "keep.rs");
        touch(tmp.path(), "skip.rs");
        fs::write(tmp.path().join(".gitignore"), "skip.rs\n").unwrap();
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        let mut paths = paths(&outcome);
        paths.sort();
        assert_eq!(paths, vec!["keep.rs".to_string(), "skip.rs".to_string()]);
    }

    // ─── User-glob exclude ──────────────────────────────────────────

    #[test]
    fn user_glob_exclude_filters_matching_files() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "keep.rs");
        touch(tmp.path(), "vendored/skip.rs");
        let cfg = cfg_for(
            tmp.path(),
            vec!["vendored/**".into()],
            vec!["rs".into()],
            true,
        );
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        assert_eq!(paths(&outcome), vec!["keep.rs".to_string()]);
    }

    // ─── Symlink handling (skip + diagnostic) ───────────────────────

    #[cfg(unix)]
    #[test]
    fn symlinked_file_is_skipped_with_other_diagnostic() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "real.rs");
        std::os::unix::fs::symlink(tmp.path().join("real.rs"), tmp.path().join("link.rs")).unwrap();
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();
        assert_eq!(paths(&outcome), vec!["real.rs".to_string()]);
        assert_eq!(
            outcome.diagnostics.len(),
            1,
            "expected one symlink diagnostic, got {:?}",
            outcome.diagnostics,
        );
        let diag = &outcome.diagnostics[0];
        assert_eq!(diag.kind, SourceDiagnosticKind::Other);
        assert!(
            diag.message.contains("symlink"),
            "diag message should mention 'symlink', got {:?}",
            diag.message,
        );
    }

    // ─── Permission-denied (gated #[cfg(unix)]) ─────────────────────

    #[cfg(unix)]
    #[test]
    fn permission_denied_subdirectory_yields_diagnostic_and_walk_continues() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "accessible/a.rs");
        touch(tmp.path(), "denied/b.rs");
        let denied = tmp.path().join("denied");

        fs::set_permissions(&denied, fs::Permissions::from_mode(0o000)).unwrap();
        // Scope-guard: chmod back to 0o755 before TempDir drops, even
        // if the assertions below panic. Pristine stderr — agentic
        // loops misread cleanup noise as real test failures.
        let _guard = PermissionGuard {
            path: denied.clone(),
            restore_mode: 0o755,
        };

        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files().unwrap();

        assert_eq!(paths(&outcome), vec!["accessible/a.rs".to_string()]);
        assert_eq!(
            outcome.diagnostics.len(),
            1,
            "expected exactly one diagnostic, got {:?}",
            outcome.diagnostics,
        );
        let diag = &outcome.diagnostics[0];
        assert_eq!(diag.kind, SourceDiagnosticKind::PermissionDenied);
        let diag_path = diag.path.as_path().display().to_string();
        assert!(
            diag_path.contains("denied"),
            "expected 'denied' in diagnostic path, got {diag_path}",
        );
    }

    // ─── classify_walk_error branch tests (shaping A7b) ─────────────

    fn io(kind: std::io::ErrorKind, msg: &str) -> ignore::Error {
        ignore::Error::Io(std::io::Error::new(kind, msg))
    }

    #[test]
    fn classify_with_path_io_permission_denied_returns_permission_denied() {
        let inner = io(std::io::ErrorKind::PermissionDenied, "denied");
        let err = ignore::Error::WithPath {
            path: PathBuf::from("/tmp/scrap-classify/a"),
            err: Box::new(inner),
        };
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::PermissionDenied);
        // strip_prefix(/tmp/fallback) fails because the WithPath path
        // is /tmp/scrap-classify/a — we expect the raw fallback shape.
        assert_eq!(diag.path, FilePath::new("/tmp/scrap-classify/a"));
    }

    #[test]
    fn classify_with_path_io_other_returns_midwalk_io() {
        let inner = io(std::io::ErrorKind::NotFound, "gone");
        let err = ignore::Error::WithPath {
            path: PathBuf::from("/tmp/scrap-classify/b"),
            err: Box::new(inner),
        };
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::MidwalkIo);
        assert_eq!(diag.path, FilePath::new("/tmp/scrap-classify/b"));
    }

    #[test]
    fn classify_nested_with_depth_with_path_peels_correctly() {
        let inner = io(std::io::ErrorKind::PermissionDenied, "denied");
        let with_path = ignore::Error::WithPath {
            path: PathBuf::from("/tmp/scrap-classify/c"),
            err: Box::new(inner),
        };
        let with_depth = ignore::Error::WithDepth {
            depth: 3,
            err: Box::new(with_path),
        };
        let diag = classify_walk_error(&with_depth, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::PermissionDenied);
        assert_eq!(diag.path, FilePath::new("/tmp/scrap-classify/c"));
    }

    #[test]
    fn classify_loop_returns_other() {
        let err = ignore::Error::Loop {
            ancestor: PathBuf::from("/tmp/a"),
            child: PathBuf::from("/tmp/a/b"),
        };
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::Other);
        // No WithPath attribution → falls back to walk root → the
        // strip_prefix succeeds with an empty trailing path.
        assert_eq!(diag.path, FilePath::new(""));
    }

    #[test]
    fn classify_partial_empty_returns_other() {
        let err = ignore::Error::Partial(vec![]);
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::Other);
    }

    #[test]
    fn classify_partial_with_inner_permission_denied_still_returns_other() {
        // Pin the deliberate non-peel: inner `WithPath{Io(PermissionDenied)}`
        // does NOT bubble up through Partial. The classifier collapses
        // the multi-error case to `Other` so the adapter doesn't have
        // to choose between conflicting inner attributions; callers
        // read the `message` field for the verbose ignore::Error
        // formatting if they need the gory detail.
        let inner_io = io(std::io::ErrorKind::PermissionDenied, "denied");
        let inner_with_path = ignore::Error::WithPath {
            path: PathBuf::from("/tmp/scrap-classify/inner"),
            err: Box::new(inner_io),
        };
        let err = ignore::Error::Partial(vec![inner_with_path]);
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::Other);
    }

    #[test]
    fn classify_glob_returns_other() {
        let err = ignore::Error::Glob {
            glob: Some("[bad".into()),
            err: "missing ]".into(),
        };
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::Other);
    }

    #[test]
    fn classify_unrecognized_file_type_returns_other() {
        let err = ignore::Error::UnrecognizedFileType("xyz".into());
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::Other);
    }

    #[test]
    fn classify_invalid_definition_returns_other() {
        let err = ignore::Error::InvalidDefinition;
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        assert_eq!(diag.kind, SourceDiagnosticKind::Other);
    }

    #[test]
    fn classify_without_with_path_falls_back_to_root() {
        let err = ignore::Error::Loop {
            ancestor: PathBuf::from("/tmp/a"),
            child: PathBuf::from("/tmp/a/b"),
        };
        let diag = classify_walk_error(&err, Path::new("/tmp/fallback"));
        // Falls back to walk root → strip_prefix(walk_root) on itself
        // yields an empty path. Callers read `message` for the verbose
        // form when path attribution is empty.
        assert_eq!(diag.path, FilePath::new(""));
    }
}
