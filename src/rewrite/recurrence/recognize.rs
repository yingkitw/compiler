//! Recognising the linear-recurrence shape.
//!
//! A rewrite that fires on the wrong program is a miscompile, so this half
//! is deliberately narrow: it either recognises the shape exactly, or it
//! gives up and the program is left as it was written.

use crate::ast::*;

use super::super::common::{is_pure, is_var};
use super::Recurrence;

/// Recognise the linear-recurrence shape, or give up.
pub(super) fn match_recurrence(
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
        ..
    } = &body[0]
    else {
        return None;
    };
    if !els.is_empty() || then.len() != 1 {
        return None;
    }
    let Stmt::Return {
        value: Some(base),
        ..
    } = &then[0]
    else {
        return None;
    };
    // The base expression must be free of side effects: the source program
    // evaluates it once per leaf of the call tree, the rewrite evaluates it
    // once per starting value.
    if !is_pure(base) {
        return None;
    }

    // The rest of the body is a single return.
    let [Stmt::Return {
        value: Some(tail),
        ..
    }] = &body[1..]
    else {
        return None;
    };

    // The expression must call the function, at constant offsets, and
    // everything else in it must be free of side effects.
    let mut offsets = Vec::new();
    if !collect_offsets(tail, name, params, &mut offsets, 0, forbid_param_reads) {
        return None;
    }
    let width = *offsets.iter().max()?;
    // The stride: the largest number every offset the expression uses is a
    // multiple of. A stride of one is the plain lattice — one value per
    // argument. A stride of two or more means the arguments the recursion
    // reaches all lie on one lattice (the ones congruent to the initial
    // argument), so the loop can step by the stride and keep one
    // accumulator per lattice position instead of one per argument.
    let mut stride = 0i64;
    for offset in &offsets {
        stride = gcd(*offset, stride);
    }

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
        stride,
        tail: tail.clone(),
    })
}

/// The largest number that divides both arguments.
fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 { a } else { gcd(b, a % b) }
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
            receiver: None,
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
        Expr::Num(_) | Expr::Bool(_) | Expr::Str(_) => true,
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
        Expr::Cond { .. } | Expr::BlockExpr { .. } => false,
        Expr::Array(_) | Expr::Index { .. } | Expr::Member { .. } => false,
    }
}


/// The offset of a self-call: how much smaller its first argument is than
/// the first parameter.
pub(super) fn call_offset(args: &[Expr], params: &[(String, Type)]) -> Option<i64> {
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
