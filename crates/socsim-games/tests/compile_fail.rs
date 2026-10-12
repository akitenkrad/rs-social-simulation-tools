#[test]
fn p1_type_boundaries_are_enforced() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/compile_fail/*.rs");
}

#[cfg(feature = "factory")]
#[test]
fn factory_type_boundaries_are_enforced() {
    let tests = trybuild::TestCases::new();
    tests.compile_fail("tests/compile_fail_factory/*.rs");
}
