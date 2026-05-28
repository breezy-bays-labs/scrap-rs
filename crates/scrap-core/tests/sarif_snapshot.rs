//! Wire-shape lock for the SARIF 2.1.0 reporter (scrap-rs#17).
//!
//! Separate fixture from the JSON envelope's `wire_envelope_snapshot`
//! (different output shape entirely). Pins the full SARIF document
//! produced by `scrap_core::adapters::reporters::sarif::emit` so a
//! drift in struct shape, rename attrs, severity mapping, or column
//! arithmetic surfaces as a snapshot diff.
//!
//! Uses a NEUTRAL adapter name ("test-adapter") — this is an
//! integration test under `tests/` (outside the source-only
//! adapter-name purity CI gate's scope), but the SARIF snapshot is
//! kept neutral so it stays forward-compatible across adapters
//! (scrap4rs / scrap4ts both emit byte-identical SARIF modulo the meta
//! strings). The existing `wire_envelope_snapshot` fixtures keep their
//! real adapter names (sibling scope, scrap-rs#37).
//!
//! To update after an intentional change:
//! `cargo insta review` → accept (NEVER blind-accept).

use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::adapters::reporters::sarif::emit;
use scrap_core::domain::classification::{Actionability, Severity};
use scrap_core::domain::finding::Finding;
use scrap_core::domain::report::{FileReport, Report};
use scrap_core::domain::smell::{Smell, SmellCategory};
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

/// Neutral adapter meta — see module docstring for why the SARIF
/// snapshot stays adapter-name-agnostic.
fn neutral_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: "test-adapter",
        language: "rust",
        tool_version: "0.1.0",
        long_version: "0.1.0 (snapshot 2026-05-27)",
        about: "SARIF snapshot fixture",
        long_about: "Snapshot-test fixture AdapterMeta for the SARIF reporter.",
        after_help: "",
        extensions: &["rs"],
        tool_info_uri: "https://example.invalid/test-adapter",
        rule_help_uri: "https://example.invalid/test-adapter#rules",
        config_file_name: "test-adapter.toml",
        default_excludes: &[],
        parse_hint: "ensure --src points at a workspace with test files",
    }
}

/// Two findings exercising both the smell-span and test-span region
/// paths, multiple categories, and all three severities so the
/// snapshot pins the full projection surface.
fn fixture_report() -> Report {
    // Finding A: two smells, one with a narrower smell-span.
    let test_a = TestIdentity::new(
        FilePath::new("crates/foo/src/bar.rs"),
        QualifiedName::new("foo::bar::tests::it_does_a_thing"),
        Span::new(42, 51, 5, 1),
    );
    let finding_a = Finding::new(
        test_a,
        vec![
            Smell::new(
                SmellCategory::ZeroAssertion,
                Severity::High,
                Actionability::AutoRefactor,
                10,
                None,
            ),
            Smell::new(
                SmellCategory::TautologicalAssertion,
                Severity::High,
                Actionability::AutoRefactor,
                10,
                Some(Span::new(45, 45, 9, 24)),
            ),
        ],
    );

    // Finding B: one moderate-severity smell (test-span region).
    let test_b = TestIdentity::new(
        FilePath::new("crates/foo/src/baz.rs"),
        QualifiedName::new("foo::baz::tests::reads_a_file"),
        Span::new(10, 18, 5, 6),
    );
    let finding_b = Finding::new(
        test_b,
        vec![Smell::new(
            SmellCategory::SurfaceOnlyIo,
            Severity::Moderate,
            Actionability::ReviewFirst,
            6,
            None,
        )],
    );

    Report {
        files: vec![
            FileReport::new(FilePath::new("crates/foo/src/bar.rs"), vec![finding_a]),
            FileReport::new(FilePath::new("crates/foo/src/baz.rs"), vec![finding_b]),
        ],
        ..Report::default()
    }
}

#[test]
fn sarif_full_document_shape() {
    let report = fixture_report();
    let meta = neutral_meta();
    let mut buf = Vec::new();
    emit(&report, &meta, &mut buf).expect("emit succeeds");
    let pretty = String::from_utf8(buf).expect("emit produced valid UTF-8");
    insta::assert_snapshot!("sarif_v2_1_0", pretty);
}
