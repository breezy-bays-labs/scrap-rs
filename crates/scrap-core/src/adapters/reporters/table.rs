//! v0.1 comfy-table terminal reporter — free function `emit()` that
//! renders a `Report` as a human-readable table for the default
//! `--format table` CLI dispatch (scrap-rs#16).
//!
//! Sibling to [`crate::adapters::reporters::json`] — same free-function
//! pattern per
//! [`crap4rs/adr-free-functions-over-reporter-trait`](https://github.com/breezy-bays-labs/ops/blob/main/decisions/crap4rs/adr-free-functions-over-reporter-trait.md)
//! (D1) and codified in `crates/scrap-core/src/ports/mod.rs:8-13`.
//!
//! ## tracked
//!
//! - scrap-rs#73 — `adr-port-surface-and-domain-conventions` ADR
//!   not yet authored; references existing `ports/mod.rs:8-13`
//!   docstring + `adr-free-functions-over-reporter-trait` as
//!   load-bearing.
