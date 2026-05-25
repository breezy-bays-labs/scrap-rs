// Fixture: every test-attribute variant scrap-rs#12 S2.1 recognises.
//
// The parser must project each `#[test]`-like attribute into a
// `ParsedAttribute { name, raw }` entry on the corresponding
// `ParsedTest::attributes` vec. The `name` is the unqualified last
// path segment; `raw` is `None` for bare attrs, `Some("")` for empty
// parens, `Some(arg_text)` otherwise.
//
// `name` storage convention (pinned in S0.1): the leaf segment of the
// attribute path. So `tokio::test` becomes `name == "test"`, the same
// as bare `#[test]`. Detectors discriminate via `raw` or path-prefix
// if they ever need to.
//
// This fixture is parsed by the cucumber harness and the insta
// snapshot at `tests/parser_snapshots.rs::snapshot_attribute_variants`.

#[test]
fn bare_test() {}

#[tokio::test]
async fn tokio_test() {}

#[rstest]
fn rstest_test() {}

#[test]
#[should_panic]
fn should_panic_test() {}

#[test]
#[ignore]
fn bare_ignore() {}

#[test]
#[ignore = "flaky"]
fn ignore_with_reason() {}

#[test]
#[ignore(slow)]
fn ignore_with_list() {}
