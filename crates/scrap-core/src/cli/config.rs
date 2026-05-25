//! Project-level TOML config schema for the scrap-rs adapter ecosystem.
//!
//! Public surface (lands incrementally across W1–W6 of the
//! `scrap4rs/scrap-rs-20260524-config-schema` pipeline):
//!
//! - [`FileConfig`] — POD struct tree mirroring `scrap4rs.toml`
//!   (top-level `src`, `exclude`, `extensions`, `[opt_outs]`,
//!   `[detectors]`, `[[overrides]]`). Per `adr-port-surface-and-domain-conventions`
//!   D8, the loaded config is POD; methods beyond `Default::default()`
//!   and serde derives live as free functions outside the struct.
//! - [`discover_config`] — adapter-name-agnostic walk-upward discovery.
//!   `discover_config(start: &Path, file_name: &str)` walks via
//!   `Path::parent()` from `start` until it finds a file with `file_name`
//!   or reaches the filesystem root. The CLI in scrap-rs#21 calls this
//!   with `meta.config_file_name` — the per-adapter literal (the Rust
//!   adapter's `scrap4rs.toml`, the future TS adapter's `scrap4ts.toml`)
//!   lives in the binary crate, never in scrap-core.
//! - [`load_config`] — strict deserialization with
//!   `#[serde(deny_unknown_fields)]` at every level. Returns a POD
//!   `FileConfig` after a `validate_raw_config` pass that surfaces
//!   invalid globs with `<file>:<line>` context (subsumes scrap-rs#34).
//! - [`resolve_detector_for_path`] — pub free function returning the
//!   canonical interpretation of `[[overrides]]` last-match-wins. Both
//!   scrap4rs (#21) and scrap4ts (v0.6+) call this from their CLI merge
//!   paths so the override-resolution rule lives in exactly one place.
//! - [`ConfigError`] — error enum co-located with the loader. Fresh
//!   enum (not a wrap of `crate::ports::source::SourceError`); the
//!   config loader is a different port boundary from the file walker.
//!
//! ## Config-file resolution precedence
//!
//! Owned by the CLI in scrap-rs#21; this module only ships the loader
//! API. Precedence:
//!
//! 1. CLI `--config <path>` flag → `load_config(path)` directly.
//! 2. Otherwise `discover_config(--src, meta.config_file_name)`
//!    → if `Ok(Some(path))`, `load_config(path)`.
//! 3. Otherwise fall back to `FileConfig::default()`.
//!
//! ## Adapter-name-agnostic discipline
//!
//! This module's source and tests contain **zero** double-quoted
//! adapter-binary-name literals. All adapter-name plumbing flows
//! through the `file_name: &str` parameter to `discover_config`. The
//! matching source-only CI gate ships in W7.1; the layer-4 gate in
//! scrap-rs#37 expands the same gate to `tests/`, `tests/features/`,
//! and `tests/cucumber_steps/`.
//!
//! ## Sibling precedent and deliberate divergences
//!
//! Modeled on `crap-rs`'s `crap-core/src/adapters/config.rs`
//! (`load_config` + `discover_config` driven by `meta.config_file_name`).
//! Two deliberate divergences:
//!
//! - **Walk-upward vs CWD-only**: scrap-rs walks upward via
//!   `Path::parent()` (matches `rustfmt` convention); crap-rs walks
//!   only the current working directory. Users running
//!   `scrap4rs --src crates/scrap-core` from a workspace root expect
//!   the loader to find `scrap4rs.toml` at the workspace root, not
//!   require it in each sub-crate.
//! - **Fresh `ConfigError` vs `anyhow::Error`**: scrap-core stays
//!   `anyhow`-free; per-port error enums derive `thiserror::Error`
//!   for typed `#[source]` chaining. The CLI binary in scrap-rs#21
//!   wraps `ConfigError` in its own `anyhow::Error` at the outermost
//!   user-facing boundary.
//!
//! ## `exclude` entries: tracked discipline
//!
//! Per `~/.claude/rules/exclusions.md` and `CONTRIBUTING.md`: every
//! `exclude = [...]` entry in user-authored `scrap4rs.toml` files
//! SHOULD carry an inline `# tracked: <repo>#<n> — <reason>` comment
//! (or `# adr: <path>` if the exclusion is a permanent design
//! decision). Documented in the schema's project-wide CONTRIBUTING
//! guide; surfaced here for visibility.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::opt_outs::OptOut;
use crate::domain::smell::SmellCategory;

/// Project-level config schema parsed from `<adapter>.toml`.
///
/// Plain Old Data per `adr-port-surface-and-domain-conventions` D8 —
/// `Default::default()` + serde derives are the entire method surface.
/// The CLI in scrap-rs#21 owns the merge between this POD and the
/// parsed `Cli` struct to produce the runtime `AnalyzeOptions`.
///
/// Wire shape uses `BTreeMap` for the `detectors` table so that
/// `toml::to_string_pretty(&config)` produces deterministic key order
/// (required by the round-trip property test in W6.1). `Override`
/// uses the same shape for the same reason.
///
/// **Forward-compat**: new top-level fields land with
/// `#[serde(default)]` + `skip_serializing_if = "..."` so existing
/// configs continue to parse cleanly under `deny_unknown_fields`. Per
/// ADR-nested-json-envelope's enum vs struct discipline, the
/// wire-shape *struct* itself does not carry `#[non_exhaustive]`; the
/// loader's `ConfigError` enum does.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    /// Workspace root the analyzer walks. CLI `--src <path>` wins over
    /// this value at merge time in scrap-rs#21.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub src: Option<PathBuf>,

    /// User-supplied exclude globs. Compiled into a negative-override
    /// matcher by `FsWalker::try_new`. Validator pass in `load_config`
    /// rejects invalid globs eagerly with `<file>:<line>` context.
    ///
    /// Per `~/.claude/rules/exclusions.md`: every entry SHOULD carry
    /// an inline `# tracked: <repo>#<n>` or `# adr: <path>` comment in
    /// the user's `scrap4rs.toml`. The loader doesn't enforce this
    /// rule (TOML comments aren't part of the parsed value); the
    /// `CONTRIBUTING.md` doc + this docstring surface the discipline.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,

    /// File extensions the walker keeps. `None` defers to
    /// `AdapterMeta::extensions` set by the binary crate; `Some(v)`
    /// overrides it. `Some(vec![])` means "include all files the
    /// walker visits" (matches `AnalysisConfig::extensions` semantics
    /// in `domain/config.rs`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Vec<String>>,

    /// Project policy on which per-test `#[allow(scrap::*)]`
    /// suppressions are honored. See [`OptOutPolicy`] for the
    /// three-state semantics.
    #[serde(default, skip_serializing_if = "OptOutPolicy::is_empty")]
    pub opt_outs: OptOutPolicy,

    /// Per-smell detector tunables. `BTreeMap` for deterministic
    /// round-trip — `SmellCategory` derives `Ord`; `HashMap` would
    /// produce nondeterministic key order in `toml::to_string_pretty`
    /// output and break the W6.1 round-trip property test.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub detectors: BTreeMap<SmellCategory, DetectorConfig>,

    /// Glob-matched overrides. Last-match-wins per shape R7 — see
    /// [`resolve_detector_for_path`] (lands in W6.1) for the canonical
    /// resolver shared by every adapter binary.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub overrides: Vec<Override>,
}

/// Project policy on per-test `#[allow(scrap::*)]` honor list.
///
/// Three-state semantics on `honor`:
///
/// - `None` — honor every `OptOut` variant. This is the v0.1 default
///   (back-compat with configs predating `[opt_outs]`).
/// - `Some(vec![])` — honor none, the strictest project policy. Any
///   `#[allow(scrap::no_asserts)]` etc. on a test is IGNORED and the
///   smell fires anyway.
/// - `Some(vec![OptOut::NoAsserts, OptOut::NoOp])` — honor only the
///   listed variants. Per-test suppressions for any other variant are
///   IGNORED.
///
/// `is_empty` (named for "no honor list" — see advisory #10 fold-in)
/// returns `true` when the policy is the v0.1 default. The struct
/// stays POD per ADR D8; consumers in #24/#25/#30 read the field
/// when emitting findings.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptOutPolicy {
    /// Honor list. See type-level doc for the three-state semantics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub honor: Option<Vec<OptOut>>,
}

impl OptOutPolicy {
    /// `true` when the policy carries no explicit honor list — i.e.
    /// honors every `OptOut` variant per the v0.1 default. Used in
    /// `FileConfig`'s `skip_serializing_if` to keep round-trip output
    /// minimal for default configs.
    ///
    /// Naming reflects "no honor list" (advisory #10), not "matches
    /// `Default`" — the two are equivalent today but the former
    /// survives future field additions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.honor.is_none()
    }
}

/// Per-smell detector configuration (`[detectors.{smell}]` table).
///
/// All fields are `Option<T>` so the loader can distinguish "user
/// explicitly mentioned this smell" from "user left it to defaults".
/// The CLI merge in scrap-rs#21 supplies v0.1 defaults (`enabled =
/// true`, per-smell penalty from the epic #1 detection table).
///
/// `line_threshold` is only meaningful for
/// `SmellCategory::LargeExample`. The W3.1 validator pass rejects
/// this key on any other smell with `ConfigError::InvalidValue` —
/// `deny_unknown_fields` does NOT cover known-but-semantically-
/// inapplicable values (the field name is recognized; the runtime
/// rejection is the only safety net).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DetectorConfig {
    /// `Some(true)` enables the detector; `Some(false)` disables it.
    /// `None` defers to the v0.1 default (`true` for all detectors in
    /// the epic #1 table).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    /// Penalty contribution when this smell fires. `None` defers to
    /// the v0.1 default. The validator rejects `Some(0)` as
    /// silently-neutering the detector.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub penalty: Option<u32>,

    /// Body-line threshold above which the smell fires. Only
    /// meaningful for `SmellCategory::LargeExample`; the validator
    /// rejects this key on any other smell.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_threshold: Option<u32>,
}

/// Glob-matched override block (`[[overrides]]` array entry).
///
/// Each override matches if ANY of its `r#match` globs match the file
/// path being analyzed (OR-within-match-list). Among multiple matching
/// overrides, the LAST one in document order wins per detector key —
/// see [`resolve_detector_for_path`] for the canonical resolver.
///
/// The `r#match` field uses `#[serde(rename = "match")]` so the TOML
/// surface reads `match = [...]` not `r#match = [...]` (Rust raw-ident
/// syntax doesn't leak into the wire format).
///
/// Same `BTreeMap` rationale as [`FileConfig::detectors`]: deterministic
/// round-trip serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Override {
    /// Path globs (compiled by the validator in `load_config`).
    /// Renamed from `r#match` so the TOML key reads as plain `match`.
    #[serde(rename = "match")]
    pub r#match: Vec<String>,

    /// Per-smell overrides for paths matching any of `r#match`. The
    /// canonical resolver merges these in reverse document order
    /// (last match wins per smell key).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub detectors: BTreeMap<SmellCategory, DetectorConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Smoke: default + module compile ────────────────────────────────

    #[test]
    fn default_config_is_empty_pod() {
        let cfg = FileConfig::default();
        assert!(cfg.src.is_none());
        assert!(cfg.exclude.is_empty());
        assert!(cfg.extensions.is_none());
        assert!(cfg.opt_outs.is_empty());
        assert!(cfg.detectors.is_empty());
        assert!(cfg.overrides.is_empty());
    }

    #[test]
    fn default_round_trips_to_empty_toml() {
        // Default POD has every field at its skip_serializing_if
        // value; serialized form is empty (no top-level keys, no
        // tables). Re-parsed empty TOML restores Default exactly.
        let cfg = FileConfig::default();
        let serialized = toml::to_string(&cfg).unwrap();
        assert_eq!(
            serialized, "",
            "default config should serialize to empty TOML"
        );
        let reparsed: FileConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(cfg, reparsed);
    }

    // ── BTreeMap<SmellCategory, _> key serialization (locked-risk smoke) ─

    #[test]
    fn btreemap_smellcategory_key_deserializes_snake_case() {
        // Validates the shaping doc's flagged risk: BTreeMap<SmellCategory, _>
        // must deserialize from [detectors.zero_assertion] via the existing
        // #[serde(rename_all = "snake_case")] on SmellCategory.
        let toml_str = r"
[detectors.zero_assertion]
enabled = true
penalty = 10
";
        let cfg: FileConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.detectors.len(), 1);
        let cfg_for_zero = cfg
            .detectors
            .get(&SmellCategory::ZeroAssertion)
            .expect("zero_assertion key deserializes");
        assert_eq!(cfg_for_zero.enabled, Some(true));
        assert_eq!(cfg_for_zero.penalty, Some(10));
        assert_eq!(cfg_for_zero.line_threshold, None);
    }

    #[test]
    fn btreemap_smellcategory_key_serializes_snake_case() {
        let mut cfg = FileConfig::default();
        cfg.detectors.insert(
            SmellCategory::TautologicalAssertion,
            DetectorConfig {
                enabled: Some(false),
                penalty: None,
                line_threshold: None,
            },
        );
        let serialized = toml::to_string(&cfg).unwrap();
        assert!(
            serialized.contains("[detectors.tautological_assertion]"),
            "expected snake_case key in output, got:\n{serialized}",
        );
    }

    // ── Override match-keyword rename ──────────────────────────────────

    #[test]
    fn override_match_field_renames_correctly() {
        // The Rust raw-identifier r#match must serialize as plain
        // `match = [...]` so the TOML surface is keyword-free.
        let ov = Override {
            r#match: vec!["tests/**".to_string()],
            detectors: BTreeMap::new(),
        };
        let mut cfg = FileConfig::default();
        cfg.overrides.push(ov);
        let serialized = toml::to_string(&cfg).unwrap();
        assert!(
            serialized.contains("match = ["),
            "expected `match = [` in output, got:\n{serialized}",
        );
        assert!(
            !serialized.contains("r#match"),
            "raw-ident syntax must not leak to TOML, got:\n{serialized}",
        );
        // Round-trip restores the value.
        let reparsed: FileConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(cfg, reparsed);
    }

    // ── OptOutPolicy three-state coverage (MUST-FIX #2) ────────────────

    #[test]
    fn opt_outs_omitted_deserializes_to_none() {
        // No [opt_outs] block → honor stays None (honor-all default).
        let cfg: FileConfig = toml::from_str("").unwrap();
        assert!(cfg.opt_outs.is_empty());
        assert_eq!(cfg.opt_outs.honor, None);
    }

    #[test]
    fn opt_outs_honor_empty_serializes_and_reparses_to_some_empty() {
        // Strictest project policy: honor NO per-test suppressions.
        // Some(vec![]) must NOT collapse to None across round-trip.
        let cfg = FileConfig {
            opt_outs: OptOutPolicy {
                honor: Some(vec![]),
            },
            ..Default::default()
        };
        let serialized = toml::to_string(&cfg).unwrap();
        let reparsed: FileConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(
            reparsed.opt_outs.honor,
            Some(vec![]),
            "Some(empty) must survive round-trip, got: {:?}",
            reparsed.opt_outs.honor,
        );
        assert!(
            !reparsed.opt_outs.is_empty(),
            "Some(vec![]) is NOT empty per is_empty semantics — only None is",
        );
    }

    #[test]
    fn opt_outs_honor_with_variants_round_trips() {
        // Populated honor list survives byte-equivalent round-trip.
        let cfg = FileConfig {
            opt_outs: OptOutPolicy {
                honor: Some(vec![OptOut::NoAsserts, OptOut::NoOp]),
            },
            ..Default::default()
        };
        let serialized = toml::to_string(&cfg).unwrap();
        let reparsed: FileConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(reparsed, cfg);
        assert_eq!(
            reparsed.opt_outs.honor,
            Some(vec![OptOut::NoAsserts, OptOut::NoOp]),
        );
    }
}
