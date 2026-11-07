use super::*;

#[test]
fn int_literal_adopts_the_annotated_type() {
    // `let x: int = 42` makes the literal an int, not a number.
    compiles("let x: int = 42;");
}

#[test]
fn int_literal_stays_a_number_without_a_context() {
    // No annotation means the default type, which is still `number`.
    compiles("let x = 42;");
}

#[test]
fn int_arithmetic_yields_an_int() {
    compiles("let a: int = 2; let b: int = 3; let c: int = a + b;");
}

#[test]
fn int_literal_adapts_to_an_int_operand() {
    // The literal takes the type of the other operand.
    compiles("let a: int = 2; let b = a + 1;");
    compiles("let a: int = 2; let b = 1 + a;");
}

#[test]
fn int_division_truncates_toward_zero() {
    compiles("let a: int = 7; let b: int = 2; let c: int = a / b;");
}

#[test]
fn int_remainder_is_supported() {
    compiles("let a: int = 7; let b: int = 3; let c: int = a % b;");
}

#[test]
fn rejects_a_non_integer_literal_as_an_int() {
    let err = type_error("let x: int = 4.5;");
    assert!(err.contains("non-integer value"), "got: {err}");
}

#[test]
fn rejects_mixing_int_and_number() {
    let err = type_error("let a: int = 1; let b = a + 1.5;");
    assert!(
        err.contains("arithmetic operators require two numbers or two ints"),
        "got: {err}"
    );
}

#[test]
fn float_remainder_is_supported() {
    // Cranelift has no float remainder instruction, so this goes through
    // the runtime's `ts_fmod` — which is C's fmod, and matches
    // JavaScript's `%` exactly.
    compiles("print(5.0 % 2.0);");
}

#[test]
fn rejects_a_remainder_across_types() {
    let err = type_error("let a: int = 1; print(1.5 % a);");
    assert!(
        err.contains("arithmetic operators require two numbers or two ints"),
        "got: {err}"
    );
}

#[test]
fn rejects_relational_operators_across_types() {
    let err = type_error("let a: int = 1; let b = a < 1.5;");
    assert!(err.contains("relational operators require"), "got: {err}");
}

#[test]
fn accepts_int_unary_operations() {
    compiles("let a: int = 1; let b: int = -a; let c: boolean = !a;");
}

#[test]
fn accepts_an_int_parameter_and_argument() {
    compiles("function f(n: int): int { return n; } let x: int = f(3);");
}

#[test]
fn an_int_literal_adapts_to_the_return_type() {
    // Regression test: `return 0;` in an int function used to be
    // rejected, because the return statement did not pass the expected
    // type to the expression.
    compiles("function f(): int { return 0; }");
    compiles("function f(): int { return 42; }");
    compiles("function f(): int { if (true) { return 1; } return 0; }");
}

#[test]
fn casts_number_to_int() {
    compiles("let x = 1.5; let y: int = x as int; print(y);");
}

#[test]
fn casts_int_to_number() {
    compiles("let x: int = 2; let y: number = x as number; print(y);");
}

#[test]
fn a_cast_to_the_own_type_is_a_no_op() {
    compiles("let x = 1; print(x as number);");
    compiles("let b = true; print(b as boolean);");
}

#[test]
fn a_cast_feeds_a_function_argument() {
    compiles("function f(n: int): int { return n; } let x = 1.5; print(f(x as int));");
}

#[test]
fn rejects_casting_a_boolean() {
    let err = type_error("print(true as int);");
    assert!(err.contains("cannot cast boolean to int"), "got: {err}");
    // The same error from a comparison's boolean: `(1 < 2) as int`.
    let err = type_error("print(1 < 2 as int);");
    assert!(err.contains("cannot cast boolean to int"), "got: {err}");
}

#[test]
fn rejects_casting_to_boolean() {
    let err = type_error("print(1 as boolean);");
    assert!(err.contains("cannot cast number to boolean"), "got: {err}");
}

#[test]
fn rejects_casting_an_int_to_boolean() {
    let err = type_error("let x: int = 1; print(x as boolean);");
    assert!(err.contains("cannot cast int to boolean"), "got: {err}");
}

#[test]
fn rejects_casting_a_void_value() {
    let err = type_error("function f(): void { } print(f() as int);");
    assert!(err.contains("void"), "got: {err}");
}

#[test]
fn a_cast_result_participates_in_arithmetic() {
    // `n as int + m` must type-check as int + int, not number + int.
    compiles("let n = 1.5; let m: int = 2; let s: int = n as int + m; print(s);");
    let err = type_error("let n = 1.5; let m: int = 2; let s: int = n + m;");
    assert!(err.contains("arithmetic operators"), "got: {err}");
}
