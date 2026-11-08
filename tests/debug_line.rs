//! The generated object carries a DWARF line table back to the `.ts` source.

mod common;

use common::out_dir;

use std::path::PathBuf;
use std::process::Command;

#[test]
fn the_object_file_maps_code_to_source_lines() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/hello.ts");
    let obj = out_dir().join("hello_debug.o");

    let status = Command::new(env!("CARGO_BIN_EXE_tsc"))
        .arg(&src)
        .arg("-c")
        .arg("-o")
        .arg(&obj)
        .status()
        .expect("failed to run tsc");
    assert!(status.success(), "compile -c failed");

    let dump = Command::new("dwarfdump")
        .arg("--debug-line")
        .arg(&obj)
        .output()
        .or_else(|_| {
            Command::new("llvm-dwarfdump")
                .arg(&obj)
                .arg("-debug-line")
                .output()
        })
        .expect("dwarfdump or llvm-dwarfdump must be on PATH");
    assert!(
        dump.status.success(),
        "dwarfdump failed: {}",
        String::from_utf8_lossy(&dump.stderr)
    );
    let text = String::from_utf8_lossy(&dump.stdout);
    assert!(
        text.contains("hello.ts"),
        "line table should name hello.ts, got:\n{text}"
    );
    assert!(
        text.contains("      2 ") || text.contains("\t\t2\t"),
        "hello.ts line 2 should appear in the line table, got:\n{text}"
    );
}
