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
//! tree of calls has 2^depth leaves. The values, however, satisfy a
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
//! It is a much bigger hammer than tail-call elimination: it changes a
//! program from exponential to linear, so the shape it accepts has to be
//! recognised exactly. It fires only when
//!
//! * the function returns a `number` or an `int`,
//! * the base case is the first statement of the body, with the same rules
//!   as the tail-call rewrite — a statement before it would run once per
//!   recursion level in the source program and once in the rewritten one,
//! * the base expression has no side effects. The source program evaluates
//!   it once per leaf of the call tree; the rewrite evaluates it exactly
//!   twice, for the two starting values,
//! * the rest of the body is a single `return` whose expression combines
//!   *two* self-calls — one at `p0 - 1` and one at `p0 - 2` — with a single
//!   arithmetic operator, and nothing else, and
//! * every other argument of those calls is a parameter passed on unchanged.
//!
//! The offsets are exactly one and two because the loop keeps the two
//! previous values; a recurrence with a wider gap (`f(p0 - 1) + f(p0 - 3)`)
//! needs more accumulators, and is left alone.

use crate::ast::*;

use super::common::{is_pure, is_var, names_in_use, fresh_names};

// --- Linear recurrences -----------------------------------------------------

/// The names of the generated accumulators. They are checked against the
/// names already in use, so they cannot collide with a program's own
/// variables.
const REC_NAMES: [&str; 3] = ["__ts_rec_a", "__ts_rec_b", "__ts_rec_c"];

/// A linear recurrence that the rewrite knows how to turn into a loop.
struct Recurrence {
    /// The largest argument that reaches the base case.
    threshold: i64,
    /// How the two recursive calls are combined.
    op: BinOp,
    /// Whether the *older* call — the one with the larger offset — is the
    /// left operand of the expression.
    older_on_left: bool,
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
    let Some(ty) = ret else { return };

    let names = fresh_names(&REC_NAMES, &names_in_use(params, body));
    let a_name = &names[0];
    let b_name = &names[1];
    let c_name = &names[2];
    let p0 = &params[0].0;

    // The two starting values: f(threshold - 1) and f(threshold). Both reach
    // the base case, so each costs one call.
    let call_at = |value: i64| Expr::Call {
        name: name.to_string(),
        args: {
            let mut args = vec![Expr::Num(NumLit::new(value as f64, Some(value)))];
            for (pname, _) in params.iter().skip(1) {
                args.push(Expr::Var(pname.clone()));
            }
            args
        },
    };

    // The loop's update combines the newer value (f(p0 - 1), kept in `b`)
    // with the older one (f(p0 - 2), kept in `a`), in the order the source
    // expression had them.
    let (lhs, rhs) = if rec.older_on_left {
        (Expr::Var(a_name.clone()), Expr::Var(b_name.clone()))
    } else {
        (Expr::Var(b_name.clone()), Expr::Var(a_name.clone()))
    };
    let update = Expr::Bin {
        op: rec.op,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    };

    let loop_body = vec![
        Stmt::Let {
            name: c_name.clone(),
            ty: Some(ty),
            init: Some(update),
            mutable: true,
        },
        Stmt::Expr(Expr::Assign {
            name: a_name.clone(),
            value: Box::new(Expr::Var(b_name.clone())),
        }),
        Stmt::Expr(Expr::Assign {
            name: b_name.clone(),
            value: Box::new(Expr::Var(c_name.clone())),
        }),
        // The parameter counts the values still to be computed.
        Stmt::Expr(Expr::Assign {
            name: p0.clone(),
            value: Box::new(Expr::Bin {
                op: BinOp::Sub,
                lhs: Box::new(Expr::Var(p0.clone())),
                rhs: Box::new(Expr::Num(NumLit::new(1.0, Some(1)))),
            }),
        }),
    ];

    // The base case stays exactly as it was.
    let out: Vec<Stmt> = vec![
        body[0].clone(),
        Stmt::Let {
            name: a_name.clone(),
            ty: Some(ty),
            init: Some(call_at(rec.threshold - 1)),
            mutable: true,
        },
        Stmt::Let {
            name: b_name.clone(),
            ty: Some(ty),
            init: Some(call_at(rec.threshold)),
            mutable: true,
        },
        Stmt::While {
            cond: Expr::Bin {
                op: BinOp::Gt,
                lhs: Box::new(Expr::Var(p0.clone())),
                rhs: Box::new(Expr::Num(NumLit::new(
                    rec.threshold as f64,
                    Some(rec.threshold),
                ))),
            },
            body: loop_body,
        },
        Stmt::Return(Some(Expr::Var(b_name.clone()))),
    ];

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
    // exactly twice, for the two starting values.
    if !is_pure(base) {
        return None;
    }

    // The rest of the body is a single return.
    let [Stmt::Return(Some(tail))] = &body[1..] else {
        return None;
    };

    // The expression combines two self-calls and nothing else.
    let Expr::Bin { op, lhs, rhs } = tail else {
        return None;
    };
    let older_on_left = if is_call_at_offset(lhs, name, params, 2) && is_call_at_offset(rhs, name, params, 1) {
        true
    } else if is_call_at_offset(lhs, name, params, 1) && is_call_at_offset(rhs, name, params, 2) {
        false
    } else {
        return None;
    };
    // Only an operation with an identity can be carried along a loop.
    if !matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul) {
        return None;
    }

    let threshold = base_threshold(cond, &params[0].0)?;

    Some(Recurrence {
        threshold,
        op: *op,
        older_on_left,
    })
}

/// Is `expr` a call to `name` whose first argument is the first parameter
/// minus `offset`, and whose other arguments are the other parameters passed
/// on unchanged?
fn is_call_at_offset(
    expr: &Expr,
    name: &str,
    params: &[(String, Type)],
    offset: i64,
) -> bool {
    let Expr::Call {
        name: callee,
        args,
    } = expr
    else {
        return false;
    };
    if callee != name || args.len() != params.len() {
        return false;
    }
    // The first argument steps down by `offset`.
    let Expr::Bin {
        op: BinOp::Sub,
        lhs,
        rhs,
    } = &args[0]
    else {
        return false;
    };
    let Expr::Var(v) = &**lhs else {
        return false;
    };
    if *v != params[0].0 {
        return false;
    }
    let Expr::Num(lit) = &**rhs else {
        return false;
    };
    if lit.int != Some(offset) {
        return false;
    }
    // Every other argument is a parameter, passed on unchanged.
    params
        .iter()
        .enumerate()
        .skip(1)
        .all(|(i, (pname, _))| matches!(&args[i], Expr::Var(v) if v == pname))
}

/// The largest argument that reaches the base case, from a condition that
/// compares the parameter against a literal.
fn base_threshold(cond: &Expr, param: &str) -> Option<i64> {
    let Expr::Bin { op, lhs, rhs } = cond else {
        return None;
    };
    let (op, literal) = match (is_var(lhs, param), is_var(rhs, param)) {
        (true, false) => (*op, rhs),
        (false, true) => (*op, lhs),
        _ => return None,
    };
    let Expr::Num(lit) = &**literal else {
        return None;
    };
    // The comparison must be against an exact integer.
    let value = lit.int?;
    match op {
        // `n < value` reaches the base case up to `value - 1`.
        BinOp::Lt => value.checked_sub(1),
        // `n <= value` reaches it up to `value`.
        BinOp::Le => Some(value),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
