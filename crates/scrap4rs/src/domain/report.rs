//! Aggregated report types — `Report`, `FileReport`, `Summary`,
//! `Distribution`.
//!
//! `Finding` (in `finding.rs`) is the per-test record that lands flat
//! in the wire envelope's `result.findings[]` array. `FileReport`
//! groups findings by file for reporters that surface findings in
//! source order (markdown, table); the JSON reporter flattens
//! `Report.files` into the wire's flat `result.findings[]` array.
//!
//! `Summary` mirrors the wire envelope's `result.summary` shape from
//! kickstart plan §6: `total_tests`, `total_files`, `exceeding_threshold`,
//! `by_smell`, `by_severity` flat under `summary`. `Distribution` is the
//! domain aggregator that owns the two counter maps; `#[serde(flatten)]`
//! lifts its fields onto `Summary` at the wire boundary so the on-disk
//! shape matches the spec.

use crate::domain::classification::Severity;
use crate::domain::finding::Finding;
use crate::domain::smell::SmellCategory;
use crate::domain::types::FilePath;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-file aggregation. `file_path` is duplicated from
/// `Finding.test.file_path` so the markdown/table reporter has a
/// stable per-file header even when the inner findings are filtered or
/// sorted; the JSON reporter ignores this denormalization and emits
/// findings flat. `FileReport::new` enforces the agreement between the
/// outer file path and the inner test paths via debug-assert.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileReport {
    pub file_path: FilePath,
    pub findings: Vec<Finding>,
}

impl FileReport {
    pub fn new(file_path: FilePath, findings: Vec<Finding>) -> Self {
        debug_assert!(
            findings.iter().all(|f| f.test.file_path == file_path),
            "FileReport::new: inner findings reference a different file_path than the outer FileReport",
        );
        Self {
            file_path,
            findings,
        }
    }
}

/// Counts of findings broken down by smell category and severity.
///
/// Stored as `BTreeMap` for stable ordering on the wire — JSON object
/// keys come out in `Ord` (declaration) order, which matches the §6
/// envelope example and produces reproducible snapshots.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Distribution {
    pub by_smell: BTreeMap<SmellCategory, u32>,
    pub by_severity: BTreeMap<Severity, u32>,
}

impl Distribution {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, category: SmellCategory, severity: Severity) {
        *self.by_smell.entry(category).or_insert(0) += 1;
        *self.by_severity.entry(severity).or_insert(0) += 1;
    }

    /// Total number of recorded smells. Equal to the sum across either
    /// axis (each smell contributes one count to each map).
    pub fn total(&self) -> u32 {
        self.by_smell.values().sum()
    }
}

/// Top-level run summary. Mirrors the wire envelope's `result.summary`
/// block (kickstart plan §6) — `by_smell` and `by_severity` are flat
/// fields under `summary` thanks to `#[serde(flatten)]` on
/// `distribution`.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Summary {
    pub total_tests: u32,
    pub total_files: u32,
    pub exceeding_threshold: u32,
    #[serde(flatten)]
    pub distribution: Distribution,
    pub max_scrap_score: f64,
    pub average_scrap_score: f64,
}

/// Top-level domain report. The JSON reporter wraps this in the
/// `schema_version` envelope (constructed at the adapter boundary, not
/// in `domain/`) and flattens `files[].findings` into the flat
/// `result.findings[]` array; markdown/table reporters render `files`
/// directly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub files: Vec<FileReport>,
    pub summary: Summary,
    pub passed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::smell::Smell;
    use crate::domain::types::{QualifiedName, Span, TestIdentity};
    use proptest::prelude::*;

    fn finding_in(
        file: &str,
        category: SmellCategory,
        severity: Severity,
        penalty: u32,
    ) -> Finding {
        let test = TestIdentity::new(
            FilePath::new(file),
            QualifiedName::new("a::tests::t"),
            Span::new(1, 5),
        );
        Finding::new(
            test,
            vec![Smell::new(
                category,
                severity,
                Actionability::AutoRefactor,
                penalty,
            )],
        )
    }

    #[test]
    fn distribution_records_both_axes() {
        let mut d = Distribution::new();
        d.record(SmellCategory::ZeroAssertion, Severity::High);
        d.record(SmellCategory::ZeroAssertion, Severity::High);
        d.record(SmellCategory::LargeExample, Severity::Low);

        assert_eq!(d.by_smell[&SmellCategory::ZeroAssertion], 2);
        assert_eq!(d.by_smell[&SmellCategory::LargeExample], 1);
        assert_eq!(d.by_severity[&Severity::High], 2);
        assert_eq!(d.by_severity[&Severity::Low], 1);
        assert_eq!(d.total(), 3);
    }

    #[test]
    fn distribution_axes_have_equal_totals() {
        let mut d = Distribution::new();
        d.record(SmellCategory::NoOpIo, Severity::Moderate);
        d.record(SmellCategory::SurfaceOnlyIo, Severity::Moderate);
        let smell_total: u32 = d.by_smell.values().sum();
        let sev_total: u32 = d.by_severity.values().sum();
        assert_eq!(smell_total, sev_total);
    }

    #[test]
    fn summary_serializes_flat_by_smell_by_severity() {
        let mut summary = Summary {
            total_tests: 412,
            total_files: 38,
            exceeding_threshold: 3,
            max_scrap_score: 18.0,
            average_scrap_score: 1.2,
            ..Summary::default()
        };
        summary
            .distribution
            .record(SmellCategory::ZeroAssertion, Severity::High);
        summary
            .distribution
            .record(SmellCategory::NoOpIo, Severity::High);
        summary
            .distribution
            .record(SmellCategory::SurfaceOnlyIo, Severity::High);

        let json = serde_json::to_value(&summary).unwrap();
        // Must be FLAT under summary, not nested under "distribution" —
        // matches kickstart plan §6 envelope spec.
        assert!(json.get("distribution").is_none());
        assert_eq!(json["by_smell"]["zero_assertion"], 1);
        assert_eq!(json["by_smell"]["no_op_io"], 1);
        assert_eq!(json["by_smell"]["surface_only_io"], 1);
        assert_eq!(json["by_severity"]["high"], 3);
        assert_eq!(json["total_tests"], 412);
        assert_eq!(json["max_scrap_score"], 18.0);
    }

    #[test]
    fn report_default_is_empty_and_failing_off() {
        let r = Report::default();
        assert!(r.files.is_empty());
        assert_eq!(r.summary.total_tests, 0);
        assert!(!r.passed);
    }

    #[test]
    fn file_report_round_trip_through_json() {
        let fr = FileReport::new(
            FilePath::new("src/lib.rs"),
            vec![finding_in(
                "src/lib.rs",
                SmellCategory::ZeroAssertion,
                Severity::High,
                10,
            )],
        );
        let json = serde_json::to_string(&fr).unwrap();
        let back: FileReport = serde_json::from_str(&json).unwrap();
        assert_eq!(fr, back);
    }

    proptest! {
        #[test]
        fn distribution_total_equals_record_count(
            records in proptest::collection::vec(
                (
                    prop_oneof![
                        Just(SmellCategory::ZeroAssertion),
                        Just(SmellCategory::TautologicalAssertion),
                        Just(SmellCategory::NoOpIo),
                        Just(SmellCategory::SurfaceOnlyIo),
                        Just(SmellCategory::LargeExample),
                    ],
                    prop_oneof![
                        Just(Severity::Low),
                        Just(Severity::Moderate),
                        Just(Severity::High),
                    ],
                ),
                0..50,
            ),
        ) {
            let mut d = Distribution::new();
            for (cat, sev) in &records {
                d.record(*cat, *sev);
            }
            prop_assert_eq!(d.total() as usize, records.len());
            let sev_total: u32 = d.by_severity.values().sum();
            prop_assert_eq!(d.total(), sev_total);
        }
    }
}
