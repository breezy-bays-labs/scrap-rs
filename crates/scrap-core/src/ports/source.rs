//! `SourcePort` — language-agnostic test-file discovery.
//!
//! Two adapter implementations live in `crate::adapters::source`:
//!
//! - [`crate::adapters::source::fs::FsWalker`] — disk-backed walker
//!   built on `ignore::WalkBuilder` + `globset`. Honours
//!   `.gitignore` / `.ignore` / `.git/info/exclude` per
//!   [`crate::domain::config::AnalysisConfig::respect_gitignore`];
//!   user-supplied negative globs compile through `OverrideBuilder`
//!   at construction time.
//! - [`crate::adapters::source::memory::MemorySource`] — test-only
//!   adapter that returns a fixed `(files, diagnostics)` pair without
//!   touching disk.
//!
//! Object-safe (`&self`); usable as `Box<dyn SourcePort>`. No
//! `Send + Sync` bound on the trait itself — those add at the
//! `core::analyze<S, P>` call site if/when rayon parallelism arrives
//! (per [`adr-port-surface-and-domain-conventions`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-port-surface-and-domain-conventions.md)
//! D11). Both shipped adapters happen to be `Send + Sync` as an
//! emergent property; smoke tests in `tests/source_walker.rs` pin both
//! the deliberate-absence at the trait level and the emergent presence
//! at the adapter level.

use crate::domain::source::DiscoveryOutcome;
use crate::domain::types::{FilePath, SourceRoot};

/// Discover the candidate test files under a `SourceRoot`.
///
/// Implementations enumerate the workspace and return a
/// [`DiscoveryOutcome`] that bundles the matching files with any
/// non-fatal mid-walk diagnostics (permission-denied subdirectories,
/// recoverable I/O failures the walker skipped past). I/O failures
/// that abort the walk surface as `Err(SourceError::Io)` instead.
///
/// Filtering by include/exclude globs and respect for VCS ignore files
/// (`.gitignore`, etc.) lives in the adapter — the trait surface is
/// intentionally minimal.
pub trait SourcePort {
    /// Walk `root` and return the test-file candidates plus any
    /// non-fatal diagnostics the walker collected.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] when the filesystem walk fails fatally
    /// (missing root, root-is-file, mid-walk I/O the adapter chose not
    /// to skip past) or a configured glob is invalid.
    fn discover_test_files(&self, root: &SourceRoot) -> Result<DiscoveryOutcome, SourceError>;
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
    /// `OverrideBuilder::build()` rejected the assembled override
    /// matcher despite each individual `.add()` call having succeeded.
    /// **NOT** fired by `WalkBuilder::build()` (which is infallible,
    /// returning `Walk` directly). This variant is a forward-compat
    /// hatch: it is exceedingly rare in practice but keeps the surface
    /// honest if `ignore`'s `OverrideBuilder` grows new validation in a
    /// minor bump.
    #[error("ignore override builder rejected the assembled matcher")]
    Ignore {
        /// Underlying `ignore` crate error from `OverrideBuilder::build`.
        #[source]
        source: ignore::Error,
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

    #[test]
    fn ignore_variant_displays_message_and_preserves_source() {
        // `ignore::Error` is a public enum; constructing the `Glob`
        // variant directly keeps this smoke independent of the
        // `OverrideBuilder.build()` failure mode (which is rare and
        // not stably reproducible across `ignore` minor versions).
        let ignore_err = ignore::Error::Glob {
            glob: Some("[unclosed".into()),
            err: "missing ']'".into(),
        };
        let err = SourceError::Ignore { source: ignore_err };
        assert!(
            err.to_string().contains("override"),
            "Display should mention `override`; got: {err}",
        );
        assert!(err.source().is_some());
    }
}
