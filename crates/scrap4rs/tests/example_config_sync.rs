//! Byte-identity sync test for the committed `scrap.example.toml`
//! (workspace root) against fresh `scrap4rs init` output — the rot
//! guard from scrap-rs#107 ("documentation rots; CI doesn't",
//! mirroring crap-rs#347).
//!
//! Runs the REAL binary (via `CARGO_BIN_EXE_scrap4rs`) rather than the
//! library generator so the committed example is pinned to the
//! end-to-end surface a user actually invokes — including the
//! `AdapterMeta` literal that lives in `main.rs` and never leaves the
//! binary crate.
//!
//! The tempdir gets a `crates/` subdirectory so `detect_src_layout`
//! resolves `src = "crates"` (workspace-shaped, matching this repo) —
//! keeping the committed example identical to what `init` produces at
//! the scrap-rs root.

use std::process::Command;

#[test]
fn committed_example_matches_fresh_init_output() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(dir.path().join("crates")).expect("create crates/ marker dir");

    let output = Command::new(env!("CARGO_BIN_EXE_scrap4rs"))
        .arg("init")
        .current_dir(dir.path())
        .output()
        .expect("spawn `scrap4rs init`");
    assert!(
        output.status.success(),
        "`scrap4rs init` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let generated = std::fs::read_to_string(dir.path().join("scrap.toml"))
        .expect("init wrote scrap.toml in the tempdir");
    let committed = include_str!("../../../scrap.example.toml");

    assert_eq!(
        generated, committed,
        "scrap.example.toml drifted from `scrap4rs init` output.\n\
         Regenerate: in an empty dir containing a `crates/` subdir, run\n\
         `cargo run -p scrap4rs -- init` and copy the resulting\n\
         scrap.toml over scrap.example.toml at the workspace root."
    );
}
