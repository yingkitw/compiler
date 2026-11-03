//! The parser's tests: precedence, associativity, statement forms, and
//! parse errors.

use super::*;
use crate::lexer;

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
        Stmt::Expr(e) => e.clone(),
        other => panic!("expected an expression statement, got {other:?}"),
    }
}

#[test]
fn parses_let_with_type_annotation() {
    let prog = parse_src("let x: number = 1;");
    match &prog.stmts[0] {
        Stmt::Let { name, ty, init, mutable } => {
            assert_eq!(name, "x");
            assert_eq!(ty, &Some(Type::Number));
            assert!(init.is_some());
            assert!(mutable);
        }
        other => panic!("expected a let, got {other:?}"),
    }
}

#[test]
fn parses_const_as_immutable() {
    let prog = parse_src("const x = 1;");
    match &prog.stmts[0] {
        Stmt::Let { mutable, ty, init, .. } => {
            assert!(!mutable);
            assert_eq!(ty, &None);
            assert!(init.is_some());
        }
        other => panic!("expected a let, got {other:?}"),
    }
}

#[test]
fn parses_function_declaration() {
    let prog = parse_src("function f(a: number, b: boolean): void { }");
    match &prog.stmts[0] {
        Stmt::Func { name, params, ret, .. } => {
            assert_eq!(name, "f");
            assert_eq!(params.len(), 2);
            assert_eq!(params[0], ("a".to_string(), Type::Number));
            assert_eq!(params[1], ("b".to_string(), Type::Boolean));
            assert_eq!(ret, &Some(Type::Void));
        }
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn function_without_return_type_defaults_to_void() {
    let prog = parse_src("function f() { }");
    match &prog.stmts[0] {
        Stmt::Func { ret, params, .. } => {
            assert_eq!(ret, &None);
            assert!(params.is_empty());
        }
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn parses_if_else() {
    let prog = parse_src("if (x) { y(); } else { z(); }");
    match &prog.stmts[0] {
        Stmt::If { then, els, .. } => {
            assert_eq!(then.len(), 1);
            assert_eq!(els.len(), 1);
        }
        other => panic!("expected an if, got {other:?}"),
    }
}

#[test]
fn parses_if_without_else() {
    let prog = parse_src("if (x) { y(); }");
    match &prog.stmts[0] {
        Stmt::If { then, els, .. } => {
            assert_eq!(then.len(), 1);
            assert!(els.is_empty(), "an absent else must be empty");
        }
        other => panic!("expected an if, got {other:?}"),
    }
}

#[test]
fn parses_while() {
    let prog = parse_src("while (x) { y(); }");
    match &prog.stmts[0] {
        Stmt::While { body, .. } => assert_eq!(body.len(), 1),
        other => panic!("expected a while, got {other:?}"),
    }
}

#[test]
fn parses_return_with_and_without_value() {
    let prog = parse_src("function f(): number { return 1; }");
    match &prog.stmts[0] {
        Stmt::Func { body, .. } => match &body[0] {
            Stmt::Return(Some(_)) => {}
            other => panic!("expected a return with a value, got {other:?}"),
        },
        other => panic!("expected a function, got {other:?}"),
    }
    let prog = parse_src("function f(): void { return; }");
    match &prog.stmts[0] {
        Stmt::Func { body, .. } => match &body[0] {
            Stmt::Return(None) => {}
            other => panic!("expected a bare return, got {other:?}"),
        },
        other => panic!("expected a function, got {other:?}"),
    }
}

#[test]
fn multiplication_binds_tighter_than_addition() {
    // 1 + 2 * 3 must parse as Add(1, Mul(2, 3)).
    match only_expr("1 + 2 * 3;") {
        Expr::Bin { op: BinOp::Add, rhs, .. } => match *rhs {
            Expr::Bin { op: BinOp::Mul, .. } => {}
            other => panic!("expected a multiplication on the right, got {other:?}"),
        },
        other => panic!("expected an addition, got {other:?}"),
    }
}

#[test]
fn comparison_binds_tighter_than_equality() {
    // a == b < c must parse as Eq(a, Lt(b, c)).
    match only_expr("a == b < c;") {
        Expr::Bin { op: BinOp::Eq, rhs, .. } => match *rhs {
            Expr::Bin { op: BinOp::Lt, .. } => {}
            other => panic!("expected a comparison on the right, got {other:?}"),
        },
        other => panic!("expected an equality, got {other:?}"),
    }
}

#[test]
fn and_binds_tighter_than_or() {
    match only_expr("a || b && c;") {
        Expr::Bin { op: BinOp::Or, rhs, .. } => match *rhs {
            Expr::Bin { op: BinOp::And, .. } => {}
            other => panic!("expected an and on the right, got {other:?}"),
        },
        other => panic!("expected an or, got {other:?}"),
    }
}

#[test]
fn arithmetic_is_left_associative() {
    // 1 - 2 - 3 must parse as Sub(Sub(1, 2), 3), not Sub(1, Sub(2, 3)).
    match only_expr("1 - 2 - 3;") {
        Expr::Bin { op: BinOp::Sub, lhs, rhs } => {
            assert!(matches!(*lhs, Expr::Bin { op: BinOp::Sub, .. }));
            assert!(matches!(*rhs, Expr::Num(_)));
        }
        other => panic!("expected a subtraction, got {other:?}"),
    }
}

#[test]
fn assignment_is_right_associative() {
    // a = b = c must parse as Assign(a, Assign(b, c)).
    match only_expr("a = b = c;") {
        Expr::Assign { name, value } => {
            assert_eq!(name, "a");
            match *value {
                Expr::Assign { name, .. } => assert_eq!(name, "b"),
                other => panic!("expected a nested assignment, got {other:?}"),
            }
        }
        other => panic!("expected an assignment, got {other:?}"),
    }
}

#[test]
fn unary_minus_binds_tighter_than_multiplication() {
    // -2 * 3 must parse as Mul(Neg(2), 3).
    match only_expr("-2 * 3;") {
        Expr::Bin { op: BinOp::Mul, lhs, .. } => match *lhs {
            Expr::Un { op: UnOp::Neg, .. } => {}
            other => panic!("expected a negation on the left, got {other:?}"),
        },
        other => panic!("expected a multiplication, got {other:?}"),
    }
}

#[test]
fn parses_call_arguments() {
    match only_expr("f(1, true);") {
        Expr::Call { name, args } => {
            assert_eq!(name, "f");
            assert_eq!(args.len(), 2);
        }
        other => panic!("expected a call, got {other:?}"),
    }
}

#[test]
fn parses_parenthesised_expression() {
    // (1 + 2) * 3 must parse as Mul(Add(1, 2), 3).
    match only_expr("(1 + 2) * 3;") {
        Expr::Bin { op: BinOp::Mul, lhs, .. } => match *lhs {
            Expr::Bin { op: BinOp::Add, .. } => {}
            other => panic!("expected an addition on the left, got {other:?}"),
        },
        other => panic!("expected a multiplication, got {other:?}"),
    }
}

#[test]
fn rejects_missing_semicolon() {
    let err = parse_err("let x = 1");
    assert!(err.contains("expected ';'"), "got: {err}");
}

#[test]
fn rejects_unknown_type() {
    let err = parse_err("let x: string = 1;");
    assert!(err.contains("unsupported type 'string'"), "got: {err}");
}

#[test]
fn rejects_let_without_type_or_initializer() {
    let err = parse_err("let x;");
    assert!(err.contains("needs a type annotation or an initializer"), "got: {err}");
}

#[test]
fn rejects_assignment_to_non_variable() {
    let err = parse_err("1 = 2;");
    assert!(err.contains("invalid assignment target"), "got: {err}");
}

#[test]
fn rejects_call_to_non_function() {
    // A call target must be a bare name; `(1)(2)` is not callable.
    let err = parse_err("(1)(2);");
    assert!(err.contains("can only call named functions"), "got: {err}");
}

#[test]
fn parses_call_on_a_named_variable() {
    // `x(1)` parses fine — whether `x` is a function is a codegen concern.
    match only_expr("x(1);") {
        Expr::Call { name, args } => {
            assert_eq!(name, "x");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected a call, got {other:?}"),
    }
}

#[test]
fn reports_the_line_of_the_error() {
    let err = parse_err("let a = 1;\nlet b = 2;\nlet c;");
    assert!(err.starts_with("line 3:"), "got: {err}");
}
