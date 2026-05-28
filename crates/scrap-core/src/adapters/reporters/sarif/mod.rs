//! SARIF 2.1.0 reporter — for GitHub Code Scanning + IDE SARIF viewers.
//!
//! Free function `emit()` per the locked reporter design (see
//! [`crate::adapters::reporters`] module docstring +
//! [`crap4rs/adr-free-functions-over-reporter-trait`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/crap4rs/adr-free-functions-over-reporter-trait.md)
//! D1) — NOT a `dyn OutputPort` impl (there is no such trait). Lives in
//! `scrap-core` (ADR `adr-hexagonal-layout` D1) so every adapter binary
//! shares one SARIF projection.
//!
//! ## Granularity: one Result per Smell (scrap-rs#17 D2)
//!
//! `Finding.smells` is a `Vec` — `category` / `severity` / the narrower
//! per-assertion `span` all live on the [`Smell`], not the `Finding`.
//! A multi-smell finding therefore maps to MULTIPLE SARIF results, one
//! per smell: `ruleId` = [`SmellCategory::as_wire_str`], `level` mapped
//! from [`Severity`], region from `smell.span.unwrap_or(test.span)`.
//! Zero-smell findings emit no result.
//!
//! ## Adapter-name purity
//!
//! `tool.driver.name` comes from [`AdapterMeta::tool_name`] — NEVER a
//! hardcoded adapter-name literal (scrap-core is adapter-agnostic; the
//! source-only adapter-name purity CI gate bans the literal). The repo
//! / rule-help URIs come from `meta.tool_info_uri` / `meta.rule_help_uri`
//! so each adapter's SARIF links to its own repo.
//!
//! ## Hand-rolled subset (no `serde_sarif` dep)
//!
//! Per the issue's Discovery decision, the SARIF 2.1.0 schema is
//! hand-rolled as the minimal subset Code Scanning consumes. The structs
//! derive both `Serialize` and `Deserialize` so the roundtrip test can
//! parse emitted SARIF back into [`SarifLog`].
//!
//! ## Columns
//!
//! `domain::Span` columns are required + 1-based (never 0). SARIF
//! `region.startColumn` is 1-based inclusive (passes through);
//! `region.endColumn` is "one greater than the column of the last
//! character" (SARIF 2.1.0 §3.30.7 — exclusive end), so the reporter
//! emits `end_column + 1`. Both columns are always present (no
//! omit-when-unknown branch — the domain guarantees real columns).

use serde::{Deserialize, Serialize};

use crate::adapter_meta::AdapterMeta;
use crate::domain::classification::Severity;
use crate::domain::report::Report;
use crate::domain::smell::{Smell, SmellCategory};
use crate::domain::types::Span;

use super::sarif::constants::rule_description;

pub mod constants;

/// SARIF 2.1.0 schema URL (`$schema`) — the JSON Schema Store mirror
/// GitHub Code Scanning recognises.
const SCHEMA_URI: &str = "https://json.schemastore.org/sarif-2.1.0.json";

/// SARIF version string. Pinned to the 2.1.0 spec this reporter targets.
const SARIF_VERSION: &str = "2.1.0";

// ────────────────────────────────────────────────────────────────────
// Public emit function
// ────────────────────────────────────────────────────────────────────

/// Serialize a [`Report`] into SARIF 2.1.0 JSON and write to `writer`.
///
/// One SARIF result per [`Smell`] across every finding (scrap-rs#17 D2):
/// `ruleId` = the smell category's wire string, `level` mapped from the
/// smell's severity, `region` from `smell.span` (or the enclosing test
/// span when the smell carries no narrower span). `tool.driver.name` is
/// `meta.tool_name`; the rule set is one `reportingDescriptor` per
/// `SmellCategory` regardless of which categories appear in the report
/// (so consumers can introspect the full rule schema).
///
/// `partialFingerprints` carries the
/// `(file_relative_path, smell_category, function_qualified_name)`
/// triple per the issue Discovery — a stable id for finding continuity
/// across PRs.
///
/// # Errors
///
/// Returns [`serde_json::Error`] on writer I/O failure (wrapped via
/// [`serde_json::Error::io`]); serialization itself is effectively
/// infallible (all owned strings + numbers).
pub fn emit<W: std::io::Write>(
    report: &Report,
    meta: &AdapterMeta,
    writer: &mut W,
) -> Result<(), serde_json::Error> {
    let log = build_log(report, meta);
    serde_json::to_writer_pretty(writer, &log)
}

// ────────────────────────────────────────────────────────────────────
// Projection helpers
// ────────────────────────────────────────────────────────────────────

/// Build the full [`SarifLog`] from a report + adapter meta. Split from
/// [`emit`] so tests can assert on the structured log without a writer.
fn build_log(report: &Report, meta: &AdapterMeta) -> SarifLog {
    let results: Vec<SarifResult> = report
        .files
        .iter()
        .flat_map(|file| file.findings.iter())
        .flat_map(|finding| {
            finding.smells.iter().map(move |smell| {
                result_for_smell(
                    smell,
                    finding.test.file_path.to_string(),
                    finding.test.qualified_name.as_str(),
                    finding.test.span,
                )
            })
        })
        .collect();

    SarifLog {
        schema: SCHEMA_URI.to_string(),
        version: SARIF_VERSION.to_string(),
        runs: vec![SarifRun {
            tool: SarifTool {
                driver: SarifDriver {
                    name: meta.tool_name.to_string(),
                    version: meta.tool_version.to_string(),
                    information_uri: meta.tool_info_uri.to_string(),
                    rules: all_rules(meta.rule_help_uri),
                },
            },
            results,
        }],
    }
}

/// One `reportingDescriptor` per `SmellCategory` (v0.1 slate). The rule
/// set is emitted in full regardless of which categories the report
/// contains so SARIF consumers can introspect the schema.
fn all_rules(help_uri: &str) -> Vec<SarifRule> {
    [
        SmellCategory::ZeroAssertion,
        SmellCategory::TautologicalAssertion,
        SmellCategory::NoOpIo,
        SmellCategory::SurfaceOnlyIo,
        SmellCategory::LargeExample,
    ]
    .into_iter()
    .map(|category| {
        let desc = rule_description(category);
        SarifRule {
            id: category.as_wire_str().to_string(),
            name: desc.name.to_string(),
            short_description: SarifText {
                text: desc.short_description.to_string(),
            },
            full_description: SarifText {
                text: desc.full_description.to_string(),
            },
            help_uri: help_uri.to_string(),
        }
    })
    .collect()
}

/// Build a SARIF result for one smell. `file_path` / `qualified_name`
/// are the enclosing test's identity; `test_span` is the fallback region
/// when the smell carries no narrower span.
fn result_for_smell(
    smell: &Smell,
    file_path: String,
    qualified_name: &str,
    test_span: Span,
) -> SarifResult {
    let region_span = smell.span.unwrap_or(test_span);
    let rule_id = smell.category.as_wire_str().to_string();
    // Fingerprint triple per scrap-rs#17 Discovery:
    // (file_relative_path, smell_category, function_qualified_name).
    let fingerprint = format!("{file_path}:{rule_id}:{qualified_name}");

    SarifResult {
        rule_id: rule_id.clone(),
        level: level_for(smell.severity).to_string(),
        message: SarifText {
            text: smell.ai_actionability_message.clone(),
        },
        locations: vec![SarifLocation {
            physical_location: SarifPhysicalLocation {
                artifact_location: SarifArtifactLocation { uri: file_path },
                region: region_for_span(region_span),
            },
        }],
        partial_fingerprints: SarifPartialFingerprints {
            test_identity: fingerprint,
        },
    }
}

/// Project a [`Span`] into a SARIF region. Lines pass through (both
/// 1-based inclusive). `startColumn` passes through (1-based inclusive
/// in both the domain and SARIF). `endColumn` is `span.end_column + 1`
/// per SARIF 2.1.0 §3.30.7 (exclusive end = one past the last
/// character). Columns are always emitted — the domain guarantees real
/// 1-based columns (never the 0 "unknown" sentinel crap-rs guards
/// against), so there is no omit-when-unknown branch.
fn region_for_span(span: Span) -> SarifRegion {
    SarifRegion {
        start_line: span.start_line,
        end_line: span.end_line,
        start_column: span.start_column,
        end_column: span.end_column.saturating_add(1),
    }
}

/// Map a domain [`Severity`] to a SARIF result `level`.
///
/// `High → error`, `Moderate → warning`, `Low → note`. `Severity` is
/// `#[non_exhaustive]`, but the match is exhaustive within scrap-core
/// (the defining crate sees every variant), so a future variant
/// addition is a compile error HERE — the desired forcing function for
/// an explicit severity→level decision rather than a silent default.
fn level_for(severity: Severity) -> &'static str {
    match severity {
        Severity::High => "error",
        Severity::Moderate => "warning",
        Severity::Low => "note",
    }
}

// ────────────────────────────────────────────────────────────────────
// SARIF 2.1.0 envelope structs (hand-rolled minimal subset)
//
// Derive Serialize + Deserialize: Serialize for emit, Deserialize for
// the roundtrip test (emit → parse back into SarifLog). The rename
// attrs (`$schema`, `informationUri`, `ruleId`, `startColumn`, …) are
// the SARIF wire spelling.
// ────────────────────────────────────────────────────────────────────

/// SARIF `{"text": ...}` wrapper used by message / description fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifText {
    /// The text payload.
    pub text: String,
}

/// Top-level SARIF log object.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifLog {
    /// `$schema` — the SARIF 2.1.0 JSON Schema URL.
    #[serde(rename = "$schema")]
    pub schema: String,
    /// SARIF spec version (`"2.1.0"`).
    pub version: String,
    /// Analysis runs (always exactly one for scrap-rs).
    pub runs: Vec<SarifRun>,
}

/// One SARIF run — a single analysis pass by one tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifRun {
    /// The tool that produced this run.
    pub tool: SarifTool,
    /// Per-smell results.
    pub results: Vec<SarifResult>,
}

/// SARIF tool wrapper (`{"driver": ...}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifTool {
    /// The tool's primary driver component.
    pub driver: SarifDriver,
}

/// SARIF driver — the tool's identity + its rule catalogue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifDriver {
    /// Tool name (from `AdapterMeta::tool_name`).
    pub name: String,
    /// Tool version (from `AdapterMeta::tool_version`).
    pub version: String,
    /// Repo / docs URL (from `AdapterMeta::tool_info_uri`).
    #[serde(rename = "informationUri")]
    pub information_uri: String,
    /// One reporting descriptor per smell category.
    pub rules: Vec<SarifRule>,
}

/// SARIF `reportingDescriptor` — one smell category's rule metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifRule {
    /// Rule id = the smell category's wire string.
    pub id: String,
    /// `PascalCase` human-facing rule name.
    pub name: String,
    /// One-line summary.
    #[serde(rename = "shortDescription")]
    pub short_description: SarifText,
    /// Multi-sentence detail.
    #[serde(rename = "fullDescription")]
    pub full_description: SarifText,
    /// Rule-help URL (from `AdapterMeta::rule_help_uri`).
    #[serde(rename = "helpUri")]
    pub help_uri: String,
}

/// SARIF result — one per smell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifResult {
    /// Which rule this result references (smell category wire string).
    #[serde(rename = "ruleId")]
    pub rule_id: String,
    /// Severity level (`error` / `warning` / `note`).
    pub level: String,
    /// Result message — the smell's actionability suggestion.
    pub message: SarifText,
    /// Physical source locations (always exactly one for scrap-rs).
    pub locations: Vec<SarifLocation>,
    /// Stable fingerprint for cross-PR continuity.
    #[serde(rename = "partialFingerprints")]
    pub partial_fingerprints: SarifPartialFingerprints,
}

/// SARIF location wrapper (`{"physicalLocation": ...}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifLocation {
    /// The physical (file + region) location.
    #[serde(rename = "physicalLocation")]
    pub physical_location: SarifPhysicalLocation,
}

/// SARIF physical location — artifact (file) + region (line/col range).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifPhysicalLocation {
    /// The source file.
    #[serde(rename = "artifactLocation")]
    pub artifact_location: SarifArtifactLocation,
    /// The line + column region within the file.
    pub region: SarifRegion,
}

/// SARIF artifact location — the source file URI (relative repo path).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifArtifactLocation {
    /// Relative repo path to the source file.
    pub uri: String,
}

/// SARIF region — 1-based line + column range. `endColumn` is exclusive
/// per SARIF 2.1.0 §3.30.7.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifRegion {
    /// 1-based inclusive start line.
    #[serde(rename = "startLine")]
    pub start_line: u32,
    /// 1-based inclusive end line.
    #[serde(rename = "endLine")]
    pub end_line: u32,
    /// 1-based inclusive start column.
    #[serde(rename = "startColumn")]
    pub start_column: u32,
    /// 1-based exclusive end column (`span.end_column + 1`).
    #[serde(rename = "endColumn")]
    pub end_column: u32,
}

/// SARIF `partialFingerprints` — stable cross-PR finding id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SarifPartialFingerprints {
    /// `(file_relative_path, smell_category, function_qualified_name)`
    /// triple, colon-joined.
    #[serde(rename = "testIdentity")]
    pub test_identity: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::finding::Finding;
    use crate::domain::report::{FileReport, Report};
    use crate::domain::smell::{Smell, SmellCategory};
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

    /// Neutral test-fixture meta. Uses `test-adapter` (NOT a concrete
    /// adapter-name literal) because `#[cfg(test)] mod tests` here lives
    /// under `crates/scrap-core/src/` — the source-only adapter-name
    /// purity CI gate applies. Forward-compatible per scrap-rs#37.
    fn test_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test 2026-05-27)",
            about: "sarif-test fixture",
            long_about: "Test-fixture AdapterMeta for the SARIF reporter unit tests.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/test-adapter",
            rule_help_uri: "https://example.invalid/test-adapter#rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &[],
            parse_hint: "ensure --src points at a workspace with test files",
        }
    }

    fn finding_with(path: &str, name: &str, test_span: Span, smells: Vec<Smell>) -> Finding {
        let test = TestIdentity::new(FilePath::new(path), QualifiedName::new(name), test_span);
        Finding::new(test, smells)
    }

    fn report_with(findings: Vec<Finding>) -> Report {
        use std::collections::BTreeMap;
        let mut by_path: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
        for f in findings {
            let key = f.test.file_path.as_path().to_string_lossy().into_owned();
            by_path.entry(key).or_default().push(f);
        }
        let files = by_path
            .into_iter()
            .map(|(path, fs)| FileReport::new(FilePath::new(&path), fs))
            .collect();
        Report {
            files,
            ..Report::default()
        }
    }

    fn smell(category: SmellCategory, severity: Severity, span: Option<Span>) -> Smell {
        Smell::new(category, severity, Actionability::AutoRefactor, 10, span)
    }

    fn emit_to_string(report: &Report, meta: &AdapterMeta) -> String {
        let mut buf = Vec::new();
        emit(report, meta, &mut buf).expect("emit succeeds");
        String::from_utf8(buf).expect("emit produces valid UTF-8")
    }

    fn parse(json: &str) -> serde_json::Value {
        serde_json::from_str(json).expect("emit must produce valid JSON")
    }

    // ── Top-level shape ────────────────────────────────────────────

    #[test]
    fn top_level_shape_is_sarif_2_1_0() {
        let report = report_with(vec![]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        assert_eq!(v["$schema"], SCHEMA_URI);
        assert_eq!(v["version"], "2.1.0");
        assert!(v["runs"].is_array());
        assert_eq!(v["runs"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn driver_name_comes_from_adapter_meta_not_a_literal() {
        let report = report_with(vec![]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        // The neutral adapter name proves the name is threaded from
        // AdapterMeta::tool_name, not hardcoded.
        assert_eq!(v["runs"][0]["tool"]["driver"]["name"], "test-adapter");
        assert_eq!(v["runs"][0]["tool"]["driver"]["version"], "0.1.0");
        assert_eq!(
            v["runs"][0]["tool"]["driver"]["informationUri"],
            "https://example.invalid/test-adapter",
        );
    }

    #[test]
    fn empty_report_produces_empty_results_array() {
        let report = report_with(vec![]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 0);
    }

    // ── Rules ──────────────────────────────────────────────────────

    #[test]
    fn rules_present_for_every_category_even_when_no_results() {
        // The full rule catalogue is emitted regardless of which
        // categories appear, so consumers can introspect the schema.
        let report = report_with(vec![]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        let rules = v["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
        assert_eq!(rules.len(), 5, "one rule per v0.1 SmellCategory variant");
        let ids: Vec<&str> = rules.iter().map(|r| r["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"zero_assertion"));
        assert!(ids.contains(&"large_example"));
        // helpUri threads from meta.rule_help_uri.
        assert_eq!(
            rules[0]["helpUri"],
            "https://example.invalid/test-adapter#rules"
        );
    }

    #[test]
    fn rule_id_is_smell_category_wire_string() {
        let report = report_with(vec![]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        let rules = v["runs"][0]["tool"]["driver"]["rules"].as_array().unwrap();
        for rule in rules {
            // Every rule id is a snake_case wire string (never PascalCase).
            let id = rule["id"].as_str().unwrap();
            assert!(
                !id.chars().next().unwrap().is_uppercase(),
                "rule id must be the snake_case wire string, got `{id}`",
            );
        }
    }

    // ── Per-Smell granularity (D2) ─────────────────────────────────

    #[test]
    fn one_result_per_smell_not_per_finding() {
        // A single finding with TWO smells of different categories →
        // TWO SARIF results (D2: per-Smell granularity).
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(1, 10, 1, 1),
            vec![
                smell(SmellCategory::ZeroAssertion, Severity::High, None),
                smell(SmellCategory::LargeExample, Severity::Low, None),
            ],
        );
        let report = report_with(vec![finding]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        let results = v["runs"][0]["results"].as_array().unwrap();
        assert_eq!(results.len(), 2, "two smells → two results");
        let rule_ids: Vec<&str> = results
            .iter()
            .map(|r| r["ruleId"].as_str().unwrap())
            .collect();
        assert!(rule_ids.contains(&"zero_assertion"));
        assert!(rule_ids.contains(&"large_example"));
    }

    #[test]
    fn zero_smell_finding_emits_no_result() {
        let finding = finding_with("src/lib.rs", "tests::it", Span::new(1, 5, 1, 1), vec![]);
        let report = report_with(vec![finding]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        assert_eq!(v["runs"][0]["results"].as_array().unwrap().len(), 0);
    }

    // ── Severity mapping ───────────────────────────────────────────

    #[test]
    fn severity_maps_to_sarif_level() {
        let report = report_with(vec![
            finding_with(
                "a.rs",
                "a::t",
                Span::new(1, 2, 1, 1),
                vec![smell(SmellCategory::ZeroAssertion, Severity::High, None)],
            ),
            finding_with(
                "b.rs",
                "b::t",
                Span::new(1, 2, 1, 1),
                vec![smell(SmellCategory::NoOpIo, Severity::Moderate, None)],
            ),
            finding_with(
                "c.rs",
                "c::t",
                Span::new(1, 2, 1, 1),
                vec![smell(SmellCategory::LargeExample, Severity::Low, None)],
            ),
        ]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        let mut levels: Vec<String> = v["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["level"].as_str().unwrap().to_string())
            .collect();
        levels.sort();
        assert_eq!(levels, vec!["error", "note", "warning"]);
    }

    // ── Region / columns ───────────────────────────────────────────

    #[test]
    fn region_uses_smell_span_when_present() {
        // Smell carries a narrower span than the enclosing test.
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(1, 20, 1, 1),
            vec![smell(
                SmellCategory::TautologicalAssertion,
                Severity::High,
                Some(Span::new(7, 7, 5, 18)),
            )],
        );
        let report = report_with(vec![finding]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        let region = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(
            region["startLine"], 7,
            "uses smell span line, not test span"
        );
        assert_eq!(region["endLine"], 7);
        assert_eq!(
            region["startColumn"], 5,
            "startColumn passes through (1-based)"
        );
        assert_eq!(
            region["endColumn"], 19,
            "endColumn = span.end_column + 1 (SARIF §3.30.7 exclusive end)",
        );
    }

    #[test]
    fn region_falls_back_to_test_span_when_smell_span_none() {
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(3, 9, 2, 6),
            vec![smell(SmellCategory::ZeroAssertion, Severity::High, None)],
        );
        let report = report_with(vec![finding]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        let region = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];
        assert_eq!(region["startLine"], 3);
        assert_eq!(region["endLine"], 9);
        assert_eq!(region["startColumn"], 2);
        assert_eq!(region["endColumn"], 7, "test span end_column 6 + 1");
    }

    #[test]
    fn artifact_uri_is_the_relative_file_path() {
        let finding = finding_with(
            "crates/foo/src/bar.rs",
            "foo::tests::it",
            Span::new(1, 2, 1, 1),
            vec![smell(SmellCategory::ZeroAssertion, Severity::High, None)],
        );
        let report = report_with(vec![finding]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        let uri = &v["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]
            ["uri"];
        assert_eq!(uri, "crates/foo/src/bar.rs");
    }

    // ── Fingerprints ───────────────────────────────────────────────

    #[test]
    fn partial_fingerprint_is_path_category_qualified_name_triple() {
        let finding = finding_with(
            "src/lib.rs",
            "mod::tests::it_works",
            Span::new(1, 2, 1, 1),
            vec![smell(SmellCategory::ZeroAssertion, Severity::High, None)],
        );
        let report = report_with(vec![finding]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        assert_eq!(
            v["runs"][0]["results"][0]["partialFingerprints"]["testIdentity"],
            "src/lib.rs:zero_assertion:mod::tests::it_works",
        );
    }

    #[test]
    fn message_text_is_the_smell_actionability_message() {
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(1, 2, 1, 1),
            vec![smell(SmellCategory::ZeroAssertion, Severity::High, None)],
        );
        let report = report_with(vec![finding]);
        let v = parse(&emit_to_string(&report, &test_meta()));
        assert_eq!(
            v["runs"][0]["results"][0]["message"]["text"],
            Smell::default_message(SmellCategory::ZeroAssertion),
        );
    }

    // ── Roundtrip (emit → Deserialize into SarifLog) ───────────────

    #[test]
    fn emitted_sarif_round_trips_into_sarif_log_struct() {
        let finding = finding_with(
            "src/lib.rs",
            "tests::it",
            Span::new(1, 10, 1, 1),
            vec![
                smell(SmellCategory::ZeroAssertion, Severity::High, None),
                smell(
                    SmellCategory::TautologicalAssertion,
                    Severity::Moderate,
                    Some(Span::new(4, 4, 8, 20)),
                ),
            ],
        );
        let report = report_with(vec![finding]);
        let json = emit_to_string(&report, &test_meta());
        let log: SarifLog = serde_json::from_str(&json).expect("SARIF parses back into SarifLog");
        assert_eq!(log.version, "2.1.0");
        assert_eq!(log.runs.len(), 1);
        assert_eq!(log.runs[0].results.len(), 2);
        assert_eq!(log.runs[0].tool.driver.name, "test-adapter");
        assert_eq!(log.runs[0].tool.driver.rules.len(), 5);
        // Re-serialize the parsed struct and confirm it matches the
        // original bytes (full structural roundtrip stability).
        let reserialized = serde_json::to_string_pretty(&log).unwrap();
        assert_eq!(reserialized, json, "roundtrip is byte-stable");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::finding::Finding;
    use crate::domain::report::{FileReport, Report};
    use crate::domain::smell::{Smell, SmellCategory};
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
    use proptest::prelude::*;

    fn pt_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test)",
            about: "sarif proptest fixture",
            long_about: "Proptest fixture AdapterMeta for the SARIF reporter.",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/test-adapter",
            rule_help_uri: "https://example.invalid/test-adapter#rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &[],
            parse_hint: "hint",
        }
    }

    fn arb_category() -> impl Strategy<Value = SmellCategory> {
        prop_oneof![
            Just(SmellCategory::ZeroAssertion),
            Just(SmellCategory::TautologicalAssertion),
            Just(SmellCategory::NoOpIo),
            Just(SmellCategory::SurfaceOnlyIo),
            Just(SmellCategory::LargeExample),
        ]
    }

    fn arb_severity() -> impl Strategy<Value = Severity> {
        prop_oneof![
            Just(Severity::Low),
            Just(Severity::Moderate),
            Just(Severity::High),
        ]
    }

    prop_compose! {
        fn arb_span()(
            start in 1u32..10_000,
            len in 0u32..1_000,
            start_column in 1u32..500,
            col_len in 0u32..500,
        ) -> Span {
            Span::new(start, start + len, start_column, start_column + col_len)
        }
    }

    prop_compose! {
        fn arb_finding()(
            path in "[a-z][a-z/]{0,20}\\.rs",
            name in "[a-z][a-z_:]{0,30}",
            test_span in arb_span(),
            smells in proptest::collection::vec(
                (arb_category(), arb_severity(), proptest::option::of(arb_span())),
                0..4,
            ),
        ) -> Finding {
            let test = TestIdentity::new(
                FilePath::new(&path),
                QualifiedName::new(&name),
                test_span,
            );
            let smells = smells
                .into_iter()
                .map(|(cat, sev, span)| {
                    Smell::new(cat, sev, Actionability::AutoRefactor, 10, span)
                })
                .collect();
            Finding::new(test, smells)
        }
    }

    fn report_from(findings: Vec<Finding>) -> Report {
        use std::collections::BTreeMap;
        let mut by_path: BTreeMap<String, Vec<Finding>> = BTreeMap::new();
        for f in findings {
            let key = f.test.file_path.as_path().to_string_lossy().into_owned();
            by_path.entry(key).or_default().push(f);
        }
        let files = by_path
            .into_iter()
            .map(|(path, fs)| FileReport::new(FilePath::new(&path), fs))
            .collect();
        Report {
            files,
            ..Report::default()
        }
    }

    fn emit_str(report: &Report) -> String {
        let mut buf = Vec::new();
        emit(report, &pt_meta(), &mut buf).expect("emit succeeds");
        String::from_utf8(buf).unwrap()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn prop_emit_always_valid_json(findings in proptest::collection::vec(arb_finding(), 0..6)) {
            let report = report_from(findings);
            let out = emit_str(&report);
            let _: serde_json::Value =
                serde_json::from_str(&out).expect("SARIF must be parseable JSON");
        }

        #[test]
        fn prop_result_count_equals_total_smells(
            findings in proptest::collection::vec(arb_finding(), 0..6),
        ) {
            let report = report_from(findings);
            let expected: usize = report
                .files
                .iter()
                .flat_map(|f| f.findings.iter())
                .map(|f| f.smells.len())
                .sum();
            let v: serde_json::Value = serde_json::from_str(&emit_str(&report)).unwrap();
            let actual = v["runs"][0]["results"].as_array().unwrap().len();
            prop_assert_eq!(actual, expected);
        }

        #[test]
        fn prop_every_result_has_mandatory_fields(
            findings in proptest::collection::vec(arb_finding(), 0..6),
        ) {
            let report = report_from(findings);
            let v: serde_json::Value = serde_json::from_str(&emit_str(&report)).unwrap();
            for r in v["runs"][0]["results"].as_array().unwrap() {
                prop_assert!(r["ruleId"].is_string());
                prop_assert!(matches!(
                    r["level"].as_str().unwrap(),
                    "error" | "warning" | "note",
                ));
                prop_assert!(r["message"]["text"].is_string());
                let region = &r["locations"][0]["physicalLocation"]["region"];
                prop_assert!(region["startLine"].is_u64());
                prop_assert!(region["endLine"].is_u64());
                prop_assert!(region["startColumn"].is_u64());
                prop_assert!(region["endColumn"].is_u64());
                prop_assert!(r["partialFingerprints"]["testIdentity"].is_string());
            }
        }

        #[test]
        fn prop_round_trips_into_sarif_log(
            findings in proptest::collection::vec(arb_finding(), 0..6),
        ) {
            let report = report_from(findings);
            let json = emit_str(&report);
            let log: SarifLog = serde_json::from_str(&json)
                .expect("emitted SARIF parses back into SarifLog");
            let reserialized = serde_json::to_string_pretty(&log).unwrap();
            prop_assert_eq!(reserialized, json);
        }
    }
}
