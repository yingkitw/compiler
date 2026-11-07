use super::*;

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
fn the_decrement_form_desugars_into_a_subtraction() {
    let prog = parse_src("x--;");
    match &prog.stmts[0] {
        Stmt::Expr(Expr::Assign { value, .. }) => {
            assert!(
                matches!(&**value, Expr::Bin { op: BinOp::Sub, .. }),
                "expected a subtraction, got {value:?}"
            );
        }
        other => panic!("expected an assignment statement, got {other:?}"),
    }
}
