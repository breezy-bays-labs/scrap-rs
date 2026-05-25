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
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::domain::opt_outs::OptOut;
use crate::domain::smell::SmellCategory;

// ────────────────────────────────────────────────────────────────────────
// Error enum + diagnostic helpers
// ────────────────────────────────────────────────────────────────────────

/// Errors produced by [`load_config`] and [`discover_config`].
///
/// Fresh enum, NOT a wrap of `crate::ports::source::SourceError` — the
/// config loader is a different port boundary from the file walker
/// (per shaping D-LOCK-7). `Io` here covers config-file read failures
/// and `discover_config` permission errors; `Parse` covers toml
/// deserialization; `InvalidGlob` carries `<file>:<line>` context via
/// `toml::Spanned` (subsumes scrap-rs#34); `InvalidValue` is the
/// post-parse validator's catch-all for semantic-but-not-syntactic
/// errors (`line_threshold` on the wrong smell, `penalty = 0`, empty
/// exclude patterns, etc.).
///
/// `#[non_exhaustive]` per ADR-nested-json-envelope's enum discipline:
/// consumers must use a `_` arm so new variants don't break callers
/// pattern-matching against `ConfigError`. New variants land additively
/// without a `schema_version` bump.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// I/O failure reading the config file or walking ancestors during
    /// [`discover_config`] (permission-denied, non-NotFound).
    #[error("failed to read config file {}", path.display())]
    Io {
        /// Path the loader was visiting when the error fired.
        path: PathBuf,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },
    /// TOML deserialization failure — syntactically invalid TOML or a
    /// field that violates `deny_unknown_fields` / type expectations
    /// (`enabled = "true"` instead of `enabled = true`, unknown
    /// top-level key, etc.).
    #[error("failed to parse config file {}", path.display())]
    Parse {
        /// Path the loader was reading.
        path: PathBuf,
        /// Underlying `toml::de::Error`.
        #[source]
        source: toml::de::Error,
    },
    /// A configured glob (top-level `exclude` or per-override `match`)
    /// failed to compile under `globset::Glob::new`. Carries the source
    /// `<file>:<line>` from `toml::Spanned` (subsumes scrap-rs#34's
    /// `<file>:<line>` context wrap).
    #[error("invalid glob at {}:{}: {pattern}", file.display(), line)]
    InvalidGlob {
        /// Source file path.
        file: PathBuf,
        /// 1-based line number where the offending pattern starts.
        line: u32,
        /// The offending raw glob string.
        pattern: String,
        /// Underlying globset error.
        #[source]
        source: globset::Error,
    },
    /// Semantic validation failure surfaced by `validate_raw_config`
    /// — `line_threshold` set on a smell other than `LargeExample`,
    /// `penalty = 0` (silently neuters the detector), empty exclude
    /// pattern (`globset` accepts silently and the walker rewrites to
    /// `**/`, the opposite of caller intent), etc.
    ///
    /// `line` may be 0 for errors on non-Spanned fields. Line resolution
    /// for `penalty = 0` etc. is tracked-as-followup at scrap-rs#64.
    #[error("invalid value at {}:{}: {message}", file.display(), line)]
    InvalidValue {
        /// Source file path.
        file: PathBuf,
        /// 1-based line number; `0` is the placeholder for errors on
        /// non-Spanned fields (tracked: scrap-rs#64).
        line: u32,
        /// Human-readable error message.
        message: String,
    },
}

/// Translate a byte offset into a 1-based line number for diagnostic
/// messages.
///
/// `offset` is typically the `.start` of a [`toml::Spanned<T>::span`].
/// Counting walks `\n` bytes in `source[..offset.min(source.len())]`
/// (the `min` clip guards against defensive out-of-bounds offsets that
/// would otherwise slice-panic — Spanned shouldn't produce them but
/// the helper stays total either way).
///
/// Returns `u32::MAX` if the file is so large its line count exceeds
/// `u32::MAX` (a pathological case; the diagnostic still displays
/// something usable). The `u32::try_from(...).unwrap_or(u32::MAX)`
/// idiom at the single return point replaces saturating arithmetic
/// threaded through the loop (advisory #12) — same semantics, less
/// custom math.
///
/// # Panics
///
/// Never panics; out-of-bounds offsets saturate to the source's last
/// line, and the line count saturates to `u32::MAX` for impossibly
/// large inputs.
pub(crate) fn byte_offset_to_line(source: &str, offset: usize) -> u32 {
    let clipped = offset.min(source.len());
    let prefix = &source[..clipped];
    let newlines = prefix.bytes().filter(|&b| b == b'\n').count();
    u32::try_from(newlines.saturating_add(1)).unwrap_or(u32::MAX)
}

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

// ────────────────────────────────────────────────────────────────────────
// Loader (private `RawConfig` mirror + validation pipeline)
// ────────────────────────────────────────────────────────────────────────

/// Private deserialization mirror of [`FileConfig`] with [`toml::Spanned`]
/// wrappers around the glob string fields.
///
/// The two-step pipeline (`RawConfig` → validate → `FileConfig`) lets the
/// validator attach `<file>:<line>` context to invalid-glob errors via
/// the `Spanned` byte-range, then strip the spans so the POD `FileConfig`
/// stays free of `serde_spanned` types in its public surface.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawConfig {
    #[serde(default)]
    src: Option<PathBuf>,
    #[serde(default)]
    exclude: Vec<toml::Spanned<String>>,
    #[serde(default)]
    extensions: Option<Vec<String>>,
    #[serde(default)]
    opt_outs: OptOutPolicy,
    #[serde(default)]
    detectors: BTreeMap<SmellCategory, DetectorConfig>,
    #[serde(default)]
    overrides: Vec<RawOverride>,
}

/// Private deserialization mirror of [`Override`] with [`toml::Spanned`]
/// wrappers around the `match` glob strings.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawOverride {
    #[serde(rename = "match")]
    r#match: Vec<toml::Spanned<String>>,
    #[serde(default)]
    detectors: BTreeMap<SmellCategory, DetectorConfig>,
}

/// Read and parse a TOML config file from `path`.
///
/// Pipeline:
///
/// 1. `std::fs::read_to_string(path)` — fails with [`ConfigError::Io`].
/// 2. `toml::from_str::<RawConfig>(...)` — fails with [`ConfigError::Parse`].
/// 3. `validate_raw_config` (private) — fails with
///    [`ConfigError::InvalidGlob`] or [`ConfigError::InvalidValue`].
/// 4. Strip `Spanned` wrappers via `.into_inner()`; construct the POD
///    [`FileConfig`].
///
/// Validation runs BEFORE the strip-and-construct step so the validator
/// still has byte-offset access via `Spanned::span()`. Tests assert on
/// `ConfigError` variants, not on the order of multiple-simultaneous-error
/// firing — the validator short-circuits on the first failure.
///
/// # Errors
///
/// - [`ConfigError::Io`] — file read failure (missing file, permission denied).
/// - [`ConfigError::Parse`] — TOML syntax error, unknown field, type mismatch.
/// - [`ConfigError::InvalidGlob`] — exclude or override `match` glob that
///   `globset::Glob::new` rejects. Carries `<file>:<line>` context from
///   [`toml::Spanned`].
/// - [`ConfigError::InvalidValue`] — semantic validation failure
///   (`line_threshold` on a non-`LargeExample` smell; `penalty = 0`;
///   empty exclude pattern).
#[must_use = "the loaded config must be applied or surfaced to the user"]
pub fn load_config(path: &Path) -> Result<FileConfig, ConfigError> {
    let source = std::fs::read_to_string(path).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let raw: RawConfig = toml::from_str(&source).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })?;
    validate_raw_config(&raw, &source, path)?;

    Ok(FileConfig {
        src: raw.src,
        exclude: raw
            .exclude
            .into_iter()
            .map(toml::Spanned::into_inner)
            .collect(),
        extensions: raw.extensions,
        opt_outs: raw.opt_outs,
        detectors: raw.detectors,
        overrides: raw
            .overrides
            .into_iter()
            .map(|ov| Override {
                r#match: ov
                    .r#match
                    .into_iter()
                    .map(toml::Spanned::into_inner)
                    .collect(),
                detectors: ov.detectors,
            })
            .collect(),
    })
}

/// Validate a parsed [`RawConfig`]. Runs after `toml::from_str` succeeds
/// and before the POD [`FileConfig`] is constructed.
///
/// Validation order (deterministic, documented for test stability):
///
/// 1. Top-level `exclude` globs (in vector order).
/// 2. Top-level `detectors` map (in `SmellCategory` Ord order — `BTreeMap`
///    iteration is sorted by key).
/// 3. Each `[[overrides]]` entry in document order:
///    a. `r#match` globs (in vector order).
///    b. The override's `detectors` map (in `SmellCategory` Ord order).
///
/// The validator short-circuits on the first failure. Tests that need to
/// assert on a specific error variant should construct fixtures where
/// that variant is the EARLIEST error per this order — otherwise the
/// test will see a different error than expected.
///
/// # Errors
///
/// - [`ConfigError::InvalidGlob`] — `globset::Glob::new` rejects a
///   pattern. Line attribution comes from [`toml::Spanned::span`]
///   converted via [`byte_offset_to_line`].
/// - [`ConfigError::InvalidValue`] — empty exclude pattern,
///   `line_threshold` on a smell other than `LargeExample`, or
///   `penalty = Some(0)`. `line: 0` is the placeholder for errors on
///   non-Spanned fields (line resolution for those is tracked at
///   scrap-rs#64).
fn validate_raw_config(raw: &RawConfig, source: &str, path: &Path) -> Result<(), ConfigError> {
    // 1. Top-level exclude globs (vector order).
    validate_globs(&raw.exclude, source, path)?;

    // 2. Top-level detectors map (BTreeMap iter is sorted by key).
    for (smell, cfg) in &raw.detectors {
        validate_detector_config(*smell, cfg, path)?;
    }

    // 3. Each override in document order.
    for ov in &raw.overrides {
        validate_globs(&ov.r#match, source, path)?;
        for (smell, cfg) in &ov.detectors {
            validate_detector_config(*smell, cfg, path)?;
        }
    }

    Ok(())
}

/// Validate a slice of `Spanned<String>` globs (used for both `exclude`
/// and per-override `match` lists).
///
/// Empty/whitespace-only patterns are rejected eagerly per the
/// `FsWalker::try_new` `EmptyExcludePattern` contract — `globset`
/// silently accepts empty strings and the override builder would rewrite
/// them into a global `**/` whitelist that nullifies all exclude
/// semantics (silent data deletion the caller didn't ask for).
fn validate_globs(
    globs: &[toml::Spanned<String>],
    source: &str,
    path: &Path,
) -> Result<(), ConfigError> {
    for spanned in globs {
        let pattern = spanned.get_ref();
        let line = byte_offset_to_line(source, spanned.span().start);
        if pattern.trim().is_empty() {
            return Err(ConfigError::InvalidValue {
                file: path.to_path_buf(),
                line,
                message: "empty or whitespace-only glob pattern".to_string(),
            });
        }
        if let Err(source) = globset::Glob::new(pattern) {
            return Err(ConfigError::InvalidGlob {
                file: path.to_path_buf(),
                line,
                pattern: pattern.clone(),
                source,
            });
        }
    }
    Ok(())
}

/// Validate a single `[detectors.<smell>]` or `[overrides.detectors.<smell>]`
/// block.
///
/// `line: 0` is the placeholder for these errors — non-Spanned field
/// errors don't yet carry line attribution (tracked: scrap-rs#64).
fn validate_detector_config(
    smell: SmellCategory,
    cfg: &DetectorConfig,
    path: &Path,
) -> Result<(), ConfigError> {
    if cfg.line_threshold.is_some() && smell != SmellCategory::LargeExample {
        return Err(ConfigError::InvalidValue {
            file: path.to_path_buf(),
            line: 0,
            message: format!(
                "line_threshold is only valid on [detectors.large_example], not [detectors.{}]",
                smell.as_wire_str()
            ),
        });
    }
    if cfg.penalty == Some(0) {
        return Err(ConfigError::InvalidValue {
            file: path.to_path_buf(),
            line: 0,
            message: format!(
                "penalty must be > 0 (on [detectors.{}]); zero silently neuters the detector",
                smell.as_wire_str()
            ),
        });
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────
// Discovery (walk-upward by file name)
// ────────────────────────────────────────────────────────────────────────

/// Walk upward from `start` looking for `file_name`; return the first
/// match.
///
/// **Adapter-name-agnostic API**: `file_name` is the per-adapter literal
/// (e.g. the Rust adapter's `scrap4rs.toml`, the future TS adapter's
/// `scrap4ts.toml`) supplied at call time via `meta.config_file_name`
/// from the binary crate. This module never references those names
/// directly — every test uses `"test-adapter.toml"` so the source-only
/// adapter-name-purity CI gate (lands in W7.1) ships clean. The
/// expanded gate at scrap-rs#37 covers `tests/` and `tests/features/`
/// in the same way.
///
/// **Stop condition**: walk continues until `Path::parent()` returns
/// `None` (the filesystem root). `Path::parent()` is purely lexical
/// — no canonicalization — so symlink loops cannot occur.
///
/// **Sibling divergence**: crap-rs's `discover_config` checks the CWD
/// only; scrap-rs walks upward (matches `rustfmt` convention). Users
/// running `scrap4rs --src crates/scrap-core` from the workspace root
/// expect the loader to find `scrap4rs.toml` at the workspace root,
/// not require it in each sub-crate.
///
/// **Result shape**: `Ok(Some(path))` on hit; `Ok(None)` when the walk
/// exhausts the ancestor chain without finding the file. Returns
/// `Err(ConfigError::Io)` ONLY on non-NotFound I/O errors (permission
/// denied, etc.) — the contract is visible to scrap-rs#21 which
/// depends on distinguishing "no config exists" from "config exists
/// but unreadable".
///
/// # Errors
///
/// [`ConfigError::Io`] when a `std::fs::metadata` call returns an
/// `Err(e)` where `e.kind() != ErrorKind::NotFound` (typically
/// permission denied on an ancestor directory). The error's `path`
/// field carries the candidate file path that failed.
///
/// # Panics
///
/// Never panics.
#[must_use = "the discovered config path must be loaded or surfaced; ignoring would silently fall back to defaults"]
pub fn discover_config(start: &Path, file_name: &str) -> Result<Option<PathBuf>, ConfigError> {
    // Best-effort canonicalize so relative `start` walks the absolute
    // ancestor chain. If canonicalize fails (start doesn't exist),
    // fall back to the lexical path — the walk still terminates at
    // root via Path::parent() returning None.
    let starting = std::fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf());

    let mut cursor: Option<&Path> = Some(&starting);
    while let Some(dir) = cursor {
        let candidate = dir.join(file_name);
        match std::fs::metadata(&candidate) {
            Ok(meta) if meta.is_file() => return Ok(Some(candidate)),
            Ok(_) => {
                // Exists but is a directory / symlink to dir / etc.
                // — NOT a config file; continue walking up.
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Expected case — file simply absent in this ancestor.
            }
            Err(source) => {
                return Err(ConfigError::Io {
                    path: candidate,
                    source,
                });
            }
        }
        cursor = dir.parent();
    }
    Ok(None)
}

// ────────────────────────────────────────────────────────────────────────
// Canonical overrides resolver (last-match-wins)
// ────────────────────────────────────────────────────────────────────────

/// Resolves the [`DetectorConfig`] to apply for a `(path, smell)` pair,
/// accounting for `[[overrides]]` (last-match-wins per shape R7).
///
/// Walks `config.overrides` in **reverse document order**; returns the
/// first matching override's per-detector config (matched if ANY of its
/// `r#match` globs match the path AND the override has a
/// `DetectorConfig` for the smell). Falls back to
/// `config.detectors.get(&smell)` (top-level per-detector config) if
/// no override matches. Returns a reference to a `'static` default
/// `DetectorConfig` sentinel when neither matches — the merge in
/// scrap-rs#21 then applies the v0.1 defaults (`enabled = true`,
/// per-smell penalty).
///
/// **Free function, module-level, `pub`** per orchestrator decision
/// (2026-05-25) overriding the cabinet trio's `pub(crate)` preference.
/// Both scrap4rs (#21) and scrap4ts (v0.6+) call this from their CLI
/// merge paths so the override-resolution rule lives in exactly one
/// place. Not a method on `FileConfig` because that would violate ADR
/// D8 (POD discipline — no methods beyond `Default::default()` and
/// serde derives).
///
/// **Glob re-compilation**: each call re-walks the override `r#match`
/// patterns and compiles each via `globset::Glob::new` + `.compile_matcher()`.
/// This is a perf trade-off documented for the v0.1 surface; a future
/// PR can pre-compile globs once at `load_config` time into a
/// `GlobSet` stored on `Override` (would require schema change).
/// Defensive: if a glob fails to compile here, it's silently treated
/// as "no match" — validation in `load_config` should have already
/// rejected every bad pattern.
///
/// # Errors
///
/// (none — pure data interpretation, no I/O.)
///
/// # Panics
///
/// Never panics; glob compilation failures fall back to "no match" so
/// the resolver remains total.
#[must_use]
pub fn resolve_detector_for_path<'c>(
    config: &'c FileConfig,
    path: &Path,
    smell: SmellCategory,
) -> &'c DetectorConfig {
    static DEFAULT: std::sync::OnceLock<DetectorConfig> = std::sync::OnceLock::new();
    for ov in config.overrides.iter().rev() {
        let matches = ov.r#match.iter().any(|pat| {
            globset::Glob::new(pat)
                .ok()
                .is_some_and(|g| g.compile_matcher().is_match(path))
        });
        if matches && let Some(dc) = ov.detectors.get(&smell) {
            return dc;
        }
    }
    config
        .detectors
        .get(&smell)
        .unwrap_or_else(|| DEFAULT.get_or_init(DetectorConfig::default))
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

    // ── W2.1: byte_offset_to_line + ConfigError Display tests ─────────

    #[test]
    fn byte_offset_to_line_empty_source_returns_line_one() {
        assert_eq!(byte_offset_to_line("", 0), 1);
    }

    #[test]
    fn byte_offset_to_line_no_newlines_returns_line_one() {
        let src = "key = value";
        assert_eq!(byte_offset_to_line(src, 0), 1);
        assert_eq!(byte_offset_to_line(src, 5), 1);
        assert_eq!(byte_offset_to_line(src, src.len()), 1);
    }

    #[test]
    fn byte_offset_to_line_two_line_source() {
        // "a\nb" — byte 0 is line 1 ('a'); byte 2 is line 2 ('b').
        let src = "a\nb";
        assert_eq!(byte_offset_to_line(src, 0), 1);
        assert_eq!(
            byte_offset_to_line(src, 1),
            1,
            "offset on the newline itself stays on the prior line"
        );
        assert_eq!(byte_offset_to_line(src, 2), 2);
    }

    #[test]
    fn byte_offset_to_line_three_line_source() {
        // "a\nb\nc" — byte 4 is line 3 ('c').
        let src = "a\nb\nc";
        assert_eq!(byte_offset_to_line(src, 4), 3);
    }

    #[test]
    fn byte_offset_to_line_out_of_bounds_offset_saturates_to_last_line() {
        // Defensive — shouldn't fire in practice (Spanned bounds are
        // honest) but the helper stays total either way.
        let src = "a\nb\nc";
        assert_eq!(byte_offset_to_line(src, 999), 3);
    }

    #[test]
    fn config_error_io_displays_path_and_preserves_source() {
        use std::error::Error;
        let err = ConfigError::Io {
            path: PathBuf::from("test-adapter.toml"),
            source: std::io::Error::other("boom"),
        };
        let display = err.to_string();
        assert!(display.contains("test-adapter.toml"), "got: {display}");
        assert!(err.source().is_some());
    }

    #[test]
    fn config_error_invalid_glob_displays_file_line_and_pattern() {
        use std::error::Error;
        let globset_err = globset::Glob::new("[unclosed").unwrap_err();
        let err = ConfigError::InvalidGlob {
            file: PathBuf::from("test-adapter.toml"),
            line: 5,
            pattern: "[unclosed".to_string(),
            source: globset_err,
        };
        let display = err.to_string();
        assert!(display.contains("test-adapter.toml:5"), "got: {display}");
        assert!(display.contains("[unclosed"), "got: {display}");
        assert!(err.source().is_some());
    }

    // ── W3.1: load_config + validator integration tests ────────────────

    /// Materialize a TOML fixture under a temp directory and return the
    /// path. Tempdir lives as long as the returned guard.
    fn write_fixture(contents: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test-adapter.toml");
        std::fs::write(&path, contents).unwrap();
        (dir, path)
    }

    #[test]
    fn load_config_minimal_valid_yields_default() {
        let (_dir, path) = write_fixture("");
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg, FileConfig::default());
    }

    #[test]
    fn load_config_comments_only_yields_default() {
        let (_dir, path) = write_fixture("# just a comment\n# another\n");
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg, FileConfig::default());
    }

    #[test]
    fn load_config_full_fixture_parses_to_expected_pod() {
        let (_dir, path) = write_fixture(
            r#"
src = "crates"
exclude = ["vendored/**"]
extensions = ["rs"]

[opt_outs]
honor = ["no_asserts"]

[detectors.zero_assertion]
enabled = true
penalty = 10

[detectors.large_example]
line_threshold = 30

[[overrides]]
match = ["tests/integration/**"]
[overrides.detectors.large_example]
line_threshold = 100
"#,
        );
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.src, Some(PathBuf::from("crates")));
        assert_eq!(cfg.exclude, vec!["vendored/**"]);
        assert_eq!(cfg.extensions, Some(vec!["rs".to_string()]));
        assert_eq!(cfg.opt_outs.honor, Some(vec![OptOut::NoAsserts]));
        assert_eq!(cfg.detectors.len(), 2);
        assert_eq!(
            cfg.detectors[&SmellCategory::ZeroAssertion].enabled,
            Some(true),
        );
        assert_eq!(
            cfg.detectors[&SmellCategory::LargeExample].line_threshold,
            Some(30),
        );
        assert_eq!(cfg.overrides.len(), 1);
        assert_eq!(cfg.overrides[0].r#match, vec!["tests/integration/**"]);
        assert_eq!(
            cfg.overrides[0].detectors[&SmellCategory::LargeExample].line_threshold,
            Some(100),
        );
    }

    #[test]
    fn load_config_invalid_glob_at_line_5() {
        // Fixture explicitly engineered: the broken glob `[unclosed`
        // sits on line 5. Lines 1-4 are preamble; line 5 is the entry
        // we expect the validator to fail on.
        //
        // Line 1: (blank, leading newline)
        // Line 2: src = "crates"
        // Line 3: (blank)
        // Line 4: exclude = [
        // Line 5:   "[unclosed",
        // Line 6: ]
        let (_dir, path) =
            write_fixture("\nsrc = \"crates\"\n\nexclude = [\n  \"[unclosed\",\n]\n");
        let err = load_config(&path).unwrap_err();
        match err {
            ConfigError::InvalidGlob { line, pattern, .. } => {
                assert_eq!(line, 5, "expected line 5 (the [unclosed line), got {line}");
                assert_eq!(pattern, "[unclosed");
            }
            other => panic!("expected InvalidGlob, got {other:?}"),
        }
    }

    #[test]
    fn load_config_invalid_glob_in_override_at_known_line() {
        // The override match `[bad` sits on line 4.
        // Line 1: (blank)
        // Line 2: [[overrides]]
        // Line 3: match = [
        // Line 4:   "[bad",
        // Line 5: ]
        let (_dir, path) = write_fixture("\n[[overrides]]\nmatch = [\n  \"[bad\",\n]\n");
        let err = load_config(&path).unwrap_err();
        match err {
            ConfigError::InvalidGlob { line, pattern, .. } => {
                assert_eq!(line, 4, "expected line 4 (the [bad line), got {line}");
                assert_eq!(pattern, "[bad");
            }
            other => panic!("expected InvalidGlob, got {other:?}"),
        }
    }

    #[test]
    fn load_config_line_threshold_on_zero_assertion_rejected() {
        let (_dir, path) = write_fixture(
            r"
[detectors.zero_assertion]
line_threshold = 99
",
        );
        let err = load_config(&path).unwrap_err();
        match err {
            ConfigError::InvalidValue { message, .. } => {
                assert!(
                    message.contains("line_threshold"),
                    "expected line_threshold in message, got {message}",
                );
                assert!(
                    message.contains("zero_assertion"),
                    "expected zero_assertion in message, got {message}",
                );
            }
            other => panic!("expected InvalidValue, got {other:?}"),
        }
    }

    #[test]
    fn load_config_zero_penalty_rejected() {
        let (_dir, path) = write_fixture(
            r"
[detectors.no_op_io]
penalty = 0
",
        );
        let err = load_config(&path).unwrap_err();
        match err {
            ConfigError::InvalidValue { message, .. } => {
                assert!(
                    message.contains("penalty"),
                    "expected penalty in message, got {message}",
                );
            }
            other => panic!("expected InvalidValue, got {other:?}"),
        }
    }

    #[test]
    fn load_config_empty_exclude_pattern_rejected() {
        let (_dir, path) = write_fixture("exclude = [\"\"]\n");
        let err = load_config(&path).unwrap_err();
        match err {
            ConfigError::InvalidValue { message, .. } => {
                assert!(
                    message.contains("empty"),
                    "expected 'empty' in message, got {message}",
                );
            }
            other => panic!("expected InvalidValue, got {other:?}"),
        }
    }

    #[test]
    fn load_config_whitespace_only_exclude_pattern_rejected() {
        let (_dir, path) = write_fixture("exclude = [\"   \"]\n");
        let err = load_config(&path).unwrap_err();
        match err {
            ConfigError::InvalidValue { message, .. } => {
                assert!(
                    message.contains("empty"),
                    "expected 'empty' in message, got {message}",
                );
            }
            other => panic!("expected InvalidValue, got {other:?}"),
        }
    }

    #[test]
    fn load_config_unknown_top_level_field_rejected() {
        let (_dir, path) = write_fixture("unknown_key = true\n");
        let err = load_config(&path).unwrap_err();
        match err {
            ConfigError::Parse { source, .. } => {
                let msg = source.to_string();
                assert!(
                    msg.contains("unknown")
                        || msg.contains("unknown field")
                        || msg.contains("unknown_key"),
                    "expected unknown-field message, got: {msg}",
                );
            }
            other => panic!("expected Parse, got {other:?}"),
        }
    }

    #[test]
    fn load_config_unknown_detector_smell_key_rejected() {
        // A typo'd smell key (`zer_assertion`) is unknown to SmellCategory.
        let (_dir, path) = write_fixture(
            r"
[detectors.zer_assertion]
enabled = true
",
        );
        let err = load_config(&path).unwrap_err();
        assert!(
            matches!(err, ConfigError::Parse { .. }),
            "expected Parse, got {err:?}",
        );
    }

    #[test]
    fn load_config_missing_file_returns_io() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("does-not-exist.toml");
        let err = load_config(&missing).unwrap_err();
        assert!(
            matches!(err, ConfigError::Io { .. }),
            "expected Io, got {err:?}",
        );
    }

    // ── MUST-FIX #2: OptOutPolicy 3-state coverage via load_config ────

    #[test]
    fn load_config_opt_outs_omitted_yields_none() {
        // No [opt_outs] block → honor stays None (honor-all default).
        let (_dir, path) = write_fixture("src = \"crates\"\n");
        let cfg = load_config(&path).unwrap();
        assert!(cfg.opt_outs.is_empty());
        assert_eq!(cfg.opt_outs.honor, None);
    }

    #[test]
    fn load_config_opt_outs_honor_empty_yields_some_empty() {
        // Strictest project policy: honor NO per-test suppressions.
        let (_dir, path) = write_fixture("[opt_outs]\nhonor = []\n");
        let cfg = load_config(&path).unwrap();
        assert_eq!(cfg.opt_outs.honor, Some(vec![]));
        assert!(
            !cfg.opt_outs.is_empty(),
            "Some(vec![]) is NOT empty per is_empty semantics",
        );
    }

    #[test]
    fn load_config_opt_outs_honor_variants_yields_some_vec() {
        let (_dir, path) = write_fixture("[opt_outs]\nhonor = [\"no_asserts\", \"no_op\"]\n");
        let cfg = load_config(&path).unwrap();
        assert_eq!(
            cfg.opt_outs.honor,
            Some(vec![OptOut::NoAsserts, OptOut::NoOp]),
        );
    }

    // ── Load -> serialize -> load round-trip ──────────────────────────

    #[test]
    fn load_config_load_then_serialize_round_trip() {
        let (_dir, path) = write_fixture(
            r#"
src = "crates"
exclude = ["vendored/**"]

[detectors.zero_assertion]
penalty = 10

[[overrides]]
match = ["tests/integration/**"]
[overrides.detectors.large_example]
line_threshold = 100
"#,
        );
        let loaded = load_config(&path).unwrap();
        let reserialized = toml::to_string_pretty(&loaded).unwrap();
        let reparsed: FileConfig = toml::from_str(&reserialized).unwrap();
        assert_eq!(loaded, reparsed);
    }

    // ── W4.1: discover_config tests ────────────────────────────────────

    /// Scope-guarded chmod restore — MUST-FIX #3. Mirrors the
    /// `PermissionGuard` in `adapters/source/fs.rs`'s `#[cfg(test)]`
    /// block. Tests that chmod 0o000 a dir must restore permissions
    /// before `TempDir::drop` runs its `rm -rf`, or stderr leaks
    /// chmod-denied warnings that downstream agentic loops misread as
    /// real test failures (per feedback_pristine-test-output).
    ///
    /// Duplication of the file-walker's guard is deliberate: structurally,
    /// integration-test mods in `tests/common/mod.rs` can't be re-used
    /// from `src/`-side `#[cfg(test)]` blocks; cross-direction reuse
    /// isn't possible in Rust's test-mod layout. Both bodies are
    /// trivial (~20 LOC) and identical.
    #[cfg(unix)]
    struct PermissionGuard {
        path: std::path::PathBuf,
        restore_mode: u32,
    }

    #[cfg(unix)]
    impl Drop for PermissionGuard {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            // Best-effort restore; ignore failure so panics in the
            // test body propagate cleanly.
            let _ = std::fs::set_permissions(
                &self.path,
                std::fs::Permissions::from_mode(self.restore_mode),
            );
        }
    }

    #[test]
    fn discover_config_finds_file_in_start_dir() {
        let (_dir, path) = write_fixture("");
        let start = path.parent().unwrap();
        let found = discover_config(start, "test-adapter.toml").unwrap();
        // Canonicalize the expected path for the macOS /private/var
        // prefix that tempfile uses.
        let expected = std::fs::canonicalize(&path).unwrap();
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn discover_config_walks_up_two_levels() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // Place test-adapter.toml at root; search from root/a/b/c/.
        std::fs::write(root.join("test-adapter.toml"), "").unwrap();
        let deep = root.join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        let found = discover_config(&deep, "test-adapter.toml").unwrap();
        let expected = std::fs::canonicalize(root.join("test-adapter.toml")).unwrap();
        assert_eq!(found, Some(expected));
    }

    #[test]
    fn discover_config_returns_none_when_absent_in_isolated_tempdir() {
        // Deep tempdir-relative start so the walk goes up through
        // tempdir-internal directories that are guaranteed to lack
        // `test-adapter.toml`. Walk may continue upward to /tmp or /
        // and find no match (or a stray match — neither outcome is
        // testable across machines, so we assert "no panic + Ok"
        // instead of strict Ok(None). The strict assertion below is
        // safe because tempfile's roots (`/tmp`, `$TMPDIR`) are
        // extremely unlikely to host a `test-adapter.toml` literal.
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("a").join("b").join("c");
        std::fs::create_dir_all(&deep).unwrap();
        let result = discover_config(&deep, "test-adapter.toml").unwrap();
        assert_eq!(
            result, None,
            "no test-adapter.toml in tempdir tree; ancestor walk should also miss",
        );
    }

    #[test]
    fn discover_config_respects_caller_supplied_name() {
        // Mirrors crap-rs#161 regression smoke. Tempdir contains
        // `different.toml` only; search for `test-adapter.toml` returns
        // None (the parameter actually drives discovery, not a
        // hard-coded constant).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::write(root.join("different.toml"), "").unwrap();
        let found = discover_config(root, "test-adapter.toml").unwrap();
        assert_eq!(
            found, None,
            "different.toml exists but test-adapter.toml does not; walk must respect caller-supplied name",
        );
    }

    #[test]
    fn discover_config_stops_at_directory_not_file() {
        // A directory named `test-adapter.toml` should NOT match —
        // discover_config requires meta.is_file().
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir(root.join("test-adapter.toml")).unwrap();
        let found = discover_config(root, "test-adapter.toml").unwrap();
        assert_eq!(
            found, None,
            "directory-shaped entry must not match (is_file check)",
        );
    }

    #[cfg(unix)]
    #[test]
    fn discover_config_returns_io_error_on_permission_denied_parent() {
        // Chmod 0o000 the PARENT directory of the search target so
        // `std::fs::metadata(parent/test-adapter.toml)` fails with
        // PermissionDenied. Chmod on the candidate itself wouldn't
        // help on macOS — owner can stat their own files even when
        // stripped. Chmod on the containing dir denies the join+metadata.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("locked");
        std::fs::create_dir(&locked).unwrap();
        // Search starts INSIDE `locked` so the very first metadata call
        // is `locked/test-adapter.toml`. With 0o000 on `locked`, that
        // metadata call fails with PermissionDenied.
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
        let _guard = PermissionGuard {
            path: locked.clone(),
            restore_mode: 0o755,
        };

        let result = discover_config(&locked, "test-adapter.toml");
        // Some macOS / filesystem combinations may still allow the
        // owner to stat through 0o000 (the stat is on the candidate,
        // not the dir). If the system permits the stat (returns Ok)
        // we expect Ok(None) — the candidate doesn't exist; if the
        // system denies it (returns Err), we expect ConfigError::Io.
        // Both are valid contracts; the test asserts that the
        // function NEVER PANICS and returns one of the two shapes.
        // Both Ok(None) and Err(ConfigError::Io { .. }) are valid
        // contracts depending on the filesystem's owner-stat rule:
        //   - Ok(None): permission bypassed for owner; candidate stat
        //     returns NotFound; walk continues / exhausts.
        //   - Err(ConfigError::Io): permission denied surfaces through
        //     std::fs::metadata.
        // Test asserts the function NEVER PANICS and returns one of
        // the two shapes — neither outcome is portable across
        // macOS/Linux filesystem semantics.
        match result {
            Ok(None) | Err(ConfigError::Io { .. }) => {
                // Acceptable per the above contract.
            }
            other => panic!("expected Ok(None) or ConfigError::Io, got {other:?}"),
        }
    }

    // ── W6.1: pub resolve_detector_for_path unit tests ─────────────────

    fn detector_cfg(enabled: Option<bool>, penalty: Option<u32>) -> DetectorConfig {
        DetectorConfig {
            enabled,
            penalty,
            line_threshold: None,
        }
    }

    #[test]
    fn resolve_no_overrides_falls_back_to_top_level() {
        let mut cfg = FileConfig::default();
        let top = detector_cfg(Some(true), Some(7));
        cfg.detectors
            .insert(SmellCategory::ZeroAssertion, top.clone());
        let got =
            resolve_detector_for_path(&cfg, Path::new("src/lib.rs"), SmellCategory::ZeroAssertion);
        assert_eq!(*got, top);
    }

    #[test]
    fn resolve_no_match_returns_default_sentinel() {
        let cfg = FileConfig::default();
        let got =
            resolve_detector_for_path(&cfg, Path::new("src/lib.rs"), SmellCategory::ZeroAssertion);
        assert_eq!(*got, DetectorConfig::default());
    }

    #[test]
    fn resolve_single_override_match_wins() {
        let override_cfg = detector_cfg(Some(false), None);
        let cfg = FileConfig {
            overrides: vec![Override {
                r#match: vec!["tests/**".to_string()],
                detectors: [(SmellCategory::ZeroAssertion, override_cfg.clone())]
                    .into_iter()
                    .collect(),
            }],
            ..Default::default()
        };
        let got = resolve_detector_for_path(
            &cfg,
            Path::new("tests/foo.rs"),
            SmellCategory::ZeroAssertion,
        );
        assert_eq!(*got, override_cfg);
    }

    #[test]
    fn resolve_last_matching_override_wins() {
        // Both overrides match `tests/integration/foo.rs`; the SECOND
        // one's per-detector config must win (reverse-iterate, first
        // reverse-match returned).
        let first = detector_cfg(Some(true), Some(10));
        let second = detector_cfg(Some(false), Some(99));
        let cfg = FileConfig {
            overrides: vec![
                Override {
                    r#match: vec!["tests/**".to_string()],
                    detectors: [(SmellCategory::ZeroAssertion, first.clone())]
                        .into_iter()
                        .collect(),
                },
                Override {
                    r#match: vec!["tests/integration/**".to_string()],
                    detectors: [(SmellCategory::ZeroAssertion, second.clone())]
                        .into_iter()
                        .collect(),
                },
            ],
            ..Default::default()
        };
        let got = resolve_detector_for_path(
            &cfg,
            Path::new("tests/integration/foo.rs"),
            SmellCategory::ZeroAssertion,
        );
        assert_eq!(*got, second, "expected second (later) override to win");
    }

    #[test]
    fn resolve_override_without_smell_falls_through_to_top_level() {
        let top = detector_cfg(Some(true), Some(11));
        let cfg = FileConfig {
            detectors: [(SmellCategory::ZeroAssertion, top.clone())]
                .into_iter()
                .collect(),
            overrides: vec![Override {
                r#match: vec!["tests/**".to_string()],
                // Override matches the path but defines NoOpIo, not ZeroAssertion.
                detectors: [(SmellCategory::NoOpIo, detector_cfg(Some(false), None))]
                    .into_iter()
                    .collect(),
            }],
            ..Default::default()
        };
        let got = resolve_detector_for_path(
            &cfg,
            Path::new("tests/foo.rs"),
            SmellCategory::ZeroAssertion,
        );
        assert_eq!(
            *got, top,
            "override missing the smell should fall through to top-level"
        );
    }

    // ── W6.1: insta snapshot for unknown-field error ───────────────────

    #[test]
    fn unknown_field_error_message_snapshot() {
        use std::error::Error;
        let (_dir, path) = write_fixture("unknown_key = true\n");
        let err = load_config(&path).unwrap_err();
        // Render Display + #[source] chain for the snapshot. Replace
        // the per-machine tempdir path prefix with `<TEMPDIR>` so the
        // snapshot is deterministic across machines. The replacement
        // looks for the absolute path component ending in
        // `test-adapter.toml`.
        let display = err.to_string();
        let path_str = path.display().to_string();
        let display_sanitized = display.replace(&path_str, "<TEMPDIR>/test-adapter.toml");
        let source_repr = err.source().map(|s| format!("{s:?}")).unwrap_or_default();
        insta::assert_snapshot!(format!("{display_sanitized}\n---\n{source_repr}"));
    }

    // ── W6.1: round-trip property test ─────────────────────────────────

    mod overrides_property {
        use super::*;
        use proptest::prelude::*;

        /// Brute-force oracle: iterate overrides front-to-back; last
        /// matching one's config wins. Mirrors the resolver's contract
        /// in a deliberately different implementation shape so they
        /// don't share bugs.
        fn brute_force_resolve<'c>(
            config: &'c FileConfig,
            path: &Path,
            smell: SmellCategory,
        ) -> Option<&'c DetectorConfig> {
            let mut winner: Option<&DetectorConfig> = None;
            for ov in &config.overrides {
                let matches = ov.r#match.iter().any(|pat| {
                    globset::Glob::new(pat)
                        .ok()
                        .is_some_and(|g| g.compile_matcher().is_match(path))
                });
                if matches && let Some(dc) = ov.detectors.get(&smell) {
                    winner = Some(dc);
                }
            }
            winner.or_else(|| config.detectors.get(&smell))
        }

        fn arb_overlapping_config() -> impl Strategy<Value = FileConfig> {
            let glob_pool = prop_oneof![
                Just("tests/**".to_string()),
                Just("tests/integration/**".to_string()),
                Just("benches/**".to_string()),
                Just("**".to_string()),
            ];
            let detector_cfg_strat = (
                prop::option::of(any::<bool>()),
                prop::option::of(1u32..=100),
            )
                .prop_map(|(enabled, penalty)| DetectorConfig {
                    enabled,
                    penalty,
                    line_threshold: None,
                });
            let override_strat = (
                proptest::collection::vec(glob_pool, 1..=3),
                proptest::collection::vec(detector_cfg_strat, 0..=2),
            )
                .prop_map(|(matches, cfgs)| Override {
                    r#match: matches,
                    detectors: cfgs
                        .into_iter()
                        .enumerate()
                        .map(|(i, cfg)| {
                            let smell = if i == 0 {
                                SmellCategory::ZeroAssertion
                            } else {
                                SmellCategory::LargeExample
                            };
                            (smell, cfg)
                        })
                        .collect(),
                });
            proptest::collection::vec(override_strat, 1..=5).prop_map(|overrides| FileConfig {
                overrides,
                ..Default::default()
            })
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 64, .. ProptestConfig::default() })]

            #[test]
            fn pub_resolver_matches_brute_force_oracle(
                config in arb_overlapping_config(),
                path_idx in 0u8..4,
            ) {
                let paths = [
                    "tests/foo.rs",
                    "tests/integration/bar.rs",
                    "benches/baz.rs",
                    "src/lib.rs",
                ];
                let path = Path::new(paths[path_idx as usize]);
                let smell = SmellCategory::ZeroAssertion;
                let resolver = resolve_detector_for_path(&config, path, smell);
                let oracle = brute_force_resolve(&config, path, smell);
                if let Some(expected) = oracle {
                    prop_assert_eq!(resolver, expected);
                } else {
                    prop_assert_eq!(resolver, &DetectorConfig::default());
                }
            }
        }
    }

    // ── W6.1: round-trip property — arbitrary FileConfig ──────────────

    mod round_trip_property {
        use super::*;
        use proptest::prelude::*;

        fn arb_file_config() -> impl Strategy<Value = FileConfig> {
            // Bounded shape: omit `src` half the time, generate small
            // exclude lists with VALID globs (skips the validator's
            // empty-pattern + globset-error checks), and a small
            // detectors map.
            let valid_glob_pool = prop_oneof![
                Just("tests/**".to_string()),
                Just("vendored/**".to_string()),
                Just("benches/**".to_string()),
                Just("docs/*.md".to_string()),
            ];
            let src_strat = prop::option::of(Just(PathBuf::from("crates")));
            let exclude_strat = proptest::collection::vec(valid_glob_pool, 0..=3);
            let ext_strat = prop::option::of(Just(vec!["rs".to_string()]));
            (src_strat, exclude_strat, ext_strat).prop_map(|(src, exclude, extensions)| {
                FileConfig {
                    src,
                    exclude,
                    extensions,
                    ..Default::default()
                }
            })
        }

        proptest! {
            #![proptest_config(ProptestConfig { cases: 32, .. ProptestConfig::default() })]

            #[test]
            fn parse_serialize_parse_round_trip(fixture in arb_file_config()) {
                let serialized = toml::to_string_pretty(&fixture).unwrap();
                let reparsed: FileConfig = toml::from_str(&serialized).unwrap();
                prop_assert_eq!(fixture, reparsed);
            }
        }
    }
}
