//! Binary operations, comparisons, operand ordering, and short-circuit logic.

use crate::ast::*;

use super::super::asm::reg::{D, FP, V, X};
use super::super::asm::{Cond, Mem};
use super::super::{Fn, Val};
use super::{as_bool, gen_expr, gen_expr_as, int_hint, load_temp, store_temp};

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
            let (lt, rt) = gen_operands(f, lhs, rhs)?;
            if lt != rt {
                return Err(format!("cannot compare {} with {}", lt, rt));
            }
            compare(f, lt, op);
            Ok((Val::Int, Type::Boolean))
        }
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let (lt, rt) = gen_operands(f, lhs, rhs)?;
            if lt != rt {
                return Err(format!(
                    "relational operators require two numbers or two ints, got {} and {}",
                    lt, rt
                ));
            }
            compare(f, lt, op);
            Ok((Val::Int, Type::Boolean))
        }
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
            if matches!(op, BinOp::Add)
                && (expr_might_be_string(f, lhs) || expr_might_be_string(f, rhs))
            {
                return gen_string_concat(f, lhs, rhs);
            }
            let (lt, rt) = gen_operands(f, lhs, rhs)?;
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
                    match op {
                        BinOp::Add => asm.add(X(0), X(0), X(1)),
                        BinOp::Sub => asm.sub(X(0), X(0), X(1)),
                        BinOp::Mul => asm.mul(X(0), X(0), X(1)),
                        BinOp::Div => {
                            let div_zero = div_zero.expect("label reserved for div");
                            let past_div = past_div.expect("label reserved past div");
                            asm.cmp_imm(X(1), 0);
                            asm.branch_if(Cond::Eq, &div_zero);
                            asm.sdiv(X(0), X(0), X(1));
                            asm.branch(&past_div);
                            asm.place(&div_zero);
                            asm.call(&Fn::symbol("ts_int_div_zero"));
                            asm.place(&past_div);
                        }
                        BinOp::Rem => asm.srem(X(0), X(0), X(1)),
                        _ => unreachable!(),
                    }
                    Ok((Val::Int, Type::Int))
                }
                (Type::Number, Type::Number) => {
                    let asm = f.asm();
                    match op {
                        BinOp::Add => asm.fadd(D(0), D(0), D(1)),
                        BinOp::Sub => asm.fsub(D(0), D(0), D(1)),
                        BinOp::Mul => asm.fmul(D(0), D(0), D(1)),
                        BinOp::Div => asm.fdiv(D(0), D(0), D(1)),
                        BinOp::Rem => {
                            asm.fmov_x_from_d(X(0), D(0));
                            asm.mov_v_from_x(V(0), X(0));
                            asm.fmov_x_from_d(X(0), D(1));
                            asm.mov_v_from_x(V(1), X(0));
                            asm.call(&Fn::symbol("ts_fmod"));
                            asm.fmov_d_from_v(D(0), V(0));
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

/// Compare the two operands, leaving the answer — true or false — in `x0`.
///
/// The left operand is in `x0`/`d0` and the right one in `x1`/`d1`.
fn compare(f: &mut Fn, t: Type, op: BinOp) {
    let asm = f.asm();
    match t {
        Type::Number => {
            asm.fcmp(D(0), D(1));
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
            asm.cmp(X(0), X(1));
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

/// The two operands of a binary operation, in `x0`/`d0` and `x1`/`d1`.
fn gen_operands(f: &mut Fn, lhs: &Expr, rhs: &Expr) -> Result<(Type, Type), String> {
    let literal = |e: &Expr| matches!(e, Expr::Num(_));
    let mark = f.temp_mark();
    if literal(lhs) && !literal(rhs) {
        let (_, rt) = gen_expr(f, rhs)?;
        let slot = f.temp();
        store_temp(f.asm(), slot, rt);
        let (_, lt) = gen_lit_or_expr(f, lhs, int_hint(rt))?;
        let mem = Mem { base: FP, offset: slot };
        match rt {
            Type::Number => f.asm().load_double(D(1), mem),
            _ => f.asm().load(X(1), mem),
        }
        f.release_temps(mark);
        Ok((lt, rt))
    } else {
        let (_, lt) = gen_expr(f, lhs)?;
        let slot = f.temp();
        store_temp(f.asm(), slot, lt);
        let (_, rt) = gen_lit_or_expr(f, rhs, int_hint(lt))?;
        spill_right(f, rt);
        load_temp(f.asm(), slot, lt);
        f.release_temps(mark);
        Ok((lt, rt))
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

fn expr_might_be_string(f: &Fn, e: &Expr) -> bool {
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
