//! Linear recurrences: a function defined in terms of its own previous
//! values becomes a loop.
//!
//! A *linear recurrence* is a function defined in terms of its own previous
//! values:
//!
//! ```text
//! function f(p0) {
//!     if (p0 < 2) { return p0; }
//!     return f(p0 - 1) + f(p0 - 2);
//! }
//! ```
//!
//! That is fibonacci, and the source program computes it exponentially: the
//! tree of calls has `F(depth)` leaves. The values, however, satisfy a
//! recurrence that can be *iterated*:
//!
//! ```text
//! function f(p0) {
//!     if (p0 < 2) { return p0; }
//!     let a = f(0);                       // f(p0 - 2)
//!     let b = f(1);                       // f(p0 - 1)
//!     while (p0 > 1) {
//!         let c = b + a;
//!         a = b;
//!         b = c;
//!         p0 = p0 - 1;
//!     }
//!     return b;
//! }
//! ```
//!
//! — linear time, constant stack, and the same value. The loop performs the
//! same additions, in the same order, as the recursion does on its way back
//! up, so the result is bit for bit the same. This is the classical
//! transform (Kapur, Kogge & Palem, *Efficient compilation of linear
//! recursive functions into object level loops*, 1986).
//!
//! For a `number` parameter the loop cannot count a fixed lattice — the
//! recursion's arguments are `n`, `n-1`, `n-2`, … at whatever fraction `n`
//! carries, and the loop has to visit exactly those values. So it walks the
//! argument down to the base case first, with the same subtractions the
//! recursion performs, and then counts *down* from the argument, replaying
//! that same descent — the counter becomes the top base point bit for bit,
//! so the number of iterations is exact, and the values the loop computes
//! sit on the recursion's own lattice:
//!
//! ```text
//! function f(p0) {
//!     if (p0 < 2) { return p0; }
//!     let b = p0;
//!     while (b >= 2) { b = b - 1; }      // the top base point
//!     let a = f(b - 1);                  // f(p0 - 2) on the same lattice
//!     let bb = f(b);                     // f(p0 - 1)
//!     let i = p0;
//!     while (i > b) {                    // replays the descent exactly
//!         let c = bb + a;
//!         a = bb;
//!         bb = c;
//!         i = i - 1;
//!     }
//!     return bb;
//! }
//! ```
//!
//! It is a much bigger hammer than tail-call elimination: it changes a
//! program from exponential to linear, so the shape it accepts has to be
//! recognised exactly. It fires only when
//!
//! * the function returns a `number` or an `int`, and its first parameter is
//!   an `int` — or a `number` whose expression does not read the parameter
//!   outside the self-calls. An `int` loop can count a fixed lattice, and a
//!   term that reads the parameter can be given the level's own value. A
//!   `number` loop cannot: an upward index would leave the recursion's
//!   lattice (adding one does not undo subtracting one, in floating point),
//!   so a `number` rewrite counts down from the argument instead — and a
//!   term that reads the parameter has no value it could see, so such a term
//!   refuses the rewrite,
//! * the base case is the first statement of the body, with the same rules
//!   as the tail-call rewrite — a statement before it would run once per
//!   recursion level in the source program and once in the rewritten one,
//! * the base expression has no side effects. The source program evaluates
//!   it once per leaf of the call tree; the rewrite evaluates it once per
//!   starting value,
//! * the rest of the body is a single `return` whose expression calls the
//!   function at least once, always at a constant offset — `p0 - 1`,
//!   `p0 - 3`, … — and whose every other part is free of side effects, and
//! * every other argument of those calls is a parameter passed on unchanged.
//!
//! The loop keeps as many values as the widest offset needs, so a
//! recurrence with a gap (`f(p0 - 1) + f(p0 - 3)`) is rewritten too. The
//! expression's shape is preserved exactly — the loop performs the same
//! operations, in the same order, on the same values — so the answer is the
//! same, bit for bit.
//!
//! The loop counts with an induction variable of its own rather than with
//! the parameter: the recursion's level and the loop's index are not the
//! same number, and a term that reads the parameter must see the level's
//! own value.
//!
//! Both schemes change how a program that never terminates *fails*. The
//! source program exhausts the stack and is reported; the loop keeps
//! counting. When the descent cannot make progress — an `int` argument near
//! `i64::MAX`, a `number` argument at or above 2^53, or an infinity — the
//! rewritten program loops where the source program was reported as a stack
//! overflow. Every finite argument below those bounds computes the same
//! answer, bit for bit.

use crate::ast::*;

use super::common::{fresh_name, fresh_numbered, is_pure, is_var, names_in_use};

// --- Linear recurrences -----------------------------------------------------

/// The base name of the generated accumulators — one per value the loop has
/// to keep. The names are checked against the names already in use, so they
/// cannot collide with a program's own variables.
const REC_ACC: &str = "__ts_rec_v";

/// The name of the generated induction variable.
const REC_INDEX: &str = "__ts_rec_i";

/// The name of the generated base pointer — the largest argument, on the
/// argument's own lattice, that reaches the base case. Only a `number`
/// parameter needs one: its lattice is not known until run time.
const REC_BASE: &str = "__ts_rec_b";

/// The name of the generated value the loop computes.
const REC_NEXT: &str = "__ts_rec_c";

/// A self-recursive function whose values satisfy a recurrence.
struct Recurrence {
    /// The largest argument that reaches the base case — what an `int`
    /// parameter's fixed lattice needs.
    threshold: i64,
    /// The comparison the descent loop uses to find the top base point —
    /// what a `number` parameter's run-time lattice needs. `n < L` descends
    /// while `b >= L`; `n <= L` while `b > L`.
    descent: BinOp,
    /// The literal the base case compares the parameter against.
    limit: i64,
    /// How many values the loop has to keep: the largest offset the
    /// expression calls the function at.
    width: i64,
    /// The expression that computes the next value.
    tail: Expr,
}

/// Rewrite every linear recurrence in the program into a loop.
pub fn eliminate_recurrences(prog: &mut Program) {
    for stmt in prog.stmts.iter_mut() {
        if let Stmt::Func {
            name,
            params,
            ret,
            body,
        } = stmt
        {
            rewrite_recurrence(name, params, *ret, body);
        }
    }
}

/// Rewrite one function's body in place.
fn rewrite_recurrence(
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

/// Recognise the linear-recurrence shape, or give up.
fn match_recurrence(
    name: &str,
    params: &[(String, Type)],
    ret: Option<Type>,
    body: &[Stmt],
) -> Option<Recurrence> {
    // Only a number or an int can be combined arithmetically.
    match ret {
        Some(Type::Number) | Some(Type::Int) => {}
        _ => return None,
    }
    // The parameter the recurrence counts down has to be an `int`, or a
    // `number` the expression does not read outside the self-calls. An
    // `int` loop counts a fixed lattice of values, so a fractional argument
    // would make the loop's count and the recursion's depth disagree: the
    // recursion descends by whole offsets and reaches `2.5 - 1 = 1.5`, while
    // the loop counts integers. A `number` loop avoids that by deriving the
    // lattice from the argument at run time — but an upward index cannot
    // reproduce it (adding one does not undo subtracting one, in floating
    // point), so a term that reads the parameter would see a value that is
    // not its level's, and such a term refuses the rewrite instead.
    let forbid_param_reads = match params.first().map(|(_, t)| *t) {
        Some(Type::Int) => false,
        Some(Type::Number) => true,
        _ => return None,
    };
    if params.is_empty() || body.len() < 2 {
        return None;
    }

    // The base case must be the *first* statement of the body, for the same
    // reason as in the tail-call rewrite.
    let Stmt::If {
        cond,
        then,
        els,
    } = &body[0]
    else {
        return None;
    };
    if !els.is_empty() || then.len() != 1 {
        return None;
    }
    let Stmt::Return(Some(base)) = &then[0] else {
        return None;
    };
    // The base expression must be free of side effects: the source program
    // evaluates it once per leaf of the call tree, the rewrite evaluates it
    // once per starting value.
    if !is_pure(base) {
        return None;
    }

    // The rest of the body is a single return.
    let [Stmt::Return(Some(tail))] = &body[1..] else {
        return None;
    };

    // The expression must call the function, at constant offsets, and
    // everything else in it must be free of side effects.
    let mut offsets = Vec::new();
    if !collect_offsets(tail, name, params, &mut offsets, 0, forbid_param_reads) {
        return None;
    }
    let width = *offsets.iter().max()?;

    let (op, limit) = base_comparison(cond, &params[0].0)?;
    // The threshold is the `int` lattice's top base point; the descent
    // comparison is its run-time counterpart. `n < L` reaches the base case
    // below `L`, so the descent stops when it is no longer `>= L`; `n <= L`
    // reaches it at `L`, so it stops when it is no longer `> L`.
    let (threshold, descent) = match op {
        BinOp::Lt => (limit.checked_sub(1)?, BinOp::Ge),
        BinOp::Le => (limit, BinOp::Gt),
        _ => return None,
    };

    Some(Recurrence {
        threshold,
        descent,
        limit,
        width,
        tail: tail.clone(),
    })
}

/// Collect the offsets the expression calls the function at, and check that
/// everything else in it is free of side effects. When `forbid_param_reads`
/// is set, a read of the first parameter outside a self-call's offset also
/// refuses — the `number` scheme has no value such a term could see.
fn collect_offsets(
    expr: &Expr,
    name: &str,
    params: &[(String, Type)],
    offsets: &mut Vec<i64>,
    depth: usize,
    forbid_param_reads: bool,
) -> bool {
    if depth > MAX_AST_DEPTH {
        // Too deep to look at: leave the program alone.
        return false;
    }
    match expr {
        Expr::Call {
            name: callee,
            args,
        } if callee == name => match call_offset(args, params) {
            Some(offset) => {
                offsets.push(offset);
                true
            }
            None => false,
        },
        // A call to another function has side effects the rewrite would
        // reorder, so it is not accepted.
        Expr::Call { .. } => false,
        Expr::Num(_) | Expr::Bool(_) => true,
        Expr::Var(v) => !(forbid_param_reads && *v == params[0].0),
        Expr::Bin { lhs, rhs, .. } => {
            collect_offsets(lhs, name, params, offsets, depth + 1, forbid_param_reads)
                && collect_offsets(rhs, name, params, offsets, depth + 1, forbid_param_reads)
        }
        Expr::Un { expr, .. } => {
            collect_offsets(expr, name, params, offsets, depth + 1, forbid_param_reads)
        }
        // A cast is a pure conversion of its operand.
        Expr::Cast { expr, .. } => {
            collect_offsets(expr, name, params, offsets, depth + 1, forbid_param_reads)
        }
        // An assignment has a side effect.
        Expr::Assign { .. } => false,
    }
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

/// The offset of a self-call: how much smaller its first argument is than
/// the first parameter.
fn call_offset(args: &[Expr], params: &[(String, Type)]) -> Option<i64> {
    if args.len() != params.len() {
        return None;
    }
    let Expr::Bin {
        op: BinOp::Sub,
        lhs,
        rhs,
    } = &args[0]
    else {
        return None;
    };
    let Expr::Var(v) = &**lhs else {
        return None;
    };
    if *v != params[0].0 {
        return None;
    }
    let Expr::Num(lit) = &**rhs else {
        return None;
    };
    // The offset must be a positive, exact integer: the recursion descends.
    let offset = lit.int?;
    if offset <= 0 {
        return None;
    }
    // Every other argument is a parameter, passed on unchanged.
    if !params
        .iter()
        .enumerate()
        .skip(1)
        .all(|(i, (pname, _))| matches!(&args[i], Expr::Var(v) if v == pname))
    {
        return None;
    }
    Some(offset)
}

/// The base case's comparison against the parameter: its operator (which
/// must be `<` or `<=`) and the exact integer it compares against.
fn base_comparison(cond: &Expr, param: &str) -> Option<(BinOp, i64)> {
    let Expr::Bin { op, lhs, rhs } = cond else {
        return None;
    };
    let (op, literal) = match (is_var(lhs, param), is_var(rhs, param)) {
        (true, false) => (*op, rhs),
        (false, true) => (*op, lhs),
        _ => return None,
    };
    if !matches!(op, BinOp::Lt | BinOp::Le) {
        return None;
    }
    let Expr::Num(lit) = &**literal else {
        return None;
    };
    // The comparison must be against an exact integer.
    Some((op, lit.int?))
}

#[cfg(test)]
mod tests;
