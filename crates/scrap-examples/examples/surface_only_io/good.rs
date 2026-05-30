//! Surface-only-io FIX — same shape as `bad.rs`, but reads the content
//! back and asserts on the substantive payload, not just existence.
//!
//! Replacing the surface-only `assert!(path.exists())` with a read-back
//! (`fs::read_to_string(path)`) projects a `FilesystemRead` fact on the
//! SAME path key as the write. The read disarms the surface-only-io
//! correlation for that key — the test now inspects what was actually
//! written, so a regression that wrote the wrong bytes WOULD fail the
//! test. No finding emits.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn writes_a_file_and_checks_its_content() {
    let path = "/tmp/scrap-surface-only-io-example.txt";
    std::fs::write(path, b"important payload").unwrap();
    // Read the content back and assert on the substantive payload.
    let got = std::fs::read_to_string(path).unwrap();
    assert_eq!(got, "important payload");
}
