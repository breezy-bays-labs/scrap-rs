//! `TestParserPort` — language-specific source-to-`ParsedTestFile`.
//!
//! Implemented per source language:
//!   - `scrap4rs::SynParser` — Rust adapter via `syn` (lands with
//!     scrap-rs#6).
//!   - `scrap4ts::SwcParser` — TypeScript adapter joining at v0.6+
//!     (parser-library choice between `swc_ecma_parser` and
//!     `oxc_parser` is open as of 2026-05).
//!
//! **IO boundary**: core opens the file; the adapter parses bytes
//! (`&str`). Cleaner test ergonomics (no disk fixtures) and rayon-ready
//! parallelism control from `core::analyze<S, P>`. The wrapper that
//! reads the file owns its own error type (`AnalyzeFileError`, lands
//! with `core::analyze_file()` in a dedicated sub-issue); [`ParseError`]
//! here is adapter-only.
//!
//! **Semantic Facts pattern**: the adapter pre-computes typed
//! classification flags onto `ParsedAssertion` / `ParsedTest`; detectors
//! read those flags and emit `Finding`s. AST shape never crosses the
//! port boundary.
//!
//! **`Result::Err` vs `ParseDiagnostic` contract**: `Err(ParseError::Syntax)`
//! means "the file's complete test inventory is unknowable" — the adapter
//! could not produce a usable projection. Partial-recovery observations
//! that did NOT cause any test to be omitted from `tests` belong in
//! [`ParsedTestFile::diagnostics`], not as `Err`. An adapter that returns
//! `Ok(file)` MUST NOT silently truncate `file.tests`.
//!
//! Object-safe (`&self`); usable as `Box<dyn TestParserPort>`. No
//! `Send + Sync` bound on the trait itself — those add at the
//! `core::analyze<S, P>` call site if/when rayon parallelism arrives.

use crate::domain::parsed::ParsedTestFile;
use crate::domain::types::{FilePath, Span};

/// Parse a single test source file into language-agnostic facts.
pub trait TestParserPort {
    /// Parse `source` (the file's bytes) into a [`ParsedTestFile`].
    ///
    /// `path` accompanies `source` for `FilePath` population on the
    /// result; the implementation does not read it from disk.
    ///
    /// # Errors
    ///
    /// Returns [`ParseError::Syntax`] when the parser cannot recover a
    /// usable projection. Partial-recovery observations belong on
    /// [`ParsedTestFile::diagnostics`]; see the module-level
    /// `Result::Err vs ParseDiagnostic contract`. I/O failures are not
    /// part of this trait's surface.
    fn parse_test_source(
        &self,
        source: &str,
        path: &FilePath,
    ) -> Result<ParsedTestFile, ParseError>;
}

/// Errors produced by [`TestParserPort`] implementations.
///
/// `#[non_exhaustive]` — language adapters add variants (e.g. TS module
/// resolution failure) without breaking pattern-match callers. I/O
/// failures are not part of this type's surface — they are owned by the
/// `core::analyze_file()` wrapper (forthcoming, see module docs).
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Adapter could not parse the source. `message` is human-readable;
    /// `span` localizes when the adapter recovers a position. The caller
    /// of `parse_test_source` annotates this with the path it passed.
    #[error("syntax error: {message}")]
    Syntax {
        /// Human-readable detail.
        message: String,
        /// Position of the syntax error, when recoverable. `None` means
        /// the syntax error is not localizable to a line range
        /// (whole-file failure typical for `syn`); adapters that
        /// genuinely cannot recover a position MUST note this in
        /// `message`.
        span: Option<Span>,
    },
}

// Compile-time invariants on the port trait: object-safe (so
// `Box<dyn TestParserPort>` works), and *deliberately* not `Send + Sync`
// (parallelism bounds belong at the `core::analyze<S, P>` call site).
#[cfg(test)]
static_assertions::assert_obj_safe!(TestParserPort);
#[cfg(test)]
static_assertions::assert_not_impl_any!(dyn TestParserPort: Send, Sync);

#[cfg(test)]
mod error_smoke {
    use super::*;
    use std::error::Error;

    #[test]
    fn syntax_error_displays_message() {
        let err = ParseError::Syntax {
            message: "unexpected token".into(),
            span: Some(Span::new(3, 3, 1, 1)),
        };
        assert_eq!(err.to_string(), "syntax error: unexpected token");
        // No #[source] on Syntax — no underlying error to chain.
        assert!(err.source().is_none());
    }
}
