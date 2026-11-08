use super::*;

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
        Expr::Call {
            name,
            receiver: None,
            args,
        } => {
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
fn parses_call_on_a_named_variable() {
    // `x(1)` parses fine — whether `x` is a function is a codegen concern.
    match only_expr("x(1);") {
        Expr::Call {
            name,
            receiver: None,
            args,
        } => {
            assert_eq!(name, "x");
            assert_eq!(args.len(), 1);
        }
        other => panic!("expected a call, got {other:?}"),
    }
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
fn a_compound_assignment_desugars_into_an_assignment() {
    // `x += e` is `x = x + e` — the desugar is in the parser, so the AST
    // holds the plain assignment.
    match only_expr("x += 1;") {
        Expr::Assign { name, value } => {
            assert_eq!(name, "x");
            match &*value {
                Expr::Bin { op, lhs, rhs } => {
                    assert_eq!(*op, BinOp::Add);
                    assert!(matches!(&**lhs, Expr::Var(v) if v == "x"));
                    assert!(matches!(&**rhs, Expr::Num(_)));
                }
                other => panic!("expected a binary expression, got {other:?}"),
            }
        }
        other => panic!("expected an assignment, got {other:?}"),
    }
}

#[test]
fn parses_a_block_expression() {
    match only_expr("({ let y = 1; y + 1; });") {
        Expr::BlockExpr { stmts, value } => {
            assert_eq!(stmts.len(), 1);
            assert!(matches!(&*value, Expr::Bin { op: BinOp::Add, .. }));
        }
        other => panic!("expected a block expression, got {other:?}"),
    }
}

#[test]
fn a_conditional_binds_looser_than_addition() {
    match only_expr("true ? 1 : 2 + 3;") {
        Expr::Cond { else_expr, .. } => match &*else_expr {
            Expr::Bin { op: BinOp::Add, .. } => {}
            other => panic!("expected else branch to add, got {other:?}"),
        },
        other => panic!("expected a conditional, got {other:?}"),
    }
}

#[test]
fn every_compound_assignment_has_its_own_operator() {
    for (src, op) in [
        ("x += 1;", BinOp::Add),
        ("x -= 1;", BinOp::Sub),
        ("x *= 1;", BinOp::Mul),
        ("x /= 1;", BinOp::Div),
        ("x %= 1;", BinOp::Rem),
        ("x &&= true;", BinOp::And),
        ("x ||= false;", BinOp::Or),
    ] {
        match only_expr(src) {
            Expr::Assign { value, .. } => match &*value {
                Expr::Bin { op: got, .. } => assert_eq!(*got, op, "for {src}"),
                other => panic!("expected a binary expression, got {other:?}"),
            },
            other => panic!("expected an assignment, got {other:?}"),
        }
    }
}

#[test]
fn the_increment_forms_desugar_into_assignments() {
    // Both `x++` and `++x` mean `x = x + 1`; they are statements, so the
    // parser sees them where a statement is expected.
    for src in ["x++;", "++x;"] {
        let prog = parse_src(src);
        match &prog.stmts[0] {
            Stmt::Expr {
                expr: Expr::Assign { name, value },
                ..
            } => {
                assert_eq!(name, "x");
                assert!(
                    matches!(&**value, Expr::Bin { op: BinOp::Add, .. }),
                    "expected an addition, got {value:?}"
                );
            }
            other => panic!("expected an assignment statement, got {other:?}"),
        }
    }
}
