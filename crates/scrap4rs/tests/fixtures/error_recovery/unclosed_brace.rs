// Fixture: literal unclosed-brace source.
//
// The parser must surface `ParseError::Syntax { message, span }`
// with a localised `Span` (proc-macro2 reports the error position).
// Exercises the happy path through `parse_error_from_syn_error`.
//
// This file is NOT compiled — it's parsed as a string by
// parser_snapshots.rs's `snapshot_unclosed_brace` test, which
// asserts the Err shape via insta YAML snapshot.

fn missing_brace() {
