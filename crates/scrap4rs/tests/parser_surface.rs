//! Compile-time invariant smoke tests for the `TestParserPort` trait
//! and its `SynTestParser` impl.
//!
//! Behavioural scenarios live in `tests/features/parser.feature`
//! (executed by `tests/cucumber.rs`); per-fixture insta snapshot
//! assertions live in `tests/parser_snapshots.rs` (lands S2.1+).
//! This file pins the *type-level* contracts only — if it compiles +
//! links, the assertions below hold.
//!
//! Pinned invariants:
//! - `TestParserPort` is object-safe — `Box<dyn TestParserPort>` works.
//! - `dyn TestParserPort` is **deliberately NOT** `Send + Sync` —
//!   parallelism bounds belong at the `core::analyze<S, P>` call site
//!   (per ADR `adr-port-surface-and-domain-conventions` D11).
//! - The shipped `SynTestParser` adapter IS `Send + Sync` as an
//!   emergent property — it's a zero-sized struct with no fields,
//!   so neither bound can be violated.
//!
//! Send/Sync symmetry mirrors `crates/scrap-core/tests/source_walker.rs`
//! lines 21-24: pair the `dyn` negative with concrete-type positive.

use scrap_core::ports::parser::TestParserPort;
use scrap4rs::parser::SynTestParser;

// Object-safe so `Box<dyn TestParserPort>` works (allows runtime
// adapter selection — e.g. a future CLI flag swapping parsers).
static_assertions::assert_obj_safe!(TestParserPort);

// `dyn TestParserPort` is deliberately NOT `Send + Sync` per D11.
// Concrete adapters can still BE `Send + Sync`; the trait surface
// just doesn't advertise the bound.
static_assertions::assert_not_impl_any!(dyn TestParserPort: Send, Sync);

// SynTestParser implements the port.
static_assertions::assert_impl_all!(SynTestParser: TestParserPort);

// SynTestParser is `Send + Sync` as an emergent property (mirrors the
// file-walker pattern). `core::analyze<S: SourcePort + Send + Sync,
// P: TestParserPort + Send + Sync>` will be expressible at the call
// site when parallelism arrives.
static_assertions::assert_impl_all!(SynTestParser: Send, Sync);

#[test]
fn smoke_compiles() {
    // Existence test — if this file compiles + links, the
    // static_assertions above hold. The fn body is intentionally a
    // tiny smoke check so nextest has something to enumerate
    // (otherwise the test file is build-only and `cargo nextest list`
    // wouldn't surface it as a discoverable target).
    assert!(env!("CARGO_PKG_VERSION").starts_with("0."));
}
