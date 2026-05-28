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

/// 1-based inclusive line + column range into a source file.
///
/// All four coordinates are required `u32`, 1-based. `start_line` /
/// `end_line` bound the inclusive line range; `start_column` /
/// `end_column` bound the inclusive column range (1-based, never 0 —
/// adapters convert from any 0-based source coordinate at the parser
/// boundary). Columns landed additively with the SARIF reporter
/// (scrap-rs#17) and do NOT bump the wire envelope's `schema_version`
/// per [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md)
/// D2 additive-field rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Span {
    /// 1-based inclusive line where the span begins.
    pub start_line: u32,
    /// 1-based inclusive line where the span ends.
    pub end_line: u32,
    /// 1-based inclusive column where the span begins.
    pub start_column: u32,
    /// 1-based inclusive column where the span ends.
    pub end_column: u32,
}

impl Span {
    /// Construct a span over the inclusive line range `[start_line,
    /// end_line]` and column range `[start_column, end_column]`. Caller
    /// is responsible for well-ordered input; detectors emit spans
    /// pulled directly from `syn::spanned::Spanned`, which always
    /// produces well-ordered ranges. A `debug_assert!` catches inverted
    /// ranges in dev/test builds; release builds rely on the
    /// `line_count` saturating arithmetic.
    ///
    /// The ordering invariant is line-primary, column-secondary: a span
    /// is well-ordered when `start_line < end_line`, or when
    /// `start_line == end_line` and `start_column <= end_column`. The
    /// `debug_assert!` permits a same-line span whose columns are equal
    /// (a zero-width point) and rejects only a same-line span whose end
    /// column precedes its start column.
    #[must_use]
    pub fn new(start_line: u32, end_line: u32, start_column: u32, end_column: u32) -> Self {
        debug_assert!(
            start_line < end_line || (start_line == end_line && start_column <= end_column),
            "Span::new: inverted range {start_line}:{start_column}..{end_line}:{end_column} (use Span::try_new for fallible construction)",
        );
        Self {
            start_line,
            end_line,
            start_column,
            end_column,
        }
    }

    /// Fallible constructor: returns `Err(InvertedSpan)` when
    /// `start_line > end_line`. Prefer this over `Span::new` when the
    /// caller cannot guarantee well-ordered input (e.g., reconstructing
    /// spans from external LSP positions or baseline-diff replay).
    ///
    /// Per scrap-rs#17 D3, the fallible check is line-only: column
    /// ordering is NOT validated here (a same-line span with inverted
    /// columns is a pathological synthetic-span artifact, not a
    /// reconstructed-from-external-source condition `try_new` guards
    /// against). Columns are stored verbatim. The line-primary
    /// `debug_assert!` in [`Span::new`] is the dev/test-build column
    /// guard.
    ///
    /// # Errors
    ///
    /// Returns `Err(InvertedSpan)` if `start_line > end_line`.
    pub fn try_new(
        start_line: u32,
        end_line: u32,
        start_column: u32,
        end_column: u32,
    ) -> Result<Self, InvertedSpan> {
        if start_line > end_line {
            Err(InvertedSpan {
                start_line,
                end_line,
            })
        } else {
            Ok(Self {
                start_line,
                end_line,
                start_column,
                end_column,
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
/// **Documented exception**: when an `FsWalker` mid-walk diagnostic
/// attributes itself to a path outside the walked tree (e.g. a
/// `~/.config/git/ignore` parse failure when `git_global(true)` is
/// honouring the global gitignore), the diagnostic carries the raw
/// absolute path. The relative-shape convention applies to the
/// happy-path entries inside [`crate::domain::source::DiscoveryOutcome::files`];
/// diagnostic attribution prefers fidelity over relativisation.
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
        assert_eq!(Span::new(1, 1, 1, 1).line_count(), 1);
        assert_eq!(Span::new(10, 19, 1, 1).line_count(), 10);
    }

    #[test]
    fn span_serializes_snake_case() {
        let span = Span::new(42, 51, 5, 12);
        let json = serde_json::to_value(span).unwrap();
        assert_eq!(json["start_line"], 42);
        assert_eq!(json["end_line"], 51);
        assert_eq!(json["start_column"], 5);
        assert_eq!(json["end_column"], 12);
    }

    #[test]
    fn span_new_carries_columns() {
        let span = Span::new(3, 7, 4, 9);
        assert_eq!(span.start_column, 4);
        assert_eq!(span.end_column, 9);
    }

    #[test]
    fn span_new_same_line_equal_columns_is_zero_width_point() {
        // A zero-width point (same line, same column) is well-ordered —
        // the debug_assert permits it (no panic).
        let span = Span::new(5, 5, 7, 7);
        assert_eq!(span.start_column, span.end_column);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "inverted range")]
    fn span_new_same_line_inverted_columns_panics_in_debug() {
        // Same line, end_column < start_column: rejected by the
        // line-primary column-secondary debug_assert.
        let _ = Span::new(5, 5, 12, 4);
    }

    #[test]
    fn span_try_new_rejects_inverted_range() {
        let err = Span::try_new(10, 5, 1, 1).unwrap_err();
        assert_eq!(err.start_line, 10);
        assert_eq!(err.end_line, 5);
        assert_eq!(err.to_string(), "inverted span: start_line 10 > end_line 5");
    }

    #[test]
    fn span_try_new_accepts_equal_lines() {
        let span = Span::try_new(7, 7, 1, 8).unwrap();
        assert_eq!(span.line_count(), 1);
        assert_eq!(span.start_column, 1);
        assert_eq!(span.end_column, 8);
    }

    #[test]
    fn span_try_new_does_not_validate_column_order() {
        // D3: try_new is line-only — a same-line span with inverted
        // columns is accepted and stored verbatim (not an error).
        let span = Span::try_new(4, 4, 20, 3).unwrap();
        assert_eq!(span.start_column, 20);
        assert_eq!(span.end_column, 3);
    }

    #[test]
    fn span_round_trips_columns_through_json() {
        let span = Span::new(2, 9, 6, 14);
        let json = serde_json::to_value(span).unwrap();
        let back: Span = serde_json::from_value(json).unwrap();
        assert_eq!(back, span);
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
            start_column in 1u32..1_000,
            col_len in 0u32..1_000,
        ) {
            let end = start + len;
            let end_column = start_column + col_len;
            prop_assert_eq!(Span::new(start, end, start_column, end_column).line_count(), len + 1);
        }

        #[test]
        fn span_try_new_rejects_strictly_inverted(
            end in 0u32..1_000_000,
            len in 1u32..10_000,
        ) {
            let start = end + len;
            // start > end: must reject (line-only check).
            prop_assert!(Span::try_new(start, end, 1, 1).is_err());
        }

        #[test]
        fn span_try_new_accepts_well_ordered(
            start in 1u32..1_000_000,
            len in 0u32..10_000,
            start_column in 1u32..1_000,
            col_len in 0u32..1_000,
        ) {
            let end = start + len;
            let end_column = start_column + col_len;
            let span = Span::try_new(start, end, start_column, end_column).unwrap();
            prop_assert_eq!(span.line_count(), len + 1);
            prop_assert_eq!(span.start_column, start_column);
            prop_assert_eq!(span.end_column, end_column);
        }

        #[test]
        fn span_columns_round_trip_through_json(
            start in 1u32..1_000_000,
            len in 0u32..10_000,
            start_column in 1u32..1_000,
            col_len in 0u32..1_000,
        ) {
            let end = start + len;
            let end_column = start_column + col_len;
            let span = Span::new(start, end, start_column, end_column);
            let json = serde_json::to_value(span).unwrap();
            let back: Span = serde_json::from_value(json).unwrap();
            prop_assert_eq!(back, span);
        }
    }
}
