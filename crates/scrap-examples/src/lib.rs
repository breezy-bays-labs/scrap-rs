//! `scrap-examples` — curated bad-by-design fixture corpus and
//! golden-snapshot harness for the scrap-rs detector ecosystem.
//!
//! This crate ships no library logic. The corpus lives at
//! `crates/scrap-examples/examples/<smell>/` and the harness lives at
//! `crates/scrap-examples/tests/snapshots.rs`. See `README.md` for
//! the layout convention and bless workflow.
//!
//! `publish = false` — never reaches crates.io. `autoexamples = false`
//! — files under `examples/` are fixture source files for the harness
//! to parse, NOT cargo binary targets.

#![warn(missing_docs)]
