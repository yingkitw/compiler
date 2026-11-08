//! Calls to `print` and to user functions.

use crate::ast::*;

use super::super::asm::reg::{D, V, X};
use super::super::{Fn, Val};
use super::array;
use super::{gen_expr, gen_expr_as, load_temp, store_temp};

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
    let mut evaluated = Vec::new();
    for (arg, ty) in args.iter().zip(sig.params.iter()) {
        let (_, at) = gen_expr_as(f, arg, Some(*ty))?;
        if at != *ty {
            return Err(format!(
                "argument of '{}' must be {}, got {}",
                name, ty, at
            ));
        }
        let slot = f.temp_for(*ty);
        store_temp(f.asm(), slot, *ty);
        evaluated.push((slot, *ty));
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

    for ((slot, ty), slot_plan) in evaluated.iter().zip(plan.iter()).rev() {
        match (ty, slot_plan) {
            (Type::Number, RegPlan::Num(n)) => {
                load_temp(f.asm(), *slot, Type::Number);
                f.asm().fmov_x_from_d(X(0), D(0));
                f.asm().mov_v_from_x(V(*n), X(0));
            }
            (Type::String, RegPlan::String(x)) => {
                load_temp(f.asm(), *slot, Type::String);
                f.asm().mov_reg(X(*x), X(0));
                f.asm().mov_reg(X(*x + 1), X(1));
            }
            (_, RegPlan::Int(x)) => {
                load_temp(f.asm(), *slot, *ty);
                f.asm().mov_reg(X(*x), X(0));
            }
            _ => unreachable!("plan matches argument type"),
        }
    }

    f.asm().call(&Fn::symbol(&format!("tsfn_{}", name)));
    if sig.ret == Type::Number {
        f.asm().fmov_d_from_v(D(0), V(0));
    }
    f.release_temps(mark);
    match sig.ret {
        Type::Void => Ok((Val::Int, Type::Void)),
        t => Ok((Val::of(t), t)),
    }
}
