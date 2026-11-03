//! Check the generated code against the C ABI.
//!
//! The compiler emits a relocatable object file and links it with a C
//! runtime, so the calling convention has to be the platform's — not just
//! Cranelift's idea of it. The check is a C program that declares the
//! exported functions the way C would and calls them: if the parameter
//! passing, the return value or the width of a `boolean` disagreed, the
//! numbers would not come out right.

use std::path::PathBuf;
use std::process::Command;

fn out_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("tsc-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The program the C driver calls into.
const SOURCE: &str = "function scale(n: int, k: int): int {\n\
    return n * k;\n\
}\n\
function half(x: number): number {\n\
    return x / 2;\n\
}\n\
function truthy(b: boolean): boolean {\n\
    return !b;\n\
}\n";

/// A C driver that calls the exported functions the way C would.
///
/// It does not define `main`: the generated object has one, and the link
/// below gives the driver its own entry point instead.
const DRIVER: &str = r#"#include <stdio.h>
#include <stdint.h>

/* The compiler exports every function as `tsfn_<name>`. */
extern int64_t tsfn_scale(int64_t, int64_t);
extern double tsfn_half(double);
extern signed char tsfn_truthy(signed char);

int driver_entry(void) {
    printf("%lld\n", (long long)tsfn_scale(6, 7));
    printf("%g\n", tsfn_half(5.0));
    printf("%d\n", (int)tsfn_truthy(0));
    return 0;
}
"#;

/// The generated object's own `main` calls this before anything else. The
/// driver never runs it, but the linker still wants the symbol.
const STUB: &str = "void ts_runtime_init(void) {}\n";

#[test]
fn the_generated_code_follows_the_c_abi() {
    let dir = out_dir();
    let src = dir.join("abi.ts");
    let obj = dir.join("abi.o");
    let driver = dir.join("abi_driver.c");
    let stub = dir.join("abi_stub.c");
    let bin = dir.join("abi_bin");
    std::fs::write(&src, SOURCE).unwrap();
    std::fs::write(&driver, DRIVER).unwrap();
    std::fs::write(&stub, STUB).unwrap();

    // Compile to an object file: the C driver links against it.
    let status = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-c")
        .arg("-o")
        .arg(&obj)
        .status()
        .expect("failed to run tsc");
    assert!(status.success(), "compilation failed");

    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let mut cmd = Command::new(&cc);
    if let Ok(flags) = std::env::var("CFLAGS") {
        cmd.args(flags.split_whitespace());
    }
    let status = cmd
        .arg("-Wl,-e,_driver_entry")
        .arg(&obj)
        .arg(&driver)
        .arg(&stub)
        .arg("-lm")
        .arg("-o")
        .arg(&bin)
        .status()
        .expect("failed to run the C compiler");
    assert!(status.success(), "linking the C driver failed");

    let out = Command::new(&bin).output().expect("failed to run binary");
    assert!(out.status.success(), "the driver exited with failure");
    assert_eq!(
        String::from_utf8(out.stdout).unwrap(),
        "42\n2.5\n1\n",
        "the C driver saw the wrong values"
    );
}
