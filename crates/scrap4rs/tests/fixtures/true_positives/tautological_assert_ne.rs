// Fixture: assert_ne!(x, x) — tautological via arguments_identical.
//
// Sibling to `tautological.rs` (which covers the assert!(true) +
// assert_eq!(1, 1) shapes). This fixture exercises the assert_ne!
// path through the same arguments_identical: true mechanism.
//
// Expected parsed shape:
//   assertions: [ParsedAssertion {
//     name: "assert_ne",
//     arguments_identical: true,
//     single_arg_value: None,
//     ...
//   }]
// Detector verdict: TautologicalAssertion smell, penalty 10.

#[test]
fn assert_ne_x_x_is_tautological() {
    let x = 1;
    assert_ne!(x, x);
}
