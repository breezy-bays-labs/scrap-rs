//! Core — orchestration. Wires adapters through ports and exposes
//! `analyze()` for the CLI and embedding consumers.
//!
//! `analyze()` is the public API surface for embedding scrap4rs in
//! other tools (e.g. mokumo's quality crate). It takes a workspace
//! root + config and returns a `domain::Report`. Lands in v0.1 once
//! enough adapters and detectors exist to make the call meaningful.
