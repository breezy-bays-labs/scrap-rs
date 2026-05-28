//! Caller-supplied workspace analysis configuration + project-level
//! POD config schema types parsed from `<adapter>.toml`.
//!
//! Two distinct POD shapes live here:
//!
//! - [`AnalysisConfig`] — the runtime walker-config the CLI hands to
//!   `SourcePort::discover_test_files`. Built from the merge of the
//!   parsed `Cli` + `FileConfig` (in `cli::bootstrap`); consumed by
//!   `FsWalker::try_new`. Pre-existing since scrap-rs#13.
//! - [`FileConfig`] + [`OptOutPolicy`] + [`DetectorConfig`] +
//!   [`Override`] — the `<adapter>.toml` schema. Pre-existing
//!   POD-tree shipped in scrap-rs#18; **relocated** to this module
//!   in scrap-rs#21 per cabinet MF-1 fold so `detectors/` and `core/`
//!   can depend on the type without violating
//!   `adr-hexagonal-layout` (detectors/ + core/ may not depend on
//!   cli/). The loader pipeline (`load_config`, `discover_config`,
//!   `resolve_detector_for_path`, `ConfigError`, private
//!   `RawConfig`/`RawOverride` mirrors, `byte_offset_to_line`,
//!   `validate_*` helpers) stays in `cli::config` — it carries the
//!   I/O + validation concerns that are CLI-edge by nature.
//!
//! `cli::config` re-exports the POD types via
//! `pub use crate::domain::config::{FileConfig, OptOutPolicy,
//! DetectorConfig, Override};` so existing `use cli::config::FileConfig`
//! imports keep compiling without a flag-day update. Direct
//! imports from `domain::config` are preferred for new code.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::domain::opt_outs::OptOut;
use crate::domain::smell::SmellCategory;
use crate::domain::types::SourceRoot;

// ────────────────────────────────────────────────────────────────────────
// AnalysisConfig — runtime walker-config (pre-existing since scrap-rs#13)
// ────────────────────────────────────────────────────────────────────────

/// Caller-supplied configuration for one analysis run.
///
/// `src` carries the workspace root the adapter walks; `exclude` is a
/// list of raw user globs the adapter compiles into a negative
/// override matcher; `extensions` is the bare-extension whitelist
/// (`["rs"]` for `scrap4rs`, `["ts", "tsx", ...]` for future
/// `scrap4ts`); `respect_gitignore` toggles `.gitignore` /
/// `.ignore` / `.git/info/exclude` honouring at the walk layer.
///
/// Construction is infallible — the canonical `::new` constructor
/// stores the raw `exclude` strings without compilation. Glob
/// validation lives in `FsWalker::try_new`, which surfaces invalid
/// patterns as `SourceError::InvalidGlob`. Keeping validation in the
/// adapter keeps `domain/` free of `globset::Error` (per shaping
/// Shape A — adapter-owns-validation).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnalysisConfig {
    /// Workspace root passed into `SourcePort::discover_test_files`.
    pub src: SourceRoot,
    /// Raw user-supplied exclude globs. Adapter compiles to an
    /// `ignore::overrides::Override` at `try_new` time.
    pub exclude: Vec<String>,
    /// Bare extensions (no leading dot, case-insensitive) the walker
    /// keeps. Empty list means include every file the walker visits.
    pub extensions: Vec<String>,
    /// When `true`, the walker honours `.gitignore`, `.ignore`, and
    /// `.git/info/exclude` (rg/fd default behaviour).
    pub respect_gitignore: bool,
}

impl AnalysisConfig {
    /// Canonical constructor (D10). Infallible — adapter validates
    /// `exclude` patterns when building the walker.
    #[must_use]
    pub fn new(
        src: SourceRoot,
        exclude: Vec<String>,
        extensions: Vec<String>,
        respect_gitignore: bool,
    ) -> Self {
        Self {
            src,
            exclude,
            extensions,
            respect_gitignore,
        }
    }
}

// ────────────────────────────────────────────────────────────────────────
// FileConfig POD tree (relocated from cli/config.rs in scrap-rs#21
// per cabinet MF-1 fold)
// ────────────────────────────────────────────────────────────────────────

/// Project-level config schema parsed from `<adapter>.toml`.
///
/// Plain Old Data per `adr-port-surface-and-domain-conventions` D8 —
/// `Default::default()` + serde derives are the entire method surface.
/// The CLI in scrap-rs#21 owns the merge between this POD and the
/// parsed `Cli` struct to produce the runtime [`AnalysisConfig`].
///
/// Wire shape uses `BTreeMap` for the `detectors` table so that
/// `toml::to_string_pretty(&config)` produces deterministic key order
/// (required by the round-trip property test in
/// `crates/scrap-core/src/cli/config.rs` loader tests). `Override`
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
    /// walker visits" (matches [`AnalysisConfig::extensions`]
    /// semantics).
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
    /// output and break the round-trip property test.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub detectors: BTreeMap<SmellCategory, DetectorConfig>,

    /// Glob-matched overrides. Last-match-wins per shape R7 — see
    /// `cli::config::resolve_detector_for_path` for the canonical
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
/// `SmellCategory::LargeExample`. The loader's validator pass rejects
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
/// see `cli::config::resolve_detector_for_path` for the canonical
/// resolver.
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
    /// Path globs (compiled by the validator in `cli::config::load_config`).
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

    // ── AnalysisConfig (pre-existing test) ─────────────────────────────

    #[test]
    fn analysis_config_wire_keys() {
        let cfg = AnalysisConfig::new(
            SourceRoot::new("crates/scrap-core"),
            vec!["vendored/**".into()],
            vec!["rs".into()],
            true,
        );
        let json = serde_json::to_value(&cfg).unwrap();
        for key in ["src", "exclude", "extensions", "respect_gitignore"] {
            assert!(json.get(key).is_some(), "missing wire key: {key}");
        }
    }

    // ── FileConfig POD shape tests (relocated from cli/config.rs::tests
    //    in scrap-rs#21 per cabinet MF-1 fold; bodies unchanged) ────────

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
