//! End-to-end tests: compile each example to a native binary and run it.

mod common;

use common::{compile_and_run, compile_example, compile_fails, out_dir, run_source};

use std::process::Command;

#[test]
fn hello() {
    assert_eq!(compile_and_run("hello"), "42\n");
}

#[test]
fn fib() {
    assert_eq!(compile_and_run("fib"), "6765\n");
}

#[test]
fn control() {
    assert_eq!(
        compile_and_run("control"),
        "45\ntrue\n1\nfalse\ntrue\ntrue\n"
    );
}

#[test]
fn main_fn() {
    assert_eq!(compile_and_run("main"), "42\ntrue\n12.56636\n");
}

#[test]
fn for_loop() {
    // The example documents each figure; the desugar must produce exactly
    // the hand-written `while` program's output.
    assert_eq!(
        compile_and_run("for_loop"),
        "10\n3\n2\n1\n130\n14\n1024\n"
    );
}

/// The desugared `for` must mean the same thing as the `while` it becomes.
/// Differential, like the rewrite tests: compile both, assert the same.
#[test]
fn a_for_loop_means_the_same_as_the_while_it_desugars_to() {
    let for_program = "function sum(n: int): int {\n\
                       \x20   let total: int = 0;\n\
                       \x20   for (let i: int = 0; i <= n; i = i + 1) {\n\
                       \x20       total = total + i;\n\
                       \x20   }\n\
                       \x20   return total;\n\
                       }\n\
                       print(sum(100));\n";
    let while_program = "function sum(n: int): int {\n\
                         \x20   let total: int = 0;\n\
                         \x20   let i: int = 0;\n\
                         \x20   while (i <= n) {\n\
                         \x20       total = total + i;\n\
                         \x20       i = i + 1;\n\
                         \x20   }\n\
                         \x20   return total;\n\
                         }\n\
                         print(sum(100));\n";
    assert_eq!(
        run_source("for_desugar_for", for_program),
        run_source("for_desugar_while", while_program)
    );
}

/// A `for` whose init declares the loop variable must not leak it: the
/// variable is scoped to the desugared block.
#[test]
fn a_for_loops_init_variable_is_scoped_to_the_loop() {
    let source = "for (let i = 0; i < 3; i = i + 1) { print(i); }\n\
                  print(i);\n";
    compile_fails(source, "undefined variable 'i'");
}

/// Two loops may each declare the same name in their inits.
#[test]
fn two_for_loops_may_redeclare_the_init_name() {
    let source = "let total = 0;\n\
                  for (let i = 0; i < 3; i = i + 1) { total = total + i; }\n\
                  for (let i = 0; i < 4; i = i + 1) { total = total + i; }\n\
                  print(total);\n";
    // 0+1+2 = 3, then +0+1+2+3 = 9.
    assert_eq!(run_source("for_redeclare", source), "9\n");
}

#[test]
fn number_formatting_matches_javascript() {
    // Regression test: printing used to use "%g", which silently truncated to
    // six significant digits. The runtime now emits the shortest decimal
    // string that round-trips, exactly like JavaScript's Number::toString.
    assert_eq!(
        compile_and_run("numbers"),
        "42\n\
         12.56636\n\
         0.5\n\
         0.1\n\
         1000000\n\
         100000000000000000000\n\
         1e+21\n\
         0.000001\n\
         1e-7\n\
         0.3333333333333333\n\
         Infinity\n\
         -0.5\n"
    );
}

#[test]
fn integers_are_exact() {
    // The `int` type keeps the exact value of a literal: 2^53 + 1 is not
    // representable as an f64, so a compiler that parsed literals as doubles
    // would silently print a different number.
    assert_eq!(
        compile_and_run("integers"),
        "52\n\
         32\n\
         420\n\
         4\n\
         2\n\
         false\n\
         true\n\
         43\n\
         9007199254740993\n\
         9227465\n\
         0\n"
    );
}

#[test]
fn stack_overflow_is_reported_instead_of_crashing() {
    // Unbounded recursion exhausts the stack. The program must fail with a
    // diagnostic and a non-zero exit status, not die from SIGSEGV.
    let bin = compile_example("stack_overflow");
    let out = Command::new(&bin).output().expect("failed to run binary");

    assert!(
        !out.status.success(),
        "expected a non-zero exit status, got {:?}",
        out.status
    );
    // `code()` is `None` when the process was killed by a signal, which is
    // exactly the failure mode this test guards against.
    assert_eq!(
        out.status.code(),
        Some(1),
        "expected a clean exit with status 1, got {:?}",
        out.status
    );

    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("maximum recursion depth exceeded"),
        "unexpected stderr: {:?}",
        stderr
    );

    // What the program printed before the overflow is preserved.
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "42\n");
}

#[test]
fn tail_call() {
    assert_eq!(
        compile_and_run("tail_call"),
        "3\n\
         500000500000\n\
         85\n\
         81\n\
         9\n\
         500000500000\n\
         3\n\
         2\n\
         1\n"
    );
}

#[test]
fn a_tail_recursion_can_be_much_deeper_than_the_stack() {
    // A self-recursive call in tail position becomes a loop, so it costs one
    // frame instead of one per level. Ten million levels would need about
    // 500 MiB of stack, which no machine has.
    let src = out_dir().join("very_deep_tail_recursion.ts");
    std::fs::write(
        &src,
        "function f(n: int): int {\n\
         \x20   if (n < 1) { return 0; }\n\
         \x20   return f(n - 1) + 1;\n\
         }\n\
         print(f(10000000));\n",
    )
    .unwrap();
    let bin = out_dir().join("very_deep_tail_recursion");
    let status = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .status()
        .expect("failed to run tsc");
    assert!(status.success(), "compilation failed");

    let out = Command::new(&bin).output().expect("failed to run binary");
    assert!(
        out.status.success(),
        "the program exited with failure: {:?}",
        out.status
    );
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "10000000\n");
}

#[test]
fn deep_but_finite_recursion_still_works() {
    // The detector must not reject a program that fits in the stack: 50,000
    // frames is roughly 2.4 MiB of an 8 MiB stack.
    //
    // The shape is deliberately *not* a tail call — the result of the
    // recursive call is used after it returns — so the recursion really does
    // use the stack, and this test still exercises the detector.
    let src = out_dir().join("deep_recursion.ts");
    std::fs::write(
        &src,
        "function f(n: number): number {\n\
         \x20   if (n < 1) { return 0; }\n\
         \x20   let x = f(n - 1);\n\
         \x20   return x + 1;\n\
         }\n\
         print(f(50000));\n",
    )
    .unwrap();
    let bin = out_dir().join("deep_recursion");
    let status = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .status()
        .expect("failed to run tsc");
    assert!(status.success(), "compilation failed");

    let out = Command::new(&bin).output().expect("failed to run binary");
    assert!(out.status.success(), "the program exited with failure");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "50000\n");
}

#[test]
fn remainder_matches_javascript() {
    // `%` on doubles truncates toward zero and takes the sign of the
    // dividend — JavaScript's rule, and C's `fmod`, which is what the
    // runtime calls because Cranelift has no float remainder instruction.
    assert_eq!(
        compile_and_run("remainder"),
        "1.5\n\
         -1.5\n\
         1.5\n\
         2\n\
         1\n\
         1\n\
         -1\n"
    );
}

#[test]
fn type_mismatch_rejected() {
    compile_fails("let x: number = true;", "cannot initialize 'x: number'");
}

#[test]
fn const_assignment_rejected() {
    compile_fails("const c = 1; c = 2;", "cannot assign to const 'c'");
}

#[test]
fn undefined_variable_rejected() {
    compile_fails("print(nope);", "undefined variable 'nope'");
}

#[test]
fn missing_return_rejected() {
    compile_fails(
        "function f(): number { let a = 1; }",
        "must return number on all paths",
    );
}

#[test]
fn bad_arithmetic_rejected() {
    compile_fails(
        "let x = 1 + true;",
        "arithmetic operators require two numbers or two ints",
    );
}

#[test]
fn wrong_arg_count_rejected() {
    compile_fails(
        "function f(a: number): void {}\nf(1, 2);",
        "'f' takes 1 arguments, got 2",
    );
}

#[test]
fn negative_int_literals_are_ints() {
    // A negative literal is a unary minus over a literal, and it has to
    // adopt the context's type the way a positive one does — including
    // i64::MIN, whose positive form does not fit in an i64.
    let source = "let a: int = -5;\n\
                  print(a);\n\
                  let b: int = -9223372036854775808;\n\
                  print(b);\n\
                  function f(): int { return -7; }\n\
                  print(f());\n";
    assert_eq!(run_source("neg_int", source), "-5\n-9223372036854775808\n-7\n");
}

#[test]
fn an_int_division_by_zero_is_reported() {
    // The hardware traps on a zero divisor; the runtime turns that into a
    // diagnostic and a non-zero exit status, the same way it does for a
    // stack overflow. Output printed before the fault is preserved.
    let source = "print(1);\n\
                  let a: int = 1;\n\
                  let b: int = 0;\n\
                  print(a / b);\n\
                  print(2);\n";
    let src = out_dir().join("int_div_zero.ts");
    std::fs::write(&src, source).unwrap();
    let bin = out_dir().join("int_div_zero");
    let status = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .status()
        .expect("failed to run tsc");
    assert!(status.success(), "compilation failed");

    let out = Command::new(&bin).output().expect("failed to run binary");
    assert_eq!(out.status.code(), Some(1), "expected a clean exit 1");
    assert_eq!(String::from_utf8(out.stdout).unwrap(), "1\n");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains("integer division by zero"),
        "unexpected stderr: {stderr:?}"
    );
}

#[test]
fn void_is_not_a_variable_or_parameter_type() {
    compile_fails(
        "let x: void;",
        "a variable or a parameter cannot have type 'void'",
    );
    compile_fails(
        "function f(x: void): void {}",
        "a variable or a parameter cannot have type 'void'",
    );
}

#[test]
fn deeply_nested_programs_are_rejected_not_crashed() {
    // Every stage recurses over the AST, and before the depth limits a
    // program like these died with a stack overflow instead of a diagnostic.
    let parens = format!("print({}1{});", "(".repeat(100_000), ")".repeat(100_000));
    compile_fails(&parens, "nested too deeply");

    let sum = format!("print({});", ["1"].repeat(200_000).join(" + "));
    compile_fails(&sum, "nested too deeply");
}
