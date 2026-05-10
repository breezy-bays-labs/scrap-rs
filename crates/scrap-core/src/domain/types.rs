//! Source-coordinate primitives shared by every domain type.
//!
//! These newtypes carry no I/O or AST dependency, so every adapter
//! binary in the workspace can produce them from its own parser.

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
    #[must_use]
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
    ///
    /// # Errors
    ///
    /// Returns `Err(InvertedSpan)` if `start_line > end_line`.
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
    #[must_use]
    pub fn line_count(&self) -> u32 {
        self.end_line
            .saturating_sub(self.start_line)
            .saturating_add(1)
    }
}

/// Filesystem path of a source file. By adapter convention the
/// concrete shape is **relative to the source root** when emitted by
/// [`crate::adapters::source::fs::FsWalker`]; in-memory adapters
/// ([`crate::adapters::source::memory::MemorySource`]) emit the paths
/// the test author supplies verbatim.
///
/// Newtype rather than bare `PathBuf` so adapters can't accidentally pass
/// an unrelated path through the domain layer; every Location carries a
/// `FilePath` constructed at the source-discovery boundary.
///
/// Deliberately does NOT derive `PartialOrd` / `Ord`. The natural
/// `PathBuf` ordering is component-wise (`a/b.rs` precedes `a.rs`
/// because the first component compares `"a" < "a.rs"`), which clashes
/// with the byte-wise ordering [`crate::adapters::source::fs::FsWalker`]
/// uses for its post-collect sort. If a future call site needs ordering,
/// it must choose explicitly between component-wise (sort the inner
/// `PathBuf`) and byte-wise (sort on `as_path().as_os_str()`) and
/// document the choice.
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
    #[must_use]
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl std::fmt::Display for FilePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(f)
    }
}

/// Fully qualified Rust path of a test function (e.g.
/// `foo::bar::tests::it_does_a_thing`).
///
/// Newtype around `String` so reporters and aggregators can match on
/// `QualifiedName` without trafficking arbitrary strings through the
/// domain. The wire format stays a flat string forever — adapter
/// crates can join with `::` (Rust paths in `scrap4rs`) or `.`/`/`
/// (TS paths in `scrap4ts`) at the parser boundary without changing
/// this newtype's wire shape.
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
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for QualifiedName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
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
    #[must_use]
    pub fn new(file_path: FilePath, qualified_name: QualifiedName, span: Span) -> Self {
        Self {
            file_path,
            qualified_name,
            span,
        }
    }
}

/// Root directory under which test discovery operates.
///
/// Type-level boundary marker (no path validation): constructed at the
/// CLI/test boundary so `core::analyze()` cannot accidentally accept a
/// raw `&Path` from a fixture or argument vector. Carried by reference
/// into `SourcePort::discover_test_files` and never decomposed inside
/// `core/` or `ports/`. The adapter is responsible for surfacing
/// non-existent or non-directory paths as `SourceError::Io`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceRoot(PathBuf);

impl SourceRoot {
    /// Wrap any `PathBuf`-convertible value as a `SourceRoot`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Borrow the wrapped path. Use when interoperating with std I/O
    /// at the source-discovery adapter boundary.
    #[must_use]
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

impl std::fmt::Display for SourceRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.display().fmt(f)
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
    fn source_root_is_transparent() {
        let root = SourceRoot::new("crates/scrap4rs");
        assert_eq!(
            serde_json::to_value(&root).unwrap(),
            serde_json::Value::String("crates/scrap4rs".into()),
        );
    }

    #[test]
    fn source_root_as_path_round_trips() {
        let root = SourceRoot::new("a/b/c");
        assert_eq!(root.as_path(), std::path::Path::new("a/b/c"));
    }

    #[test]
    fn newtype_display_unwraps_inner() {
        assert_eq!(FilePath::new("a/b/c.rs").to_string(), "a/b/c.rs");
        assert_eq!(
            SourceRoot::new("crates/scrap4rs").to_string(),
            "crates/scrap4rs"
        );
        assert_eq!(QualifiedName::new("foo::bar").to_string(), "foo::bar");
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
