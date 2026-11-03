//! The harness the end-to-end tests share: compile a program, run it, and
//! compare what it printed.

use std::path::PathBuf;
use std::process::Command;

pub fn out_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("tsc-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn compile_example(example: &str) -> PathBuf {
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

pub fn compile_and_run(example: &str) -> String {
    let bin = compile_example(example);

    let out = Command::new(&bin).output().expect("failed to run binary");
    assert!(out.status.success(), "{} exited with failure", example);
    String::from_utf8(out.stdout).unwrap()
}

#[allow(dead_code)] // used by the e2e tests only
pub fn compile_fails(source: &str, expected_error: &str) {
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
#[allow(dead_code)] // used by the rewrite tests only
pub fn run_source(name: &str, source: &str) -> String {
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
