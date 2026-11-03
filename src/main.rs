//! tsc — a tiny TypeScript-subset compiler that produces native binaries.
//!
//! Usage: tsc <input.ts> [-o <output>] [-c]

mod ast;
mod codegen;
mod lexer;
mod parser;
mod rewrite;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut compile_only = false;

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-o" | "--output" => {
                let Some(path) = args.next() else {
                    eprintln!("error: -o requires a path");
                    return ExitCode::from(2);
                };
                output = Some(PathBuf::from(path));
            }
            "-c" | "--compile-only" => compile_only = true,
            "-V" | "--version" => {
                println!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "-h" | "--help" => {
                println!("usage: tsc <input.ts> [-o <output>] [-c]");
                println!("  -o <path>   output path (default: input file name without extension)");
                println!("  -c          emit an object file only, do not link");
                println!("  -V          print the version and exit");
                return ExitCode::SUCCESS;
            }
            _ if arg.starts_with('-') => {
                eprintln!("error: unknown flag '{}'", arg);
                return ExitCode::from(2);
            }
            _ => {
                if input.is_some() {
                    eprintln!("error: multiple input files given");
                    return ExitCode::from(2);
                }
                input = Some(PathBuf::from(arg));
            }
        }
    }

    let Some(input) = input else {
        eprintln!("usage: tsc <input.ts> [-o <output>] [-c]");
        return ExitCode::from(2);
    };

    match run(&input, output, compile_only) {
        Ok(out) => {
            println!("{}", out.display());
            ExitCode::SUCCESS
        }
        Err(msg) => {
            eprintln!("error: {}", msg);
            ExitCode::FAILURE
        }
    }
}

fn run(input: &Path, output: Option<PathBuf>, compile_only: bool) -> Result<PathBuf, String> {
    let src = std::fs::read_to_string(input)
        .map_err(|e| format!("cannot read {}: {}", input.display(), e))?;

    let toks = lexer::lex(&src)?;
    let mut prog = parser::parse(toks)?;
    // The source-to-source rewrites run between parsing and code generation:
    // a self-recursive call in tail position becomes a loop, and a linear
    // recurrence becomes a loop as well.
    rewrite::optimise(&mut prog);
    let obj = codegen::compile(&prog)?;

    let stem = input.with_extension("");
    if compile_only {
        let out = output.unwrap_or_else(|| stem.with_extension("o"));
        std::fs::write(&out, obj).map_err(|e| format!("cannot write {}: {}", out.display(), e))?;
        return Ok(out);
    }

    // Link the object with the tiny C runtime using the system C compiler.
    let out = output.unwrap_or(stem);
    let tmp = std::env::temp_dir().join(format!("tsc-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let obj_path = tmp.join("a.o");
    let rt_path = tmp.join("runtime.c");
    std::fs::write(&obj_path, obj).map_err(|e| e.to_string())?;
    std::fs::write(&rt_path, codegen::RUNTIME_C).map_err(|e| e.to_string())?;

    let status = std::process::Command::new("cc")
        .arg(&obj_path)
        .arg(&rt_path)
        // The runtime uses libm's `fmod` for the float remainder. On macOS
        // libm is part of libSystem and this flag is a no-op; on Linux it is
        // needed to resolve the symbol.
        .arg("-lm")
        .arg("-o")
        .arg(&out)
        .status()
        .map_err(|e| format!("failed to run 'cc' (is a C compiler installed?): {}", e))?;
    let _ = std::fs::remove_dir_all(&tmp);
    if !status.success() {
        return Err("linker failed".into());
    }
    Ok(out)
}
