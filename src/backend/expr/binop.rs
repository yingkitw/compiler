//! Binary operations, comparisons, operand ordering, and short-circuit logic.

use crate::ast::*;

use super::super::asm::reg::{D, X};
use super::super::asm::Cond;
use super::super::{Fn, Val};
use super::{as_bool, gen_expr, gen_expr_as, int_hint, load_temp, store_temp, store_local};

/// A binary operation.
pub(super) fn gen_binop(
    f: &mut Fn,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
) -> Result<(Val, Type), String> {
    match op {
        BinOp::And | BinOp::Or => gen_short_circuit(f, op, lhs, rhs),
        BinOp::Eq | BinOp::Ne => {
            if expr_might_be_string(f, lhs) || expr_might_be_string(f, rhs) {
                return gen_string_equal(f, lhs, rhs, op);
            }
            let (lt, rt, l, r) = gen_operands(f, lhs, rhs)?;
            if lt != rt {
                return Err(format!("cannot compare {} with {}", lt, rt));
            }
            compare(f, lt, op, l, r);
            Ok((Val::Int, Type::Boolean))
        }
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let (lt, rt, l, r) = gen_operands(f, lhs, rhs)?;
            if lt != rt {
                return Err(format!(
                    "relational operators require two numbers or two ints, got {} and {}",
                    lt, rt
                ));
            }
            compare(f, lt, op, l, r);
            Ok((Val::Int, Type::Boolean))
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
            if matches!(op, BinOp::Add)
                && (expr_might_be_string(f, lhs) || expr_might_be_string(f, rhs))
            {
                return gen_string_concat(f, lhs, rhs);
            }
            let (lt, rt, l, r) = gen_operands(f, lhs, rhs)?;
            match (lt, rt) {
                (Type::Int, Type::Int) => {
                    let div_zero = if matches!(op, BinOp::Div) {
                        Some(f.label())
                    } else {
                        None
                    };
                    let past_div = if matches!(op, BinOp::Div) {
                        Some(f.label())
                    } else {
                        None
                    };
                    let asm = f.asm();
                    let (l, r) = (X(l), X(r));
                    match op {
                        BinOp::Add => asm.add(X(0), l, r),
                        BinOp::Sub => asm.sub(X(0), l, r),
                        BinOp::Mul => asm.mul(X(0), l, r),
                        BinOp::Div => {
                            let div_zero = div_zero.expect("label reserved for div");
                            let past_div = past_div.expect("label reserved past div");
                            asm.cmp_imm(r, 0);
                            asm.branch_if(Cond::Eq, &div_zero);
                            asm.sdiv(X(0), l, r);
                            asm.branch(&past_div);
                            asm.place(&div_zero);
                            asm.call(&Fn::symbol("ts_int_div_zero"));
                            asm.place(&past_div);
                        }
                        BinOp::Rem => asm.srem(X(0), l, r),
                        _ => unreachable!(),
                    }
                    Ok((Val::Int, Type::Int))
                }
                (Type::Number, Type::Number) => {
                    let asm = f.asm();
                    let (l, r) = (D(l), D(r));
                    match op {
                        BinOp::Add => asm.fadd(D(0), l, r),
                        BinOp::Sub => asm.fsub(D(0), l, r),
                        BinOp::Mul => asm.fmul(D(0), l, r),
                        BinOp::Div => asm.fdiv(D(0), l, r),
                        BinOp::Rem => {
                            // The runtime takes `(lhs, rhs)` in `v0`/`v1`,
                            // whose low halves are `d0`/`d1` — and writing
                            // those clobbers operands that live there, so
                            // stage through scratch registers.
                            asm.fmov_d_from_d(D(2), l);
                            asm.fmov_d_from_d(D(3), r);
                            asm.fmov_d_from_d(D(0), D(2));
                            asm.fmov_d_from_d(D(1), D(3));
                            asm.call(&Fn::symbol("ts_fmod"));
                        }
                        _ => unreachable!(),
                    }
                    Ok((Val::Num, Type::Number))
                }
                _ => Err(format!(
                    "arithmetic operators require two numbers or two ints, got {} and {}",
                    lt, rt
                )),
            }
        }
    }
}

/// An assignment statement (`x = a + b;`): when the target is a register
/// resident and the value is arithmetic of its own type, the operation
/// writes the resident register directly — no trip through `d0`, so a
/// loop-carried value's chain is just the operation's own latency. Every
/// other case is the general expression path.
pub(super) fn gen_assign_stmt(f: &mut Fn, name: &str, value: &Expr) -> Result<(), String> {    let local = f.lookup(name)?;
    if !local.mutable {
        return Err(format!("cannot assign to const '{}'", name));
    }
    if let Some((reg, _)) = f.cached(&local)
        && let Expr::Bin { op, lhs, rhs } = value
    {
        let arithmetic = matches!(
            op,
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
        );
        let concat = matches!(op, BinOp::Add)
            && (expr_might_be_string(f, lhs) || expr_might_be_string(f, rhs));
        if arithmetic && !concat {
            let t = gen_arith_into(f, *op, lhs, rhs, reg, local.ty)?;
            if t != local.ty {
                return Err(format!("cannot assign {} to '{}: {}'", t, name, local.ty));
            }
            f.mark_dirty(&local);
            return Ok(());
        }
    }
    let (_, vt) = gen_expr(f, value)?;
    if vt != local.ty {
        return Err(format!("cannot assign {} to '{}: {}'", vt, name, local.ty));
    }
    store_local(f, &local);
    Ok(())
}

/// An arithmetic operation written straight into a register — the
/// destination a cached local lives in. A type the local cannot hold
/// still computes (into the result register) so the caller reports the
/// assignment error the general path would; every other type error is
/// the binary operation's own.
fn gen_arith_into(
    f: &mut Fn,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
    dst: u8,
    expected: Type,
) -> Result<Type, String> {
    let (lt, rt, l, r) = gen_operands(f, lhs, rhs)?;
    let (num, dst) = match (lt, rt) {
        (Type::Number, Type::Number) => (true, if expected == Type::Number { dst } else { 0 }),
        (Type::Int, Type::Int) => (false, if expected == Type::Int { dst } else { 0 }),
        _ => {
            return Err(format!(
                "arithmetic operators require two numbers or two ints, got {} and {}",
                lt, rt
            ))
        }
    };
    if num {
        let asm = f.asm();
        let (l, r) = (D(l), D(r));
        match op {
            BinOp::Add => asm.fadd(D(dst), l, r),
            BinOp::Sub => asm.fsub(D(dst), l, r),
            BinOp::Mul => asm.fmul(D(dst), l, r),
            BinOp::Div => asm.fdiv(D(dst), l, r),
            BinOp::Rem => {
                asm.fmov_d_from_d(D(2), l);
                asm.fmov_d_from_d(D(3), r);
                asm.fmov_d_from_d(D(0), D(2));
                asm.fmov_d_from_d(D(1), D(3));
                asm.call(&Fn::symbol("ts_fmod"));
                asm.fmov_d_from_d(D(dst), D(0));
            }
            _ => unreachable!("checked by the caller"),
        }
        Ok(Type::Number)
    } else {
        let div_labels = if matches!(op, BinOp::Div) {
            let div_zero = f.label();
            let past_div = f.label();
            Some((div_zero, past_div))
        } else {
            None
        };
        let asm = f.asm();
        let (l, r) = (X(l), X(r));
        match op {
            BinOp::Add => asm.add(X(dst), l, r),
            BinOp::Sub => asm.sub(X(dst), l, r),
            BinOp::Mul => asm.mul(X(dst), l, r),
            BinOp::Div => {
                let (div_zero, past_div) =
                    div_labels.expect("labels reserved for div");
                asm.cmp_imm(r, 0);
                asm.branch_if(Cond::Eq, &div_zero);
                asm.sdiv(X(dst), l, r);
                asm.branch(&past_div);
                asm.place(&div_zero);
                asm.call(&Fn::symbol("ts_int_div_zero"));
                asm.place(&past_div);
            }
            BinOp::Rem => asm.srem(X(dst), l, r),
            _ => unreachable!("checked by the caller"),
        }
        Ok(Type::Int)
    }
}

/// Compare the two operands, leaving the answer — true or false — in `x0`.
///
/// The registers holding the operands may be result registers or the
/// callee-saved homes of cached locals; the comparison itself is always
/// `lhs <op> rhs`.
fn compare(f: &mut Fn, t: Type, op: BinOp, l: u8, r: u8) {
    let asm = f.asm();
    match t {
        Type::Number => {
            asm.fcmp(D(l), D(r));
            let cond = match op {
                BinOp::Eq => Cond::Eq,
                BinOp::Ne => Cond::Ne,
                BinOp::Lt => Cond::Mi,
                BinOp::Le => Cond::Ls,
                BinOp::Gt => Cond::Gt,
                BinOp::Ge => Cond::Ge,
                _ => unreachable!(),
            };
            asm.set(X(0), cond);
        }
        _ => {
            asm.cmp(X(l), X(r));
            let cond = match op {
                BinOp::Eq => Cond::Eq,
                BinOp::Ne => Cond::Ne,
                BinOp::Lt => Cond::Lt,
                BinOp::Le => Cond::Le,
                BinOp::Gt => Cond::Gt,
                BinOp::Ge => Cond::Ge,
                _ => unreachable!(),
            };
            asm.set(X(0), cond);
        }
    }
}

/// The two operands of a binary operation: their types, and the registers
/// holding them — `d`-registers for a `number` pair, `x`-registers
/// otherwise. A register may be the callee-saved home of a cached local,
/// which the other operand's evaluation cannot touch.
pub(super) type Operands = (Type, Type, u8, u8);

/// An expression that may be evaluated while the other operand's value
/// waits in `x1`/`d1`: its evaluation touches nothing but the result
/// register and it has no side effects, so the waiting value survives it.
///
/// A `string` variable is excluded — loading one also fills `x1` with its
/// length — and a cast to `int` is excluded because it calls the runtime.
/// Anything that can assign, call, or branch is excluded by shape.
fn is_simple(f: &Fn, e: &Expr) -> bool {
    match e {
        Expr::Num(_) | Expr::Bool(_) => true,
        Expr::Var(name) => f
            .lookup(name)
            .is_ok_and(|local| local.ty != Type::String),
        Expr::Un {
            op: UnOp::Neg,
            expr,
        } => is_simple(f, expr),
        Expr::Cast { expr, ty } => *ty == Type::Number && is_simple(f, expr),
        _ => false,
    }
}

/// A variable operand that lives in a callee-saved register: `(register,
/// name)`. The register can be used as an operand directly, because
/// nothing expression evaluation does touches it — with one exception the
/// caller checks: an assignment to the same variable inside the other
/// operand.
fn cached_var(f: &Fn, e: &Expr) -> Option<(u8, String)> {
    let Expr::Var(name) = e else { return None };
    let local = f.lookup(name).ok()?;
    f.cached(&local).map(|(reg, _)| (reg, name.clone()))
}

/// The two operands of a binary operation.
///
/// A variable cached in a callee-saved register is used where it lives —
/// its value is already in a register the other operand's evaluation
/// cannot clobber. The exception: the other operand assigns that same
/// variable, as `x + (x = 5)` does — then the cached value is copied
/// aside first, because the register is live storage, not a snapshot.
///
/// Otherwise, the operand evaluated *last* is one the other's value can
/// wait for: a number literal (so it can adopt the other operand's type —
/// see `int_hint` — and literals are trivially `is_simple`), or whichever
/// side `is_simple`. When the last operand is simple, the first one's
/// value moves to `x1`/`d1` and stays there; the stack is only needed
/// when *neither* operand is simple, because evaluating the second one
/// may then clobber any register. The program's effects keep their order
/// either way: the operand moved later has none.
pub(super) fn gen_operands(f: &mut Fn, lhs: &Expr, rhs: &Expr) -> Result<Operands, String> {
    match (cached_var(f, lhs), cached_var(f, rhs)) {
        (Some((l, ln)), Some((r, rn))) => {
            return Ok((hint_ty(f, &ln), hint_ty(f, &rn), l, r));
        }
        (Some((l, name)), None) => {
            let hint = hint_ty(f, &name);
            if assigns_to(rhs, &name) {
                // The register is live storage, not a snapshot: copy the
                // operand's value aside before the assignment replaces it.
                snapshot(f, l, hint);
                let (_, rt) = gen_lit_or_expr(f, rhs, int_hint(hint))?;
                return Ok((hint, rt, 1, 0));
            }
            let (_, rt) = gen_lit_or_expr(f, rhs, int_hint(hint))?;
            return Ok((hint, rt, l, 0));
        }
        (None, Some((r, name))) => {
            let hint = hint_ty(f, &name);
            let (_, lt) = gen_lit_or_expr(f, lhs, int_hint(hint))?;
            // `lhs` ran first, so if it assigned the variable, its
            // register already holds the new value — what a left-to-right
            // evaluation of the pair sees.
            return Ok((lt, hint, 0, r));
        }
        (None, None) => {}
    }

    let lhs_lit = matches!(lhs, Expr::Num(_));
    let rhs_lit = matches!(rhs, Expr::Num(_));
    let second_is_lhs = if lhs_lit != rhs_lit {
        lhs_lit
    } else {
        !is_simple(f, rhs) && is_simple(f, lhs)
    };
    let (first, second) = if second_is_lhs { (rhs, lhs) } else { (lhs, rhs) };

    if is_simple(f, second) {
        // Registers: the first operand's value waits in x1/d1 while the
        // second one is evaluated into x0/d0 — so the second operand is
        // in R0 and the first in R1.
        let (_, t1) = gen_expr(f, first)?;
        spill_right(f, t1);
        let (_, t2) = gen_lit_or_expr(f, second, int_hint(t1))?;
        let (lt, rt) = if second_is_lhs { (t2, t1) } else { (t1, t2) };
        Ok((lt, rt, !second_is_lhs as u8, second_is_lhs as u8))
    } else {
        // Neither operand is simple: the first one's value must survive
        // the second one's evaluation, which may clobber any register —
        // so it takes a frame slot, and is reloaded into x0/d0 after.
        let mark = f.temp_mark();
        let (_, t1) = gen_expr(f, first)?;
        let slot = f.temp_for(t1);
        store_temp(f.asm(), slot, t1);
        let (_, t2) = gen_lit_or_expr(f, second, int_hint(t1))?;
        spill_right(f, t2);
        load_temp(f.asm(), slot, t1);
        f.release_temps(mark);
        let (lt, rt) = if second_is_lhs { (t2, t1) } else { (t1, t2) };
        Ok((lt, rt, second_is_lhs as u8, !second_is_lhs as u8))
    }
}

/// Copy a cached operand's value into `d1`/`x1`, where an operation
/// expects its other operand to wait.
fn snapshot(f: &mut Fn, reg: u8, ty: Type) {
    if ty == Type::Number {
        f.asm().fmov_d_from_d(D(1), D(reg));
    } else {
        f.asm().mov_reg(X(1), X(reg));
    }
}

/// The declared type of a variable operand.
fn hint_ty(f: &Fn, name: &str) -> Type {
    f.lookup(name).map(|l| l.ty).unwrap_or(Type::Number)
}

/// Whether evaluating `e` may assign the local `name` — including through
/// nested expressions and the statements of a block expression. Used
/// conservatively: a false positive only costs a register copy.
fn assigns_to(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Assign { name: n, value } => n == name || assigns_to(value, name),
        Expr::Bin { lhs, rhs, .. } => assigns_to(lhs, name) || assigns_to(rhs, name),
        Expr::Un { expr, .. } | Expr::Cast { expr, .. } => assigns_to(expr, name),
        Expr::Cond {
            cond,
            then_expr,
            else_expr,
        } => {
            assigns_to(cond, name) || assigns_to(then_expr, name) || assigns_to(else_expr, name)
        }
        Expr::BlockExpr { stmts, value } => {
            stmts.iter().any(|s| stmt_assigns_to(s, name)) || assigns_to(value, name)
        }
        Expr::Array(elems) => elems.iter().any(|e| assigns_to(e, name)),
        Expr::Index { base, index } => assigns_to(base, name) || assigns_to(index, name),
        Expr::Member { base, .. } => assigns_to(base, name),
        Expr::Call {
            receiver, args, ..
        } => {
            receiver.as_deref().map(|r| assigns_to(r, name)).unwrap_or(false)
                || args.iter().any(|a| assigns_to(a, name))
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Var(_) => false,
    }
}

/// The statement half of `assigns_to`.
fn stmt_assigns_to(s: &Stmt, name: &str) -> bool {
    match s {
        Stmt::Let { init, .. } => init.as_ref().is_some_and(|e| assigns_to(e, name)),
        Stmt::If { cond, then, els, .. } => {
            assigns_to(cond, name)
                || then.iter().any(|s| stmt_assigns_to(s, name))
                || els.iter().any(|s| stmt_assigns_to(s, name))
        }
        Stmt::While { cond, body, .. } => {
            assigns_to(cond, name) || body.iter().any(|s| stmt_assigns_to(s, name))
        }
        Stmt::Block { stmts, .. } => stmts.iter().any(|s| stmt_assigns_to(s, name)),
        Stmt::Expr { expr, .. } => assigns_to(expr, name),
        Stmt::Return { value, .. } => {
            value.as_ref().is_some_and(|e| assigns_to(e, name))
        }
        Stmt::Func { .. } => false,
    }
}

fn spill_right(f: &mut Fn, t: Type) {
    let asm = f.asm();
    match t {
        Type::Number => asm.fmov_d_from_d(D(1), D(0)),
        _ => asm.mov_reg(X(1), X(0)),
    }
}

fn gen_lit_or_expr(f: &mut Fn, e: &Expr, expected: Option<Type>) -> Result<(Val, Type), String> {
    if matches!(e, Expr::Num(_)) && let Ok(v) = gen_expr_as(f, e, expected) {
        return Ok(v);
    }
    gen_expr(f, e)
}

/// `&&` and `||`: the right operand is only evaluated when the left one
/// does not decide the answer.
fn gen_short_circuit(
    f: &mut Fn,
    op: BinOp,
    lhs: &Expr,
    rhs: &Expr,
) -> Result<(Val, Type), String> {
    let (lv, lt) = gen_expr(f, lhs)?;
    let _ = as_bool(f.asm(), lv, lt)?;
    let slot = f.temp();
    store_temp(f.asm(), slot, Type::Boolean);

    let rhs_label = f.label();
    let short_label = f.label();
    let merge_label = f.label();

    f.asm().cmp_imm(X(0), 0);
    match op {
        BinOp::And => f.asm().branch_if(Cond::Eq, &short_label),
        BinOp::Or => f.asm().branch_if(Cond::Ne, &short_label),
        _ => unreachable!(),
    }

    f.asm().branch(&rhs_label);
    f.asm().place(&rhs_label);
    let (rv, rt) = gen_expr(f, rhs)?;
    let _ = as_bool(f.asm(), rv, rt)?;
    store_temp(f.asm(), slot, Type::Boolean);
    f.asm().branch(&merge_label);

    f.asm().place(&short_label);
    load_temp(f.asm(), slot, Type::Boolean);
    f.asm().branch(&merge_label);

    f.asm().place(&merge_label);
    load_temp(f.asm(), slot, Type::Boolean);
    Ok((Val::Int, Type::Boolean))
}

pub(super) fn expr_might_be_string(f: &Fn, e: &Expr) -> bool {
    match e {
        Expr::Str(_) => true,
        Expr::Var(name) => f
            .lookup(name)
            .ok()
            .is_some_and(|local| local.ty == Type::String),
        Expr::Bin { op: BinOp::Add, lhs, rhs } => {
            expr_might_be_string(f, lhs) || expr_might_be_string(f, rhs)
        }
        _ => false,
    }
}

fn gen_string_pair(f: &mut Fn, lhs: &Expr, rhs: &Expr) -> Result<(), String> {
    let mark = f.temp_mark();
    let (_, lt) = gen_expr(f, lhs)?;
    if lt != Type::String {
        return Err(format!("expected string, got {}", lt));
    }
    let slot = f.temp_for(Type::String);
    store_temp(f.asm(), slot, Type::String);
    let (_, rt) = gen_expr(f, rhs)?;
    if rt != Type::String {
        return Err(format!("expected string, got {}", rt));
    }
    f.asm().mov_reg(X(2), X(0));
    f.asm().mov_reg(X(3), X(1));
    load_temp(f.asm(), slot, Type::String);
    f.release_temps(mark);
    Ok(())
}

fn gen_string_concat(f: &mut Fn, lhs: &Expr, rhs: &Expr) -> Result<(Val, Type), String> {
    gen_string_pair(f, lhs, rhs)?;
    f.asm().call(&Fn::symbol("ts_str_concat"));
    Ok((Val::Str, Type::String))
}

fn gen_string_equal(f: &mut Fn, lhs: &Expr, rhs: &Expr, op: BinOp) -> Result<(Val, Type), String> {
    gen_string_pair(f, lhs, rhs)?;
    f.asm().call(&Fn::symbol("ts_str_equal"));
    f.asm().cmp_imm(X(0), 1);
    let cond = if op == BinOp::Eq { Cond::Eq } else { Cond::Ne };
    f.asm().set(X(0), cond);
    Ok((Val::Int, Type::Boolean))
}
