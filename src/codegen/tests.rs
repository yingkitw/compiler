//! The code generator's tests: the type rules, the `int` type, and the
//! runtime entry points.
//!
//! The tests live in three files under this directory: `types.rs` (the type
//! rules), `errors.rs` (the rejections) and `programs.rs` (the programs the
//! compiler must accept, and the wiring guard). The helpers they share are
//! here.

use super::*;
use crate::lexer;
use crate::parser;

mod errors;
mod programs;
mod types;

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

