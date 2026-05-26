//! Adapter-supplied identity for the JSON envelope (and future reporters).
//!
//! Every adapter binary (`scrap4rs`, future `scrap4ts`, future
//! adapters) constructs one of these and threads it into
//! `scrap_core::adapters::reporters::*::emit` and the forthcoming
//! `scrap_core::cli::run` entry point. POD per the constructor-pattern
//! convention used across `scrap-core::domain` — `Default::default()`
//! is intentionally absent here (every field is binary-specific; a
//! `Default` impl would either lie or violate the source-only
//! adapter-name purity CI gate at scrap-rs#37 / scrap-rs#52).
//!
//! ## Why top-level (not under `cli/`)
//!
//! Reporters consume `AdapterMeta` in addition to the (future) CLI
//! entry point. Programmatic embedders that bypass the CLI surface
//! (library callers wiring `analyze` + `emit` directly) need to
//! construct one without pulling in clap-derived types. Top-level
//! module placement keeps the type accessible from every layer that
//! needs it without crossing module boundaries.
//!
//! ## Why `&'static str` (not `String`)
//!
//! Every field is a compile-time literal from the adapter binary
//! (`env!("CARGO_PKG_VERSION")` macros + hard-coded literals). Using
//! `&'static str` zero-allocates and matches the existing
//! `cli::config::discover_config(start: &Path, file_name: &str)` API
//! that already accepts `&'static str` for the per-adapter config
//! basename.
//!
//! ## Adapter-name purity
//!
//! `scrap-core` source MUST NOT carry adapter-binary-name literals
//! (`"scrap4rs"`, `"scrap4ts"`) per the source-only `scrap-core
//! adapter-name literal purity` CI gate. This module references the
//! pattern abstractly; concrete literals live in adapter-binary
//! `main.rs` and in `crates/scrap-core/tests/` test fixtures (the
//! source-only gate scopes to `crates/scrap-core/src/`).
//!
//! tracked: scrap-rs#73 — `adr-port-surface-and-domain-conventions`
//! ADR not yet authored; this module's design references the existing
//! `crates/scrap-core/src/ports/mod.rs:8-13` docstring and the
//! `adr-nested-json-envelope` ADR as load-bearing constraints.

/// Adapter-binary identity bundle.
///
/// Constructed by each adapter binary's `main.rs` and threaded into
/// the reporter (`scrap_core::adapters::reporters::json::emit`) and
/// the forthcoming generic CLI entry point
/// (`scrap_core::cli::run`, scrap-rs#21).
///
/// **NOT** wrapped in a `Default` impl by design — see module
/// docstring "Why no Default" rationale.
#[derive(Debug, Clone)]
pub struct AdapterMeta {
    /// Adapter tool name, e.g. `"scrap4rs"`. Emitted verbatim into the
    /// JSON envelope's `tool` field.
    pub tool: &'static str,
    /// Source-language identifier, e.g. `"rust"` / `"typescript"`.
    /// Emitted verbatim into the JSON envelope's `language` field.
    pub language: &'static str,
    /// Adapter binary version, typically `env!("CARGO_PKG_VERSION")`.
    /// Emitted verbatim into the JSON envelope's `tool_version`
    /// field.
    pub tool_version: &'static str,
    /// Per-adapter config-file basename (e.g. `"scrap4rs.toml"`).
    /// Used by `cli::config::discover_config(start, file_name)`;
    /// lives here so every adapter-binary metadata lives in one
    /// place.
    pub config_file_name: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-fixture meta. The `"scrap4rs"` literal here is OK because
    // the source-only adapter-name literal purity CI gate scopes to
    // `crates/scrap-core/src/`, NOT `tests/` or `#[cfg(test)] mod`
    // blocks within src/. scrap-rs#37 expands the gate to tests/ via
    // a `tracked: scrap-rs#37` per-line grandfather marker — when
    // that lands, this fixture stays valid because it's in a
    // `#[cfg(test)]` block under src/, not in `tests/`.
    //
    // Constructed-inline below rather than as a const because
    // `AdapterMeta` doesn't impl `Copy` and a `const fn` constructor
    // would over-restrict future field shapes.
    fn fixture_meta() -> AdapterMeta {
        AdapterMeta {
            tool: "scrap4rs",
            language: "rust",
            tool_version: "0.1.0",
            config_file_name: "scrap4rs.toml",
        }
    }

    #[test]
    fn adapter_meta_constructs_with_all_fields() {
        let meta = fixture_meta();
        assert_eq!(meta.tool, "scrap4rs");
        assert_eq!(meta.language, "rust");
        assert_eq!(meta.tool_version, "0.1.0");
        assert_eq!(meta.config_file_name, "scrap4rs.toml");
    }

    #[test]
    fn adapter_meta_clones_to_independent_value() {
        let meta = fixture_meta();
        let clone = meta.clone();
        assert_eq!(meta.tool, clone.tool);
        assert_eq!(meta.language, clone.language);
        assert_eq!(meta.tool_version, clone.tool_version);
        assert_eq!(meta.config_file_name, clone.config_file_name);
    }
}
