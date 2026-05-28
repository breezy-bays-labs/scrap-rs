//! Real-name emission verification for the scrap4rs adapter
//! (scrap-rs#37).
//!
//! This is the genuine "this adapter emits ITS OWN name into the wire"
//! claim, and it belongs in the adapter crate that owns the identity —
//! NOT in adapter-agnostic `scrap-core`. The scrap-core wire-reporter
//! fixtures deliberately use neutral names (`adapter-a` /
//! `test-adapter`) so they stay adapter-name-pure and remain
//! coverage-superior against name-hardcoding in `emit()`; this test is
//! their counterpart, asserting the concrete `scrap4rs` string lands
//! in the `tool` field when the real shipping `AdapterMeta` is threaded
//! through.
//!
//! The meta below mirrors the env-sourced fields of the literal in
//! `scrap4rs/src/main.rs` so the fixture can't drift from the binary:
//! `tool_name` flows from `env!("CARGO_PKG_NAME")` (resolves to
//! `scrap4rs` for this crate's test binary) and `long_version` from the
//! build.rs-stamped `env!("SCRAP4RS_LONG_VERSION")` — NOT hardcoded
//! strings — so a crate rename or version bump automatically re-points
//! both the binary and this assertion. The `"scrap4rs"` literal in the
//! assertion is legal here because this crate IS the scrap4rs adapter;
//! the purity gate scopes to `crates/scrap-core/` only.
//!
//! scrap4ts equivalent: tracked for when the TS adapter lands (see the
//! follow-up issue referenced in the scrap-rs#37 PR body).

use scrap_core::adapter_meta::AdapterMeta;
use scrap_core::adapters::reporters::json::{EmitOptions, emit};
use scrap_core::domain::report::Report;
use scrap_core::domain::threshold::ThresholdMode;

const FIXED_TIMESTAMP: &str = "2026-05-26T00:00:00Z";

/// Reconstruct the real shipping `AdapterMeta` from `scrap4rs/src/main.rs`
/// for the fields that drive the wire envelope. `tool_name` flows from
/// `CARGO_PKG_NAME` exactly as the binary does — the assertion below
/// proves that the package name (`scrap4rs`) reaches the wire `tool`
/// field unmodified.
fn shipping_meta() -> AdapterMeta {
    AdapterMeta {
        tool_name: env!("CARGO_PKG_NAME"),
        language: "rust",
        tool_version: env!("CARGO_PKG_VERSION"),
        // Mirror main.rs exactly: the build.rs-stamped long version,
        // NOT CARGO_PKG_VERSION. build.rs emits this via
        // `cargo:rustc-env`, which propagates to this integration test's
        // compile context — so the fixture can't drift from the binary.
        long_version: env!("SCRAP4RS_LONG_VERSION"),
        about: "Static test smell detector for Rust",
        long_about: "",
        after_help: "",
        extensions: &["rs"],
        tool_info_uri: "https://github.com/breezy-bays-labs/scrap-rs",
        rule_help_uri: "https://github.com/breezy-bays-labs/scrap-rs#detection-rules",
        config_file_name: "scrap4rs.toml",
        default_excludes: &["tests/**", "benches/**", "examples/**"],
        parse_hint: "ensure --src points at a Cargo workspace with test files",
    }
}

#[test]
fn shipping_meta_emits_scrap4rs_in_wire_tool_field() {
    let report = Report::default();
    let meta = shipping_meta();
    let mut buf = Vec::new();
    emit(
        &report,
        &meta,
        &EmitOptions::default(),
        FIXED_TIMESTAMP,
        ThresholdMode::Default,
        &mut buf,
    )
    .expect("emit succeeds");
    let value: serde_json::Value = serde_json::from_slice(&buf).expect("envelope is valid JSON");

    // The real adapter's identity (its package name) must reach the
    // wire `tool` field unmodified. This is the assertion that the
    // neutral-name scrap-core fixtures intentionally cannot make —
    // it pins the concrete shipping name to the wire.
    assert_eq!(
        value["tool"],
        serde_json::json!("scrap4rs"),
        "scrap4rs's shipping AdapterMeta must emit its own name in the wire `tool` field",
    );

    // The package name (`CARGO_PKG_NAME`) is the source of truth — assert
    // the threaded value equals it, so a crate rename can't silently
    // drift the wire `tool` away from the adapter's actual identity.
    assert_eq!(
        value["tool"].as_str(),
        Some(env!("CARGO_PKG_NAME")),
        "wire `tool` must equal CARGO_PKG_NAME (the adapter's real identity)",
    );

    // Language pins the adapter's source-language identity too.
    assert_eq!(value["language"], serde_json::json!("rust"));
}
