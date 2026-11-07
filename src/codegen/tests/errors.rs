use super::*;

#[test]
fn rejects_arithmetic_on_booleans() {
    let err = type_error("let x = 1 + true;");
    assert!(
        err.contains("arithmetic operators require two numbers or two ints"),
        "got: {err}"
    );
}

#[test]
fn rejects_relational_operators_on_booleans() {
    let err = type_error("let x = true < false;");
    assert!(
        err.contains("relational operators require two numbers or two ints"),
        "got: {err}"
    );
}

#[test]
fn rejects_comparing_mismatched_types() {
    let err = type_error("let x = 1 == true;");
    assert!(err.contains("cannot compare number with boolean"), "got: {err}");
}

#[test]
fn rejects_initializer_type_mismatch() {
    let err = type_error("let x: number = true;");
    assert!(err.contains("cannot initialize 'x: number'"), "got: {err}");
}

#[test]
fn rejects_assignment_type_mismatch() {
    let err = type_error("let x: number = 1; x = true;");
    assert!(err.contains("cannot assign boolean to 'x: number'"), "got: {err}");
}

#[test]
fn rejects_assignment_to_const() {
    let err = type_error("const c = 1; c = 2;");
    assert!(err.contains("cannot assign to const 'c'"), "got: {err}");
}

#[test]
fn rejects_undefined_variable() {
    let err = type_error("print(nope);");
    assert!(err.contains("undefined variable 'nope'"), "got: {err}");
}

#[test]
fn rejects_undefined_function() {
    let err = type_error("nope();");
    assert!(err.contains("undefined function 'nope'"), "got: {err}");
}

#[test]
fn rejects_wrong_argument_count() {
    let err = type_error("function f(a: number): void { }\nf(1, 2);");
    assert!(err.contains("'f' takes 1 arguments, got 2"), "got: {err}");
}

#[test]
fn rejects_wrong_argument_type() {
    let err = type_error("function f(a: number): void { }\nf(true);");
    assert!(err.contains("argument of 'f' must be number, got boolean"), "got: {err}");
}

#[test]
fn rejects_missing_return() {
    let err = type_error("function f(): number { let a = 1; }");
    assert!(err.contains("must return number on all paths"), "got: {err}");
}

#[test]
fn rejects_returning_a_value_from_a_void_function() {
    let err = type_error("function f(): void { return 1; }");
    assert!(err.contains("cannot return a value from a void function"), "got: {err}");
}

#[test]
fn rejects_returning_the_wrong_type() {
    let err = type_error("function f(): number { return true; }");
    assert!(err.contains("cannot return boolean from a function returning number"), "got: {err}");
}

#[test]
fn rejects_bare_return_from_a_value_function() {
    let err = type_error("function f(): number { return; }");
    assert!(err.contains("must return a value of type number"), "got: {err}");
}

#[test]
fn rejects_printing_void() {
    let err = type_error("function f(): void { }\nprint(f());");
    assert!(err.contains("cannot print void"), "got: {err}");
}

#[test]
fn rejects_redefining_print() {
    let err = type_error("function print(x: number): void { }");
    assert!(err.contains("'print' is a built-in"), "got: {err}");
}

#[test]
fn rejects_a_main_with_parameters() {
    let err = type_error("function main(a: number): void { }");
    assert!(err.contains("`main` must be declared as `function main(): void`"), "got: {err}");
}

#[test]
fn rejects_a_main_returning_a_value() {
    let err = type_error("function main(): number { return 1; }");
    assert!(err.contains("`main` must be declared as `function main(): void`"), "got: {err}");
}

#[test]
fn rejects_nested_function_declarations() {
    let err = type_error("function outer(): void { function inner(): void { } }");
    assert!(err.contains("nested function declarations are not supported"), "got: {err}");
}

#[test]
fn rejects_redeclaring_a_variable_in_the_same_scope() {
    let err = type_error("let x = 1; let x = 2;");
    assert!(err.contains("already declared in this scope"), "got: {err}");
}

#[test]
fn a_compound_assignment_cannot_reassign_a_const() {
    let err = type_error("const x = 1; x += 2;");
    assert!(err.contains("cannot assign to const 'x'"), "got: {err}");
}

#[test]
fn an_increment_cannot_reassign_a_const() {
    let err = type_error("const x = 1; x++;");
    assert!(err.contains("cannot assign to const 'x'"), "got: {err}");
}

#[test]
fn a_compound_assignment_type_checks_like_the_assignment_it_desugars_to() {
    // `x += 1` is `x = x + 1`, so a boolean is refused for the same reason
    // `x = x + 1` is.
    let err = type_error("let b = true; b += 1;");
    assert!(err.contains("arithmetic operators"), "got: {err}");
    // And the types must still match: an `int` variable cannot take a
    // `number` result — the desugared `x = x + 1.5` is refused for the
    // same reason `x = x + 1.5` is.
    let err = type_error("let x: int = 1; x += 1.5;");
    assert!(err.contains("arithmetic operators"), "got: {err}");
}
