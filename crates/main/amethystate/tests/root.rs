#[cfg(all(feature = "redb", target_os = "windows"))]
#[test]
fn test_expansion() {
    macrotest::expand("tests/expand/*.rs");
}
