//! Source-coordinate primitives shared by every domain type.
//!
//! These newtypes carry no I/O or AST dependency — they extract cleanly
//! into `scrap-core` at v1.0 without rename.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// 1-based inclusive line range into a source file.
///
/// Columns are intentionally omitted for v0.1 — every detector operates
/// on whole-test-body granularity. SARIF reporters that need columns can
/// extend the wire envelope additively at v0.2 without breaking
/// `schema_version: 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Span {
    pub start_line: u32,
    pub end_line: u32,
}

impl Span {
    /// Construct a span over the inclusive line range `[start_line,
    /// end_line]`. Caller is responsible for `start_line <= end_line`;
    /// detectors emit spans pulled directly from `syn::spanned::Spanned`,
    /// which always produces well-ordered ranges.
    pub fn new(start_line: u32, end_line: u32) -> Self {
        Self {
            start_line,
            end_line,
        }
    }

    /// Number of source lines covered, inclusive on both ends.
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
#[non_exhaustive]
pub struct FilePath(PathBuf);

impl FilePath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }
}

/// Fully qualified Rust path of a test function (e.g.
/// `foo::bar::tests::it_does_a_thing`).
///
/// Newtype around `String` so reporters and aggregators can match on
/// `QualifiedName` without trafficking arbitrary strings through the
/// domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[non_exhaustive]
pub struct QualifiedName(String);

impl QualifiedName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Concrete location of a test in the analyzed tree.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Location {
    pub file_path: FilePath,
    pub span: Span,
}

impl Location {
    pub fn new(file_path: FilePath, span: Span) -> Self {
        Self { file_path, span }
    }
}

/// Identity of a single example (Rust `#[test]` fn) — file + qualified
/// name + span. Each `Finding` and each `ExampleReport` carries exactly
/// one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct TestIdentity {
    pub file_path: FilePath,
    pub qualified_name: QualifiedName,
    pub span: Span,
}

impl TestIdentity {
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
        fn span_line_count_saturates_when_inverted(
            start in 1u32..1_000_000,
            end in 0u32..,
        ) {
            // When end < start (defensive, never produced by adapters),
            // line_count() must not panic and must return at least 1.
            let count = Span::new(start, end).line_count();
            prop_assert!(count >= 1);
        }
    }
}
