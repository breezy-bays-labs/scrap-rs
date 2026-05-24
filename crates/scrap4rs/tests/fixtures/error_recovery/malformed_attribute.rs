// Fixture: malformed attribute syntax (unclosed paren inside
// attribute args).
//
// The parser must surface `ParseError::Syntax { message, span }`
// with a localised `Span` pointing into the attribute. Exercises
// `parse_error_from_syn_error` on a different syn-error class than
// `unclosed_brace.rs` — attribute-parse failure vs item-parse
// failure.

#[test(garbage(
fn it() {}
