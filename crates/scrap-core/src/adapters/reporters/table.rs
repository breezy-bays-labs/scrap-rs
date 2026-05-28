//! v0.1 comfy-table terminal reporter — free function `emit()` that
//! renders a `Report` as a human-readable table for the default
//! `--format table` CLI dispatch (scrap-rs#16).
//!
//! Sibling to [`crate::adapters::reporters::json`] — same free-function
//! pattern per
//! [`crap4rs/adr-free-functions-over-reporter-trait`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/crap4rs/adr-free-functions-over-reporter-trait.md)
//! (D1) and codified in `crates/scrap-core/src/ports/mod.rs:8-13`.
//!
//! ## Public surface (v0.1)
//!
//! - `emit` — free function rendering a `Report` to any
//!   `std::io::Write`. Header (from [`crate::adapter_meta::AdapterMeta`]
//!   identity + summary counts) → table body (dispatched on
//!   [`RowGrouping`]) → footer (`PASSED`/`FAILED` + threshold-mode).
//!   Lands in W5; the public function definition is the entry point
//!   the CLI scrap-rs#21 dispatch calls.
//! - [`TableOptions`] — display-shaping options (`top`, `only_failing`,
//!   `use_color`, `grouping`). Mirrors `json::EmitOptions` field shape
//!   for `top` + `only_failing` so the CLI #21 boundary maps one
//!   `clap::Args` block to both reporter options.
//! - [`RowGrouping`] — non-exhaustive enum dispatching renderer
//!   choice. Default `Smell` (one row per `Smell`); `Finding`
//!   collapses multi-smell tests into one row. Serializable for
//!   config-file override (`[table] grouping = "smell|finding"` in
//!   `scrap4rs.toml`).
//!
//! ## tracked
//!
//! - scrap-rs#73 — `adr-port-surface-and-domain-conventions` ADR
//!   not yet authored; references existing `ports/mod.rs:8-13`
//!   docstring + `adr-free-functions-over-reporter-trait` as
//!   load-bearing.
//! - SF-1 from /plan close cabinet — 1-LOC duplication of
//!   `if options.only_failing && finding.scrap_score == 0.0 { continue; }`
//!   between `render_smell_rows` and `render_finding_rows`. Kept
//!   inline (cabinet CAO SF-1 verdict: NO LIFT). Re-evaluate if a
//!   third reporter recurs the same shape.

use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

// ────────────────────────────────────────────────────────────────────
// Public input types
// ────────────────────────────────────────────────────────────────────

/// Row grouping strategy for the table reporter.
///
/// Locked at scrap-rs#16 D-GROUPING-1. The default ([`Self::Smell`])
/// matches the issue body contract; [`Self::Finding`] collapses
/// multi-Smell tests into one row at the cost of per-rule
/// attribution detail. Future variants (`File`, `Severity`,
/// `Detector`, etc.) slot in without breaking external
/// pattern-matchers via `#[non_exhaustive]`.
///
/// Config-file override flows through serde (`[table] grouping =
/// "smell|finding"` in scrap4rs.toml deserializes via kebab-case).
/// CLI override is added by scrap-rs#21 (clap `ValueEnum` derive
/// lands in that PR — not here, because scrap-core does not yet
/// list `clap` in its `[dependencies]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RowGrouping {
    /// One row per Smell — `file:line / smell / severity / penalty / score`.
    Smell,
    /// One row per Finding (test) — `File / Test / Smells / Score / Pass-Fail`.
    Finding,
}

impl Default for RowGrouping {
    fn default() -> Self {
        Self::Smell
    }
}

/// Display-shaping options for the table reporter.
///
/// Mirrors the json reporter's `EmitOptions` field shape (`top` +
/// `only_failing`) and adds two table-specific fields (`use_color` +
/// `grouping`). Separate struct (not a shared base) because reporters
/// will evolve their presentation needs independently (scrap-rs#16
/// D-OPT-1).
///
/// `Default::default()` produces "no filter, no truncation, no color,
/// per-Smell grouping" — the default invocation behavior.
///
/// CLI scrap-rs#21 owns the `Args → TableOptions` adapter; consumers
/// of `scrap-core` who bypass the CLI surface (library embedders)
/// construct `TableOptions` directly.
#[derive(Debug, Clone, Default)]
pub struct TableOptions {
    /// `--top N`: truncate the rendered rows to N (post-filter,
    /// counted as **rows** — not Findings — so a multi-Smell Finding
    /// under [`RowGrouping::Smell`] may be partially shown). `None` =
    /// no truncation. `NonZeroUsize` rules out the `--top 0` footgun
    /// at the type level (mirrors `json::EmitOptions::top`).
    pub top: Option<NonZeroUsize>,
    /// `--only-failing`: drop Findings whose `scrap_score == 0.0`
    /// before grouping. Same semantics as
    /// `json::EmitOptions::only_failing`.
    pub only_failing: bool,
    /// Color output toggle. CLI binary boundary resolves `--color
    /// auto|always|never` to a concrete bool via
    /// `std::io::IsTerminal` (stable Rust 1.70; MSRV 1.93 — no crate
    /// dep needed). Reporter takes the resolved value. `false` =
    /// plain ASCII output, no ANSI escapes.
    pub use_color: bool,
    /// Row grouping strategy. See [`RowGrouping`].
    pub grouping: RowGrouping,
}

// ────────────────────────────────────────────────────────────────────
// Unit tests — types only (W2). Renderer + emit tests land in W3–W6.
// ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── RowGrouping ────────────────────────────────────────────────

    #[test]
    fn row_grouping_default_is_smell() {
        assert_eq!(RowGrouping::default(), RowGrouping::Smell);
    }

    #[test]
    fn row_grouping_serializes_kebab_case() {
        let cases = [
            (RowGrouping::Smell, "smell"),
            (RowGrouping::Finding, "finding"),
        ];
        for (grouping, wire) in cases {
            assert_eq!(
                serde_json::to_value(grouping).unwrap(),
                serde_json::Value::String(wire.into()),
            );
        }
    }

    #[test]
    fn row_grouping_deserializes_kebab_case() {
        let smell: RowGrouping =
            serde_json::from_value(serde_json::Value::String("smell".into())).unwrap();
        assert_eq!(smell, RowGrouping::Smell);

        let finding: RowGrouping =
            serde_json::from_value(serde_json::Value::String("finding".into())).unwrap();
        assert_eq!(finding, RowGrouping::Finding);
    }

    #[test]
    fn row_grouping_round_trips_through_toml() {
        // Exercises the config-file override path:
        // `[table] grouping = "finding"` in scrap4rs.toml.
        #[derive(serde::Deserialize)]
        struct Section {
            grouping: RowGrouping,
        }
        let parsed: Section = toml::from_str("grouping = \"finding\"\n").unwrap();
        assert_eq!(parsed.grouping, RowGrouping::Finding);
    }

    // ── TableOptions ───────────────────────────────────────────────

    #[test]
    fn table_options_default_no_filter_no_truncate_no_color_smell_grouping() {
        let opts = TableOptions::default();
        assert!(opts.top.is_none(), "default top = None");
        assert!(!opts.only_failing, "default only_failing = false");
        assert!(!opts.use_color, "default use_color = false");
        assert_eq!(opts.grouping, RowGrouping::Smell);
    }
}
