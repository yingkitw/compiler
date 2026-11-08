//! The parser's tests: precedence, associativity, statement forms, and
//! parse errors.
//!
//! The tests live in three files under this directory: `stmts.rs` (the
//! statement forms), `exprs.rs` (the expressions and their sugars) and
//! `errors.rs` (the rejections). The helpers they share are here.

use super::*;
use crate::lexer;

mod errors;
mod exprs;
mod stmts;

fn parse_src(src: &str) -> Program {
    parse(lexer::lex(src).expect("lex failed")).expect("parse failed")
}

fn parse_err(src: &str) -> String {
    parse(lexer::lex(src).expect("lex failed")).expect_err("expected a parse error")
}

/// The single top-level expression statement of a program, if there is one.
fn only_expr(src: &str) -> Expr {
    let prog = parse_src(src);
    assert_eq!(prog.stmts.len(), 1, "expected exactly one statement");
    match &prog.stmts[0] {
        Stmt::Expr { expr, .. } => expr.clone(),
        other => panic!("expected an expression statement, got {other:?}"),
    }
}

