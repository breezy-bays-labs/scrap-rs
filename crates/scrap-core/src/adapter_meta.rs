//! Adapter-supplied identity for the JSON envelope, the CLI surface,
//! and all reporters.
//!
//! Every adapter binary constructs one of these and threads it into
//! `scrap_core::cli::parse_args` + `scrap_core::cli::run` (scrap-rs#21)
//! and `scrap_core::adapters::reporters::*::emit`. POD per the
//! constructor-pattern convention used across `scrap-core::domain` —
//! `Default::default()` is intentionally absent here (every field is
//! binary-specific; a `Default` impl would either lie or violate the
//! source-only adapter-name purity CI gate at scrap-rs#37 / scrap-rs#52).
//!
//! ## Why top-level (not under `cli/`)
//!
//! Reporters consume `AdapterMeta` in addition to the CLI entry point.
//! Programmatic embedders that bypass the CLI surface (library callers
//! wiring `analyze` + `emit` directly) need to construct one without
//! pulling in clap-derived types. Top-level module placement keeps the
//! type accessible from every layer that needs it without crossing
//! module boundaries.
//!
//! ## Why `&'static str` (not `String`) + `Copy`
//!
//! Every field is a compile-time literal from the adapter binary
//! (`env!("CARGO_PKG_VERSION")` macros + hard-coded literals). Using
//! `&'static str` zero-allocates and matches the existing
//! `cli::config::discover_config(start: &Path, file_name: &str)` API
//! that already accepts `&'static str` for the per-adapter config
//! basename. `Copy` lets every consumer thread `&meta` or `*meta`
//! freely without lifetime gymnastics — per the crap-rs#161 retrofit
//! lesson the scrap-rs#21 issue body explicitly cites.
//!
//! ## Adapter-name purity
//!
//! `scrap-core` source MUST NOT carry adapter-binary-name literals
//! per the source-only `scrap-core adapter-name literal purity` CI
//! gate. This module references the pattern abstractly; concrete
//! literals live in adapter-binary `main.rs` and in
//! `crates/scrap-core/tests/` test fixtures (the source-only gate
//! scopes to `crates/scrap-core/src/`, the latter is covered by
//! per-line `tracked: scrap-rs#37` grandfather markers when test
//! fixtures need realism).
//!
//! tracked: scrap-rs#73 — `adr-port-surface-and-domain-conventions`
//! ADR not yet authored; this module's design references the existing
//! `crates/scrap-core/src/ports/mod.rs:8-13` docstring and the
//! `adr-nested-json-envelope` ADR as load-bearing constraints.

/// Adapter-binary identity bundle.
///
/// Constructed by each adapter binary's `main.rs` and threaded into
/// the reporter (`scrap_core::adapters::reporters::json::emit`) and
/// the generic CLI entry point (`scrap_core::cli::run`, scrap-rs#21).
///
/// **NOT** wrapped in a `Default` impl by design — see module
/// docstring "Why no Default" rationale.
///
/// **13 fields** total (scrap-rs#21 expansion from the original
/// 4-field seed shipped with scrap-rs#14). 12 of the 13 are
/// enumerated in the scrap-rs#21 issue body; `language` predates the
/// expansion (required by the JSON envelope's `language` wire key
/// since scrap-rs#14). Three crap-rs fields are deliberately omitted:
/// `default_metric` (scrap-rs has no metric axis), `forced_excludes`
/// (no language-mandated exclude analog of crap4ts's `*.d.ts`), and
/// `display_name` (no HTML reporter yet; lands additively when #39
/// needs it).
#[derive(Debug, Clone, Copy)]
pub struct AdapterMeta {
    /// Adapter binary name (concrete adapter binaries supply the
    /// literal; `scrap-core` source stays adapter-name-agnostic per
    /// CI gate). Drives clap's `--version` output, the JSON envelope's
    /// `tool` field, the SARIF run's `name` field, and the
    /// stdout/markdown/table reporter headers. Renamed from `tool` →
    /// `tool_name` at scrap-rs#21 per FORK-1 (single-tree-shape commit;
    /// crap-rs#161 retrofit lesson).
    pub tool_name: &'static str,
    /// Source-language identifier (e.g., a `rust` or `typescript`
    /// token). Emitted verbatim into the JSON envelope's `language`
    /// field. Pre-existing since scrap-rs#14.
    pub language: &'static str,
    /// Adapter binary's package version, typically
    /// `env!("CARGO_PKG_VERSION")` at the binary crate's compile time.
    /// Emitted verbatim into the JSON envelope's `tool_version` field
    /// and clap's short `-V` output.
    pub tool_version: &'static str,
    /// Long-version string with git hash + build date, e.g.
    /// `"0.1.0 (abc1234 2026-05-27)"`. From the adapter binary's
    /// `build.rs`-stamped `SCRAP4RS_LONG_VERSION` env var (W5).
    /// Displayed by clap's long `--version` output.
    pub long_version: &'static str,
    /// Short adapter-flavored help text (one-line, shown by `--help`
    /// summary line). Spliced into clap's `Command::about` at runtime
    /// via `build_command(meta)`.
    pub about: &'static str,
    /// Long adapter-flavored help text (multi-paragraph, shown by
    /// `--help` in full mode). Spliced into clap's
    /// `Command::long_about` at runtime.
    pub long_about: &'static str,
    /// `after_help` block with adapter-specific examples (e.g.,
    /// `scrap4rs --src crates/scrap-core --format json`). May be
    /// empty. Spliced into clap's `Command::after_help` only when
    /// non-empty.
    pub after_help: &'static str,
    /// File extensions the walker keeps (e.g., `&["rs"]` for
    /// scrap4rs; `&["ts", "tsx"]` for future scrap4ts). Threaded into
    /// `AnalyzeOptions.extensions` via [`AdapterMeta::extensions_owned`].
    pub extensions: &'static [&'static str],
    /// Adapter repo URL for SARIF's
    /// `runs[0].tool.driver.informationUri`. Distinct per adapter so
    /// scrap4ts SARIF links to scrap4ts's repo, not scrap4rs's.
    pub tool_info_uri: &'static str,
    /// Adapter rule-help URL for SARIF's
    /// `runs[0].tool.driver.rules[0].helpUri`.
    pub rule_help_uri: &'static str,
    /// Per-adapter config-file basename (concrete adapter binaries
    /// supply the literal). Threaded into
    /// `cli::config::discover_config(start, file_name)`; lives here
    /// so every adapter-binary metadata lives in one place.
    pub config_file_name: &'static str,
    /// Commented-out exclude patterns the `init` subcommand emits into
    /// the generated config (e.g., `&["tests/**", "benches/**",
    /// "examples/**"]` for Rust). Init-template only — NOT applied at
    /// analysis time. May be empty.
    pub default_excludes: &'static [&'static str],
    /// Adapter-specific remediation hint shown when the parser fails
    /// on every input file (Rust: `"ensure --src points at a Cargo
    /// workspace with test files"`). Consumed by
    /// `AnalyzeError::AllFilesFailedToParse` render in
    /// `cli::dispatch::render_error`.
    pub parse_hint: &'static str,
}

impl AdapterMeta {
    /// Allocate an owned `Vec<String>` from `extensions` for inclusion
    /// in `AnalyzeOptions` (which owns its config rather than borrowing
    /// from the meta, decoupling analysis lifetime from CLI lifetime).
    #[must_use]
    pub fn extensions_owned(&self) -> Vec<String> {
        self.extensions.iter().map(|e| (*e).to_string()).collect()
    }

    /// Trip on construction with empty required string fields.
    /// `extensions` and `default_excludes` are allowed to be empty
    /// (a language with no extension whitelist or no init-template
    /// pre-fills is a legitimate adapter shape). The other 11
    /// `&'static str` fields are mandatory for help/SARIF/`--version`
    /// rendering, and a silent empty string here would produce
    /// malformed output that's hard to trace back to the meta.
    /// Debug-only so release builds stay zero-cost; production
    /// binaries should never hit these (their meta is `env!()` /
    /// `const`).
    ///
    /// Called by `cli::parse_args` at the very top, immediately after
    /// the meta enters scrap-core's surface — catches accidentally-
    /// empty `env!()` strings or per-adapter constant typos at
    /// first-run time in dev/test builds.
    pub(crate) fn debug_assert_required_fields(&self) {
        debug_assert!(
            !self.tool_name.is_empty(),
            "AdapterMeta.tool_name must not be empty"
        );
        debug_assert!(
            !self.language.is_empty(),
            "AdapterMeta.language must not be empty"
        );
        debug_assert!(
            !self.tool_version.is_empty(),
            "AdapterMeta.tool_version must not be empty"
        );
        debug_assert!(
            !self.long_version.is_empty(),
            "AdapterMeta.long_version must not be empty"
        );
        debug_assert!(
            !self.about.is_empty(),
            "AdapterMeta.about must not be empty"
        );
        debug_assert!(
            !self.long_about.is_empty(),
            "AdapterMeta.long_about must not be empty"
        );
        debug_assert!(
            !self.tool_info_uri.is_empty(),
            "AdapterMeta.tool_info_uri must not be empty"
        );
        debug_assert!(
            !self.rule_help_uri.is_empty(),
            "AdapterMeta.rule_help_uri must not be empty"
        );
        debug_assert!(
            !self.config_file_name.is_empty(),
            "AdapterMeta.config_file_name must not be empty"
        );
        debug_assert!(
            !self.parse_hint.is_empty(),
            "AdapterMeta.parse_hint must not be empty"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-fixture meta uses a generic adapter-name placeholder so
    // the source-only adapter-name literal purity CI gate
    // (scrap-rs#18) stays green even on `#[cfg(test)] mod` blocks
    // under `src/` — that gate scopes to `crates/scrap-core/src/`
    // and does NOT exempt cfg(test) blocks. Real adapter-name
    // literals live in `tests/` fixtures (out of source-gate scope)
    // and in adapter-binary `main.rs` (a different crate).
    pub(crate) fn fixture_meta() -> AdapterMeta {
        AdapterMeta {
            tool_name: "test-adapter",
            language: "rust",
            tool_version: "0.1.0",
            long_version: "0.1.0 (test 2026-05-27)",
            about: "test adapter for AdapterMeta tests",
            long_about: "longer about text for unit tests",
            after_help: "",
            extensions: &["rs"],
            tool_info_uri: "https://example.invalid/scrap",
            rule_help_uri: "https://example.invalid/scrap/rules",
            config_file_name: "test-adapter.toml",
            default_excludes: &["tests/**", "benches/**"],
            parse_hint: "ensure --src points at a workspace with test files",
        }
    }

    #[test]
    fn adapter_meta_constructs_with_all_13_fields() {
        let meta = fixture_meta();
        assert_eq!(meta.tool_name, "test-adapter");
        assert_eq!(meta.language, "rust");
        assert_eq!(meta.tool_version, "0.1.0");
        assert_eq!(meta.long_version, "0.1.0 (test 2026-05-27)");
        assert_eq!(meta.about, "test adapter for AdapterMeta tests");
        assert_eq!(meta.long_about, "longer about text for unit tests");
        assert_eq!(meta.after_help, "");
        assert_eq!(meta.extensions, &["rs"]);
        assert_eq!(meta.tool_info_uri, "https://example.invalid/scrap");
        assert_eq!(meta.rule_help_uri, "https://example.invalid/scrap/rules");
        assert_eq!(meta.config_file_name, "test-adapter.toml");
        assert_eq!(meta.default_excludes, &["tests/**", "benches/**"]);
        assert_eq!(
            meta.parse_hint,
            "ensure --src points at a workspace with test files"
        );
    }

    #[test]
    fn adapter_meta_copies_to_independent_value() {
        // Post-Copy derive: assignment is a copy, not a move. The
        // original stays usable + the copy compares equal field-wise.
        let meta = fixture_meta();
        let copy = meta;
        assert_eq!(meta.tool_name, copy.tool_name);
        assert_eq!(meta.config_file_name, copy.config_file_name);
        // Use the original after the copy — proves Copy semantics.
        assert_eq!(meta.long_version, "0.1.0 (test 2026-05-27)");
    }

    #[test]
    fn adapter_meta_is_copy() {
        // Compile-time check that AdapterMeta implements Copy. A
        // future PR that adds a non-Copy field (e.g. String) would
        // fail this assertion.
        fn assert_copy<T: Copy>() {}
        assert_copy::<AdapterMeta>();
    }

    #[test]
    fn extensions_owned_returns_owned_vec() {
        let meta = fixture_meta();
        let owned: Vec<String> = meta.extensions_owned();
        assert_eq!(owned, vec!["rs".to_string()]);
    }

    #[test]
    fn extensions_owned_empty_returns_empty_vec() {
        let mut meta = fixture_meta();
        meta.extensions = &[];
        let owned: Vec<String> = meta.extensions_owned();
        assert!(owned.is_empty());
    }

    #[test]
    #[should_panic = "AdapterMeta.tool_name must not be empty"]
    fn debug_assert_required_fields_panics_on_empty_tool_name() {
        let mut meta = fixture_meta();
        meta.tool_name = "";
        meta.debug_assert_required_fields();
    }

    #[test]
    #[should_panic = "AdapterMeta.parse_hint must not be empty"]
    fn debug_assert_required_fields_panics_on_empty_parse_hint() {
        let mut meta = fixture_meta();
        meta.parse_hint = "";
        meta.debug_assert_required_fields();
    }

    #[test]
    fn debug_assert_required_fields_accepts_empty_after_help() {
        // after_help is intentionally optional — adapters with no
        // EXAMPLES block (test-adapter, future minimal adapters) pass
        // the assert. Verifies the "11 mandatory of 13" semantics.
        let mut meta = fixture_meta();
        meta.after_help = "";
        meta.debug_assert_required_fields();
    }

    #[test]
    fn debug_assert_required_fields_accepts_empty_extensions() {
        let mut meta = fixture_meta();
        meta.extensions = &[];
        meta.debug_assert_required_fields();
    }

    #[test]
    fn debug_assert_required_fields_accepts_empty_default_excludes() {
        let mut meta = fixture_meta();
        meta.default_excludes = &[];
        meta.debug_assert_required_fields();
    }
}
