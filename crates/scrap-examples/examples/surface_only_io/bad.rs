//! Surface-only-io smell — minimal triggering example.
//!
//! The body writes a file and then asserts ONLY on its surface — that
//! the path exists — without ever reading the content back. The test
//! genuinely asserts (so it is NOT zero-assertion: `assert!(...)` records
//! a real assertion), and the `.unwrap()` on the write means nothing is
//! discarded (so it is NOT no-op-io). But it only ever inspected the
//! file's *existence*, not its *content*: a regression that wrote the
//! wrong bytes — or empty bytes — would leave this test green.
//!
//! This is the canonical surface-only-io shape and the honest demo of
//! the detector's value: a test can assert and STILL only look at the
//! surface. The detector groups the located filesystem facts by path key
//! and fires because the `lit:` key for the written path has a write +
//! a surface check (`exists()`) but no content read-back.
//!
//! Because the test DOES assert (suppressing zero-assertion) and does NOT
//! discard a Result (suppressing no-op-io), this fixture trips ONLY the
//! `surface-only-io` detector (penalty 6) — a clean single-smell example.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn writes_a_file_but_only_checks_existence() {
    let path = "/tmp/scrap-surface-only-io-example.txt";
    std::fs::write(path, b"important payload").unwrap();
    // Surface-only: we assert the file EXISTS, never that it holds the
    // bytes we wrote. The detector fires.
    assert!(std::path::Path::new(path).exists());
}
