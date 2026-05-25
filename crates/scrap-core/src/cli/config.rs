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

// Bootstrap smoke — removed at end of W1.1 when real types land.
#[cfg(test)]
mod tests {
    #[test]
    fn module_compiles() {
        // Single placeholder confirming the module body builds end-to-end
        // before W1 brings in `FileConfig` and friends. Removed at the
        // end of W1.1 per impl-plan §Pre-flight step 6.
    }
}
