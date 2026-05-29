// Fixture: a #[test] fn that performs work whose Result is discarded
// and never checked — the canonical no-op-io smell.
//
// The body opens a file and drops the Result via `let _ = ...;` with
// NO follow-up assertion, no implicit-assertion source, and no
// `.unwrap()`/`.expect()` chain. The parser must surface a
// `BehavioralFact::ResultDiscarded { kind: Call }`, and BOTH the
// no-op-io detector (scrap-rs#25, penalty 8) AND the zero-assertion
// detector (penalty 10) must fire — they STACK to scrap_score 18
// (Option A; precedence policy deferred to scrap-rs#32).

#[test]
fn writes_a_file_but_checks_nothing() {
    let _ = std::fs::write("/tmp/scrap-no-op-io-fixture.txt", b"data");
}
