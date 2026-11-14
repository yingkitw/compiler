//! Expression translation: the type rules and the code they emit.
//!
//! The rules match the earlier type-checking backend, word for word:
//! this file is where a program's types are checked, and the code is
//! emitted as it goes. The instructions themselves are written by the
//! `asm` module — this file says *what* to emit, not how it looks.

mod array;
mod binop;
mod call;

/// An assignment statement, with its value dead — `binop::gen_assign_stmt`
/// may then write a register resident directly.
pub(super) fn gen_assign_stmt(f: &mut Fn, name: &str, value: &Expr) -> Result<(), String> {
    binop::gen_assign_stmt(f, name, value)
}

use crate::ast::*;

use super::asm::reg::{D, FP, X};
use super::asm::{Asm, Cond, Label, Mem};
use super::{Fn, Local, Val};

/// The type a literal should adopt given the other operand's type. Only
/// `int` is contagious: a `number` literal in an `int` context stays a
/// `number`, because widening is not implicit.
pub fn int_hint(t: Type) -> Option<Type> {
    if t == Type::Int { Some(Type::Int) } else { None }
}

/// Translate an expression, leaving its value in the register its type
/// calls for.
pub fn gen_expr(f: &mut Fn, e: &Expr) -> Result<(Val, Type), String> {
    match e {
        Expr::Num(lit) => {
            let asm = f.asm();
            if !asm.fmov_d_imm(D(0), lit.value) {
                asm.load_imm(X(0), lit.value.to_bits() as i64);
                asm.fmov_d_from_x(D(0), X(0));
            }
            Ok((Val::Num, Type::Number))
        }
        Expr::Bool(x) => {
            f.asm().mov_imm(X(0), i64::from(*x));
            Ok((Val::Int, Type::Boolean))
        }
        Expr::Str(s) => {
            let label = f.string_label();
            let bytes = s.as_bytes();
            f.asm().emit_string_rodata(&label, bytes);
            f.asm().load_pie_addr(X(0), &label);
            f.asm()
                .mov_x_imm(X(1), i64::try_from(bytes.len()).expect("string fits in i64"));
            Ok((Val::Str, Type::String))
        }
        Expr::Var(name) => {
            let local = f.lookup(name)?;
            load_local(f, &local);
            Ok((Val::of(local.ty), local.ty))
        }
        Expr::Assign { name, value } => {
            let local = f.lookup(name)?;
            if !local.mutable {
                return Err(format!("cannot assign to const '{}'", name));
            }
            let (_, vt) = gen_expr(f, value)?;
            if vt != local.ty {
                return Err(format!("cannot assign {} to '{}: {}'", vt, name, local.ty));
            }
            store_local(f, &local);
            Ok((Val::of(local.ty), local.ty))
        }
        Expr::Cond {
            cond,
            then_expr,
            else_expr,
        } => gen_cond(f, cond, then_expr, else_expr),
        Expr::BlockExpr { stmts, value } => {
            f.push_scope();
            for s in stmts {
                if super::stmt::gen_stmt(f, s)? {
                    f.pop_scope();
                    return Err(
                        "block expression cannot contain a return on every path before its value"
                            .into(),
                    );
                }
            }
            let result = gen_expr(f, value)?;
            f.pop_scope();
            if result.1 == Type::Void {
                return Err("block expression value cannot be void".into());
            }
            Ok(result)
        }
        Expr::Bin { op, lhs, rhs } => binop::gen_binop(f, *op, lhs, rhs),
        Expr::Un { op, expr } => {
            let (v, t) = gen_expr(f, expr)?;
            match op {
                UnOp::Neg => match t {
                    Type::Number => {
                        f.asm().fneg(D(0));
                        Ok((v, t))
                    }
                    Type::Int => {
                        f.asm().neg(X(0));
                        Ok((v, t))
                    }
                    Type::String
                    | Type::Boolean
                    | Type::Void
                    | Type::NumberArray
                    | Type::BooleanArray => Err(format!("cannot negate {}", t)),
                },
                UnOp::Not => {
                    as_bool(f.asm(), v, t)?;
                    f.asm().cmp_imm(X(0), 0);
                    f.asm().set(X(0), Cond::Eq);
                    Ok((Val::Int, Type::Boolean))
                }
            }
        }
        Expr::Cast { expr, ty } => {
            let (v, t) = gen_expr(f, expr)?;
            cast(f, v, t, *ty)
        }
        Expr::Array(elems) => {
            if elems.is_empty() {
                return Err("empty array literal needs a type annotation on the variable".into());
            }
            array::gen_array_lit(f, elems)
        }
        Expr::Index { base, index } => array::gen_index(f, base, index),
        Expr::Member { base, name } => array::gen_member(f, base, name),
        Expr::Call { name, receiver, args } => call::gen_call(f, name, receiver.as_deref(), args),
    }
}

/// The memory a local lives at.
fn local_mem(local: &Local) -> Mem {
    Mem {
        base: FP,
        offset: local.offset,
    }
}

/// Load a local into the register its type calls for. A cached local is
/// moved from its callee-saved register; anything else comes from its
/// slot.
pub(super) fn load_local(f: &mut Fn, local: &Local) {
    if let Some((reg, true)) = f.cached(local) {
        f.asm().fmov_d_from_d(D(0), D(reg));
        return;
    }
    if let Some((reg, false)) = f.cached(local) {
        f.asm().mov_reg(X(0), X(reg));
        return;
    }
    let asm = f.asm();
    let mem = local_mem(local);
    match local.ty {
        Type::Number => asm.load_double(D(0), mem),
        Type::NumberArray | Type::BooleanArray => asm.load(X(0), mem),
        Type::String => {
            asm.load(X(0), mem);
            asm.load(
                X(1),
                Mem {
                    base: FP,
                    offset: local.offset - 8,
                },
            );
        }
        _ => asm.load(X(0), mem),
    }
}

/// Store the register into a local — its cache register when it has one,
/// its slot otherwise.
pub(super) fn store_local(f: &mut Fn, local: &Local) {
    if let Some((reg, true)) = f.cached(local) {
        f.asm().fmov_d_from_d(D(reg), D(0));
        f.mark_dirty(local);
        return;
    }
    if let Some((reg, false)) = f.cached(local) {
        f.asm().mov_reg(X(reg), X(0));
        f.mark_dirty(local);
        return;
    }
    let asm = f.asm();
    let mem = local_mem(local);
    match local.ty {
        Type::Number => asm.store_double(D(0), mem),
        Type::NumberArray | Type::BooleanArray => asm.store(X(0), mem),
        Type::String => {
            asm.store(X(0), mem);
            asm.store(
                X(1),
                Mem {
                    base: FP,
                    offset: local.offset - 8,
                },
            );
        }
        _ => asm.store(X(0), mem),
    }
}

/// Store the register into a temporary slot.
pub(super) fn store_temp(asm: &mut Asm, offset: i32, ty: Type) {
    let mem = Mem { base: FP, offset };
    match ty {
        Type::Number => asm.store_double(D(0), mem),
        Type::NumberArray | Type::BooleanArray => asm.store(X(0), mem),
        Type::String => {
            asm.store(X(0), mem);
            asm.store(X(1), Mem { base: FP, offset: offset - 8 });
        }
        _ => asm.store(X(0), mem),
    }
}

/// Load a temporary slot into the register its type calls for.
pub(super) fn load_temp(asm: &mut Asm, offset: i32, ty: Type) {
    let mem = Mem { base: FP, offset };
    match ty {
        Type::Number => asm.load_double(D(0), mem),
        Type::NumberArray | Type::BooleanArray => asm.load(X(0), mem),
        Type::String => {
            asm.load(X(0), mem);
            asm.load(X(1), Mem { base: FP, offset: offset - 8 });
        }
        _ => asm.load(X(0), mem),
    }
}

fn gen_cond(
    f: &mut Fn,
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
) -> Result<(Val, Type), String> {
    let (cv, ct) = gen_expr(f, cond)?;
    as_bool(f.asm(), cv, ct)?;

    let then_label = f.label();
    let else_label = f.label();
    let merge_label = f.label();

    f.asm().cmp_imm(X(0), 0);
    f.asm().branch_if(Cond::Eq, &else_label);

    f.asm().place(&then_label);
    let (_, t_then) = gen_expr(f, then_expr)?;
    let slot = f.temp_for(t_then);
    store_temp(f.asm(), slot, t_then);
    f.asm().branch(&merge_label);

    f.asm().place(&else_label);
    let (_, t_else) = gen_expr(f, else_expr)?;
    if t_then != t_else {
        return Err(format!(
            "conditional branches have mismatched types: {} and {}",
            t_then, t_else
        ));
    }
    store_temp(f.asm(), slot, t_else);

    f.asm().place(&merge_label);
    load_temp(f.asm(), slot, t_then);
    Ok((Val::of(t_then), t_then))
}

/// Branch to `target` when `cond` evaluates to `when`.
///
/// A comparison is fused: its flags branch directly, with the inverse
/// condition (the float inverses are chosen so an unordered comparison —
/// a `NaN` operand — takes the branch, because every comparison on `NaN`
/// is false). Anything else is materialised into a boolean and tested.
pub(super) fn branch_when(
    f: &mut Fn,
    cond: &Expr,
    target: &Label,
    when: bool,
) -> Result<(), String> {
    // `!e` inverts the test.
    if let Expr::Un {
        op: UnOp::Not,
        expr,
    } = cond
    {
        return branch_when(f, expr, target, !when);
    }
    if let Expr::Bin { op, lhs, rhs } = cond {
        let is_cmp = matches!(
            op,
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge | BinOp::Eq | BinOp::Ne
        );
        let string_eq =
            matches!(op, BinOp::Eq | BinOp::Ne) && binop::expr_might_be_string(f, lhs)
                || matches!(op, BinOp::Eq | BinOp::Ne) && binop::expr_might_be_string(f, rhs);
        if is_cmp && !string_eq {
            let (lt, rt, l, r) = binop::gen_operands(f, lhs, rhs)?;
            if lt != rt {
                return Err(match op {
                    BinOp::Eq | BinOp::Ne => {
                        format!("cannot compare {} with {}", lt, rt)
                    }
                    _ => format!(
                        "relational operators require two numbers or two ints, got {} and {}",
                        lt, rt
                    ),
                });
            }
            let asm = f.asm();
            let (holds, fails) = if lt == Type::Number {
                asm.fcmp(D(l), D(r));
                match op {
                    BinOp::Eq => (Cond::Eq, Cond::Ne),
                    BinOp::Ne => (Cond::Ne, Cond::Eq),
                    BinOp::Lt => (Cond::Mi, Cond::Pl),
                    BinOp::Le => (Cond::Ls, Cond::Hi),
                    BinOp::Gt => (Cond::Gt, Cond::Le),
                    BinOp::Ge => (Cond::Ge, Cond::Lt),
                    _ => unreachable!("checked above"),
                }
            } else {
                asm.cmp(X(l), X(r));
                match op {
                    BinOp::Eq => (Cond::Eq, Cond::Ne),
                    BinOp::Ne => (Cond::Ne, Cond::Eq),
                    BinOp::Lt => (Cond::Lt, Cond::Ge),
                    BinOp::Le => (Cond::Le, Cond::Gt),
                    BinOp::Gt => (Cond::Gt, Cond::Le),
                    BinOp::Ge => (Cond::Ge, Cond::Lt),
                    _ => unreachable!("checked above"),
                }
            };
            asm.branch_if(if when { holds } else { fails }, target);
            return Ok(());
        }
    }
    let (v, ct) = gen_expr(f, cond)?;
    as_bool(f.asm(), v, ct)?;
    f.asm().cmp_imm(X(0), 0);
    f.asm().branch_if(if when { Cond::Ne } else { Cond::Eq }, target);
    Ok(())
}

/// Convert a value to a boolean: a number is false iff it equals zero.
pub(super) fn as_bool(asm: &mut Asm, v: Val, t: Type) -> Result<Val, String> {
    match t {
        Type::Boolean => Ok(v),
        Type::Number => {
            asm.mov_imm(X(1), 0);
            asm.fmov_d_from_x(D(1), X(1));
            asm.fcmp(D(0), D(1));
            asm.set(X(0), Cond::Ne);
            Ok(Val::Int)
        }
        Type::Int => {
            asm.cmp_imm(X(0), 0);
            asm.set(X(0), Cond::Ne);
            Ok(Val::Int)
        }
        Type::String | Type::NumberArray | Type::BooleanArray => {
            Err(format!("{} expression has no truth value", t))
        }
        Type::Void => Err("void expression has no truth value".into()),
    }
}

/// A cast between the numeric types.
fn cast(f: &mut Fn, v: Val, t: Type, target: Type) -> Result<(Val, Type), String> {
    if target == t {
        return Ok((v, t));
    }
    match (target, t) {
        // number → int: truncate toward zero, with NaN becoming zero and
        // an out-of-range value saturating — the runtime does it, because
        // the hardware's conversion traps.
        (Type::Int, Type::Number) => {
            f.asm().call(&Fn::symbol("ts_double_to_int"));
            Ok((Val::Int, Type::Int))
        }
        // int → number: exact up to 2^53, and rounded beyond it.
        (Type::Number, Type::Int) => {
            f.asm().int_to_double(D(0), X(0));
            Ok((Val::Num, Type::Number))
        }
        (Type::Void, _) => Err("cannot cast to 'void'".into()),
        _ => Err(format!("cannot cast {} to {}", t, target)),
    }
}

/// Translate an expression, optionally with the type its context expects.
/// Only integer literals are affected: in an `int` context they become
/// `int` (they must be integral and in range); everywhere else they stay
/// `number`.
pub fn gen_expr_as(f: &mut Fn, e: &Expr, expected: Option<Type>) -> Result<(Val, Type), String> {
    if let (Expr::Array(elems), Some(at)) = (e, expected) {
        if elems.is_empty() && matches!(at, Type::NumberArray | Type::BooleanArray) {
            return array::gen_empty_array(f, at);
        }
    }
    if expected == Some(Type::Int) {
        // A negative literal is a unary minus over a literal, and it should
        // adopt the context's type the way a positive one does: `let x: int
        // = -5;` is an int, not a number.
        let (lit, negative) = match e {
            Expr::Num(lit) => (lit, false),
            Expr::Un {
                op: UnOp::Neg,
                expr,
            } => match &**expr {
                Expr::Num(lit) => (lit, true),
                _ => return gen_expr(f, e),
            },
            _ => return gen_expr(f, e),
        };
        // Use the literal's exact integer value when it has one. A large
        // literal such as 9007199254740993 does not fit in an f64, so the
        // double must not be the source of truth here.
        let exact = match lit.int {
            Some(i) => {
                if negative {
                    i.checked_neg().ok_or_else(|| {
                        format!("the value {} does not fit in an int", lit.value)
                    })?
                } else {
                    i
                }
            }
            None if lit.fraction_point => return gen_expr(f, e),
            None => {
                let value = if negative { -lit.value } else { lit.value };
                if value.fract() != 0.0 {
                    return Err(format!(
                        "cannot use the non-integer value {} as an int",
                        value
                    ));
                }
                if value < i64::MIN as f64 || value > i64::MAX as f64 {
                    return Err(format!("the value {} does not fit in an int", value));
                }
                value as i64
            }
        };
        f.asm().mov_x_imm(X(0), exact);
        return Ok((Val::Int, Type::Int));
    }
    gen_expr(f, e)
}
