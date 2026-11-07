//! Run the examples under the address and undefined-behaviour sanitizers.
//!
//! The generated code is not instrumented — Cranelift emits no sanitizer
//! metadata — so this checks the *runtime* the generated code is linked
//! against: the number formatting, the signal handler, and the ABI between
//! the two. It is also a gate: a change to the runtime that introduces
//! undefined behaviour fails here rather than in a user's program.
//!
//! The test skips itself when the sanitizers are not available, the same way
//! the benchmark skips a toolchain it cannot find.

use std::path::PathBuf;
use std::process::Command;

/// The flags that make a C program report undefined behaviour and abort on
/// it. Without `-fno-sanitize-recover` the sanitizers print a diagnostic and
/// carry on, which a test cannot see.
const SANITIZER_FLAGS: &str = "-fsanitize=address,undefined -fno-sanitize-recover=all";

fn out_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("tsc-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Can this machine build a program with the sanitizers?
///
/// The sanitizer runtimes ship with clang, so the check is simply whether
/// such a program compiles and runs.
fn sanitizers_available() -> bool {
    let dir = out_dir();
    let src = dir.join("sanitizer_probe.c");
    let bin = dir.join("sanitizer_probe");
    std::fs::write(
        &src,
        "int main(void) { int x = 1; return x - 1; }\n",
    )
    .unwrap();
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(&cc)
        .args(SANITIZER_FLAGS.split_whitespace())
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        .status();
    match status {
        Ok(s) if s.success() => Command::new(&bin).status().map(|s| s.success()).unwrap_or(false),
        _ => false,
    }
}

/// Compile an example with the sanitizers and run it.
///
/// Returns the child's exit status. A sanitizer report makes the program
/// abort, so a failure here is a finding, not a flake.
fn run_sanitized(example: &str) -> std::process::ExitStatus {
    let dir = out_dir();
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join(format!("{example}.ts"));
    let bin = dir.join(format!("{example}_sanitized"));

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let status = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-o")
        .arg(&bin)
        // The compiler links with `CC` and `CFLAGS`; the sanitizers have to
        // be in both, because the object and the runtime are linked together.
        .env("CC", &cc)
        .env("CFLAGS", SANITIZER_FLAGS)
        .status()
        .expect("failed to run tsc");
    assert!(status.success(), "compilation of {example} failed");

    Command::new(&bin).output().expect("failed to run binary").status
}

#[test]
fn the_examples_are_clean_under_the_sanitizers() {
    if !sanitizers_available() {
        eprintln!("skipping: the sanitizers are not available on this machine");
        return;
    }
    // `stack_overflow` is deliberately run out of stack, which is what the
    // address sanitizer intercepts before the runtime's own handler can.
    for example in [
        "hello",
        "fib",
        "control",
        "main",
        "numbers",
        "integers",
        "remainder",
        "tail_call",
        "recurrence",
        "for_loop",
        "cast",
    ] {
        let status = run_sanitized(example);
        assert!(
            status.success(),
            "{example} reported undefined behaviour under the sanitizers: {status:?}"
        );
    }
}
