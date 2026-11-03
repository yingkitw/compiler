//! Tail-call elimination: a self-recursive call in tail position becomes a
//! loop.
//!
//! A function that calls itself in tail position grows the stack once per
//! level, so a recursion of a million levels dies with "maximum recursion
//! depth exceeded". That is a property of the *call*, not of the algorithm:
//! the recursive call never needs the frame it was made in, so it can be a
//! jump instead.
//!
//! The rewrite is the classic one. A function whose body is
//!
//! ```text
//! function f(p0, ..., pm) {
//!     if (p0 <= limit) { return base; }
//!     return f(p0 - k, p1, ..., pm);              // a tail call
//! }
//! ```
//!
//! becomes
//!
//! ```text
//! function f(p0, ..., pm) {
//!     while (p0 > limit) { p0 = p0 - k; }
//!     return base;
//! }
//! ```
//!
//! — one function entry instead of one per level. The same shape with an
//! accumulator (`return f(p0 - k, ...) + e;`) becomes a loop that carries the
//! accumulator, so the frame is still not needed.
//!
//! The rewrite is deliberately narrow, because a rewrite that fires on the
//! wrong program is a miscompile, while a rewrite that does not fire is only
//! a missed optimisation. It fires only when
//!
//! * the base case is the *first* statement of the body. A statement before
//!   it would run once per recursion level in the source program and once in
//!   the rewritten one,
//! * the base case is an `if` with no `else` whose only action is to return,
//!   and whose condition compares the first parameter against a literal,
//! * the recursive call is to the function itself and is the last statement
//!   of the body — either `return f(...)`, or, in a void function, `f(...);`,
//! * every argument of the call is a parameter plus or minus a constant, and
//!   the first parameter's step is downwards, so the recursion really does
//!   descend,
//! * the accumulator term, if there is one, has no side effects: the
//!   rewrite evaluates it in the opposite order, from the outermost level
//!   inwards, and
//! * the per-level statements of a void tail call neither call the function
//!   again nor assign to anything.
//!
//! Anything else is left exactly as it was.
//!
//! The rewrite also relies on parameters being assignable, which they are
//! (TypeScript parameters are mutable, and so are ours).

use crate::ast::*;

use super::common::{
    contains_assign, contains_call_to, is_literal, is_pure, is_var, fresh_name, names_in_use,
};

/// The name of the generated accumulator. It is checked against the names
/// already in use, so it cannot collide with a program's own variables.
const ACC_NAME: &str = "__ts_tco_acc";

/// Rewrite every self-recursive tail call in the program into a loop.
pub fn eliminate_tail_calls(prog: &mut Program) {
    for stmt in prog.stmts.iter_mut() {
        if let Stmt::Func {
            name,
            params,
            ret,
            body,
        } = stmt
        {
            rewrite_func(name, params, *ret, body);
        }
    }
}

/// How the recursive call combines its result with an accumulator.
#[derive(Debug, Clone, Copy, PartialEq)]
enum AccOp {
    Add,
    Sub,
    Mul,
}

impl AccOp {
    /// How the accumulator is combined inside the loop.
    fn accumulate(self) -> BinOp {
        match self {
            AccOp::Add | AccOp::Sub => BinOp::Add,
            AccOp::Mul => BinOp::Mul,
        }
    }

    /// How the accumulator is combined with the base case at the end.
    fn combine(self) -> BinOp {
        match self {
            AccOp::Add => BinOp::Add,
            AccOp::Sub => BinOp::Sub,
            AccOp::Mul => BinOp::Mul,
        }
    }

    /// The value the accumulator starts at: the identity of the operation.
    fn identity(self) -> f64 {
        match self {
            AccOp::Add | AccOp::Sub => 0.0,
            AccOp::Mul => 1.0,
        }
    }
}

/// A self-recursive tail call that the rewrite knows how to turn into a loop.
struct TailCall {
    /// The condition under which the recursion continues, already negated:
    /// `p0 > limit` for a base case of `p0 <= limit`.
    loop_cond: Expr,
    /// The parameters that the call decrements, and by how much.
    steps: Vec<(usize, i64)>,
    /// The base case's returned expression. `None` for a void function.
    base: Option<Expr>,
    /// The accumulator term, when the tail call combines its result with one.
    acc: Option<(AccOp, Expr)>,
    /// The statements that run on every level, before the call.
    mid: Vec<Stmt>,
}

/// Rewrite one function's body in place.
fn rewrite_func(name: &str, params: &[(String, Type)], ret: Option<Type>, body: &mut Vec<Stmt>) {
    let Some(call) = match_tail_call(name, params, ret, body) else {
        return;
    };

    // The accumulator's type is the function's return type. A function that
    // returns something other than a number cannot carry an arithmetic
    // accumulator, so the rewrite does not apply to it.
    let acc_type = match (&call.acc, ret) {
        (None, _) => None,
        (Some(_), Some(t @ (Type::Int | Type::Number))) => Some(t),
        (Some(_), _) => return,
    };

    // The accumulator must not collide with a name the program already uses.
    let acc_name = fresh_name(ACC_NAME, &names_in_use(params, body));

    let mut out: Vec<Stmt> = Vec::new();

    if let Some(t) = acc_type {
        let op = call.acc.as_ref().unwrap().0;
        let init = match t {
            Type::Int => Expr::Num(NumLit::new(op.identity(), Some(op.identity() as i64))),
            _ => Expr::Num(NumLit::new(op.identity(), None)),
        };
        out.push(Stmt::Let {
            name: acc_name.clone(),
            ty: acc_type,
            init: Some(init),
            mutable: true,
        });
    }

    let mut loop_body: Vec<Stmt> = call.mid.clone();
    if let Some((op, expr)) = &call.acc {
        // The accumulator term is evaluated with the *current* parameter
        // values, before they are decremented.
        loop_body.push(Stmt::Expr(Expr::Assign {
            name: acc_name.clone(),
            value: Box::new(Expr::Bin {
                op: op.accumulate(),
                lhs: Box::new(Expr::Var(acc_name.clone())),
                rhs: Box::new(expr.clone()),
            }),
        }));
    }
    for (index, amount) in &call.steps {
        if *amount == 0 {
            continue;
        }
        let pname = &params[*index].0;
        // A negative step means the parameter goes up: `p + k`, not `p - k`.
        let (op, magnitude) = if *amount > 0 {
            (BinOp::Sub, *amount)
        } else {
            (BinOp::Add, -*amount)
        };
        loop_body.push(Stmt::Expr(Expr::Assign {
            name: pname.clone(),
            value: Box::new(Expr::Bin {
                op,
                lhs: Box::new(Expr::Var(pname.clone())),
                rhs: Box::new(Expr::Num(NumLit::new(
                    magnitude as f64,
                    Some(magnitude),
                ))),
            }),
        }));
    }

    out.push(Stmt::While {
        cond: call.loop_cond,
        body: loop_body,
    });

    // The base case's value: either the base expression on its own, or the
    // base expression combined with the accumulator.
    match (&call.base, &call.acc) {
        (Some(base), None) => out.push(Stmt::Return(Some(base.clone()))),
        (Some(base), Some((op, _))) => out.push(Stmt::Return(Some(Expr::Bin {
            op: op.combine(),
            lhs: Box::new(base.clone()),
            rhs: Box::new(Expr::Var(acc_name)),
        }))),
        (None, _) => {}
    }

    *body = out;
}

/// Recognise the tail-call shape, or give up.
fn match_tail_call(
    name: &str,
    params: &[(String, Type)],
    ret: Option<Type>,
    body: &[Stmt],
) -> Option<TailCall> {
    if params.is_empty() || body.len() < 2 {
        return None;
    }

    // The base case must be the *first* statement of the body. Anything
    // before it would run once per recursion level in the source program and
    // once in the rewritten one.
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
    let Stmt::Return(base_ret) = &then[0] else {
        return None;
    };

    let rest = &body[1..];

    // Two shapes: a function that returns the call's value, and a void
    // function whose call is a statement.
    let (base, acc, mid, args) = match (base_ret, ret) {
        // Value-returning: the return is the only statement after the base
        // case, and the function returns a value.
        (Some(base), Some(rt)) if rt != Type::Void => {
            let [Stmt::Return(Some(tail))] = rest else {
                return None;
            };
            let (args, acc) = split_tail(tail, name)?;
            (Some(base.clone()), acc, Vec::new(), args)
        }
        // Void: the call is a statement, and the statements before it run on
        // every level.
        (None, rt) if rt.is_none() || rt == Some(Type::Void) => {
            let (last, mid) = rest.split_last()?;
            let Stmt::Expr(Expr::Call {
                name: callee,
                args,
            }) = last
            else {
                return None;
            };
            if callee != name {
                return None;
            }
            // The per-level statements must not call this function again and
            // must not assign: the rewrite runs them in the same order, but
            // in one frame instead of one per level.
            if !mid.iter().all(|s| is_per_level_safe(s, name)) {
                return None;
            }
            (None, None, mid.to_vec(), args.clone())
        }
        _ => return None,
    };

    let first = &params[0].0;
    let loop_cond = negate_condition(cond, first)?;

    if args.len() != params.len() {
        return None;
    }
    let mut steps = Vec::with_capacity(args.len());
    for (index, arg) in args.iter().enumerate() {
        // An argument that is not a parameter plus or minus a constant (a
        // computed expression, say) means the rewrite does not know what the
        // loop would have to do.
        let amount = step_of(arg, &params[index].0)?;
        steps.push((index, amount));
    }
    // The recursion has to descend, or the loop would never end.
    if steps[0].1 <= 0 {
        return None;
    }

    Some(TailCall {
        loop_cond,
        steps,
        base,
        acc,
        mid,
    })
}

/// The recursive call's arguments, and the accumulator term it is combined
/// with, if any.
type SplitTail = (Vec<Expr>, Option<(AccOp, Expr)>);

/// Split a tail expression into the recursive call and, optionally, the term
/// it is combined with.
fn split_tail(tail: &Expr, name: &str) -> Option<SplitTail> {
    match tail {
        Expr::Call {
            name: callee,
            args,
        } if callee == name => Some((args.clone(), None)),
        Expr::Bin { op, lhs, rhs } => {
            let (op, args, other, call_on_left) = match (&**lhs, &**rhs) {
                (Expr::Call {
                    name: callee,
                    args,
                }, _) if callee == name => (*op, args.clone(), (**rhs).clone(), true),
                (_, Expr::Call {
                    name: callee,
                    args,
                }) if callee == name => (*op, args.clone(), (**lhs).clone(), false),
                _ => return None,
            };
            let acc_op = match op {
                BinOp::Add => AccOp::Add,
                BinOp::Sub if call_on_left => AccOp::Sub,
                BinOp::Mul => AccOp::Mul,
                _ => return None,
            };
            // The accumulator term must be free of side effects. The
            // original evaluates it while the recursion unwinds — from the
            // innermost level outwards — and the rewrite evaluates it from
            // the outermost level inwards. For a term that only reads its
            // arguments that is the same value; for a term that calls a
            // function or assigns to a variable it is a different order, and
            // therefore a different program.
            if !is_pure(&other) {
                return None;
            }
            Some((args, Some((acc_op, other))))
        }
        _ => None,
    }
}

/// If `expr` is `name - amount`, return `Some(amount)`; if it is `name + amount`,
/// return `Some(-amount)`; if it is `name` itself, return `Some(0)`.
///
/// The number is the amount the loop *subtracts* from the parameter, so a
/// positive number is a step down and a negative one is a step up.
fn step_of(expr: &Expr, name: &str) -> Option<i64> {
    match expr {
        Expr::Var(v) if v == name => Some(0),
        Expr::Bin { op, lhs, rhs } => {
            let Expr::Var(v) = &**lhs else {
                return None;
            };
            if v != name {
                return None;
            }
            let Expr::Num(lit) = &**rhs else {
                return None;
            };
            // Only an exact, non-negative integer amount is safe.
            let amount = lit.int?;
            if amount < 0 {
                return None;
            }
            match op {
                BinOp::Sub => Some(amount),
                BinOp::Add => Some(-amount),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Turn a base-case condition into the condition under which the recursion
/// continues. Only a comparison of the parameter against a literal qualifies.
fn negate_condition(cond: &Expr, param: &str) -> Option<Expr> {
    let (op, lhs, rhs) = match cond {
        Expr::Bin { op, lhs, rhs } => (*op, &**lhs, &**rhs),
        _ => return None,
    };
    let negated = match (op, is_var(lhs, param), is_var(rhs, param)) {
        (BinOp::Le, true, false) if is_literal(rhs) => BinOp::Gt,
        (BinOp::Lt, true, false) if is_literal(rhs) => BinOp::Ge,
        (BinOp::Ge, false, true) if is_literal(lhs) => BinOp::Lt,
        (BinOp::Gt, false, true) if is_literal(lhs) => BinOp::Le,
        _ => return None,
    };
    Some(Expr::Bin {
        op: negated,
        lhs: Box::new(lhs.clone()),
        rhs: Box::new(rhs.clone()),
    })
}

/// May this statement run on every level of the rewritten loop?
///
/// It may call other functions — they are not affected by the rewrite, and
/// they run in the same order — but it may not call the function being
/// rewritten (that call becomes the loop) and it may not assign (the
/// original assigns in a fresh frame per level, the rewrite in one).
fn is_per_level_safe(stmt: &Stmt, name: &str) -> bool {
    match stmt {
        Stmt::Expr(Expr::Call {
            name: callee,
            args,
        }) => {
            callee != name
                && args
                    .iter()
                    .all(|a| !contains_assign(a) && !contains_call_to(a, name))
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
