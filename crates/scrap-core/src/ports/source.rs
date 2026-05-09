//! `SourcePort` — language-agnostic test-file discovery.
//!
//! The default implementation is the `FsWalker` adapter (lands in a
//! dedicated sub-issue), backed by `walkdir` + `ignore` for tree
//! traversal and `globset` for include/exclude pattern matching. A
//! future `MemorySource` adapter for tests will return a fixed file
//! list without touching disk.
//!
//! Object-safe (`&self`); usable as `Box<dyn SourcePort>`. No
//! `Send + Sync` bound on the trait itself — those add at the
//! `core::analyze<S, P>` call site if/when rayon parallelism arrives.

use crate::domain::types::{FilePath, SourceRoot};

/// Discover the candidate test files under a `SourceRoot`.
///
/// Implementations enumerate the workspace and return absolute or
/// root-relative `FilePath`s for every file the parser should attempt.
/// Filtering by include/exclude globs and respect for VCS ignore files
/// (`.gitignore`, etc.) lives in the adapter — the trait surface is
/// intentionally minimal.
pub trait SourcePort {
    /// Walk `root` and return the test-file candidates.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] when the filesystem walk fails or a
    /// configured glob is invalid.
    fn discover_test_files(&self, root: &SourceRoot) -> Result<Vec<FilePath>, SourceError>;
}

/// Errors produced by [`SourcePort`] implementations.
///
/// `#[non_exhaustive]` — adapters add variants (e.g. permission denied,
/// loop detection) without breaking callers that pattern-match.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// I/O failure during the walk, attributed to the path the adapter
    /// was visiting when the error fired.
    #[error("io error at {path}")]
    Io {
        /// Path the walk was visiting when the error fired.
        path: FilePath,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// A configured include/exclude glob failed to compile. `pattern`
    /// is the raw user-supplied pattern; `source` is the `globset`
    /// compile error.
    #[error("invalid glob pattern: {pattern}")]
    InvalidGlob {
        /// Raw glob pattern that failed to compile.
        pattern: String,
        /// Underlying globset parse error.
        #[source]
        source: globset::Error,
    },
}

// Compile-time invariants on the port trait: object-safe (so
// `Box<dyn SourcePort>` works), and *deliberately* not `Send + Sync`
// (parallelism bounds belong at the `core::analyze<S, P>` call site).
#[cfg(test)]
static_assertions::assert_obj_safe!(SourcePort);
#[cfg(test)]
static_assertions::assert_not_impl_any!(dyn SourcePort: Send, Sync);

#[cfg(test)]
mod error_smoke {
    use super::*;
    use std::error::Error;

    #[test]
    fn io_error_displays_path_and_preserves_source() {
        let err = SourceError::Io {
            path: FilePath::new("a/b.rs"),
            source: std::io::Error::other("boom"),
        };
        assert_eq!(err.to_string(), "io error at a/b.rs");
        assert!(err.source().is_some());
    }

    #[test]
    fn invalid_glob_displays_pattern_and_preserves_source() {
        let glob_err = globset::Glob::new("[unclosed").unwrap_err();
        let err = SourceError::InvalidGlob {
            pattern: "[unclosed".into(),
            source: glob_err,
        };
        assert!(err.to_string().contains("[unclosed"));
        assert!(err.source().is_some());
    }
}
