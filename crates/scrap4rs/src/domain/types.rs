//! Source-coordinate primitives shared by every domain type.
//!
//! These newtypes carry no I/O or AST dependency — they extract cleanly
//! into `scrap-core` at v1.0 without rename.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Error returned by `Span::try_new` when the supplied range is
/// inverted (`start_line > end_line`).
///
/// Adapters that source spans from `syn::spanned::Spanned` always
/// produce well-ordered ranges and use `Span::new`. Adapters that
/// reconstruct spans from external sources (LSP positions, diff hunks,
/// baseline-diff replay) should prefer `try_new` so an inverted range
/// becomes a typed precondition violation, not a silent semantic bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InvertedSpan {
    /// The supplied start line.
    pub start_line: u32,
    /// The supplied end line.
    pub end_line: u32,
}

impl std::fmt::Display for InvertedSpan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "inverted span: start_line {} > end_line {}",
            self.start_line, self.end_line
        )
    }
}

impl std::error::Error for InvertedSpan {}

/// 1-based inclusive line range into a source file.
///
/// Columns are intentionally omitted for v0.1 — every detector operates
/// on whole-test-body granularity. SARIF reporters that need columns can
/// extend the wire envelope additively at v0.2 without breaking
/// `schema_version: 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// 1-based inclusive line where the span begins.
    pub start_line: u32,
    /// 1-based inclusive line where the span ends.
    pub end_line: u32,
}

impl Span {
    /// Construct a span over the inclusive line range `[start_line,
    /// end_line]`. Caller is responsible for `start_line <= end_line`;
    /// detectors emit spans pulled directly from `syn::spanned::Spanned`,
    /// which always produces well-ordered ranges. A `debug_assert!`
    /// catches inverted ranges in dev/test builds; release builds rely
    /// on the `line_count` saturating arithmetic.
    pub fn new(start_line: u32, end_line: u32) -> Self {
        debug_assert!(
            start_line <= end_line,
            "Span::new: inverted range {start_line}..{end_line} (use Span::try_new for fallible construction)",
        );
        Self {
            start_line,
            end_line,
        }
    }

    /// Fallible constructor: returns `Err(InvertedSpan)` when
    /// `start_line > end_line`. Prefer this over `Span::new` when the
    /// caller cannot guarantee well-ordered input (e.g., reconstructing
    /// spans from external LSP positions or baseline-diff replay).
    pub fn try_new(start_line: u32, end_line: u32) -> Result<Self, InvertedSpan> {
        if start_line > end_line {
            Err(InvertedSpan {
                start_line,
                end_line,
            })
        } else {
            Ok(Self {
                start_line,
                end_line,
            })
        }
    }

    /// Number of source lines covered, inclusive on both ends. For a
    /// well-formed span this is `end_line - start_line + 1`. For an
    /// inverted span (which `try_new` rejects but `new` permits in
    /// release builds), the saturating arithmetic returns `1` — a
    /// defensive value that prevents integer underflow but is
    /// semantically meaningless. Callers that need to distinguish
    /// "1-line span" from "inverted span" should use `try_new`.
    pub fn line_count(&self) -> u32 {
        self.end_line
            .saturating_sub(self.start_line)
            .saturating_add(1)
    }
}

/// Filesystem path of a Rust source file, relative to the analyzed root.
///
/// Newtype rather than bare `PathBuf` so adapters can't accidentally pass
/// an unrelated path through the domain layer; every Location carries a
/// `FilePath` constructed at the source-discovery boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FilePath(PathBuf);

impl FilePath {
    /// Wrap any `PathBuf`-convertible value as a `FilePath`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Borrow the wrapped path. Use when interoperating with std I/O
    /// or path-manipulation routines at the adapter boundary.
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

/// Fully qualified Rust path of a test function (e.g.
/// `foo::bar::tests::it_does_a_thing`).
///
/// Newtype around `String` so reporters and aggregators can match on
/// `QualifiedName` without trafficking arbitrary strings through the
/// domain. The wire format stays a flat string forever — when scrap-core
/// extracts at v1.0 and gains a TS adapter, segment-aware constructors
/// can join with `::` (Rust) or `/` (TS file paths) at the adapter
/// boundary without changing this newtype's wire shape.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QualifiedName(String);

impl QualifiedName {
    /// Wrap any string-convertible value as a `QualifiedName`. The
    /// caller is responsible for choosing a separator consistent with
    /// the source language (`::` for Rust paths).
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// Borrow the wrapped string. Use when formatting reports or
    /// interoperating with string-based APIs at the adapter boundary.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Identity of a single example (Rust `#[test]` fn) — file + qualified
/// name + span. Each `Finding` carries exactly one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TestIdentity {
    /// Source file containing the test.
    pub file_path: FilePath,
    /// Fully qualified Rust path of the test function.
    pub qualified_name: QualifiedName,
    /// Inclusive line range covered by the test body.
    pub span: Span,
}

impl TestIdentity {
    /// Construct a `TestIdentity` from its three coordinates. Adapters
    /// at the test-discovery boundary call this once per discovered
    /// `#[test]` fn.
    pub fn new(file_path: FilePath, qualified_name: QualifiedName, span: Span) -> Self {
        Self {
            file_path,
            qualified_name,
            span,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn span_line_count_basic() {
        assert_eq!(Span::new(1, 1).line_count(), 1);
        assert_eq!(Span::new(10, 19).line_count(), 10);
    }

    #[test]
    fn span_serializes_snake_case() {
        let span = Span::new(42, 51);
        let json = serde_json::to_value(span).unwrap();
        assert_eq!(json["start_line"], 42);
        assert_eq!(json["end_line"], 51);
    }

    #[test]
    fn span_try_new_rejects_inverted_range() {
        let err = Span::try_new(10, 5).unwrap_err();
        assert_eq!(err.start_line, 10);
        assert_eq!(err.end_line, 5);
        assert_eq!(err.to_string(), "inverted span: start_line 10 > end_line 5");
    }

    #[test]
    fn span_try_new_accepts_equal_lines() {
        let span = Span::try_new(7, 7).unwrap();
        assert_eq!(span.line_count(), 1);
    }

    #[test]
    fn file_path_and_qualified_name_are_transparent() {
        let fp = FilePath::new("crates/foo/src/bar.rs");
        let qn = QualifiedName::new("foo::bar::tests::it");
        assert_eq!(
            serde_json::to_value(&fp).unwrap(),
            serde_json::Value::String("crates/foo/src/bar.rs".into()),
        );
        assert_eq!(
            serde_json::to_value(&qn).unwrap(),
            serde_json::Value::String("foo::bar::tests::it".into()),
        );
    }

    proptest! {
        #[test]
        fn span_line_count_is_end_minus_start_plus_one(
            start in 1u32..1_000_000,
            len in 0u32..10_000,
        ) {
            let end = start + len;
            prop_assert_eq!(Span::new(start, end).line_count(), len + 1);
        }

        #[test]
        fn span_try_new_rejects_strictly_inverted(
            end in 0u32..1_000_000,
            len in 1u32..10_000,
        ) {
            let start = end + len;
            // start > end: must reject.
            prop_assert!(Span::try_new(start, end).is_err());
        }

        #[test]
        fn span_try_new_accepts_well_ordered(
            start in 1u32..1_000_000,
            len in 0u32..10_000,
        ) {
            let end = start + len;
            let span = Span::try_new(start, end).unwrap();
            prop_assert_eq!(span.line_count(), len + 1);
        }
    }
}
