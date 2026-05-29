//! Per-smell detector modules.
//!
//! Each detector is a free function that takes `&domain::ParsedTest`
//! and returns `Option<domain::Finding>` (or `Vec<Finding>` for
//! multi-finding detectors). Detectors are language-agnostic — they
//! operate on `domain::ParsedTest`, the language-agnostic projection
//! produced by the parser adapter, never on AST library types.
//!
//! Module skeleton (lands as detector PRs ship):
//! - `zero_assertion.rs` — body has no assert*!/`should_panic`/etc.
//!   and no implicit-assertion source (P13) — scrap-rs#30
//! - [`tautological_assertion`] — `assert!(true)`, `assert_eq!(x, x)`,
//!   literal-vs-literal compare (P14) — scrap-rs#24
//! - `no_op_io.rs` — body is `let _ = ...;` with no follow-up check (P15)
//! - `surface_only_io.rs` — `*.exists()` post-create without read-back (P16)
//! - `large_example.rs` — body exceeds configured line threshold (P17)
//!
//! All detectors live in `scrap-core` so every adapter binary
//! inherits them via the linkage; only the parser adapter is
//! language-specific.
//!
//! ## `detect_all` aggregator
//!
//! [`detect_all`] is the analyzer's per-test entry: takes a parsed
//! test + the resolved [`FileConfig`] and returns every [`Smell`]
//! produced by enabled detectors. The function is the single
//! integration point for `core::analyze`'s detector loop — each new
//! detector PR extends `detect_all` by routing through
//! `cli::config::resolve_detector_for_path` for the appropriate
//! `[detectors.<smell>]` table, then calling the detector's
//! `detect(parsed, cfg)` fn. Wired so far: zero-assertion (#30),
//! no-op-io (#25), tautological-assertion (#24, wired at #99). Still
//! to land: surface-only-io (#26), large-example (#31).
//!
//! Per scrap-rs#21 cabinet MF-1, `&FileConfig` is imported from
//! `crate::domain::config` (the POD-types home) NOT from
//! `crate::cli::config` (which now holds loader-only concerns).
//! `detectors/` must never depend on `cli/` per adr-hexagonal-layout.

pub mod no_op_io;
pub mod tautological_assertion;
pub mod zero_assertion;

use crate::domain::behavioral_fact::BehavioralFact;
use crate::domain::config::{FileConfig, resolve_detector_for_path};
use crate::domain::parsed::ParsedTest;
use crate::domain::smell::{Smell, SmellCategory};

/// Shared suppression predicate: does this test carry **any** positive
/// evidence that it observes the system-under-test?
///
/// Returns `true` when the parser recorded an explicit assertion, an
/// implicit-assertion source (runner shell / `#[should_panic]`), or a
/// `.unwrap()`/`.expect()`-style [`BehavioralFact::ResultAsserted`]
/// chain.
///
/// This is the literal proof of the `no-op-io` ⊂ `zero-assertion`
/// subset relationship (SHOULD-FIX #7, scrap-rs#25 cabinet): both
/// detectors suppress on exactly this predicate, so factoring it here
/// keeps them from drifting if a fourth implicit-assertion category is
/// ever added. `zero-assertion` fires when this is `false` (and nothing
/// else is wrong); `no-op-io` fires when this is `false` AND a discard
/// is present — strictly narrower, hence the stacking (Option A,
/// penalties sum) the v0.1 pipeline emits, pending the precedence
/// policy at scrap-rs#32.
#[must_use]
pub(crate) fn has_positive_check(parsed: &ParsedTest) -> bool {
    !parsed.assertions.is_empty()
        || !parsed.implicit_assertion_sources.is_empty()
        || parsed
            .behavioral_facts
            .contains(&BehavioralFact::ResultAsserted)
}

/// Run every enabled detector against `parsed` and return the union
/// of produced [`Smell`]s.
///
/// Per-detector config is resolved via
/// [`resolve_detector_for_path`] — the canonical last-match-wins
/// override resolver shipped with scrap-rs#18 (relocated to
/// `domain::config` in scrap-rs#21 per cabinet MF-1 fold). Each
/// detector receives the resolved `&DetectorConfig` view, not the
/// full `FileConfig`, so the per-detector signature stays narrow.
///
/// Wave-1 of scrap-rs#21 wires only the zero-assertion detector
/// (the sole detector that landed pre-scrap-rs#21 via #30). Future
/// detector PRs (#24/#25/#26/#31) extend this function by appending
/// their detector to the call list — a deliberate "first PR to land
/// owns the signature; subsequent PRs add lines, not rewrite the
/// shape" pattern.
///
/// **Semantic note** (cabinet pre-flag): in v0.1 the only consumer of
/// `&cfg` is `resolve_detector_for_path` → `&DetectorConfig`; the
/// stub-only iterations don't read `cfg` at all. The CLI's duplicate
/// `bootstrap()` call (scrap-rs#NN-9 follow-up) is therefore
/// semantically harmless at v0.1 — both reads produce the same
/// `&DetectorConfig` view used here.
#[must_use]
pub fn detect_all(parsed: &ParsedTest, cfg: &FileConfig) -> Vec<Smell> {
    let mut smells = Vec::new();
    // Zero-assertion (scrap-rs#30 — landed in PR #82).
    let za_cfg = resolve_detector_for_path(
        cfg,
        parsed.identity.file_path.as_path(),
        SmellCategory::ZeroAssertion,
    );
    if let Some(finding) = zero_assertion::detect(parsed, za_cfg) {
        smells.extend(finding.smells);
    }
    // No-op-io (scrap-rs#25 — discarded-Result smell, penalty 8). Strict
    // subset of zero-assertion: an all-discard body co-fires both, and
    // their penalties STACK (Option A; precedence policy deferred to the
    // scrap-rs#32 score aggregator).
    let noop_cfg = resolve_detector_for_path(
        cfg,
        parsed.identity.file_path.as_path(),
        SmellCategory::NoOpIo,
    );
    if let Some(finding) = no_op_io::detect(parsed, noop_cfg) {
        smells.extend(finding.smells);
    }
    // Tautological-assertion (scrap-rs#24 — landed dead-wired in PR #83;
    // wired here at scrap-rs#99). Penalty 10, default. CANNOT co-fire with
    // zero-assertion / no-op-io: a tautological assertion is still a
    // recorded assertion, so `has_positive_check` is true and both of
    // those suppress. Emits only when the body holds ≥1 tautology.
    let tauto_cfg = resolve_detector_for_path(
        cfg,
        parsed.identity.file_path.as_path(),
        SmellCategory::TautologicalAssertion,
    );
    if let Some(finding) = tautological_assertion::detect(parsed, tauto_cfg) {
        smells.extend(finding.smells);
    }
    // (Future detectors append here. #26 surface-only-io / #31
    // large-example each add ~3 lines.)
    smells
}
