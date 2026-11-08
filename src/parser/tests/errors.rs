use super::*;

#[test]
fn rejects_missing_semicolon() {
    let err = parse_err("let x = 1");
    assert!(err.contains("expected ';'"), "got: {err}");
}

#[test]
fn rejects_unknown_type() {
    let err = parse_err("let x: object = 1;");
    assert!(err.contains("unsupported type 'object'"), "got: {err}");
}

#[test]
fn rejects_let_without_type_or_initializer() {
    let err = parse_err("let x;");
    assert!(err.contains("needs a type annotation or an initializer"), "got: {err}");
}

#[test]
fn rejects_assignment_to_non_variable() {
    let err = parse_err("1 = 2;");
    assert!(err.contains("invalid assignment target"), "got: {err}");
}

#[test]
fn rejects_call_to_non_function() {
    // A call target must be a bare name; `(1)(2)` is not callable.
    let err = parse_err("(1)(2);");
    assert!(err.contains("can only call named functions"), "got: {err}");
}

#[test]
fn reports_the_line_of_the_error() {
    let err = parse_err("let a = 1;\nlet b = 2;\nlet c;");
    assert!(
        err.starts_with("line 3, column"),
        "got: {err}"
    );
}

#[test]
fn a_cast_is_not_an_assignment_target() {
    let err = parse_err("x as int = 5;");
    assert!(err.contains("invalid assignment target"), "got: {err}");
}

#[test]
fn rejects_a_cast_to_void() {
    let err = parse_err("x as void;");
    assert!(err.contains("cannot cast to 'void'"), "got: {err}");
}

#[test]
fn an_increment_is_not_an_expression() {
    // TypeScript's postfix `x++` yields the old value, which this subset
    // does not model — so it is a statement, not an expression.
    let err = parse_err("print(x++);");
    assert!(err.contains("PlusPlus"), "got: {err}");
}

#[test]
fn rejects_a_compound_assignment_to_something_that_is_not_a_variable() {
    let err = parse_err("1 += 2;");
    assert!(err.contains("invalid assignment target"), "got: {err}");
}

#[test]
fn rejects_a_cast_to_a_non_type() {
    let err = parse_err("x as y;");
    assert!(err.contains("unsupported type 'y'"), "got: {err}");
}
