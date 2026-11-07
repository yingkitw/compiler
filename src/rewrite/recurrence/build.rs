//! Building the loop: the rewrite that turns a recognised recurrence into
//! a loop that computes the same values.
//!
//! The recognition half lives in `recognize.rs`; this file takes what it
//! recognised and rewrites the function.

use crate::ast::*;

use super::super::common::{fresh_name, fresh_numbered, names_in_use};
use super::recognize::{call_offset, match_recurrence};
use super::{REC_ACC, REC_BASE, REC_INDEX, REC_NEXT};

/// Rewrite one function's body in place.
pub(super) fn rewrite_recurrence(
    name: &str,
    params: &[(String, Type)],
    ret: Option<Type>,
    body: &mut Vec<Stmt>,
) {
    let Some(rec) = match_recurrence(name, params, ret, body) else {
        return;
    };
    let Some(ty) = ret else { return; };

    let used = names_in_use(params, body);
    let width = rec.width as usize;
    let accs = fresh_numbered(REC_ACC, width, &used);
    let index = fresh_name(REC_INDEX, &used);
    let next = fresh_name(REC_NEXT, &used);
    let p0 = &params[0].0;

    // The loop's update: the source expression with every self-call
    // replaced by the accumulator that holds that value.
    let Some(update) = build_loop_expr(&rec.tail, name, params, &accs, &index, p0, 0) else {
        return;
    };

    // The base case stays exactly as it was.
    let mut out: Vec<Stmt> = vec![body[0].clone()];

    // One step down the argument lattice — the subtraction the recursion's
    // own call arguments perform.
    fn one() -> Expr {
        Expr::Num(NumLit::new(1.0, Some(1)))
    }
    fn down(e: Expr) -> Expr {
        Expr::Bin {
            op: BinOp::Sub,
            lhs: Box::new(e),
            rhs: Box::new(one()),
        }
    }
    let base = fresh_name(REC_BASE, &used);

    // The prologue differs by parameter type: an `int` lattice is known at
    // compile time, so the starting values are literal calls and the loop
    // counts up from the threshold; a `number` lattice is only known at run
    // time, so the loop walks the argument down to it first.
    match params[0].1 {
        Type::Int => {
            // The starting values: f(threshold - width + 1) ... f(threshold).
            // All of them reach the base case, so each costs one call.
            let call_at = |value: i64| Expr::Call {
                name: name.to_string(),
                args: {
                    let mut args =
                        vec![Expr::Num(NumLit::new(value as f64, Some(value)))];
                    for (pname, _) in params.iter().skip(1) {
                        args.push(Expr::Var(pname.clone()));
                    }
                    args
                },
            };
            for (i, acc) in accs.iter().enumerate() {
                let value = rec.threshold - (rec.width - 1 - i as i64);
                out.push(Stmt::Let {
                    name: acc.clone(),
                    ty: Some(ty),
                    init: Some(call_at(value)),
                    mutable: true,
                });
            }
            let Some(first) = rec.threshold.checked_add(1) else {
                return;
            };
            out.push(Stmt::Let {
                name: index.clone(),
                ty: Some(params[0].1),
                init: Some(Expr::Num(NumLit::new(first as f64, Some(first)))),
                mutable: true,
            });
        }
        Type::Number => {
            // Walk the argument down to the top base point, one step at a
            // time: each subtraction is one the recursion performs too, so
            // the loop and the recursion visit the same values.
            out.push(Stmt::Let {
                name: base.clone(),
                ty: Some(Type::Number),
                init: Some(Expr::Var(p0.clone())),
                mutable: true,
            });
            out.push(Stmt::While {
                cond: Expr::Bin {
                    op: rec.descent,
                    lhs: Box::new(Expr::Var(base.clone())),
                    rhs: Box::new(Expr::Num(NumLit::new(
                        rec.limit as f64,
                        Some(rec.limit),
                    ))),
                },
                body: vec![Stmt::Expr(Expr::Assign {
                    name: base.clone(),
                    value: Box::new(down(Expr::Var(base.clone()))),
                })],
            });

            // The starting values, oldest first, each found by stepping
            // down from the base point one subtraction at a time — the same
            // steps the recursion's arguments take, so the calls land on
            // the recursion's own values, bit for bit.
            let call_below = |steps: usize| Expr::Call {
                name: name.to_string(),
                args: {
                    let mut arg = Expr::Var(base.clone());
                    for _ in 0..steps {
                        arg = down(arg);
                    }
                    let mut args = vec![arg];
                    for (pname, _) in params.iter().skip(1) {
                        args.push(Expr::Var(pname.clone()));
                    }
                    args
                },
            };
            for (i, acc) in accs.iter().enumerate() {
                out.push(Stmt::Let {
                    name: acc.clone(),
                    ty: Some(ty),
                    init: Some(call_below(width - 1 - i)),
                    mutable: true,
                });
            }

            // The counter replays the descent from the argument, so it
            // reaches the base point exactly, and the loop runs exactly as
            // many times as the recursion is deep.
            out.push(Stmt::Let {
                name: index.clone(),
                ty: Some(Type::Number),
                init: Some(Expr::Var(p0.clone())),
                mutable: true,
            });
        }
        _ => return,
    }

    // The loop body: compute the next value, shift the window, count on.
    let mut loop_body = vec![Stmt::Let {
        name: next.clone(),
        ty: Some(ty),
        init: Some(update),
        mutable: true,
    }];
    for i in 0..width.saturating_sub(1) {
        loop_body.push(Stmt::Expr(Expr::Assign {
            name: accs[i].clone(),
            value: Box::new(Expr::Var(accs[i + 1].clone())),
        }));
    }
    loop_body.push(Stmt::Expr(Expr::Assign {
        name: accs[width - 1].clone(),
        value: Box::new(Expr::Var(next.clone())),
    }));
    match params[0].1 {
        // The `int` lattice is ascending and known: count up to the
        // argument.
        Type::Int => {
            loop_body.push(Stmt::Expr(Expr::Assign {
                name: index.clone(),
                value: Box::new(Expr::Bin {
                    op: BinOp::Add,
                    lhs: Box::new(Expr::Var(index.clone())),
                    rhs: Box::new(one()),
                }),
            }));
            out.push(Stmt::While {
                cond: Expr::Bin {
                    op: BinOp::Le,
                    lhs: Box::new(Expr::Var(index.clone())),
                    rhs: Box::new(Expr::Var(p0.clone())),
                },
                body: loop_body,
            });
        }
        // The `number` lattice belongs to the argument: the counter comes
        // down from it and stops at the base point the descent found.
        _ => {
            loop_body.push(Stmt::Expr(Expr::Assign {
                name: index.clone(),
                value: Box::new(down(Expr::Var(index.clone()))),
            }));
            out.push(Stmt::While {
                cond: Expr::Bin {
                    op: BinOp::Gt,
                    lhs: Box::new(Expr::Var(index.clone())),
                    rhs: Box::new(Expr::Var(base)),
                },
                body: loop_body,
            });
        }
    }
    out.push(Stmt::Return(Some(Expr::Var(accs[width - 1].clone()))));

    *body = out;
}

/// Build the expression the loop computes: the source expression with every
/// self-call replaced by the accumulator that holds that value, and the
/// parameter replaced by the induction variable — the loop counts a
/// different index than the recursion does.
fn build_loop_expr(
    expr: &Expr,
    name: &str,
    params: &[(String, Type)],
    accs: &[String],
    index: &str,
    param: &str,
    depth: usize,
) -> Option<Expr> {
    if depth > MAX_AST_DEPTH {
        return None;
    }
    match expr {
        Expr::Call {
            name: callee,
            args,
        } if callee == name => {
            let offset = call_offset(args, params)?;
            // The value from `offset` steps back is the one the
            // accumulator holds.
            let slot = accs.len().checked_sub(offset as usize)?;
            Some(Expr::Var(accs.get(slot)?.clone()))
        }
        Expr::Num(_) | Expr::Bool(_) => Some(expr.clone()),
        Expr::Var(v) => {
            if v == param {
                Some(Expr::Var(index.to_string()))
            } else {
                Some(expr.clone())
            }
        }
        Expr::Bin { op, lhs, rhs } => {
            let lhs = build_loop_expr(lhs, name, params, accs, index, param, depth + 1)?;
            let rhs = build_loop_expr(rhs, name, params, accs, index, param, depth + 1)?;
            Some(Expr::Bin {
                op: *op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        Expr::Un { op, expr } => {
            let e = build_loop_expr(expr, name, params, accs, index, param, depth + 1)?;
            Some(Expr::Un {
                op: *op,
                expr: Box::new(e),
            })
        }
        // A cast of a self-call casts the accumulator that replaces it.
        Expr::Cast { expr, ty } => {
            let e = build_loop_expr(expr, name, params, accs, index, param, depth + 1)?;
            Some(Expr::Cast {
                expr: Box::new(e),
                ty: *ty,
            })
        }
        Expr::Call { .. } | Expr::Assign { .. } => None,
    }
}
