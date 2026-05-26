//! Wire-shape lock for the v0.1 `schema_version: 1` envelope.
//!
//! These snapshots are the executable form of kickstart plan §6:
//! changes to derive output, serde attributes, or struct field order
//! must be deliberate and reflected here. Any unintentional drift fails
//! CI.
//!
//! To update after an intentional schema change:
//! `cargo insta review` → accept the new snapshot.

use scrap_core::domain::classification::{
    Actionability, BaselineVerdict, Confidence, RemediationMode, Severity,
};
use scrap_core::domain::finding::Finding;
use scrap_core::domain::report::{Distribution, FileReport, Report, Summary};
use scrap_core::domain::smell::{Smell, SmellCategory};
use scrap_core::domain::threshold::ThresholdMode;
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

fn fixture_report() -> Report {
    let test = TestIdentity::new(
        FilePath::new("crates/foo/src/bar.rs"),
        QualifiedName::new("foo::bar::tests::it_does_a_thing"),
        Span::new(42, 51),
    );
    let smell = Smell::new(
        SmellCategory::ZeroAssertion,
        Severity::High,
        Actionability::AutoRefactor,
        10,
        None,
    );
    let finding = Finding::new(test, vec![smell]);

    let mut summary = Summary {
        total_tests: 412,
        total_files: 38,
        exceeding_threshold: 1,
        max_scrap_score: 10.0,
        average_scrap_score: 0.024,
        ..Summary::default()
    };
    summary
        .distribution
        .record(SmellCategory::ZeroAssertion, Severity::High);

    Report {
        files: vec![FileReport::new(
            FilePath::new("crates/foo/src/bar.rs"),
            vec![finding],
        )],
        summary,
        passed: false,
    }
}

#[test]
fn report_v01_wire_shape() {
    // Serialize to a `serde_json::Value` so enum-keyed `BTreeMap`
    // entries (e.g. `Distribution::by_smell`) materialize as JSON
    // strings — insta's content layer doesn't traverse non-string
    // serde keys on its own.
    let value: serde_json::Value =
        serde_json::to_value(fixture_report()).expect("Report serializes to JSON");
    let pretty = serde_json::to_string_pretty(&value).expect("pretty JSON renders");
    insta::assert_snapshot!("report_v01", pretty);
}

#[test]
fn smell_category_v01_wire_strings() {
    let cases = [
        (SmellCategory::ZeroAssertion, "zero_assertion"),
        (
            SmellCategory::TautologicalAssertion,
            "tautological_assertion",
        ),
        (SmellCategory::NoOpIo, "no_op_io"),
        (SmellCategory::SurfaceOnlyIo, "surface_only_io"),
        (SmellCategory::LargeExample, "large_example"),
    ];
    for (cat, expected_wire) in cases {
        let serde_str = serde_json::to_value(cat).unwrap();
        let helper_str = serde_json::Value::String(cat.as_wire_str().to_owned());
        assert_eq!(
            serde_str, helper_str,
            "SmellCategory::{cat:?} disagreement: serde={serde_str} vs as_wire_str()={helper_str}",
        );
        assert_eq!(serde_str, serde_json::Value::String(expected_wire.into()));
    }
}

#[test]
fn threshold_mode_v01_wire_strings() {
    let cases = [
        (ThresholdMode::Strict, "strict"),
        (ThresholdMode::Default, "default"),
        (ThresholdMode::Lenient, "lenient"),
    ];
    for (mode, expected_wire) in cases {
        let serde_str = serde_json::to_value(mode).unwrap();
        let helper_str = serde_json::Value::String(mode.as_wire_str().to_owned());
        assert_eq!(
            serde_str, helper_str,
            "ThresholdMode::{mode:?} disagreement: serde={serde_str} vs as_wire_str()={helper_str}",
        );
        assert_eq!(serde_str, serde_json::Value::String(expected_wire.into()));
    }
}

#[test]
fn classification_enums_v01_wire_strings() {
    // Severity
    for (s, w) in [
        (Severity::Low, "low"),
        (Severity::Moderate, "moderate"),
        (Severity::High, "high"),
    ] {
        assert_eq!(serde_json::to_value(s).unwrap(), serde_json::json!(w));
    }
    // Actionability
    for (a, w) in [
        (Actionability::AutoRefactor, "auto_refactor"),
        (Actionability::ManualSplit, "manual_split"),
        (Actionability::ReviewFirst, "review_first"),
    ] {
        assert_eq!(serde_json::to_value(a).unwrap(), serde_json::json!(w));
    }
    // Confidence (reserved for v0.4)
    for (c, w) in [
        (Confidence::Low, "low"),
        (Confidence::Medium, "medium"),
        (Confidence::High, "high"),
    ] {
        assert_eq!(serde_json::to_value(c).unwrap(), serde_json::json!(w));
    }
    // RemediationMode (reserved for v0.5)
    for (r, w) in [
        (RemediationMode::Stable, "stable"),
        (RemediationMode::Local, "local"),
        (RemediationMode::Split, "split"),
    ] {
        assert_eq!(serde_json::to_value(r).unwrap(), serde_json::json!(w));
    }
    // BaselineVerdict (reserved for v0.4)
    for (v, w) in [
        (BaselineVerdict::Improved, "improved"),
        (BaselineVerdict::Worse, "worse"),
        (BaselineVerdict::Mixed, "mixed"),
        (BaselineVerdict::Unchanged, "unchanged"),
    ] {
        assert_eq!(serde_json::to_value(v).unwrap(), serde_json::json!(w));
    }
}

#[test]
fn distribution_used_directly_serializes_correctly() {
    // Sanity check: even when `Distribution` is used outside `Summary`
    // (where #[serde(flatten)] hoists it), it round-trips clean.
    let mut d = Distribution::new();
    d.record(SmellCategory::ZeroAssertion, Severity::High);
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["by_smell"]["zero_assertion"], 1);
    assert_eq!(json["by_severity"]["high"], 1);
}
