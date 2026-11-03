//! End-to-end tests: compile each example to a native binary and run it.

use std::path::PathBuf;
use std::process::Command;

fn out_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("tsc-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile_example(example: &str) -> PathBuf {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(format!("{}.ts", example));
    let bin = out_dir().join(example);

    let status = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .status()
        .expect("failed to run tsc");
    assert!(status.success(), "compilation of {} failed", example);
    bin
}

fn compile_and_run(example: &str) -> String {
    let bin = compile_example(example);

    let out = Command::new(&bin).output().expect("failed to run binary");
    assert!(out.status.success(), "{} exited with failure", example);
    String::from_utf8(out.stdout).unwrap()
}

fn compile_fails(source: &str, expected_error: &str) {
    let dir = out_dir();
    // Unique name per case: tests run in parallel.
    let tag = expected_error.replace(|c: char| !c.is_alphanumeric(), "_");
    let src = dir.join(format!("bad_{}.ts", tag));
    std::fs::write(&src, source).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-o")
        .arg(dir.join(format!("bad_{}", tag)))
        .output()
        .expect("failed to run tsc");
    assert!(!out.status.success(), "expected compilation to fail");
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(
        stderr.contains(expected_error),
        "expected error containing {:?}, got {:?}",
        expected_error,
        stderr
    );
}

/// Compile a program written in the test, run it, and return its stdout.
fn run_source(name: &str, source: &str) -> String {
    let dir = out_dir();
    let src = dir.join(format!("{name}.ts"));
    std::fs::write(&src, source).unwrap();
    let bin = dir.join(name);
    let status = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .status()
        .expect("failed to run tsc");
    assert!(status.success(), "compilation of {name} failed");
    let out = Command::new(&bin).output().expect("failed to run binary");
    assert!(out.status.success(), "{name} exited with failure");
    String::from_utf8(out.stdout).unwrap()
}

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
        "function f(n: number): number {\n\
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
fn a_tail_call_means_the_same_program_as_the_loop_it_replaces() {
    // The rewrite turns a self-recursive call in tail position into a loop.
    // The one thing that must not change is what the program prints, so each
    // shape is compiled twice: once with the tail call, and once as the
    // equivalent program written with a loop. The two must agree exactly.
    let cases: &[(&str, &str, &str)] = &[
        (
            "chain",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 7; }\n\
             \x20 return f(n - 1);\n\
             }\n\
             print(f(9));\n",
            "function f(n: number): number {\n\
             \x20 let r = 7;\n\
             \x20 while (n >= 1) { n = n - 1; }\n\
             \x20 return r;\n\
             }\n\
             print(f(9));\n",
        ),
        (
            "accumulator",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 0; }\n\
             \x20 return f(n - 1) + n;\n\
             }\n\
             print(f(10));\n",
            "function f(n: number): number {\n\
             \x20 let acc = 0;\n\
             \x20 while (n >= 1) { acc = acc + n; n = n - 1; }\n\
             \x20 return acc;\n\
             }\n\
             print(f(10));\n",
        ),
        (
            "accumulator_parameter",
            "function f(n: number, m: number): number {\n\
             \x20 if (n < 1) { return m; }\n\
             \x20 return f(n - 1, m + 2);\n\
             }\n\
             print(f(4, 1));\n",
            "function f(n: number, m: number): number {\n\
             \x20 while (n >= 1) { m = m + 2; n = n - 1; }\n\
             \x20 return m;\n\
             }\n\
             print(f(4, 1));\n",
        ),
        (
            "subtracting_accumulator",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 100; }\n\
             \x20 return f(n - 1) - n;\n\
             }\n\
             print(f(5));\n",
            "function f(n: number): number {\n\
             \x20 let acc = 0;\n\
             \x20 while (n >= 1) { acc = acc + n; n = n - 1; }\n\
             \x20 return 100 - acc;\n\
             }\n\
             print(f(5));\n",
        ),
        (
            "void_loop",
            "function f(n: number): void {\n\
             \x20 if (n < 1) { return; }\n\
             \x20 print(n);\n\
             \x20 f(n - 1);\n\
             }\n\
             f(4);\n",
            "function f(n: number): void {\n\
             \x20 while (n >= 1) { print(n); n = n - 1; }\n\
             }\n\
             f(4);\n",
        ),
        (
            "int_accumulator",
            "function f(n: int): int {\n\
             \x20 if (n < 1) { return 0; }\n\
             \x20 return f(n - 1) + n;\n\
             }\n\
             print(f(1000));\n",
            "function f(n: int): int {\n\
             \x20 let acc: int = 0;\n\
             \x20 while (n >= 1) { acc = acc + n; n = n - 1; }\n\
             \x20 return acc;\n\
             }\n\
             print(f(1000));\n",
        ),
        (
            "base_case_is_a_variable",
            "function f(n: number): number {\n\
             \x20 let base = 3;\n\
             \x20 if (n < 1) { return base; }\n\
             \x20 return f(n - 1);\n\
             }\n\
             print(f(4));\n",
            "function f(n: number): number {\n\
             \x20 let base = 3;\n\
             \x20 while (n >= 1) { n = n - 1; }\n\
             \x20 return base;\n\
             }\n\
             print(f(4));\n",
        ),
        (
            "two_stepping_parameters",
            "function f(n: number, m: number): number {\n\
             \x20 if (n < 1) { return m; }\n\
             \x20 return f(n - 1, m - 1);\n\
             }\n\
             print(f(4, 10));\n",
            "function f(n: number, m: number): number {\n\
             \x20 while (n >= 1) { n = n - 1; m = m - 1; }\n\
             \x20 return m;\n\
             }\n\
             print(f(4, 10));\n",
        ),
        (
            "step_of_two",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 0; }\n\
             \x20 return f(n - 2) + 1;\n\
             }\n\
             print(f(7));\n",
            "function f(n: number): number {\n\
             \x20 let acc = 0;\n\
             \x20 while (n >= 1) { acc = acc + 1; n = n - 2; }\n\
             \x20 return acc;\n\
             }\n\
             print(f(7));\n",
        ),
        (
            "reversed_condition",
            "function f(n: number): number {\n\
             \x20 if (0 >= n) { return 5; }\n\
             \x20 return f(n - 1);\n\
             }\n\
             print(f(3));\n",
            "function f(n: number): number {\n\
             \x20 while (0 < n) { n = n - 1; }\n\
             \x20 return 5;\n\
             }\n\
             print(f(3));\n",
        ),
        (
            "accumulator_on_the_right",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 0; }\n\
             \x20 return n + f(n - 1);\n\
             }\n\
             print(f(4));\n",
            "function f(n: number): number {\n\
             \x20 let acc = 0;\n\
             \x20 while (n >= 1) { acc = acc + n; n = n - 1; }\n\
             \x20 return acc;\n\
             }\n\
             print(f(4));\n",
        ),
        (
            "boolean_result",
            "function f(n: number): boolean {\n\
             \x20 if (n < 1) { return true; }\n\
             \x20 return f(n - 1);\n\
             }\n\
             print(f(3));\n",
            "function f(n: number): boolean {\n\
             \x20 while (n >= 1) { n = n - 1; }\n\
             \x20 return true;\n\
             }\n\
             print(f(3));\n",
        ),
    ];
    for (name, tail_call, loop_version) in cases {
        assert_eq!(
            run_source(&format!("tco_{name}"), tail_call),
            run_source(&format!("loop_{name}"), loop_version),
            "the {name} shape must mean the same thing either way"
        );
    }
}

#[test]
fn a_call_in_the_accumulator_keeps_its_order() {
    // Regression test. The accumulator term of a tail call is evaluated while
    // the recursion unwinds — from the innermost level outwards. A rewrite
    // that folded the term into a loop would evaluate it the other way
    // round, and a program that prints would show it.
    let tail_call = "function g(x: number): number {\n\
                     \x20 print(x);\n\
                     \x20 return x;\n\
                     }\n\
                     function f(n: number): number {\n\
                     \x20 if (n < 1) { return 0; }\n\
                     \x20 return f(n - 1) + g(n);\n\
                     }\n\
                     print(f(3));\n";
    let expected = "1\n2\n3\n6\n";
    assert_eq!(run_source("tco_order", tail_call), expected);
}

#[test]
fn a_local_before_the_base_case_keeps_its_value() {
    // Regression test. A statement before the base case runs once per
    // recursion level in the source program, so folding the recursion into a
    // loop would leave the local with the outermost level's value.
    let tail_call = "function f(n: number): number {\n\
                     \x20 let x = n * 2;\n\
                     \x20 if (n < 1) { return 0; }\n\
                     \x20 return f(n - 1) + x;\n\
                     }\n\
                     print(f(4));\n";
    // 0 + 2 + 4 + 6 + 8
    assert_eq!(run_source("tco_local", tail_call), "20\n");
}

#[test]
fn a_void_tail_recursion_can_be_much_deeper_than_the_stack() {
    // The void shape takes the same route: the call in statement position
    // becomes a loop, so a million levels cost one frame.
    let source = "function f(n: number): void {\n\
                  \x20 if (n < 1) { return; }\n\
                  \x20 f(n - 1);\n\
                  }\n\
                  f(10000000);\n\
                  print(1);\n";
    assert_eq!(run_source("tco_deep_void", source), "1\n");
}

#[test]
fn recurrence() {
    assert_eq!(
        compile_and_run("recurrence"),
        "2880067194370816120\n\
         190392490709135\n\
         -1\n\
         1\n"
    );
}

#[test]
fn a_recurrence_means_the_same_program_as_the_loop_it_replaces() {
    // A linear recurrence — a function defined in terms of its own previous
    // values — is turned into a loop. The only thing that may not change is
    // what the program prints, so each shape is compiled twice: once with
    // the recurrence, and once as the same program written so that no
    // rewrite can fire on it.
    let cases: &[(&str, &str, &str)] = &[
        (
            "fibonacci",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 return f(n - 1) + f(n - 2);\n\
             }\n\
             print(f(20));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(20));\n",
        ),
        (
            "swapped_calls",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 return f(n - 2) + f(n - 1);\n\
             }\n\
             print(f(20));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(20));\n",
        ),
        (
            "inclusive_base_case",
            "function f(n: int): int {\n\
             \x20 if (n <= 1) { return 1; }\n\
             \x20 return f(n - 1) + f(n - 2);\n\
             }\n\
             print(f(20));\n",
            "function f(n: int): int {\n\
             \x20 if (n <= 1) { return 1; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(20));\n",
        ),
        (
            "base_case_uses_the_parameter",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n * 10; }\n\
             \x20 return f(n - 1) + f(n - 2);\n\
             }\n\
             print(f(12));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n * 10; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(12));\n",
        ),
        (
            "multiplication",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return 1; }\n\
             \x20 return f(n - 1) * f(n - 2);\n\
             }\n\
             print(f(10));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return 1; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a * b;\n\
             }\n\
             print(f(10));\n",
        ),
        (
            "a_second_parameter",
            "function f(n: int, m: int): int {\n\
             \x20 if (n < 2) { return m; }\n\
             \x20 return f(n - 1, m) + f(n - 2, m);\n\
             }\n\
             print(f(15, 3));\n",
            "function f(n: int, m: int): int {\n\
             \x20 if (n < 2) { return m; }\n\
             \x20 let a = f(n - 1, m);\n\
             \x20 let b = f(n - 2, m);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(15, 3));\n",
        ),
    ];
    for (name, recurrence, spelled_out) in cases {
        assert_eq!(
            run_source(&format!("rec_{name}"), recurrence),
            run_source(&format!("plain_{name}"), spelled_out),
            "the {name} recurrence must mean the same thing either way"
        );
    }
}

#[test]
fn a_recursion_can_be_deeper_than_the_stack_once_it_becomes_a_loop() {
    // fib(90) is 2^90 calls in the source program: it could never finish,
    // and it would exhaust the stack long before that. The rewrite turns the
    // recurrence into a loop, so the program computes the answer at once.
    let source = "function f(n: int): int {\n\
                  \x20 if (n < 2) { return n; }\n\
                  \x20 return f(n - 1) + f(n - 2);\n\
                  }\n\
                  print(f(90));\n";
    assert_eq!(run_source("rec_deep", source), "2880067194370816120\n");
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
