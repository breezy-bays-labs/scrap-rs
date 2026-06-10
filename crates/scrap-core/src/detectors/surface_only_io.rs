//! `surface-only-io` detector (scrap-rs#26) — the first **correlation**
//! detector. Flags `#[test]` bodies that write to (or create) a file and
//! then check only its *surface* (existence / kind / length) without
//! ever reading the content back.
//!
//! ## Detection rule (v0.1)
//!
//! Fires when, in order:
//!
//! 1. `cfg.enabled != Some(false)` — config can disable per-detector.
//! 2. There exists a `path_key` for which the body's
//!    [`BehavioralFact`] bag carries a
//!    [`BehavioralFact::FilesystemWrite`] **and** a
//!    [`BehavioralFact::FilesystemSurfaceCheck`] **and NO**
//!    [`BehavioralFact::FilesystemRead`]. I.e.
//!    `∃ key . has_write(key) ∧ has_surface(key) ∧ ¬has_read(key)`.
//!
//! The detector groups the three located filesystem fact families by
//! their adapter-computed `path_key`, then asks the per-key question
//! above. A read-back for the same key — `fs::read_to_string(p)`,
//! `File::open(p)`, etc. — disarms the smell for that key: the test
//! inspected the substantive payload, not just the surface.
//!
//! ## Why correlation, not presence
//!
//! Unlike the v0.1 presence detectors (`zero-assertion`, `no-op-io`,
//! `large-example`), `surface-only-io` cannot decide from a single fact:
//! a write is fine, a surface check is fine, a read is fine. The smell is
//! the *relationship* — write-then-surface-check-without-read on the
//! **same path**. This realises D4 of
//! [`adr-port-surface-and-domain-conventions`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap-rs/adr-port-surface-and-domain-conventions.md):
//! the adapter emits **located** write/check/read events (D1/D2/D3 — each
//! carries a `path_key` identity + a `path_arg_span` position the adapter
//! computes); the core composes the correlation + verdict here.
//!
//! ### Fail-closed correlation: poison-on-uncertainty
//!
//! A path key correlates a write with a check **only** when the adapter
//! can prove both sites name the same value. Two sources of `opaque:<N>`
//! keys (each call site gets a *distinct* one, so they never group):
//!
//! 1. **Unresolvable path expressions** — `format!(..)`, `concat!(..)`,
//!    a field path, an interprocedural value.
//! 2. **Poisoned path keys** — the poison pre-pass
//!    (`scrap4rs::parser::body::PoisonScanner`) is **fail-closed**: it
//!    poisons any path key (a bound/used **identifier** OR a
//!    **string-literal** `lit:<value>`) that is multiply-bound, `mut`, an
//!    assignment target, OR **appears in a token region the walker did not
//!    fully analyze** (an in-macro rebind, a non-assertion macro such as
//!    `matches!`/`vec!`/`dbg!`/an unknown custom macro, or the unparseable
//!    tail of a partial assertion parse). A poisoned key resolves to a
//!    fresh `opaque:<N>` at every use.
//!
//! **FP-safety for unanalyzed contexts (by construction):** a poisoned
//! key's fresh, globally-unique opaque keys can never group with any other
//! fact, so poisoning *strictly removes* correlations — it is incapable of
//! *causing* a false positive. Because the poison harvest covers every
//! region the fact-walk does not analyze (the partition is shared via
//! `split_assertion_macro_args`, so the two walkers agree by construction)
//! and harvests **both** path-key shapes the resolver consults
//! (identifiers and string literals — the literal arm was the
//! scrap-rs#26-round-3 bypass), no false positive can arise from a read
//! hidden in an unanalyzed context. The cost is the conservative
//! direction: a path mentioned in an unanalyzed region (e.g.
//! `dbg!(read("/tmp/x"))` or `println!("{}", p)`) is suppressed even if it
//! is a genuine surface-only-io — an accepted recall tradeoff, tracked at
//! [scrap-rs#119](https://github.com/breezy-bays-labs/scrap-rs/issues/119).
//!
//! **What this does NOT cover (a disclosed, by-design residual).** The
//! guarantee above is scoped to *unanalyzed contexts*. It does NOT make
//! the detector FP-free in general. The recognized read set (widened at
//! [scrap-rs#120](https://github.com/breezy-bays-labs/scrap-rs/issues/120))
//! covers the common std and std-adjacent idioms: `fs::read` /
//! `fs::read_to_string`, `File::open`,
//! `BufReader::new(File::open(..))`, `tokio::fs::read*` /
//! `async_std`-style modules (any `fs::` container segment), `fs_err::*`
//! (incl. `fs_err::File::open` via the `File` container match), and
//! `OpenOptions::new().read(true).open(p)` builder chains. What remains
//! — **deliberately, as a static single-body analysis** — is the
//! interprocedural class: a custom read helper (`load(p)`,
//! `read_fixture(p)`), an ident-level aliased re-import
//! (`use std::fs::read as slurp;`), or an extension-trait `.read*()` on
//! a handle whose origin the body doesn't reveal. A genuine read-back
//! through one of those still over-fires. This stance is a scope
//! decision, not an oversight: resolving helper bodies or file-level
//! `use` aliases is interprocedural/whole-file analysis the v0.x parser
//! does not do, and the bounded shape (write + surface-check + helper
//! read of the SAME path) keeps the residual a tail case. (Note the
//! `.read*()`-on-write-only-handle variant is not a real read at
//! runtime — a write-only handle cannot read content — so origin-site
//! recognition of read-capable opens covers every visible-origin case.)
//!
//! ## Suppression reconciliation — does NOT consult `has_positive_check`
//!
//! `surface-only-io` deliberately does **not** call
//! `super::has_positive_check`, and the parser must **not** fold
//! filesystem facts into it. Rationale: `assert!(p.exists())` records a
//! [`crate::domain::parsed::ParsedAssertion`] (which suppresses
//! `zero-assertion` — correct, the test *does* assert) AND projects a
//! [`BehavioralFact::FilesystemSurfaceCheck`] (which fires
//! `surface-only-io` — correct, it only checked existence). **Both
//! verdicts are right simultaneously**: a test can assert and still only
//! look at the surface. Coupling the two would let an honest
//! surface-only `assert!` silence this detector, defeating its purpose.
//!
//! ## Orthogonal to the assertion-based smells (stacks)
//!
//! Like `large-example`, this detector reads only its own fact families
//! and never suppresses / is suppressed by the assertion-based smells.
//! A write-then-surface-check body that ALSO fails to assert anything
//! co-fires `surface-only-io` (6) AND `zero-assertion` (10), stacking in
//! `detectors::detect_all` (Option A; precedence policy deferred to the
//! scrap-rs#32 score aggregator).
//!
//! ## Pure-detector convention
//!
//! Mirrors the sibling detectors: does NOT consult `parsed.opt_outs`.
//! Per-test `#[allow(scrap::surface_only_io)]` honor-policy is the
//! pipeline driver's job (scrap-rs#72), applied post-emission.

use std::collections::HashMap;

use crate::domain::behavioral_fact::BehavioralFact;
use crate::domain::classification::{Actionability, Severity};
use crate::domain::config::DetectorConfig;
use crate::domain::finding::Finding;
use crate::domain::parsed::ParsedTest;
use crate::domain::smell::{Smell, SmellCategory};

/// Default penalty per the CLAUDE.md / kickstart-plan detection table.
pub(crate) const DEFAULT_PENALTY: u32 = 6;

/// Default severity: between `no-op-io`'s `Moderate` (penalty 8) and
/// `large-example`'s `Low` (penalty 4) — the table places `surface-only-io`
/// at penalty 6, so `Moderate` is the right bucket.
const DEFAULT_SEVERITY: Severity = Severity::Moderate;

/// Default actionability: the mechanical fix is to read the content back
/// and assert on it (an automatable transformation).
const DEFAULT_ACTIONABILITY: Actionability = Actionability::AutoRefactor;

/// Per-`path_key` accumulation of which filesystem fact families were
/// observed. Built in one pass over `parsed.behavioral_facts`.
#[derive(Default)]
struct KeyObservations {
    has_write: bool,
    has_surface: bool,
    has_read: bool,
}

impl KeyObservations {
    /// `true` when this key shows the surface-only-io shape: a write and
    /// a surface check, but no content read-back.
    fn fires(&self) -> bool {
        self.has_write && self.has_surface && !self.has_read
    }
}

/// Detect the `surface-only-io` smell on a parsed test.
///
/// See module-level docs for the correlation rule + the
/// suppression-reconciliation and opaque-key notes. Returns:
/// - `None` when the detector is disabled, or when no `path_key` shows
///   the write-and-surface-but-no-read shape.
/// - `Some(Finding)` carrying one [`Smell`] whose
///   `category = SmellCategory::SurfaceOnlyIo`,
///   `severity = Severity::Moderate`,
///   `actionability = Actionability::AutoRefactor`, and
///   `penalty = cfg.penalty.unwrap_or(DEFAULT_PENALTY)`.
#[must_use]
pub fn detect(parsed: &ParsedTest, cfg: &DetectorConfig) -> Option<Finding> {
    if cfg.enabled == Some(false) {
        return None;
    }
    if !has_surface_only_key(parsed) {
        return None;
    }

    let penalty = cfg.penalty.unwrap_or(DEFAULT_PENALTY);
    // Whole-test span: "this test only checked the surface" is a fn-level
    // verdict, so the smell points at `parsed.identity.span` (the full
    // `fn name(...) { .. }`) like the sibling detectors, not at any single
    // call site. (Per-`path_key` located spans ride on the underlying
    // facts and stay available for a future per-key reporter.)
    let smell = Smell::new(
        SmellCategory::SurfaceOnlyIo,
        DEFAULT_SEVERITY,
        DEFAULT_ACTIONABILITY,
        penalty,
        Some(parsed.identity.span),
    );
    Some(Finding::new(parsed.identity.clone(), vec![smell]))
}

/// `true` when at least one `path_key` in the body's behavioral-fact bag
/// shows the surface-only-io shape (write + surface check + no read).
///
/// Single pass: bucket the three located filesystem fact families by
/// `path_key`, then ask each bucket [`KeyObservations::fires`]. The
/// `HashMap` iteration order never affects the result — the output is a
/// single existential bool, so the detector stays deterministic
/// (idempotence proptest below).
fn has_surface_only_key(parsed: &ParsedTest) -> bool {
    let mut by_key: HashMap<&str, KeyObservations> = HashMap::new();
    for fact in &parsed.behavioral_facts {
        match fact {
            BehavioralFact::FilesystemWrite { path_key, .. } => {
                by_key.entry(path_key).or_default().has_write = true;
            }
            BehavioralFact::FilesystemSurfaceCheck { path_key, .. } => {
                by_key.entry(path_key).or_default().has_surface = true;
            }
            BehavioralFact::FilesystemRead { path_key, .. } => {
                by_key.entry(path_key).or_default().has_read = true;
            }
            // ResultAsserted / ResultDiscarded carry no path_key and are
            // irrelevant to the correlation — other detectors own them.
            _ => {}
        }
    }
    by_key.values().any(KeyObservations::fires)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::behavioral_fact::{FsCallKind, FsReadKind, FsSurfaceCheckKind};
    use crate::domain::parsed::ParsedAssertion;
    use crate::domain::types::{FilePath, QualifiedName, Span, TestIdentity};
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    // ── Test helpers ────────────────────────────────────────────────────

    const SPAN: Span = Span {
        start_line: 2,
        end_line: 2,
        start_column: 5,
        end_column: 12,
    };

    fn write(key: &str) -> BehavioralFact {
        BehavioralFact::FilesystemWrite {
            kind: FsCallKind::Write,
            path_key: key.into(),
            path_arg_span: SPAN,
        }
    }

    fn surface(key: &str) -> BehavioralFact {
        BehavioralFact::FilesystemSurfaceCheck {
            kind: FsSurfaceCheckKind::Exists,
            path_key: key.into(),
            path_arg_span: SPAN,
        }
    }

    fn read(key: &str) -> BehavioralFact {
        BehavioralFact::FilesystemRead {
            kind: FsReadKind::ReadToString,
            path_key: key.into(),
            path_arg_span: SPAN,
        }
    }

    /// Build a `ParsedTest` carrying exactly `facts` — all other fields
    /// empty. The detector reads only `behavioral_facts` + `identity.span`.
    fn test_with_facts(facts: Vec<BehavioralFact>) -> ParsedTest {
        ParsedTest::new(
            TestIdentity::new(
                FilePath::new("a.rs"),
                QualifiedName::new("a::tests::t"),
                Span::new(1, 5, 1, 1),
            ),
            Vec::new(),
            Vec::new(),
            3,
            Vec::new(),
            BTreeSet::new(),
            facts,
        )
    }

    // ── Negative branches: detector returns None ────────────────────────

    #[test]
    fn detect_returns_none_when_disabled_via_config() {
        let cfg = DetectorConfig {
            enabled: Some(false),
            penalty: None,
            line_threshold: None,
        };
        let pt = test_with_facts(vec![write("lit:p"), surface("lit:p")]);
        assert!(detect(&pt, &cfg).is_none());
    }

    #[test]
    fn detect_returns_none_for_empty_facts() {
        assert!(detect(&test_with_facts(Vec::new()), &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_for_write_only() {
        // A write with no surface check is not the smell.
        let pt = test_with_facts(vec![write("lit:p")]);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_for_surface_only() {
        // A surface check with no write is not the smell.
        let pt = test_with_facts(vec![surface("lit:p")]);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    // ── HEADLINE read-back guard ────────────────────────────────────────

    #[test]
    fn detect_returns_none_when_content_is_read_back() {
        // write + read-back of the SAME key, no surface check → the test
        // inspects the substantive payload → NO fire. (Models
        // `fs::write(p, b"x"); assert_eq!(fs::read_to_string(p)?, "x")`.)
        let pt = test_with_facts(vec![write("lit:p"), read("lit:p")]);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_when_write_surface_and_read_all_present() {
        // The read-back DISARMS the smell even when a surface check is
        // also present on the same key (write + exists() + read_to_string()).
        let pt = test_with_facts(vec![write("lit:p"), surface("lit:p"), read("lit:p")]);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    // ── Positive branches ────────────────────────────────────────────────

    #[test]
    #[allow(clippy::float_cmp)]
    fn detect_fires_on_write_then_surface_check_without_read() {
        let pt = test_with_facts(vec![write("lit:p"), surface("lit:p")]);
        let finding = detect(&pt, &DetectorConfig::default()).expect("write+surface fires");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(finding.smells[0].category, SmellCategory::SurfaceOnlyIo);
        assert_eq!(finding.smells[0].penalty, 6);
        assert_eq!(finding.smells[0].severity, Severity::Moderate);
        assert_eq!(finding.smells[0].actionability, Actionability::AutoRefactor);
        assert_eq!(finding.scrap_score, 6.0);
        // Whole-test span attribution.
        assert_eq!(finding.smells[0].span, Some(Span::new(1, 5, 1, 1)));
    }

    #[test]
    fn detect_fires_on_tempfile_surface_only() {
        // A temp file IS created on disk (FilesystemWrite{Tempfile}); its
        // handle is checked via `.path().exists()` and never read → fires.
        let key = "tempfile-handle:0";
        let pt = test_with_facts(vec![
            BehavioralFact::FilesystemWrite {
                kind: FsCallKind::Tempfile,
                path_key: key.into(),
                path_arg_span: SPAN,
            },
            surface(key),
        ]);
        assert!(detect(&pt, &DetectorConfig::default()).is_some());
    }

    #[test]
    fn detect_fires_on_metadata_length_only_check() {
        // A length-only `metadata().len()` projects FsSurfaceCheckKind::Metadata
        // (NOT a read). write + length-check + no content read → fires.
        let pt = test_with_facts(vec![
            write("lit:p"),
            BehavioralFact::FilesystemSurfaceCheck {
                kind: FsSurfaceCheckKind::Metadata,
                path_key: "lit:p".into(),
                path_arg_span: SPAN,
            },
        ]);
        assert!(detect(&pt, &DetectorConfig::default()).is_some());
    }

    #[test]
    fn detect_applies_custom_penalty_override() {
        let cfg = DetectorConfig {
            enabled: None,
            penalty: Some(25),
            line_threshold: None,
        };
        let pt = test_with_facts(vec![write("lit:p"), surface("lit:p")]);
        let finding = detect(&pt, &cfg).expect("fires under override");
        assert_eq!(finding.smells.len(), 1);
        assert_eq!(finding.smells[0].penalty, 25);
    }

    // ── Correlation isolation: keys must not cross-correlate ────────────

    #[test]
    fn detect_returns_none_when_write_and_surface_are_different_keys() {
        // A write to key A and a surface check on key B do NOT correlate:
        // neither key has BOTH a write and a surface check.
        let pt = test_with_facts(vec![write("lit:a"), surface("lit:b")]);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_returns_none_when_read_back_on_other_key_writes_unread_key_has_no_surface() {
        // key A: write + read (clean). key B: write only (no surface).
        // No key shows write+surface+no-read.
        let pt = test_with_facts(vec![write("lit:a"), read("lit:a"), write("lit:b")]);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    #[test]
    fn detect_fires_when_one_of_several_keys_is_surface_only() {
        // key A is clean (write+read); key B is surface-only (write+surface,
        // no read) → fires on B even though A is fine.
        let pt = test_with_facts(vec![
            write("lit:a"),
            read("lit:a"),
            write("lit:b"),
            surface("lit:b"),
        ]);
        assert!(detect(&pt, &DetectorConfig::default()).is_some());
    }

    #[test]
    fn detect_returns_none_for_distinct_opaque_keys() {
        // Two DISTINCT opaque keys (the adapter stamps a fresh N per
        // unresolvable site) never group: opaque:0 has only a write,
        // opaque:1 has only a surface check → no correlation → no fire.
        let pt = test_with_facts(vec![write("opaque:0"), surface("opaque:1")]);
        assert!(detect(&pt, &DetectorConfig::default()).is_none());
    }

    // ── Suppression reconciliation (design point #14) ───────────────────

    #[test]
    fn detect_fires_even_when_an_assertion_is_present() {
        // `assert!(p.exists())` records a ParsedAssertion (suppressing
        // zero-assertion) AND projects a FilesystemSurfaceCheck. The
        // surface-only-io detector must STILL fire: the test asserts, but
        // only on the surface. This pins that the detector does NOT consult
        // has_positive_check.
        let mut pt = test_with_facts(vec![write("lit:p"), surface("lit:p")]);
        pt.assertions.push(ParsedAssertion::new(
            "assert",
            Some("p.exists()".into()),
            Span::new(3, 3, 1, 1),
            false,
            None,
        ));
        // Sanity: a positive check IS present.
        assert!(super::super::has_positive_check(&pt));
        // ...and the detector fires anyway.
        assert!(detect(&pt, &DetectorConfig::default()).is_some());
    }

    // ── Property tests ────────────────────────────────────────────────────

    /// Arbitrary `behavioral_facts` over a SMALL key alphabet (so writes,
    /// surfaces, and reads frequently collide on the same key) plus the
    /// two non-located variants (which must never affect the verdict).
    fn arb_facts() -> impl Strategy<Value = Vec<BehavioralFact>> {
        let key = prop_oneof![Just("k0"), Just("k1"), Just("opaque:0"), Just("opaque:1")];
        let fact = (0u8..5, key).prop_map(|(tag, k)| match tag {
            0 => write(k),
            1 => surface(k),
            2 => read(k),
            3 => BehavioralFact::ResultAsserted,
            _ => BehavioralFact::ResultDiscarded {
                kind: crate::domain::behavioral_fact::ResultDiscardKind::Call,
            },
        });
        proptest::collection::vec(fact, 0..8)
    }

    fn arb_parsed_test() -> impl Strategy<Value = ParsedTest> {
        arb_facts().prop_map(test_with_facts)
    }

    fn arb_detector_config() -> impl Strategy<Value = DetectorConfig> {
        (
            proptest::option::of(any::<bool>()),
            proptest::option::of(1u32..1_000),
        )
            .prop_map(|(enabled, penalty)| DetectorConfig {
                enabled,
                penalty,
                line_threshold: None,
            })
    }

    proptest! {
        /// Determinism (the AC's idempotence intent — the literal
        /// `detect(detect(ast))` doesn't typecheck given
        /// `detect : &ParsedTest -> Option<Finding>`; the pure-function
        /// contract is what this captures, matching the sibling detectors'
        /// PR-note translation). Also pins that the `HashMap` grouping's
        /// iteration order never leaks into the verdict.
        #[test]
        fn proptest_detect_is_deterministic(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            prop_assert_eq!(detect(&pt, &cfg), detect(&pt, &cfg));
        }

        /// Cardinality: result is `None` or a single-Smell `Finding`.
        #[test]
        fn proptest_detect_emits_at_most_one_smell(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            if let Some(finding) = detect(&pt, &cfg) {
                prop_assert_eq!(finding.smells.len(), 1);
            }
        }

        /// Read-back monotonicity (the key AC): when the baseline fires,
        /// adding a `FilesystemRead` for EVERY firing key flips it to
        /// `None`. We add a read for both small keys (`k0`, `k1`) — any
        /// firing key is one of those (the opaque keys can't fire because
        /// each opaque site is distinct in real projection; in the
        /// strategy `opaque:0`/`opaque:1` are reused but a read on them
        /// disarms them too, so covering all four keys is safe and
        /// strictly stronger).
        #[test]
        fn proptest_adding_read_for_all_keys_suppresses(
            pt in arb_parsed_test(),
            cfg in arb_detector_config(),
        ) {
            if detect(&pt, &cfg).is_none() {
                return Ok(());
            }
            let mut pt_read = pt.clone();
            for k in ["k0", "k1", "opaque:0", "opaque:1"] {
                pt_read.behavioral_facts.push(read(k));
            }
            prop_assert!(
                detect(&pt_read, &cfg).is_none(),
                "a read-back on every key must disarm surface-only-io",
            );
        }
    }
}
