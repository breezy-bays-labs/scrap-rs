//! `MemorySource` — in-memory `SourcePort` impl for unit and integration tests.

use crate::domain::source::{DiscoveryOutcome, SourceDiagnostic};
use crate::domain::types::FilePath;
use crate::ports::source::{SourceError, SourcePort};

/// In-memory `SourcePort` implementation for unit and integration tests.
///
/// Returns a fixed `(files, diagnostics)` pair without touching disk.
/// Useful when a test needs to exercise downstream code paths
/// (`core::analyze`, reporters, detectors) without depending on the
/// filesystem; pair with [`crate::adapters::source::fs::FsWalker`] +
/// `tempfile` when you need real on-disk discovery.
///
/// Constructors:
/// - [`MemorySource::new`] — canonical (D10): files + diagnostics.
/// - [`MemorySource::with_files`] — convenience for the
///   diagnostics-empty case.
#[derive(Debug, Clone)]
pub struct MemorySource {
    files: Vec<FilePath>,
    diagnostics: Vec<SourceDiagnostic>,
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

    /// Borrow the configured files. Useful when a test needs to read
    /// back what it stashed; production code should call
    /// [`SourcePort::discover_test_files`].
    #[must_use]
    pub fn files(&self) -> &[FilePath] {
        &self.files
    }

    /// Borrow the configured diagnostics. Useful when a test needs to
    /// read back what it stashed; production code should call
    /// [`SourcePort::discover_test_files`].
    #[must_use]
    pub fn diagnostics(&self) -> &[SourceDiagnostic] {
        &self.diagnostics
    }
}

impl SourcePort for MemorySource {
    fn discover_test_files(&self) -> Result<DiscoveryOutcome, SourceError> {
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
        assert_eq!(src.files(), &[FilePath::new("a.rs")]);
        assert!(src.diagnostics().is_empty());
    }

    #[test]
    fn discover_test_files_returns_configured_files() {
        let files = vec![FilePath::new("x.rs"), FilePath::new("y.rs")];
        let src = MemorySource::with_files(files.clone());
        let outcome = src.discover_test_files().unwrap();
        assert_eq!(outcome.files, files);
        assert!(outcome.diagnostics.is_empty());
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
        let outcome = src.discover_test_files().unwrap();
        assert_eq!(outcome.files, files);
        assert_eq!(outcome.diagnostics, diagnostics);
    }
}
