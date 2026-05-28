//! Zero-assertion smell — minimal triggering example.
//!
//! The body invokes some computation but never asserts anything
//! about the result. No explicit assertion macros
//! (`assert!`/`assert_eq!`/`assert_ne!`/`panic!`/`unimplemented!`/
//! `todo!`); no implicit-assertion sources (`#[should_panic]`,
//! `proptest!`, `quickcheck!`, `cucumber`, `trybuild`,
//! `insta::assert_*!`, `pretty_assertions::*`, `kani::*`); no
//! `.unwrap()` / `.expect(...)` method-call chain.
//!
//! See `README.md` for the smell + fix pair.

#[test]
fn it_does_a_thing() {
    let _value = 1 + 1;
}
