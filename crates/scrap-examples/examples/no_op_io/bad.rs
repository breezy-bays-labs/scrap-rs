//! No-op-io smell — minimal triggering example.
//!
//! The body performs work (a filesystem write) but discards its
//! `Result` via `let _ = ...;` and never inspects the outcome — no
//! assertion, no implicit-assertion source, no `.unwrap()`/`.expect()`
//! chain. The test always passes regardless of whether the write
//! succeeded; it runs but checks nothing.
//!
//! Because an all-discard body ALSO has no assertions, this fixture
//! trips BOTH the `no-op-io` detector (penalty 8) AND the
//! `zero-assertion` detector (penalty 10). v0.1 STACKS the penalties
//! into one `Finding` (`scrap_score: 18.0`) — Option A; the
//! supersede-vs-stack precedence policy is deferred to the scrap-rs#32
//! score aggregator.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn writes_a_file_but_checks_nothing() {
    let _ = std::fs::write("/tmp/scrap-no-op-io-example.txt", b"data");
}
