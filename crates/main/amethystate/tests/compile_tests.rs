// The .stderr goldens quote rustc diagnostics, and rustc qualifies type
// paths differently once other backends are in scope. They are checked in
// the canonical single-backend config only.
#[cfg(all(
    feature = "redb",
    not(feature = "json"),
    not(feature = "toml"),
    not(feature = "ron"),
    not(feature = "sqlite")
))]
#[test]
fn test_macro_expansion_compilation() {
    let t = trybuild::TestCases::new();
    t.pass("tests/expand/basic.rs");
    t.pass("tests/expand/nested.rs");
    t.pass("tests/expand/composition.rs");
    t.pass("tests/expand/external_linked_nested.rs");
    t.pass("tests/expand/map_syntax.rs");

    t.compile_fail("tests/fails/lookup_wrong_name.rs");
    t.compile_fail("tests/fails/lookup_type_mismatch.rs");
    t.compile_fail("tests/fails/lookup_write_violation.rs");
    t.compile_fail("tests/fails/lookup_deep_error.rs");
    t.compile_fail("tests/fails/lookup_node_not_struct.rs");
    t.compile_fail("tests/fails/local_scope_not_send.rs");
}
