//! Calls to `print` and to user functions.

use crate::ast::*;

use super::super::asm::reg::{D, FP, X};
use super::super::asm::Mem;
use super::super::{Fn, Val};
use super::array;
use super::{gen_expr, gen_expr_as, store_temp};

#[derive(Clone, Copy)]
enum RegPlan {
    Int(u8),
    Num(u8),
    String(u8),
}

/// A call to a user function or to `print`.
pub(super) fn gen_call(
    f: &mut Fn,
    name: &str,
    receiver: Option<&Expr>,
    args: &[Expr],
) -> Result<(Val, Type), String> {
    if let Some(rcv) = receiver {
        if name == "push" {
            return array::gen_method_push(f, rcv, args);
        }
        return Err(format!("no method '{}'", name));
    }
    if name == "print" {
        if args.len() != 1 {
            return Err(format!("print takes 1 argument, got {}", args.len()));
        }
        let (_, t) = gen_expr(f, &args[0])?;
        let target = match t {
            Type::Number => "ts_print_number",
            Type::Int => "ts_print_int",
            Type::Boolean => "ts_print_bool",
            Type::String => "ts_print_string",
            Type::NumberArray | Type::BooleanArray => {
                return Err("cannot print an array".into());
            }
            Type::Void => return Err("cannot print void".into()),
        };
        f.asm().call(&Fn::symbol(target));
        return Ok((Val::Int, Type::Void));
    }

    let sig = f
        .funcs()
        .get(name)
        .cloned()
        .ok_or_else(|| format!("undefined function '{}'", name))?;
    if args.len() != sig.params.len() {
        return Err(format!(
            "'{}' takes {} arguments, got {}",
            name,
            sig.params.len(),
            args.len()
        ));
    }

    let mark = f.temp_mark();
    let n = args.len();
    // Every argument but the last is parked in a frame slot, because
    // evaluating a later argument may clobber any register. The last one
    // stays where its evaluation left it — `x0`/`d0` — and is moved
    // straight to its argument register; every earlier argument gets a
    // lower register, so loading them afterwards cannot clobber it.
    let mut evaluated: Vec<(Option<i32>, Type)> = Vec::new();
    for (i, (arg, ty)) in args.iter().zip(sig.params.iter()).enumerate() {
        let (_, at) = gen_expr_as(f, arg, Some(*ty))?;
        if at != *ty {
            return Err(format!(
                "argument of '{}' must be {}, got {}",
                name, ty, at
            ));
        }
        if i + 1 == n {
            evaluated.push((None, *ty));
        } else {
            let slot = f.temp_for(*ty);
            store_temp(f.asm(), slot, *ty);
            evaluated.push((Some(slot), *ty));
        }
    }

    let mut plan = Vec::new();
    let mut xi: u8 = 0;
    let mut vi: u8 = 0;
    for (_, ty) in &evaluated {
        match ty {
            Type::Number => {
                plan.push(RegPlan::Num(vi));
                vi += 1;
            }
            Type::String => {
                plan.push(RegPlan::String(xi));
                xi += 2;
            }
            _ => {
                plan.push(RegPlan::Int(xi));
                xi += 1;
            }
        }
    }

    // Reverse, so the last argument's register is filled first and the
    // earlier ones — in lower registers — are loaded after it.
    for ((slot, ty), slot_plan) in evaluated.iter().zip(plan.iter()).rev() {
        match (slot, ty, slot_plan) {
            (None, Type::Number, RegPlan::Num(vi)) => {
                if *vi == 0 {
                    // The argument is already in `d0`, the low half of `v0`.
                } else {
                    f.asm().fmov_d_from_d(D(*vi), D(0));
                }
            }
            (None, Type::String, RegPlan::String(x)) => {
                if *x != 0 {
                    f.asm().mov_reg(X(*x), X(0));
                    f.asm().mov_reg(X(*x + 1), X(1));
                }
            }
            (None, _, RegPlan::Int(x)) => {
                if *x != 0 {
                    f.asm().mov_reg(X(*x), X(0));
                }
            }
            (Some(slot), Type::Number, RegPlan::Num(vi)) => {
                // `dN` is the low half of `vN`, so the load lands in the
                // argument register itself.
                f.asm()
                    .load_double(D(*vi), Mem { base: FP, offset: *slot });
            }
            (Some(slot), Type::String, RegPlan::String(x)) => {
                f.asm().load(X(*x), Mem { base: FP, offset: *slot });
                f.asm()
                    .load(X(*x + 1), Mem { base: FP, offset: *slot - 8 });
            }
            (Some(slot), _, RegPlan::Int(x)) => {
                f.asm().load(X(*x), Mem { base: FP, offset: *slot });
            }
            _ => unreachable!("plan matches argument type"),
        }
    }

    f.asm().call(&Fn::symbol(&format!("tsfn_{}", name)));
    f.release_temps(mark);
    match sig.ret {
        // A `number` result arrives in `v0`, whose low half *is* `d0`.
        Type::Void => Ok((Val::Int, Type::Void)),
        t => Ok((Val::of(t), t)),
    }
}
