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
    // The stride: the largest number every offset the expression uses is a
    // multiple of. A stride of one is the plain lattice — one value per
    // argument. A stride of two or more means the arguments the recursion
    // reaches lie on one lattice (the ones congruent to the initial
    // argument), so the loop can step by the stride and keep one
    // accumulator per lattice position instead of one per argument.
    let stride = rec.stride as usize;
    let count = width / stride;
    let accs = fresh_numbered(REC_ACC, count, &used);
    let index = fresh_name(REC_INDEX, &used);
    let next = fresh_name(REC_NEXT, &used);
    let p0 = &params[0].0;

    // The loop's update: the source expression with every self-call
    // replaced by the accumulator that holds that value.
    let slots = Slots {
        accs: &accs,
        index: &index,
        param: p0,
        stride,
    };
    let Some(update) = build_loop_expr(&rec.tail, name, params, &slots, 0) else {
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
    // One lattice step — the stride, when the lattice is wider than one
    // argument.
    fn by(n: i64) -> Expr {
        Expr::Num(NumLit::new(n as f64, Some(n)))
    }
    let base = fresh_name(REC_BASE, &used);

    // The prologue differs by parameter type: an `int` lattice is known at
    // compile time, so the starting values are literal calls and the loop
    // counts up from the threshold; a `number` lattice is only known at run
    // time, so the loop walks the argument down to it first.
    match params[0].1 {
        Type::Int => {
            // The starting values: the lattice values just below the first
            // one the loop computes. All of them reach the base case, so
            // each costs one call.
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
            // The same call, with the argument as an expression — a wider
            // lattice's first value is only known at run time.
            let call_at_expr = |value: &Expr, back: usize| Expr::Call {
                name: name.to_string(),
                args: {
                    let mut args = vec![Expr::Bin {
                        op: BinOp::Sub,
                        lhs: Box::new(value.clone()),
                        rhs: Box::new(Expr::Num(NumLit::new(
                            back as f64,
                            Some(back as i64),
                        ))),
                    }];
                    for (pname, _) in params.iter().skip(1) {
                        args.push(Expr::Var(pname.clone()));
                    }
                    args
                },
            };
            let Some(first) = rec.threshold.checked_add(1) else {
                return;
            };
            // The first value the loop computes: the smallest argument
            // above the threshold that lies on the lattice. On a plain
            // lattice that is the threshold plus one, known at compile
            // time; on a wider one it is the first argument congruent to
            // the argument itself, which is only known at run time.
            let first_value = Expr::Num(NumLit::new(first as f64, Some(first)));
            let first_expr = if stride == 1 {
                first_value.clone()
            } else {
                Expr::Bin {
                    op: BinOp::Add,
                    lhs: Box::new(first_value.clone()),
                    rhs: Box::new(Expr::Bin {
                        op: BinOp::Rem,
                        lhs: Box::new(Expr::Bin {
                            op: BinOp::Sub,
                            lhs: Box::new(Expr::Var(p0.clone())),
                            rhs: Box::new(first_value.clone()),
                        }),
                        rhs: Box::new(Expr::Num(NumLit::new(
                            stride as f64,
                            Some(stride as i64),
                        ))),
                    }),
                }
            };
            for (i, acc) in accs.iter().enumerate() {
                // How far below the first value this accumulator's is.
                let back = (count - i) * stride;
                let init = if stride == 1 {
                    let value = rec.threshold - (rec.width - 1 - i as i64);
                    call_at(value)
                } else {
                    call_at_expr(&first_expr, back)
                };
                out.push(Stmt::Let {
                    name: acc.clone(),
                    ty: Some(ty),
                    init: Some(init),
                    mutable: true,
                });
            }
            out.push(Stmt::Let {
                name: index.clone(),
                ty: Some(params[0].1),
                init: Some(first_expr),
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
                    value: Box::new(if stride == 1 {
                        down(Expr::Var(base.clone()))
                    } else {
                        // A wider lattice steps by the stride, so the base
                        // point stays on the lattice the argument is on.
                        Expr::Bin {
                            op: BinOp::Sub,
                            lhs: Box::new(Expr::Var(base.clone())),
                            rhs: Box::new(by(stride as i64)),
                        }
                    }),
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
                    init: Some(call_below((count - 1 - i) * stride)),
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
    for i in 0..count.saturating_sub(1) {
        loop_body.push(Stmt::Expr(Expr::Assign {
            name: accs[i].clone(),
            value: Box::new(Expr::Var(accs[i + 1].clone())),
        }));
    }
    loop_body.push(Stmt::Expr(Expr::Assign {
        name: accs[count - 1].clone(),
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
                    rhs: Box::new(if stride == 1 {
                        one()
                    } else {
                        by(stride as i64)
                    }),
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
                value: Box::new(if stride == 1 {
                    down(Expr::Var(index.clone()))
                } else {
                    Expr::Bin {
                        op: BinOp::Sub,
                        lhs: Box::new(Expr::Var(index.clone())),
                        rhs: Box::new(by(stride as i64)),
                    }
                }),
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
    out.push(Stmt::Return(Some(Expr::Var(accs[count - 1].clone()))));

    *body = out;
}

/// What the loop-expression builder needs to know: where the accumulators
/// are, which name carries the induction variable, and how wide the lattice
/// is.
struct Slots<'a> {
    accs: &'a [String],
    index: &'a str,
    param: &'a str,
    stride: usize,
}

/// Build the expression the loop computes: the source expression with every
/// self-call replaced by the accumulator that holds that value, and the
/// parameter replaced by the induction variable — the loop counts a
/// different index than the recursion does.
fn build_loop_expr(
    expr: &Expr,
    name: &str,
    params: &[(String, Type)],
    slots: &Slots,
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
            // accumulator holds. On a wider lattice the offset counts
            // lattice positions, not arguments.
            let slot = slots.accs.len().checked_sub((offset as usize) / slots.stride)?;
            Some(Expr::Var(slots.accs.get(slot)?.clone()))
        }
        Expr::Num(_) | Expr::Bool(_) => Some(expr.clone()),
        Expr::Var(v) => {
            if v == slots.param {
                Some(Expr::Var(slots.index.to_string()))
            } else {
                Some(expr.clone())
            }
        }
        Expr::Bin { op, lhs, rhs } => {
            let lhs = build_loop_expr(lhs, name, params, slots, depth + 1)?;
            let rhs = build_loop_expr(rhs, name, params, slots, depth + 1)?;
            Some(Expr::Bin {
                op: *op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            })
        }
        Expr::Un { op, expr } => {
            let e = build_loop_expr(expr, name, params, slots, depth + 1)?;
            Some(Expr::Un {
                op: *op,
                expr: Box::new(e),
            })
        }
        // A cast of a self-call casts the accumulator that replaces it.
        Expr::Cast { expr, ty } => {
            let e = build_loop_expr(expr, name, params, slots, depth + 1)?;
            Some(Expr::Cast {
                expr: Box::new(e),
                ty: *ty,
            })
        }
        Expr::Call { .. } | Expr::Assign { .. } => None,
    }
}
