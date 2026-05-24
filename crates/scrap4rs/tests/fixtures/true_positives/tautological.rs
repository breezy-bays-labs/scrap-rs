// Fixture: a #[test] fn with two tautological assertion macros.
//
// The parser must surface `ParsedTest::assertions` with two entries:
// one `assert` (raw_args `"true"`) and one `assert_eq` (raw_args
// `"1 , 1"` — proc-macro2's `TokenStream::Display` inserts whitespace
// around the comma, which is fine for `raw_args` since the field's
// contract is "verbatim source-byte fidelity with proc-macro2's
// Display rules"; detector-side tautology classification is the
// downstream consumer that decides what to do with the args).

#[test]
fn assert_true_is_tautological() {
    assert!(true);
    assert_eq!(1, 1);
}
