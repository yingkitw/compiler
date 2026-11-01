//! End-to-end tests: compile each example to a native binary and run it.

use std::path::PathBuf;
use std::process::Command;

fn out_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("tsc-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn compile_and_run(example: &str) -> String {
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
         9227465\n"
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
