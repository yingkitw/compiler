use super::*;

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
fn compiles_a_program_with_no_statements() {
    // An empty program is valid and produces a do-nothing binary.
    let obj = compiles("");
    assert!(!obj.is_empty());
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
