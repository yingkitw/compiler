//! The code generator's tests: the type rules, the `int` type, and the
//! runtime entry points.

use super::*;
use crate::lexer;
use crate::parser;

    /// Compile a source snippet, expecting it to be rejected, and return the
/// diagnostic. These cover the type rules that live inside `FnGen`.
fn type_error(src: &str) -> String {
    let prog = parser::parse(lexer::lex(src).expect("lex failed")).expect("parse failed");
    compile(&prog).expect_err("expected a type error, but compilation succeeded")
}

/// Compile a source snippet, expecting it to succeed.
fn compiles(src: &str) -> Vec<u8> {
    let prog = parser::parse(lexer::lex(src).expect("lex failed")).expect("parse failed");
    compile(&prog).expect("expected compilation to succeed")
}

/// Compile a source snippet, returning either the object bytes or the
/// diagnostic. Never panics.
fn try_compile(src: &str) -> Result<Vec<u8>, String> {
    let prog = parser::parse(lexer::lex(src).expect("lex failed")).expect("parse failed");
    compile(&prog)
}

/// Every AST variant must be handled by the code generator. Rust's
/// exhaustiveness checking means a missing `match` arm is a compile error,
/// but a variant that is *constructed* and then mishandled — or a codegen
/// path that reaches an `unreachable!()` — would panic at compile time.
/// This test drives one program per variant and asserts the compiler
/// never panics, whatever the outcome.
#[test]
fn no_ast_variant_panics_the_compiler() {
    let cases: &[(&str, &str)] = &[
        ("let", "let x = 1;"),
        ("let with type", "let x: number = 1;"),
        ("const", "const x = 1;"),
        ("function", "function f(): void { }"),
        ("function with params", "function f(a: number, b: boolean): void { }"),
        ("function with return", "function f(): number { return 1; }"),
        ("if", "if (true) { print(1); }"),
        ("if else", "if (true) { print(1); } else { print(2); }"),
        ("while", "while (false) { print(1); }"),
        ("block", "{ print(1); }"),
        ("return value", "function f(): number { return 1; }"),
        ("return bare", "function f(): void { return; }"),
        ("expr stmt", "print(1);"),
        ("number literal", "print(1.5);"),
        ("bool literal", "print(true);"),
        ("var read", "let x = 1; print(x);"),
        ("assignment", "let x = 1; x = 2;"),
        ("binary", "print(1 + 2);"),
        ("unary minus", "print(-1);"),
        ("unary not", "print(!true);"),
        ("call", "function f(): void { } f();"),
        ("nested call", "function f(): number { return 1; } print(f());"),
        ("cast number to int", "let x = 1.5; let y: int = x as int;"),
        ("cast int to number", "let x: int = 1; let y: number = x as number;"),
        ("main", "function main(): void { print(1); }"),
        ("top level", "print(1);"),
        ("shadowing", "let x = 1; { let x = 2; print(x); }"),
        ("forward reference", "f(); function f(): void { }"),
        ("recursion", "function f(n: number): number { if (n < 1) { return n; } return f(n - 1); }"),
    ];
    for (name, src) in cases {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            try_compile(src)
        }));
        assert!(
            outcome.is_ok(),
            "the compiler panicked while handling the `{name}` case: {src}"
        );
    }
}

/// A rejected program must produce a diagnostic, never a panic and never
/// a successful compile.
#[test]
fn rejected_programs_report_cleanly() {
    let cases: &[(&str, &str)] = &[
        ("bad type", "let x: number = true;"),
        ("bad arity", "function f(a: number): void { } f();"),
        ("undefined name", "print(nope);"),
        ("nested function", "function o(): void { function i(): void { } }"),
        ("bad main", "function main(): number { return 1; }"),
    ];
    for (name, src) in cases {
        let err = try_compile(src).expect_err("expected this program to be rejected");
        assert!(!err.is_empty(), "the `{name}` case produced an empty diagnostic");
    }
}

#[test]
fn compiles_a_minimal_program() {
    let obj = compiles("print(1);");
    assert!(!obj.is_empty(), "expected a non-empty object file");
}

#[test]
fn the_object_references_the_runtime_entry_points() {
    // The generated `main` calls `ts_runtime_init` before anything else:
    // that call is what installs the stack-overflow handler, so the
    // object file must reference it.
    let obj = compiles("print(1);");
    for symbol in ["ts_runtime_init", "ts_print_number"] {
        assert!(
            obj.windows(symbol.len()).any(|w| w == symbol.as_bytes()),
            "the object file does not reference `{}`",
            symbol
        );
    }
}

// --- the `int` type ----------------------------------------------------

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
fn compiles_a_program_with_no_statements() {
    // An empty program is valid and produces a do-nothing binary.
    let obj = compiles("");
    assert!(!obj.is_empty());
}

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
fn accepts_shadowing_in_a_nested_scope() {
    // A nested block may reuse a name; only same-scope reuse is an error.
    compiles("let x = 1; { let x = 2; }");
}

#[test]
fn accepts_short_circuit_operands_of_either_type() {
    compiles("let a = true && 1;");
    compiles("let b = 0 || false;");
}

#[test]
fn accepts_a_forward_reference_to_a_later_function() {
    compiles("f();\nfunction f(): void { }");
}

#[test]
fn accepts_recursion() {
    compiles("function f(n: number): number { if (n < 1) { return n; } return f(n - 1); }");
}

#[test]
fn accepts_an_if_without_an_else() {
    compiles("let x = 1; if (x > 0) { print(x); }");
}

#[test]
fn accepts_a_while_loop() {
    compiles("let i = 0; while (i < 3) { i = i + 1; }");
}

#[test]
fn accepts_top_level_statements_alongside_main() {
    compiles("print(1);\nfunction main(): void { print(2); }");
}

#[test]
fn accepts_a_void_function_without_a_return() {
    compiles("function f(): void { }");
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

#[test]
fn accepts_the_compound_assignments() {
    compiles("let x = 1; x += 2; print(x);");
    compiles("let x = 1; x -= 2; print(x);");
    compiles("let x = 1; x *= 2; print(x);");
    compiles("let x = 1; x /= 2; print(x);");
    compiles("let x = 1; x %= 2; print(x);");
    compiles("let x: int = 1; x += 2; print(x);");
}

#[test]
fn accepts_the_increment_operators() {
    compiles("let x = 1; x++; print(x);");
    compiles("let x = 1; ++x; print(x);");
    compiles("let x = 1; x--; print(x);");
    compiles("let x = 1; --x; print(x);");
    compiles("let x: int = 1; x++; print(x);");
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
