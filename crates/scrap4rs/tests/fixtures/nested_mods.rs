// Fixture: depth-2 module nesting test discovery for scrap-rs#12 S2.1.
//
// The parser must recover the fully-qualified name
// `auth::login_tests::it_logs_in` by maintaining a path stack as it
// recurses into nested `mod` items via `visit_item_mod`.
//
// This fixture is parsed by the cucumber harness and the insta
// snapshot at `tests/parser_snapshots.rs::snapshot_nested_mods`.
// It is NOT compiled as part of the scrap4rs crate.

mod auth {
    mod login_tests {
        #[test]
        fn it_logs_in() {}
    }
}
