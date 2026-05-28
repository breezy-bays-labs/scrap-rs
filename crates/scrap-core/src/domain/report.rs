//! Aggregated report types — `Report`, `FileReport`, `Summary`,
//! `Distribution`.
//!
//! `Finding` (in `finding.rs`) is the per-test record. `FileReport`
//! groups findings by file for reporters that surface findings in
//! source order (markdown, table); the JSON reporter wraps `Report`
//! in the `schema_version` envelope (constructed at the adapter
//! boundary, not in `domain/`) and preserves `Report.files` as a
//! nested `files[].findings[]` array on the wire. Markdown/table
//! reporters render `files` directly.
//!
//! `Summary` mirrors the wire envelope's `result.summary` shape from
//! kickstart plan §6: `total_tests`, `total_files`, `exceeding_threshold`,
//! `by_smell`, `by_severity` flat under `summary`. `Distribution` is the
//! domain aggregator that owns the two counter maps; `#[serde(flatten)]`
//! lifts its fields onto `Summary` at the wire boundary so the on-disk
//! shape matches the spec.
//!
//! `Summary::from_findings` is the canonical aggregator — both the
//! analyzer pipeline (`result.summary`) and the JSON reporter
//! (`view.shown_summary`) call into it so v0.3's saturating-curve
//! score migration lands atomically across both.

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
    /// Source file these findings originated from.
    pub file_path: FilePath,
    /// Per-test findings inside this file, in source order.
    pub findings: Vec<Finding>,
}

impl FileReport {
    /// Construct a per-file aggregation. Debug-asserts that every
    /// inner finding's `test.file_path` matches the outer `file_path` —
    /// a divergence is always a constructor bug, never a runtime
    /// condition.
    #[must_use]
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
    /// Per-smell-category counter.
    pub by_smell: BTreeMap<SmellCategory, u32>,
    /// Per-severity counter.
    pub by_severity: BTreeMap<Severity, u32>,
}

impl Distribution {
    /// Construct an empty `Distribution` with both axes at zero.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Increment both axes for one detected smell.
    pub fn record(&mut self, category: SmellCategory, severity: Severity) {
        *self.by_smell.entry(category).or_insert(0) += 1;
        *self.by_severity.entry(severity).or_insert(0) += 1;
    }

    /// Total number of recorded smells. Equal to the sum across either
    /// axis (each smell contributes one count to each map).
    #[must_use]
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
    /// Total tests inspected by the analyzer.
    pub total_tests: u32,
    /// Total source files inspected.
    pub total_files: u32,
    /// Number of tests whose `scrap_score` exceeded the active
    /// threshold cutoff.
    pub exceeding_threshold: u32,
    /// Distribution counters; flattened onto `summary` on the wire so
    /// `by_smell` and `by_severity` appear as flat fields under
    /// `summary` per kickstart plan §6.
    #[serde(flatten)]
    pub distribution: Distribution,
    /// Highest `scrap_score` observed across all findings.
    pub max_scrap_score: f64,
    /// Mean `scrap_score` across all inspected tests (zero when none).
    pub average_scrap_score: f64,
}

impl Summary {
    /// Aggregate a slice of finding refs into a `Summary`.
    ///
    /// Canonical aggregator: both the analyzer pipeline
    /// (`result.summary` over the full `Report`) and the JSON
    /// reporter (`view.shown_summary` over the filtered + truncated
    /// view findings) call into this. Co-located here so v0.3's
    /// saturating-curve score upgrade migrates atomically.
    ///
    /// `total_files` is computed from the distinct `file_path` set
    /// across `findings`; `total_tests` is the iterator length;
    /// `exceeding_threshold` counts `Finding::exceeds_threshold` (set
    /// upstream by the analyzer pipeline — the reporter consumes the
    /// computed flag verbatim).
    ///
    /// `tracked: scrap-rs#75` — when CLI scrap-rs#21 lands, the
    /// analyzer pipeline calls this exact function via
    /// `report.summary = Summary::from_findings(report.files.iter().flat_map(|f| &f.findings))`.
    #[must_use]
    pub fn from_findings<'a, I>(findings: I) -> Self
    where
        I: IntoIterator<Item = &'a Finding>,
    {
        // Use HashSet on the path's `&Path` view (which IS `Hash + Eq`)
        // rather than BTreeSet on `&FilePath` (which is deliberately
        // NOT `Ord` to leave wire-byte-order semantics open per memory
        // `feedback_pathbuf-ord-component-not-bytewise`). Uniqueness
        // is all we need; ordering of file-path keys is not surfaced.
        let mut s = Summary::default();
        let mut files: std::collections::HashSet<&std::path::Path> =
            std::collections::HashSet::new();
        let mut score_sum = 0.0_f64;
        for f in findings {
            s.total_tests += 1;
            files.insert(f.test.file_path.as_path());
            if f.exceeds_threshold {
                s.exceeding_threshold += 1;
            }
            if f.scrap_score > s.max_scrap_score {
                s.max_scrap_score = f.scrap_score;
            }
            score_sum += f.scrap_score;
            for smell in &f.smells {
                s.distribution.record(smell.category, smell.severity);
            }
        }
        s.total_files = u32::try_from(files.len()).unwrap_or(u32::MAX);
        s.average_scrap_score = if s.total_tests > 0 {
            score_sum / f64::from(s.total_tests)
        } else {
            0.0
        };
        s
    }
}

/// Top-level domain report. The JSON reporter wraps this in the
/// `schema_version` envelope (constructed at the adapter boundary,
/// not in `domain/`); preserves `Report.files` as a nested
/// `files[].findings[]` array on the wire. Markdown/table reporters
/// render `files` directly.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Report {
    /// Per-file aggregations, in source order.
    pub files: Vec<FileReport>,
    /// Run-level summary across all files.
    pub summary: Summary,
    /// Gate verdict, set by the analyzer pipeline — not by domain
    /// construction. Defaults to `false` on `Report::default()`. The
    /// reporter consumes this value verbatim; it does NOT compute
    /// it. CLI scrap-rs#21 (tracked: scrap-rs#75) owns the
    /// computation (filters `Severity::Advisory` findings per
    /// scrap-rs#72 before comparing `scrap_score` to the
    /// `ThresholdMode` cutoff).
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
            Span::new(1, 5, 1, 1),
        );
        Finding::new(
            test,
            vec![Smell::new(
                category,
                severity,
                Actionability::AutoRefactor,
                penalty,
                None,
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

    // ── Summary::from_findings — domain aggregator (scrap-rs#14) ────

    #[test]
    fn summary_from_findings_aggregates_correctly() {
        // Three findings across two files: a.rs (1 finding, score 10,
        // exceeds), b.rs (2 findings, score 4 + 0, neither exceeds).
        let mut f1 = finding_in("a.rs", SmellCategory::ZeroAssertion, Severity::High, 10);
        f1.exceeds_threshold = true;
        let f2 = finding_in("b.rs", SmellCategory::LargeExample, Severity::Low, 4);
        let mut f3_test = TestIdentity::new(
            FilePath::new("b.rs"),
            QualifiedName::new("b::tests::u"),
            Span::new(2, 6, 1, 1),
        );
        // Build a finding with no smells (scrap_score = 0).
        let _ = &mut f3_test;
        let f3 = Finding::new(f3_test, vec![]);

        let findings = [f1, f2, f3];
        let s = Summary::from_findings(findings.iter());

        assert_eq!(s.total_tests, 3, "three findings counted");
        assert_eq!(s.total_files, 2, "two distinct files (a.rs, b.rs)");
        assert_eq!(s.exceeding_threshold, 1, "only f1 exceeds");
        let max_diff = (s.max_scrap_score - 10.0_f64).abs();
        assert!(max_diff < f64::EPSILON, "max_scrap_score = 10");
        let avg_expected = (10.0_f64 + 4.0 + 0.0) / 3.0;
        let avg_diff = (s.average_scrap_score - avg_expected).abs();
        assert!(avg_diff < f64::EPSILON, "average = (10+4+0)/3");
        assert_eq!(
            s.distribution.by_smell[&SmellCategory::ZeroAssertion],
            1,
            "1 ZeroAssertion smell",
        );
        assert_eq!(
            s.distribution.by_smell[&SmellCategory::LargeExample],
            1,
            "1 LargeExample smell",
        );
        assert_eq!(
            s.distribution.by_severity[&Severity::High],
            1,
            "1 High severity",
        );
        assert_eq!(
            s.distribution.by_severity[&Severity::Low],
            1,
            "1 Low severity",
        );
    }

    #[test]
    fn summary_from_findings_empty_iter_yields_default() {
        let s = Summary::from_findings(std::iter::empty());
        assert_eq!(s, Summary::default());
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
