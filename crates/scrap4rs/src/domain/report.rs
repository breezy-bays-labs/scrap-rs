//! Aggregated report types — `Report`, `FileReport`, `ExampleReport`,
//! `Summary`, `Distribution`.
//!
//! `Finding` (in `finding.rs`) is the per-test record that lands flat in
//! the wire envelope. `ExampleReport` is the same per-test record under
//! the file-grouped view used internally by aggregators and reporters
//! that surface findings in source order. The JSON reporter flattens
//! `Report.files` into the envelope's `result.findings[]` array; the
//! markdown and table reporters render the file-grouped view directly.

use crate::domain::finding::{Finding, Severity};
use crate::domain::smell::SmellCategory;
use crate::domain::types::FilePath;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-example (= per-test) report under the file-grouped view. Same
/// payload as `Finding` — kept as a distinct type so the file-grouped
/// and flat-findings views can evolve independently if v0.4 adds
/// per-example metadata that doesn't belong in the wire findings list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ExampleReport {
    pub finding: Finding,
}

impl ExampleReport {
    pub fn new(finding: Finding) -> Self {
        Self { finding }
    }
}

/// Per-file aggregation of examples.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct FileReport {
    pub file_path: FilePath,
    pub examples: Vec<ExampleReport>,
}

impl FileReport {
    pub fn new(file_path: FilePath, examples: Vec<ExampleReport>) -> Self {
        Self {
            file_path,
            examples,
        }
    }
}

/// Counts of findings broken down by smell category and severity.
///
/// Stored as `BTreeMap` for stable ordering on the wire — JSON object
/// keys come out in enum-discriminant order, which produces
/// reproducible snapshots.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
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
/// block (see kickstart plan §6).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Summary {
    pub total_tests: u32,
    pub total_files: u32,
    pub exceeding_threshold: u32,
    pub distribution: Distribution,
    pub max_scrap_score: f64,
    pub average_scrap_score: f64,
}

/// Top-level domain report. The JSON reporter wraps this in the
/// `schema_version` envelope and flattens `files[].examples[].finding`
/// into the flat `result.findings[]` array; other reporters render
/// `files` directly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Report {
    pub files: Vec<FileReport>,
    pub summary: Summary,
    pub passed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::finding::Actionability;
    use crate::domain::smell::Smell;
    use crate::domain::types::{QualifiedName, Span, TestIdentity};
    use proptest::prelude::*;

    fn finding_with(category: SmellCategory, severity: Severity, penalty: u32) -> Finding {
        let test = TestIdentity::new(
            FilePath::new("a.rs"),
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
            vec![ExampleReport::new(finding_with(
                SmellCategory::ZeroAssertion,
                Severity::High,
                10,
            ))],
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
