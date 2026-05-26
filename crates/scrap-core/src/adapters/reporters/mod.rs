//! Reporters — language-agnostic implementations of structured-output
//! formats.
//!
//! Per the locked decision in `crates/scrap-core/src/ports/mod.rs:8-13`
//! and the crap4rs ADR
//! [`adr-free-functions-over-reporter-trait`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/crap4rs/adr-free-functions-over-reporter-trait.md)
//! (D1), reporters are **free functions**, not implementations of a
//! `dyn Trait` port. Each reporter takes a `&Report` plus
//! reporter-specific configuration and emits bytes — composition
//! happens at the call site, not behind an indirection.
//!
//! Per the wire-envelope ADR
//! [`adr-nested-json-envelope`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/scrap4rs/adr-nested-json-envelope.md)
//! (D2), every adapter binary in the workspace calls the same
//! `scrap_core::adapters::reporters::*::emit` function so the wire
//! shape cannot drift across adapters by construction.
//!
//! Module roster (live):
//! - [`json`] — v0.1 nested JSON envelope (`scrap-rs#14`).
//!
//! Module roster (planned):
//! - `markdown` — GFM table reporter (`scrap-rs#15`).
//! - `stdout` — comfy-table terminal reporter (`scrap-rs#16`).
//! - `sarif` — SARIF 2.1.0 GitHub Code Scanning (`scrap-rs#17`).
//! - `scorecard_row` — mokumo `Row::TestSmell` producer (v0.2).
//!
//! tracked: scrap-rs#73 — `adr-port-surface-and-domain-conventions`
//! ADR not yet authored; this module's design references the existing
//! `ports/mod.rs:8-13` docstring + `adr-nested-json-envelope` +
//! `crap4rs/adr-free-functions-over-reporter-trait` as load-bearing
//! constraints.

pub mod json;
