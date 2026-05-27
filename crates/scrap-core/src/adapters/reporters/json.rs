//! v0.1 JSON envelope reporter — free function `emit()` that wraps a
//! `Report` in the `schema_version: 1` nested envelope.
//!
//! Wire shape per [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md)
//! (D2). Free-function design per
//! [`crap4rs/adr-free-functions-over-reporter-trait`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/crap4rs/adr-free-functions-over-reporter-trait.md)
//! (D1) and codified in `crates/scrap-core/src/ports/mod.rs:8-13`.
//!
//! ## Envelope shape (v0.1)
//!
//! ```jsonc
//! {
//!   "schema_version": 1,
//!   "tool": "your-adapter",
//!   "tool_version": "0.1.0",
//!   "language": "rust",
//!   "timestamp": "2026-05-26T00:00:00Z",
//!   "threshold_mode": "default",
//!   "result": { /* truthful gate — `Report` verbatim */ },
//!   "view":   { /* shapeable display — filtered/sorted/truncated */ },
//!   "delta":       { /* present iff baseline-diff was run (v0.4+) */ },
//!   "diagnostics": { /* present iff verbose mode populated */ }
//! }
//! ```
//!
//! (The `tool` / `language` / `tool_version` values come from the
//! adapter binary's [`crate::adapter_meta::AdapterMeta`] —
//! scrap-core stays adapter-name-agnostic per the source-only
//! purity CI gate at scrap-rs#18.)
//!
//! `result.*` is the truthful gate — immune to `EmitOptions` reshape.
//! `view.*` carries the filtered display projection. Optional
//! `delta` / `diagnostics` blocks use `Option<T>` +
//! `#[serde(skip_serializing_if = "Option::is_none")]` so output
//! without the feature in use is byte-identical to output with the
//! feature compiled but unused.
//!
//! ## `Report.passed` ownership
//!
//! The reporter consumes `Report.passed` verbatim; it does NOT
//! compute it. The analyzer pipeline (CLI scrap-rs#21, tracked:
//! scrap-rs#75) owns the computation (filters `Severity::Advisory`
//! findings per scrap-rs#72 before comparing `scrap_score` to the
//! `ThresholdMode` cutoff).
//!
//! ## `view.shown_summary` payload-doubling tradeoff
//!
//! `view.shown_summary` is computed via `Summary::from_findings` over
//! the filtered + truncated `view.shown` slice. For the default
//! invocation (no filters), `view.shown_summary` is byte-equivalent
//! to `result.summary` — the envelope payload roughly doubles. The
//! `--minimal-view` escape hatch (tracked: scrap-rs#74, v0.2+) skips
//! the `view.shown` echo when payload size becomes a concern.
//!
//! ## tracked
//!
//! - scrap-rs#73 — `adr-port-surface-and-domain-conventions` ADR
//!   not yet authored; references existing `ports/mod.rs:8-13`
//!   docstring + `adr-nested-json-envelope` as load-bearing.
//! - scrap-rs#74 — `--minimal-view` flag escape hatch for
//!   `view.shown_summary` payload-doubling (cabinet CAO F2);
//!   v0.2+ work, not in this PR.
//! - scrap-rs#75 — `Report::compute_passed(ThresholdMode)` domain
//!   helper; lands with CLI scrap-rs#21.

use crate::adapter_meta::AdapterMeta;
use crate::domain::finding::Finding;
use crate::domain::parsed::ParseDiagnostic;
use crate::domain::report::{Report, Summary};
use crate::domain::source::SourceDiagnostic;
use crate::domain::threshold::ThresholdMode;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

// ────────────────────────────────────────────────────────────────────
// Public input types
// ────────────────────────────────────────────────────────────────────

/// Display-shaping options for the JSON envelope's `view.*` block.
///
/// These reshape only `view.*` — never `result.*`. The truthful-gate
/// guarantee in [`adr-nested-json-envelope`] (D2) is enforced
/// structurally: the envelope's `result` field borrows `&Report`
/// directly; the reporter cannot mutate it.
///
/// `Default::default()` produces "no filter, no truncation" — the
/// default invocation emits `view.shown == result.flat_findings` and
/// `view.shown_summary == result.summary`.
///
/// CLI scrap-rs#21 owns the `Args → EmitOptions` adapter; consumers
/// of `scrap-core` who bypass the CLI surface (library embedders)
/// construct `EmitOptions` directly.
///
/// `tracked: scrap-rs#21` — CLI flag wiring.
/// `tracked: scrap-rs#72` — future severity filters land here.
///
/// [`adr-nested-json-envelope`]: https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md
#[derive(Debug, Clone, Default)]
pub struct EmitOptions {
    /// `--top N`: truncate `view.shown` to N findings (post-filter).
    /// `None` = no truncation. `NonZeroUsize` rules out the `--top 0`
    /// footgun at the type level — CLI scrap-rs#21 validates at parse
    /// time; the reporter's API enforces it too.
    pub top: Option<NonZeroUsize>,
    /// `--only-failing`: drop findings with `scrap_score == 0.0`
    /// from `view.shown`. Does not touch `result.files[].findings`.
    pub only_failing: bool,
}

// ────────────────────────────────────────────────────────────────────
// Public wire-shape types (envelope blocks)
//
// These are public because tests and forward-compat round-trip code
// construct them. They're NOT consumed by callers outside scrap-core
// today; CLI scrap-rs#21 will be the first cross-crate consumer.
// ────────────────────────────────────────────────────────────────────

/// `view.spec` — echoes the resolved [`EmitOptions`] onto the wire so
/// consumers can reconstruct which filters / sort / truncation
/// produced `view.shown`.
///
/// Mirrors `EmitOptions` field-for-field for v0.1. The wire shape
/// uses serde's default `Option<NonZeroUsize>` serialization (plain
/// integer when `Some`, OMITTED via `skip_serializing_if` when
/// `None`) per ADR D2's `Option<T>` policy: optional fields use
/// `#[serde(skip_serializing_if = "Option::is_none")]` so the
/// no-filter case produces a byte-identical wire shape to "feature
/// compiled but unused".
#[derive(Debug, Clone, Default, Serialize)]
pub struct ViewSpec {
    /// Echo of `EmitOptions::top` — omitted from the wire when no
    /// truncation. `Some(n)` emits as the plain integer `n` per
    /// serde's default `NonZeroUsize` Serialize impl.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top: Option<NonZeroUsize>,
    /// Echo of `EmitOptions::only_failing`.
    pub only_failing: bool,
}

/// `view` block — the shapeable display projection.
///
/// Lifetime ties to the source [`Report`]: `shown` borrows
/// `&'a Finding` slices into `report.files[].findings`. Construct
/// via `build_view` (private helper inside this module).
#[derive(Debug, Clone, Serialize)]
pub struct ViewBlock<'a> {
    /// Echo of the `EmitOptions` that produced this view.
    pub spec: ViewSpec,
    /// Post-filter, pre-truncate count of findings.
    pub eligible_count: usize,
    /// `true` when `shown.len() < eligible_count`.
    pub truncated: bool,
    /// Findings in presentation order (filtered + truncated).
    pub shown: Vec<&'a Finding>,
    /// Summary computed over `shown` (paired with `shown` — name
    /// mirrors crap4rs `shown_summary` precedent so consumer parsers
    /// stay convention-stable across the scrap-rs / crap-rs family).
    pub shown_summary: Summary,
}

/// `delta` block — reserved for v0.4 baseline-diff output.
///
/// v0.1 ships the field as `Option<DeltaBlock>` with
/// `skip_serializing_if = "Option::is_none"`; the analyzer pipeline
/// never populates it today. The struct stays empty (gains fields
/// `new`/`removed`/`unchanged`/`verdict` at v0.4 under the existing
/// name — no rename).
///
/// Empty-struct serialization wire-pinned: serializes as `{}` not
/// `null` (locked by `delta_empty_struct_serializes_as_object` in
/// `tests/wire_envelope_snapshot.rs` per cabinet CAO F4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeltaBlock {}

/// `diagnostics` block — verbose-mode pipeline diagnostics.
///
/// v0.1 ships the field as `Option<DiagnosticsBlock>` with
/// `skip_serializing_if = "Option::is_none"`. CLI scrap-rs#21's
/// `--verbose` mode populates it from the analyzer's bookkeeping;
/// v0.1 default is `None`.
///
/// Inner fields skip via `Vec::is_empty` so a verbose-mode-on-but-
/// no-diagnostics envelope emits `"diagnostics": {}` not
/// `"diagnostics": { "source": [], "parse": [] }` (cleaner wire).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiagnosticsBlock {
    /// Source discovery diagnostics (skipped paths, permission errors).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source: Vec<SourceDiagnostic>,
    /// Per-file parse recovery diagnostics.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parse: Vec<ParseDiagnostic>,
}

// ────────────────────────────────────────────────────────────────────
// Private envelope struct
// ────────────────────────────────────────────────────────────────────

/// The actual envelope serialized to the wire. Field declaration
/// order is **load-bearing** per crap4rs D7 — `JsonEnvelope` field
/// order = wire key emit order. The `envelope_field_declaration_order`
/// test in `tests/wire_envelope_snapshot.rs` pins it.
#[derive(Serialize)]
struct JsonEnvelope<'a> {
    schema_version: u32,
    tool: &'a str,
    tool_version: &'a str,
    language: &'a str,
    timestamp: &'a str,
    threshold_mode: ThresholdMode,
    result: &'a Report,
    view: ViewBlock<'a>,
    #[serde(skip_serializing_if = "Option::is_none")]
    delta: Option<DeltaBlock>,
    #[serde(skip_serializing_if = "Option::is_none")]
    diagnostics: Option<&'a DiagnosticsBlock>,
}

// ────────────────────────────────────────────────────────────────────
// Public emit function
// ────────────────────────────────────────────────────────────────────

/// Serialize a `Report` into the v0.1 JSON envelope and write to
/// `writer`.
///
/// Constructs the envelope per [`adr-nested-json-envelope`] (D2),
/// flattens `report.files[].findings` into a `view.shown` borrowed
/// slice (after `options.only_failing` filter + `options.top`
/// truncation), and writes pretty-printed JSON via
/// [`serde_json::to_writer_pretty`].
///
/// **`Report.passed` is consumed verbatim** — the reporter does NOT
/// compute it. The analyzer pipeline (CLI scrap-rs#21, tracked:
/// scrap-rs#75) owns the computation.
///
/// # Errors
///
/// Returns [`serde_json::Error`] in two cases:
///
/// - **Writer I/O failure** — wrapped via [`serde_json::Error::io`]
///   (the underlying [`std::io::Error`] is recoverable via
///   [`std::error::Error::source`]).
/// - **Serialization failure** — extremely rare with the closed
///   envelope shape; would indicate a serde derive bug. Today's
///   envelope has no fallible serializers, so this path is
///   effectively unreachable.
///
/// # Examples
///
/// ```ignore
/// use scrap_core::adapter_meta::AdapterMeta;
/// use scrap_core::adapters::reporters::json::{emit, EmitOptions};
/// use scrap_core::domain::report::Report;
/// use scrap_core::domain::threshold::ThresholdMode;
///
/// let meta = AdapterMeta {
///     tool: "your-adapter",
///     language: "rust",
///     tool_version: env!("CARGO_PKG_VERSION"),
///     config_file_name: "your-adapter.toml",
/// };
/// let report = Report::default();
/// emit(
///     &report,
///     &meta,
///     &EmitOptions::default(),
///     "2026-05-26T00:00:00Z",
///     ThresholdMode::Default,
///     &mut std::io::stdout(),
/// )?;
/// # Ok::<_, serde_json::Error>(())
/// ```
///
/// [`adr-nested-json-envelope`]: https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md
pub fn emit<W: std::io::Write>(
    report: &Report,
    meta: &AdapterMeta,
    options: &EmitOptions,
    timestamp: &str,
    threshold_mode: ThresholdMode,
    writer: &mut W,
) -> Result<(), serde_json::Error> {
    let view = build_view(report, options);
    let envelope = JsonEnvelope {
        schema_version: 1,
        tool: meta.tool,
        tool_version: meta.tool_version,
        language: meta.language,
        timestamp,
        threshold_mode,
        result: report,
        view,
        delta: None,
        diagnostics: None,
    };
    serde_json::to_writer_pretty(writer, &envelope)
}

// ────────────────────────────────────────────────────────────────────
// Private helpers
// ────────────────────────────────────────────────────────────────────

/// Build the `view.*` block from a `Report` + `EmitOptions`.
///
/// Order of operations (fixed per crap4rs D7 view-abstraction
/// precedent): filter → truncate → summarize. `eligible_count` is
/// post-filter / pre-truncate so consumers can reconstruct
/// "showing X of Y" framing.
fn build_view<'a>(report: &'a Report, options: &EmitOptions) -> ViewBlock<'a> {
    // Flatten files[].findings into a borrowed slice in source order.
    let all_findings: Vec<&'a Finding> = report
        .files
        .iter()
        .flat_map(|f| f.findings.iter())
        .collect();

    // Filter (only_failing drops scrap_score == 0.0).
    let mut filtered: Vec<&'a Finding> = if options.only_failing {
        all_findings
            .into_iter()
            .filter(|f| f.scrap_score > 0.0)
            .collect()
    } else {
        all_findings
    };

    // eligible_count = post-filter, pre-truncate.
    let eligible_count = filtered.len();

    // Truncate. `Vec::truncate` is a no-op when `n >= len`, so the
    // length guard is unnecessary — drop it for readability.
    if let Some(top) = options.top {
        filtered.truncate(top.get());
    }

    let truncated = filtered.len() < eligible_count;
    let shown_summary = Summary::from_findings(filtered.iter().copied());
    let spec = ViewSpec {
        top: options.top,
        only_failing: options.only_failing,
    };

    ViewBlock {
        spec,
        eligible_count,
        truncated,
        shown: filtered,
        shown_summary,
    }
}

// (No custom `Option<NonZeroUsize>` serializer needed — serde's
// default Serialize impl on `NonZeroUsize` already emits the inner
// primitive, and `skip_serializing_if = "Option::is_none"` on the
// field omits the key entirely when `None`. Dropped on bot review;
// see PR #77 disposition comment.)

// ────────────────────────────────────────────────────────────────────
// Unit tests — focus on `build_view()` semantics (CABINET MUST-FIX
// CQO #1). Wire-shape pins live in `tests/wire_envelope_snapshot.rs`.
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::float_cmp)] // exact-integer-derived scores in fixtures
mod tests {
    use super::*;
    use crate::domain::classification::{Actionability, Severity};
    use crate::domain::finding::Finding;
    use crate::domain::report::{FileReport, Report};
    use crate::domain::smell::{Smell, SmellCategory};
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};

    /// Build a `Finding` for `path` with one smell at the given
    /// `penalty`. `penalty = 0` produces a zero-score finding (used
    /// to verify `only_failing` filter behavior).
    fn finding_at(path: &str, name: &str, penalty: u32) -> Finding {
        let test = TestIdentity::new(
            FilePath::new(path),
            QualifiedName::new(name),
            Span::new(1, 5),
        );
        if penalty == 0 {
            // Zero-score finding: no smells, so scrap_score = 0.0.
            Finding::new(test, vec![])
        } else {
            Finding::new(
                test,
                vec![Smell::new(
                    SmellCategory::ZeroAssertion,
                    Severity::High,
                    Actionability::AutoRefactor,
                    penalty,
                    None,
                )],
            )
        }
    }

    /// Build a `Report` with `findings` (each pair: path, score).
    /// Groups by path to honor `FileReport::new`'s debug-assert that
    /// inner findings reference the outer `file_path`.
    fn report_with(findings: Vec<(&str, &str, u32)>) -> Report {
        use std::collections::BTreeMap;
        let mut by_path: BTreeMap<&str, Vec<Finding>> = BTreeMap::new();
        for (path, name, penalty) in findings {
            by_path
                .entry(path)
                .or_default()
                .push(finding_at(path, name, penalty));
        }
        let files = by_path
            .into_iter()
            .map(|(path, fs)| FileReport::new(FilePath::new(path), fs))
            .collect();
        Report {
            files,
            ..Report::default()
        }
    }

    // ── view-logic mutation-kill tests (CABINET MUST-FIX CQO #1) ────

    #[test]
    fn view_top_truncates_and_sets_truncated_flag() {
        let report = report_with(vec![
            ("a.rs", "a::tests::t1", 10),
            ("b.rs", "b::tests::t2", 10),
            ("c.rs", "c::tests::t3", 10),
        ]);
        let options = EmitOptions {
            top: Some(NonZeroUsize::new(2).unwrap()),
            only_failing: false,
        };
        let view = build_view(&report, &options);
        assert_eq!(view.shown.len(), 2, "top=2 truncates to 2");
        assert!(view.truncated, "truncated=true when shown < eligible");
        assert_eq!(
            view.eligible_count, 3,
            "eligible_count = total pre-truncate"
        );
    }

    #[test]
    fn view_top_no_truncate_when_n_gte_eligible() {
        let report = report_with(vec![
            ("a.rs", "a::tests::t1", 10),
            ("b.rs", "b::tests::t2", 10),
            ("c.rs", "c::tests::t3", 10),
        ]);
        let options = EmitOptions {
            top: Some(NonZeroUsize::new(5).unwrap()),
            only_failing: false,
        };
        let view = build_view(&report, &options);
        assert_eq!(view.shown.len(), 3, "top=5 keeps all 3");
        assert!(!view.truncated, "truncated=false when shown >= eligible");
        assert_eq!(view.eligible_count, 3);
    }

    #[test]
    fn view_only_failing_filters_zero_score() {
        // Mixed: one zero-score, two scoring.
        let report = report_with(vec![
            ("a.rs", "a::tests::t1", 10),
            ("b.rs", "b::tests::t2", 0), // zero-score — filtered out
            ("c.rs", "c::tests::t3", 4),
        ]);
        let options = EmitOptions {
            top: None,
            only_failing: true,
        };
        let view = build_view(&report, &options);
        assert_eq!(view.shown.len(), 2, "zero-score filtered out");
        assert!(
            view.shown.iter().all(|f| f.scrap_score > 0.0),
            "all shown have scrap_score > 0",
        );
        // result.files[].findings still has the zero-score one — truthful gate.
        let total_in_result: usize = report.files.iter().map(|f| f.findings.len()).sum();
        assert_eq!(
            total_in_result, 3,
            "result.files unfiltered (truthful gate)"
        );
    }

    #[test]
    fn view_combined_filter_then_truncate() {
        let report = report_with(vec![
            ("a.rs", "a::tests::t1", 10),
            ("b.rs", "b::tests::t2", 0),
            ("c.rs", "c::tests::t3", 4),
        ]);
        let options = EmitOptions {
            top: Some(NonZeroUsize::new(1).unwrap()),
            only_failing: true,
        };
        let view = build_view(&report, &options);
        assert_eq!(
            view.eligible_count, 2,
            "post-filter count = 2 (zero-score dropped)"
        );
        assert_eq!(view.shown.len(), 1, "then top=1 truncates to 1");
        assert!(view.truncated, "1 < 2 → truncated");
    }

    #[test]
    fn view_eligible_count_is_post_filter_pre_truncate() {
        // N=5 findings, 2 zero-score; only_failing+top=2.
        let report = report_with(vec![
            ("a.rs", "a::tests::t1", 10),
            ("b.rs", "b::tests::t2", 0),
            ("c.rs", "c::tests::t3", 4),
            ("d.rs", "d::tests::t4", 0),
            ("e.rs", "e::tests::t5", 6),
        ]);
        let options = EmitOptions {
            top: Some(NonZeroUsize::new(2).unwrap()),
            only_failing: true,
        };
        let view = build_view(&report, &options);
        assert_eq!(
            view.eligible_count, 3,
            "eligible = post-filter (3 scoring findings), NOT 5 (pre-filter), NOT 2 (post-truncate)",
        );
        assert_eq!(view.shown.len(), 2);
        assert!(view.truncated);
    }

    // ── Default-EmitOptions invariants ────────────────────────────

    #[test]
    fn emit_options_default_is_no_filter_no_truncate() {
        let opts = EmitOptions::default();
        assert!(opts.top.is_none(), "default top = None");
        assert!(!opts.only_failing, "default only_failing = false");
    }

    #[test]
    fn view_default_options_passes_through_findings() {
        let report = report_with(vec![
            ("a.rs", "a::tests::t1", 10),
            ("b.rs", "b::tests::t2", 0),
        ]);
        let view = build_view(&report, &EmitOptions::default());
        assert_eq!(view.shown.len(), 2, "no filter — all findings present");
        assert_eq!(view.eligible_count, 2);
        assert!(!view.truncated);
        assert_eq!(view.spec.top, None);
        assert!(!view.spec.only_failing);
    }

    // ── ViewSpec top serialization ────────────────────────────────

    #[test]
    fn view_spec_top_serializes_as_plain_integer() {
        let spec = ViewSpec {
            top: Some(NonZeroUsize::new(5).unwrap()),
            only_failing: false,
        };
        let value = serde_json::to_value(&spec).unwrap();
        assert_eq!(value["top"], 5, "top serializes as plain integer 5");
        assert_eq!(value["only_failing"], false);
    }

    #[test]
    fn view_spec_top_none_omitted_via_skip_serializing_if() {
        let spec = ViewSpec {
            top: None,
            only_failing: false,
        };
        let value = serde_json::to_value(&spec).unwrap();
        let obj = value.as_object().expect("spec serializes as object");
        assert!(
            !obj.contains_key("top"),
            "top=None must be omitted via skip_serializing_if (ADR D2 Option<T> policy); got: {value}",
        );
        // only_failing remains present (NOT an Option).
        assert_eq!(value["only_failing"], false);
    }
}
