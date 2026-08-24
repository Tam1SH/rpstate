//! The expansion goldens, behind `golden` for the same reason as
//! `compile_tests.rs`: macrotest runs the macro through a real compile per
//! case, which an ordinary run should not pay for.

#[cfg(all(feature = "golden", feature = "redb", target_os = "windows"))]
#[test]
fn test_expansion() {
    macrotest::expand("tests/expand/*.rs");
}
