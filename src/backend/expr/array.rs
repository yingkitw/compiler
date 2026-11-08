//! `number[]` / `boolean[]`: literals, indexing, `length`, and `push`.

use crate::ast::*;

use super::super::asm::reg::X;
use super::super::{Fn, Val};
use super::{gen_expr, gen_expr_as, load_temp, store_temp};

struct ArrayOps {
    new: &'static str,
    push: &'static str,
    get: &'static str,
    len: &'static str,
    elem: Type,
}

fn ops(ty: Type) -> Result<&'static ArrayOps, String> {
    Ok(match ty {
        Type::NumberArray => &ArrayOps {
            new: "ts_number_array_new",
            push: "ts_number_array_push",
            get: "ts_number_array_get",
            len: "ts_array_len",
            elem: Type::Number,
        },
        Type::BooleanArray => &ArrayOps {
            new: "ts_bool_array_new",
            push: "ts_bool_array_push",
            get: "ts_bool_array_get",
            len: "ts_array_len",
            elem: Type::Boolean,
        },
        other => return Err(format!("expected an array type, got {}", other)),
    })
}

pub(super) fn array_type_for_elem(elem: Type) -> Result<Type, String> {
    Ok(match elem {
        Type::Number => Type::NumberArray,
        Type::Boolean => Type::BooleanArray,
        other => return Err(format!("cannot build an array of {}", other)),
    })
}

pub(super) fn gen_empty_array(f: &mut Fn, at: Type) -> Result<(Val, Type), String> {
    let op = ops(at)?;
    f.asm().mov_imm(X(0), 0);
    f.asm().call(&Fn::symbol(op.new));
    Ok((Val::Int, at))
}

pub(super) fn infer_array_lit(elems: &[Expr]) -> Result<Type, String> {
    if elems.is_empty() {
        return Err("array literal needs a type annotation when it is empty".into());
    }
    let mut elem = None;
    for e in elems {
        let t = match e {
            Expr::Num(_) => Type::Number,
            Expr::Bool(_) => Type::Boolean,
            _ => {
                return Err(
                    "array literals may only contain number or boolean literals until inference improves"
                        .into(),
                );
            }
        };
        match elem {
            None => elem = Some(t),
            Some(prev) if prev == t => {}
            Some(prev) => return Err(format!("array literal mixes {} and {}", prev, t)),
        }
    }
    array_type_for_elem(elem.expect("non-empty"))
}

pub(super) fn gen_array_lit(f: &mut Fn, elems: &[Expr]) -> Result<(Val, Type), String> {
    let at = infer_array_lit(elems)?;
    let op = ops(at)?;
    let mark = f.temp_mark();
    f.asm()
        .mov_x_imm(X(0), i64::try_from(elems.len()).expect("length fits"));
    f.asm().call(&Fn::symbol(op.new));
  let arr = f.temp();
    store_temp(f.asm(), arr, at);
    for e in elems {
        let (_, et) = gen_expr_as(f, e, Some(op.elem))?;
        if et != op.elem {
            return Err(format!("array element must be {}", op.elem));
        }
        if op.elem == Type::Boolean {
            f.asm().mov_reg(X(1), X(0));
        }
        load_temp(f.asm(), arr, at);
        f.asm().call(&Fn::symbol(op.push));
    }
    load_temp(f.asm(), arr, at);
    f.release_temps(mark);
    Ok((Val::Int, at))
}

pub(super) fn gen_index(f: &mut Fn, base: &Expr, index: &Expr) -> Result<(Val, Type), String> {
    let (_, bt) = gen_expr(f, base)?;
    let op = ops(bt)?;
    let mark = f.temp_mark();
    let arr = f.temp();
    store_temp(f.asm(), arr, bt);
    let (_, it) = gen_expr_as(f, index, Some(Type::Int))?;
    if it != Type::Int {
        return Err(format!("array index must be int, got {}", it));
    }
    f.asm().mov_reg(X(1), X(0));
    load_temp(f.asm(), arr, bt);
    f.asm().call(&Fn::symbol(op.get));
    f.release_temps(mark);
    Ok((Val::of(op.elem), op.elem))
}

pub(super) fn gen_member(f: &mut Fn, base: &Expr, name: &str) -> Result<(Val, Type), String> {
    if name != "length" {
        return Err(format!("arrays have no field '{}'", name));
    }
    let (_, bt) = gen_expr(f, base)?;
    let op = ops(bt)?;
    f.asm().call(&Fn::symbol(op.len));
    Ok((Val::Int, Type::Int))
}

pub(super) fn gen_method_push(
    f: &mut Fn,
    receiver: &Expr,
    args: &[Expr],
) -> Result<(Val, Type), String> {
    if args.len() != 1 {
        return Err(format!("push takes 1 argument, got {}", args.len()));
    }
    let (_, bt) = gen_expr(f, receiver)?;
    let op = ops(bt)?;
    let arr_slot = f.temp();
    store_temp(f.asm(), arr_slot, bt);
    let (_, at) = gen_expr_as(f, &args[0], Some(op.elem))?;
    if at != op.elem {
        return Err(format!("push argument must be {}", op.elem));
    }
    if op.elem == Type::Boolean {
        f.asm().mov_reg(X(1), X(0));
    }
    load_temp(f.asm(), arr_slot, bt);
    f.asm().call(&Fn::symbol(op.push));
    Ok((Val::Int, Type::Void))
}
