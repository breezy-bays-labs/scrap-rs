//! Wire-shape lock for the v0.1 `schema_version: 1` envelope.
//!
//! Two snapshot pins live here:
//!
//! 1. `report_v01_wire_shape` — pins the inner `Report` serde shape
//!    (domain-side drift detector).
//! 2. `envelope_v01_full_shape` — pins the outer envelope produced by
//!    `scrap_core::adapters::reporters::json::emit` (reporter-side
//!    drift detector). Reuses the same `fixture_report()` so a
//!    sibling PR that updates the inner shape (e.g. `Smell.span`
//!    addition concurrent with scrap-rs#30/#24) only requires
//!    `cargo insta review` on both snapshots in one accept.
//!
//! Plus forward-compat round-trip tests and wire-key pin tests
//! that guard against silent serde drift between v0.1 and v0.2+.
//!
//! To update after an intentional schema change:
//! `cargo insta review` → accept (NEVER `cargo insta accept` blind
//! per cabinet CQO #5). Any diff that includes `Smell.span` shape
//! change or `Severity::Advisory` variant addition must be reviewed
//! line-by-line and explained in the PR body.

use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::adapters::reporters::json::{DeltaBlock, DiagnosticsBlock, EmitOptions, emit};
use scrap_core::domain::classification::{
    Actionability, BaselineVerdict, Confidence, RemediationMode, Severity,
};
use scrap_core::domain::finding::Finding;
use scrap_core::domain::parsed::ParseDiagnostic;
use scrap_core::domain::report::{Distribution, FileReport, Report, Summary};
use scrap_core::domain::smell::{Smell, SmellCategory};
use scrap_core::domain::source::{SourceDiagnostic, SourceDiagnosticKind};
use scrap_core::domain::threshold::ThresholdMode;
use scrap_core::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
use std::num::NonZeroUsize;

/// Fixed timestamp injected into snapshot tests so the snapshot is
/// deterministic. ADR D2 example format.
const FIXED_TIMESTAMP: &str = "2026-05-26T00:00:00Z";

/// Test-fixture `AdapterMeta`. Returns by value cheaply because
/// `AdapterMeta` is `Copy` post-scrap-rs#21 (FORK-1 fold).
///
/// Uses a NEUTRAL adapter name (`test-adapter`) rather than the real
/// `scrap4rs` name. Per scrap-rs#37 the adapter-name literal purity CI
/// gate scans `crates/scrap-core/` source AND tests; scrap-core is
/// adapter-name-pure, so its fixtures must be too. The neutral name is
/// also COVERAGE-SUPERIOR: `emit()` threads `meta.tool_name` into the
/// wire WITHOUT branching on it, so a `tool == "test-adapter"`
/// assertion catches a name-hardcoding regression that a
/// `tool == "scrap4rs"` assertion would silently pass. Real-name
/// emission is verified in `crates/scrap4rs/tests/wire_real_name.rs`.
/// 13-field shape per scrap-rs#21 `AdapterMeta` expansion.
fn test_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: "test-adapter",
        language: "rust",
        tool_version: "0.1.0",
        long_version: "0.1.0 (test 2026-05-27)",
        about: "test-adapter (snapshot-test fixture)",
        long_about: "Snapshot-test fixture AdapterMeta for the wire envelope reporter.",
        after_help: "",
        extensions: &["rs"],
        tool_info_uri: "https://example.invalid/scrap",
        rule_help_uri: "https://example.invalid/scrap/rules",
        config_file_name: "test-adapter.toml",
        default_excludes: &["tests/**", "benches/**", "examples/**"],
        parse_hint: "ensure --src points at a Cargo workspace with test files",
    }
}

fn fixture_report() -> Report {
    let test = TestIdentity::new(
        FilePath::new("crates/foo/src/bar.rs"),
        QualifiedName::new("foo::bar::tests::it_does_a_thing"),
        Span::new(42, 51, 1, 1),
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

// ────────────────────────────────────────────────────────────────────
// Envelope snapshot — pins the v0.1 outer envelope shape from
// `scrap_core::adapters::reporters::json::emit`. Reuses
// `fixture_report()` so the inner-shape and outer-shape snapshots
// move together on sibling-PR shape additions.
// ────────────────────────────────────────────────────────────────────

#[test]
fn envelope_v01_full_shape() {
    let report = fixture_report();
    let meta = test_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .expect("emit succeeds");
    let pretty = String::from_utf8(buf).expect("emit produced valid UTF-8");
    insta::assert_snapshot!("envelope_v01", pretty);
}

// ────────────────────────────────────────────────────────────────────
// Forward-compat round-trip tests (CABINET SHOULD-FIX — CEng #3 + CQO #3).
// `JsonEnvelope` is private to the reporter module; round-trip via
// `serde_json::Value` proves the wire-shape contract without exposing
// the type.
// ────────────────────────────────────────────────────────────────────

#[test]
fn omit_via_skip_optionals_round_trip() {
    // Emit a default envelope. `delta` and `diagnostics` are both
    // None and skip_serializing_if elides them.
    let report = fixture_report();
    let meta = test_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&buf).expect("emit emits valid JSON");
    // Both optional blocks must be absent from the wire (not present-as-null).
    assert!(
        value.get("delta").is_none(),
        "delta must be omitted via skip"
    );
    assert!(
        value.get("diagnostics").is_none(),
        "diagnostics must be omitted via skip",
    );
    // Value round-trip is byte-stable.
    let reserialized = serde_json::to_string(&value).unwrap();
    let reparsed: serde_json::Value = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(
        value, reparsed,
        "serde_json::Value round-trip must be stable"
    );
}

#[test]
fn forward_compat_delta_present_empty_round_trips() {
    // Hand-construct an envelope JSON literal with `"delta": {}` — the
    // v0.4 "delta-was-run-but-no-changes" wire shape today's empty
    // DeltaBlock would produce. Round-trip via Value and assert `{}`
    // stays `{}` (NOT collapsed to null, NOT expanded to anything).
    //
    // Note: the literal includes `"top": null` even though today's
    // live emitter omits the key via `skip_serializing_if` (post bot
    // review on PR #77). The `Value` round-trip is shape-tolerant by
    // design — third-party producers that emit a redundant
    // `"top": null` should still parse cleanly. This is a
    // forward-compat ASSURANCE: removing a key from the live emit
    // path doesn't reject incoming envelopes that still carry it.
    let literal = r#"{
  "schema_version": 1,
  "tool": "test-adapter",
  "tool_version": "0.1.0",
  "language": "rust",
  "timestamp": "2026-05-26T00:00:00Z",
  "threshold_mode": "default",
  "result": {"files": [], "summary": {"total_tests": 0, "total_files": 0, "exceeding_threshold": 0, "max_scrap_score": 0.0, "average_scrap_score": 0.0, "by_smell": {}, "by_severity": {}}, "passed": true},
  "view": {"spec": {"top": null, "only_failing": false}, "eligible_count": 0, "truncated": false, "shown": [], "shown_summary": {"total_tests": 0, "total_files": 0, "exceeding_threshold": 0, "max_scrap_score": 0.0, "average_scrap_score": 0.0, "by_smell": {}, "by_severity": {}}},
  "delta": {}
}"#;
    let value: serde_json::Value = serde_json::from_str(literal).expect("literal parses");
    let reserialized = serde_json::to_string(&value).unwrap();
    let reparsed: serde_json::Value = serde_json::from_str(&reserialized).unwrap();
    assert_eq!(value, reparsed);
    assert!(
        reparsed["delta"].is_object() && reparsed["delta"].as_object().unwrap().is_empty(),
        "delta must round-trip as empty object, got: {}",
        reparsed["delta"],
    );

    // Round-trip via the live DeltaBlock type to prove the type itself
    // serializes as `{}` not `null`.
    let block = DeltaBlock::default();
    let serialized = serde_json::to_value(&block).unwrap();
    assert!(
        serialized.is_object() && serialized.as_object().unwrap().is_empty(),
        "DeltaBlock::default() must serialize as `{{}}`, got: {serialized}",
    );
}

#[test]
fn forward_compat_diagnostics_present_empty_collapses_to_object() {
    // `DiagnosticsBlock { source: vec![], parse: vec![] }` — the
    // verbose-mode-on-but-no-diagnostics case. Per D-DIAG-1, both
    // inner fields skip via `Vec::is_empty`, so the wire emits
    // `{}` not `{"source": [], "parse": []}`.
    let empty = DiagnosticsBlock::default();
    let serialized = serde_json::to_value(&empty).unwrap();
    assert!(
        serialized.is_object(),
        "DiagnosticsBlock serializes as object, got: {serialized}",
    );
    let obj = serialized.as_object().unwrap();
    assert!(
        obj.is_empty(),
        "Empty DiagnosticsBlock must collapse to `{{}}`, got: {serialized}",
    );
}

#[test]
fn envelope_field_declaration_order() {
    // D-ORDER-1 (load-bearing per crap4rs D7): top-level key
    // declaration order is asserted on the pretty-printed string.
    // Anchor to `\n  "key"` (depth-2 indent in serde_json pretty
    // printer) so nested fields with the same name can't shadow the
    // top-level position (crap4rs CodeRabbit CR-N5 lesson).
    let report = fixture_report();
    let meta = test_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .unwrap();
    let pretty = String::from_utf8(buf).unwrap();
    let keys = [
        "schema_version",
        "tool",
        "tool_version",
        "language",
        "timestamp",
        "threshold_mode",
        "result",
        "view",
    ];
    let positions: Vec<usize> = keys
        .iter()
        .map(|k| {
            pretty
                .find(&format!("\n  \"{k}\""))
                .unwrap_or_else(|| panic!("missing top-level key {k} in:\n{pretty}"))
        })
        .collect();
    for (k_pair, p_pair) in keys.windows(2).zip(positions.windows(2)) {
        assert!(
            p_pair[0] < p_pair[1],
            "envelope key order: expected {} before {}, got positions {} and {}",
            k_pair[0],
            k_pair[1],
            p_pair[0],
            p_pair[1],
        );
    }
}

#[test]
fn ignore_unknown_v02_only_field() {
    // Construct an envelope JSON literal with a fake `result.v02_only_field`
    // that today's deserializer doesn't know about. Parsing to Value
    // tolerates it (serde's default behavior); proves v0.x → v0.x+1
    // additive changes don't break forward-compat parsers.
    let literal = r#"{
  "schema_version": 1,
  "tool": "test-adapter",
  "tool_version": "0.1.0",
  "language": "rust",
  "timestamp": "2026-05-26T00:00:00Z",
  "threshold_mode": "default",
  "result": {"files": [], "summary": {"total_tests": 0, "total_files": 0, "exceeding_threshold": 0, "max_scrap_score": 0.0, "average_scrap_score": 0.0, "by_smell": {}, "by_severity": {}}, "passed": true, "v02_only_field": "ignored"},
  "view": {"spec": {"top": null, "only_failing": false}, "eligible_count": 0, "truncated": false, "shown": [], "shown_summary": {"total_tests": 0, "total_files": 0, "exceeding_threshold": 0, "max_scrap_score": 0.0, "average_scrap_score": 0.0, "by_smell": {}, "by_severity": {}}}
}"#;
    let value: serde_json::Value = serde_json::from_str(literal).expect("literal parses");
    // All standard keys present.
    for key in [
        "schema_version",
        "tool",
        "tool_version",
        "language",
        "timestamp",
        "threshold_mode",
        "result",
        "view",
    ] {
        assert!(
            value.get(key).is_some(),
            "expected key `{key}` present in parsed envelope",
        );
    }
    // The fake field is preserved in the Value (proves serde's
    // default ignore-unknown semantics at the Value layer).
    assert_eq!(value["result"]["v02_only_field"], "ignored");
}

// ────────────────────────────────────────────────────────────────────
// Wire-key pin tests (per D-WIRE-1; mirrors `domain/classification.rs`
// belt-and-suspenders convention).
// ────────────────────────────────────────────────────────────────────

#[test]
fn envelope_top_level_keys_pinned() {
    // Lock the on-disk strings for every top-level envelope field. A
    // serde rename that drifts the wire shape fails here, separate
    // from the snapshot test (which only catches the FULL shape; this
    // catches per-key drift even within an unchanged shape).
    let report = fixture_report();
    let meta = test_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    let obj = value.as_object().expect("envelope is a JSON object");
    let actual: std::collections::BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let expected: std::collections::BTreeSet<&str> = [
        "schema_version",
        "tool",
        "tool_version",
        "language",
        "timestamp",
        "threshold_mode",
        "result",
        "view",
    ]
    .into_iter()
    .collect();
    assert_eq!(actual, expected, "top-level keys drifted");
}

#[test]
fn view_block_keys_pinned() {
    let report = fixture_report();
    let meta = test_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    let view = value.get("view").expect("view block present");
    let actual: std::collections::BTreeSet<&str> = view
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected: std::collections::BTreeSet<&str> = [
        "spec",
        "eligible_count",
        "truncated",
        "shown",
        "shown_summary",
    ]
    .into_iter()
    .collect();
    assert_eq!(actual, expected, "view block keys drifted");
}

#[test]
fn view_spec_keys_pinned() {
    // Construct with `top = Some(...)` so the `skip_serializing_if`
    // doesn't elide the field; this test pins the wire-key spelling
    // for BOTH spec fields. The omit-via-skip behavior for `top: None`
    // is pinned separately in
    // `json::tests::view_spec_top_none_omitted_via_skip_serializing_if`.
    let report = fixture_report();
    let meta = test_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions {
            top: Some(NonZeroUsize::new(1).unwrap()),
            only_failing: false,
        },
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    let spec = value
        .get("view")
        .and_then(|v| v.get("spec"))
        .expect("view.spec present");
    let actual: std::collections::BTreeSet<&str> = spec
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected: std::collections::BTreeSet<&str> = ["top", "only_failing"].into_iter().collect();
    assert_eq!(actual, expected, "view.spec keys drifted");
}

#[test]
fn diagnostics_block_keys_pinned() {
    // Populate both inner fields so the skip_serializing_if doesn't
    // elide them; assert the wire keys spell exactly `source` /
    // `parse`.
    let block = DiagnosticsBlock {
        source: vec![SourceDiagnostic::new(
            FilePath::new("a.rs"),
            SourceDiagnosticKind::PermissionDenied,
            "denied",
        )],
        parse: vec![ParseDiagnostic::new(
            scrap_core::domain::parsed::ParseDiagnosticKind::UnsupportedAttribute,
            Some(Span::new(1, 1, 1, 1)),
            "recovered",
        )],
    };
    let value = serde_json::to_value(&block).unwrap();
    let actual: std::collections::BTreeSet<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let expected: std::collections::BTreeSet<&str> = ["source", "parse"].into_iter().collect();
    assert_eq!(actual, expected, "DiagnosticsBlock keys drifted");
}

#[test]
fn delta_empty_struct_serializes_as_object() {
    // CABINET SHOULD-FIX (CAO F4): lock `{}` not `null` before v0.4
    // fills DeltaBlock with real fields.
    let value = serde_json::to_value(DeltaBlock::default()).unwrap();
    assert!(
        value.is_object(),
        "DeltaBlock must serialize as object, got: {value}",
    );
    assert!(
        value.as_object().unwrap().is_empty(),
        "DeltaBlock::default() must serialize as `{{}}` (empty object), got: {value}",
    );
}

#[test]
fn result_passed_immutability_under_view_flags() {
    // CABINET ADVISORY #8 (folded): the truthful-gate guarantee at
    // the test level. `result.passed` reflects `Report.passed`
    // verbatim regardless of view-flag reshaping.
    let mut report = fixture_report();
    report.passed = true;
    let meta = test_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions {
            top: Some(NonZeroUsize::new(1).unwrap()),
            only_failing: true,
        },
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&buf).unwrap();
    assert_eq!(
        value["result"]["passed"], true,
        "result.passed must reflect Report.passed verbatim — view flags MUST NOT reshape it",
    );
}
