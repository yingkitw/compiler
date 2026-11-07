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
fn a_for_loop_desugars_into_its_while() {
    // `for (let i = 0; i < n; i = i + 1) { body }` must parse as a block of
    // [the init, a while whose body is [the body, the step]] — the shape the
    // code generator already knows, with nothing new to learn.
    let prog = parse_src("for (let i = 0; i < n; i = i + 1) { print(i); }");
    let Stmt::Block(stmts) = &prog.stmts[0] else {
        panic!("expected a block, got {:?}", prog.stmts[0]);
    };
    assert_eq!(stmts.len(), 2, "block is [init, while]");
    match (&stmts[0], &stmts[1]) {
        (Stmt::Let { name, .. }, Stmt::While { cond, body }) => {
            assert_eq!(name, "i");
            // The condition survives intact.
            match cond {
                Expr::Bin { op: BinOp::Lt, .. } => {}
                other => panic!("expected the condition, got {other:?}"),
            }
            // The step is appended after the loop body.
            assert_eq!(body.len(), 2, "body is [loop body, step]");
            match &body[1] {
                Stmt::Expr(Expr::Assign { name, .. }) => assert_eq!(name, "i"),
                other => panic!("expected the step assignment, got {other:?}"),
            }
        }
        (init, while_) => panic!("expected [let, while], got {init:?} / {while_:?}"),
    }
}

#[test]
fn a_for_loop_without_init_or_step_is_just_the_while() {
    // No init means there is nothing to wrap in a block: `for (;;) { x; }`
    // is `while (true) { x; }`.
    let prog = parse_src("for (;;) { x; }");
    match &prog.stmts[0] {
        Stmt::While { cond, body } => {
            match cond {
                Expr::Bool(true) => {}
                other => panic!("expected a true condition, got {other:?}"),
            }
            assert_eq!(body.len(), 1);
        }
        other => panic!("expected a while, got {other:?}"),
    }
}

#[test]
fn two_for_loops_may_each_declare_the_same_name() {
    // Each init lives in its own desugared block, so this is legal — the
    // same program without `for` would be, too.
    let prog = parse_src(
        "for (let i = 0; i < 3; i = i + 1) { }\n\
         for (let i = 0; i < 3; i = i + 1) { }\n",
    );
    assert_eq!(prog.stmts.len(), 2);
}

#[test]
fn a_for_loop_rejects_a_missing_semicolon() {
    let err = parse_err("for (let i = 0 i < 3; i = i + 1) { }");
    assert!(
        err.contains("expected ';'"),
        "expected a missing-semicolon diagnostic, got: {err}"
    );
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

#[test]
fn parses_a_bare_cast() {
    match only_expr("x as int;") {
        Expr::Cast { expr, ty } => {
            assert!(matches!(&*expr, Expr::Var(v) if v == "x"));
            assert_eq!(ty, Type::Int);
        }
        other => panic!("expected a cast, got {other:?}"),
    }
}

#[test]
fn a_cast_wraps_the_expression_to_its_left() {
    // TypeScript's effective precedence: `1 + 2 as int` is `(1 + 2) as int`.
    match only_expr("1 + 2 as int;") {
        Expr::Cast { expr, ty } => {
            assert_eq!(ty, Type::Int);
            assert!(
                matches!(&*expr, Expr::Bin { op: BinOp::Add, .. }),
                "expected the cast to wrap the sum, got {expr:?}"
            );
        }
        other => panic!("expected a cast, got {other:?}"),
    }
}

#[test]
fn a_cast_result_keeps_taking_operators() {
    // `x as int * 3` is `(x as int) * 3`, not `x as (int * 3)`.
    match only_expr("x as int * 3;") {
        Expr::Bin { op: BinOp::Mul, lhs, .. } => {
            assert!(
                matches!(&*lhs, Expr::Cast { ty: Type::Int, .. }),
                "expected the cast to be the left operand, got {lhs:?}"
            );
        }
        other => panic!("expected a multiplication, got {other:?}"),
    }
}

#[test]
fn a_cast_after_a_comparison_wraps_the_whole_comparison() {
    // `a < b as int` is `(a < b) as int` — TypeScript's precedence — which
    // is then a type error: the comparison yields a boolean.
    match only_expr("a < b as int;") {
        Expr::Cast { expr, ty } => {
            assert_eq!(ty, Type::Int);
            assert!(
                matches!(&*expr, Expr::Bin { op: BinOp::Lt, .. }),
                "expected the cast to wrap the comparison, got {expr:?}"
            );
        }
        other => panic!("expected a cast, got {other:?}"),
    }
}

#[test]
fn casts_chain_left_to_right() {
    match only_expr("x as int as number;") {
        Expr::Cast { expr, ty } => {
            assert_eq!(ty, Type::Number);
            assert!(
                matches!(&*expr, Expr::Cast { ty: Type::Int, .. }),
                "expected the inner cast, got {expr:?}"
            );
        }
        other => panic!("expected the outer cast, got {other:?}"),
    }
}

#[test]
fn a_cast_is_not_an_assignment_target() {
    let err = parse_err("x as int = 5;");
    assert!(err.contains("invalid assignment target"), "got: {err}");
}

#[test]
fn rejects_a_cast_to_void() {
    let err = parse_err("x as void;");
    assert!(err.contains("cannot cast to 'void'"), "got: {err}");
}

#[test]
fn rejects_a_cast_to_a_non_type() {
    let err = parse_err("x as y;");
    assert!(err.contains("unsupported type 'y'"), "got: {err}");
}
