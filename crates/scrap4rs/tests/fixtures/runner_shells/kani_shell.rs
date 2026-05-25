// Fixture: a #[test] fn that delegates to `kani::*` (formal
// verification harness).
//
// The parser must surface `implicit_assertion_sources: [Kani]`. The
// `kani::*` prefix in recognise() matches any path under the kani
// namespace; here `kani::any!()` is the canonical sentinel.

#[test]
fn it() {
    let x: u32 = kani::any!();
    let _ = x;
}
