//! `MemorySource` — in-memory `SourcePort` impl for unit and integration tests.

use crate::domain::source::{DiscoveryOutcome, SourceDiagnostic};
use crate::domain::types::{FilePath, SourceRoot};
use crate::ports::source::{SourceError, SourcePort};

/// In-memory `SourcePort` implementation for unit and integration tests.
///
/// **This adapter ignores the `root` parameter passed to
/// [`SourcePort::discover_test_files`]; the configured files are
/// returned regardless of which root is requested.** If your test needs
/// root-sensitive behavior, construct a separate `MemorySource` per
/// root, or use [`crate::adapters::source::fs::FsWalker`] with
/// `tempfile`.
///
/// Constructors:
/// - [`MemorySource::new`] — canonical (D10): files + diagnostics.
/// - [`MemorySource::with_files`] — convenience for the
///   diagnostics-empty case.
#[derive(Debug, Clone)]
pub struct MemorySource {
    /// Files returned from every `discover_test_files` call.
    pub files: Vec<FilePath>,
    /// Diagnostics surfaced through every `DiscoveryOutcome`.
    pub diagnostics: Vec<SourceDiagnostic>,
}

impl MemorySource {
    /// Canonical constructor (D10).
    #[must_use]
    pub fn new(files: Vec<FilePath>, diagnostics: Vec<SourceDiagnostic>) -> Self {
        Self { files, diagnostics }
    }

    /// Convenience constructor for the diagnostics-empty case (the
    /// 90% test fixture path).
    #[must_use]
    pub fn with_files(files: Vec<FilePath>) -> Self {
        Self::new(files, Vec::new())
    }
}

impl SourcePort for MemorySource {
    fn discover_test_files(&self, _root: &SourceRoot) -> Result<DiscoveryOutcome, SourceError> {
        Ok(DiscoveryOutcome::new(
            self.files.clone(),
            self.diagnostics.clone(),
        ))
    }
}

#[cfg(test)]
static_assertions::assert_impl_all!(MemorySource: Send, Sync);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::source::SourceDiagnosticKind;

    #[test]
    fn with_files_constructs_with_empty_diagnostics() {
        let src = MemorySource::with_files(vec![FilePath::new("a.rs")]);
        assert_eq!(src.files, vec![FilePath::new("a.rs")]);
        assert!(src.diagnostics.is_empty());
    }

    #[test]
    fn discover_test_files_returns_configured_files_regardless_of_root() {
        let files = vec![FilePath::new("x.rs"), FilePath::new("y.rs")];
        let src = MemorySource::with_files(files.clone());
        let outcome_a = src
            .discover_test_files(&SourceRoot::new("/some/root"))
            .unwrap();
        let outcome_b = src
            .discover_test_files(&SourceRoot::new("/totally/different"))
            .unwrap();
        assert_eq!(outcome_a, outcome_b);
        assert_eq!(outcome_a.files, files);
        assert!(outcome_a.diagnostics.is_empty());
    }

    #[test]
    fn new_constructor_carries_diagnostics_through() {
        let files = vec![FilePath::new("a.rs")];
        let diagnostics = vec![SourceDiagnostic::new(
            FilePath::new("denied"),
            SourceDiagnosticKind::PermissionDenied,
            "could not read entry",
        )];
        let src = MemorySource::new(files.clone(), diagnostics.clone());
        let outcome = src.discover_test_files(&SourceRoot::new("any")).unwrap();
        assert_eq!(outcome.files, files);
        assert_eq!(outcome.diagnostics, diagnostics);
    }
}
