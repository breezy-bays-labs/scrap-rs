//! `FsWalker` — disk-backed `SourcePort` impl built on `ignore::WalkBuilder`.

use crate::domain::config::AnalysisConfig;
use crate::domain::source::{DiscoveryOutcome, SourceDiagnostic, SourceDiagnosticKind};
use crate::domain::types::{FilePath, SourceRoot};
use crate::ports::source::{SourceError, SourcePort};
use ignore::overrides::{Override, OverrideBuilder};
use std::path::Path;

/// Disk-backed `SourcePort` implementation.
///
/// Construction (`FsWalker::try_new`) eagerly compiles every user
/// exclude glob via `globset` and assembles an `ignore::overrides::Override`
/// matcher. Failures surface as `SourceError::InvalidGlob` (per-pattern
/// compile error) or `SourceError::Ignore` (the rare
/// `OverrideBuilder::build()` rejection — see the variant docstring).
///
/// `discover_test_files` runs lazily per call: pre-flights the root,
/// builds an `ignore::WalkBuilder` honouring
/// `AnalysisConfig::respect_gitignore`, iterates entries, applies a
/// post-iteration extension filter, sorts the collected paths
/// byte-wise, and returns a `DiscoveryOutcome` with non-fatal mid-walk
/// diagnostics attached.
#[derive(Debug, Clone)]
pub struct FsWalker {
    /// Caller-supplied configuration. Stored verbatim — the walker
    /// re-reads `extensions`, `respect_gitignore`, etc. per call.
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
    /// - [`SourceError::InvalidGlob`] when one of the user-supplied
    ///   exclude patterns fails `globset::Glob::new`. The variant's
    ///   `pattern` field carries the offending raw pattern.
    /// - [`SourceError::Ignore`] when `OverrideBuilder::build()`
    ///   rejects the assembled matcher despite each individual
    ///   `.add()` call having succeeded. Forward-compat hatch — see
    ///   the variant docstring.
    pub fn try_new(config: AnalysisConfig) -> Result<Self, SourceError> {
        // Validate each user exclude pattern via globset first so we
        // can surface the offending raw pattern as
        // SourceError::InvalidGlob (the OverrideBuilder swallows the
        // pattern text in its own error message).
        for pattern in &config.exclude {
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
    fn discover_test_files(&self, root: &SourceRoot) -> Result<DiscoveryOutcome, SourceError> {
        let path = root.as_path();

        // Pre-flight: surface missing/non-directory roots as fatal
        // SourceError::Io. The walk itself does NOT produce a clean
        // error here — it would silently yield an empty iterator,
        // which is indistinguishable from an empty directory.
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

        // Pre-lower the configured extension set; the per-entry check
        // is a case-insensitive bare-extension match (E2 from shaping).
        let allowed_extensions: Vec<String> = self
            .config
            .extensions
            .iter()
            .map(|e| e.to_ascii_lowercase())
            .collect();

        let mut files: Vec<FilePath> = Vec::new();
        let mut diagnostics: Vec<SourceDiagnostic> = Vec::new();

        for entry in builder.build() {
            match entry {
                Ok(entry) => {
                    if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                        continue;
                    }
                    if allowed_extensions.is_empty() {
                        files.push(FilePath::new(entry.into_path()));
                        continue;
                    }
                    let entry_ext = entry
                        .path()
                        .extension()
                        .and_then(std::ffi::OsStr::to_str)
                        .map(str::to_ascii_lowercase);
                    if let Some(ext) = entry_ext
                        && allowed_extensions.iter().any(|allowed| allowed == &ext)
                    {
                        files.push(FilePath::new(entry.into_path()));
                    }
                }
                Err(err) => diagnostics.push(classify_walk_error(&err, path)),
            }
        }

        // Post-collect byte-wise sort on the underlying OsStr (E1 from
        // shaping). NOT `files.sort()` — PathBuf's natural Ord is
        // component-wise (sorts `a/b.rs` before `a.rs` because the
        // first components compare `"a"` < `"a.rs"`). We need byte-wise
        // comparison of the full path string so the .feature data
        // table's `a.rs` before `a/b.rs` ordering holds (`.` byte 0x2E
        // < `/` byte 0x2F).
        files.sort_by(|a, b| a.as_path().as_os_str().cmp(b.as_path().as_os_str()));

        Ok(DiscoveryOutcome::new(files, diagnostics))
    }
}

/// Classify a non-fatal mid-walk `ignore::Error` into a
/// [`SourceDiagnostic`]. Wrapper variants (`WithPath`, `WithDepth`,
/// `WithLineNumber`) are peeled to find the underlying classification;
/// `WithPath` also supplies path attribution. Errors that lack a
/// `WithPath` wrapper fall back to `fallback_root` (typically the walk
/// root). Branch coverage is unit-tested below against
/// hand-constructed `ignore::Error` values.
fn classify_walk_error(err: &ignore::Error, fallback_root: &Path) -> SourceDiagnostic {
    let attributed = walk_error_attributed_path(err).unwrap_or(fallback_root);
    let kind = walk_error_kind(err);
    SourceDiagnostic::new(FilePath::new(attributed), kind, err.to_string())
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

    fn rel_paths(outcome: &DiscoveryOutcome, root: &Path) -> Vec<String> {
        outcome
            .files
            .iter()
            .map(|fp| {
                fp.as_path()
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    // ─── try_new tests (V4) ──────────────────────────────────────────

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

    // ─── Pre-flight failure tests ───────────────────────────────────

    #[test]
    fn missing_root_returns_io_error() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does/not/exist");
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files(&SourceRoot::new(&missing));
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
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker.discover_test_files(&SourceRoot::new(&file));
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
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        assert!(outcome.files.is_empty(), "{:?}", outcome.files);
        assert!(outcome.diagnostics.is_empty(), "{:?}", outcome.diagnostics);
    }

    #[test]
    fn single_file_is_returned() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "a.rs");
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        assert_eq!(rel_paths(&outcome, tmp.path()), vec!["a.rs".to_string()]);
    }

    #[test]
    fn nested_tree_returns_all_matching_files_in_deterministic_order() {
        let tmp = tempfile::tempdir().unwrap();
        for rel in ["a.rs", "a/b.rs", "a/sub/c.rs", "a/sub/d.rs", "b.rs"] {
            touch(tmp.path(), rel);
        }
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome_a = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        let outcome_b = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        let expected = vec![
            "a.rs".to_string(),
            "a/b.rs".to_string(),
            "a/sub/c.rs".to_string(),
            "a/sub/d.rs".to_string(),
            "b.rs".to_string(),
        ];
        assert_eq!(rel_paths(&outcome_a, tmp.path()), expected);
        assert_eq!(rel_paths(&outcome_b, tmp.path()), expected);
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
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        let mut paths = rel_paths(&outcome, tmp.path());
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
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        let mut paths = rel_paths(&outcome, tmp.path());
        paths.sort();
        assert_eq!(
            paths,
            vec!["a.rs".to_string(), "b.txt".to_string(), "c.md".to_string()]
        );
    }

    #[test]
    fn hidden_files_are_skipped_by_default() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "visible.rs");
        touch(tmp.path(), ".hidden.rs");
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], true);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        assert_eq!(
            rel_paths(&outcome, tmp.path()),
            vec!["visible.rs".to_string()]
        );
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
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        assert_eq!(rel_paths(&outcome, tmp.path()), vec!["keep.rs".to_string()]);
    }

    #[test]
    fn respect_gitignore_false_includes_listed_files() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "keep.rs");
        touch(tmp.path(), "skip.rs");
        fs::write(tmp.path().join(".gitignore"), "skip.rs\n").unwrap();
        let cfg = cfg_for(tmp.path(), vec![], vec!["rs".into()], false);
        let walker = FsWalker::try_new(cfg).unwrap();
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        let mut paths = rel_paths(&outcome, tmp.path());
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
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();
        assert_eq!(rel_paths(&outcome, tmp.path()), vec!["keep.rs".to_string()]);
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
        let outcome = walker
            .discover_test_files(&SourceRoot::new(tmp.path()))
            .unwrap();

        assert_eq!(
            rel_paths(&outcome, tmp.path()),
            vec!["accessible/a.rs".to_string()],
        );
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
        assert_eq!(diag.path, FilePath::new("/tmp/fallback"));
    }

    #[test]
    fn classify_partial_returns_other() {
        let err = ignore::Error::Partial(vec![]);
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
        assert_eq!(diag.path, FilePath::new("/tmp/fallback"));
    }
}
