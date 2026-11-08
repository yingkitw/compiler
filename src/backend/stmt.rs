//! Statement translation: the statements of a function, and the blocks they
//! need.

use crate::ast::*;

use super::asm::reg::{D, FP, X};
use super::asm::{Cond, Mem};
use super::Fn;

/// Translate a statement list; returns true if control flow is guaranteed
/// terminated (a `return` on every path) after the list.
pub fn gen_stmts(f: &mut Fn, stmts: &[Stmt]) -> Result<bool, String> {
    for s in stmts {
        if gen_stmt(f, s)? {
            return Ok(true); // the rest is unreachable
        }
    }
    Ok(false)
}

/// Translate one statement.
pub fn gen_stmt(f: &mut Fn, s: &Stmt) -> Result<bool, String> {
    f.asm().loc(s.line());
    match s {
        Stmt::Let {
            name,
            ty,
            init,
            mutable,
            ..
        } => {
            let (value_ty, has_value) = match (ty, init) {
                (Some(ant), Some(init)) => {
                    // The annotation is the context: an integer literal
                    // becomes an int here.
                    let (_, t) = super::expr::gen_expr_as(f, init, Some(*ant))?;
                    if t != *ant {
                        return Err(format!(
                            "cannot initialize '{}: {}' with a value of type {}",
                            name, ant, t
                        ));
                    }
                    (*ant, true)
                }
                (Some(ant), None) => (*ant, false),
                (None, Some(init)) => {
                    let (_, t) = super::expr::gen_expr(f, init)?;
                    if t == Type::Void {
                        return Err(format!(
                            "cannot infer a type for '{}' from a void expression",
                            name
                        ));
                    }
                    (t, true)
                }
                (None, None) => unreachable!("the parser rejects an untyped uninitialized let"),
            };
            if !has_value {
                // A declaration with no initializer starts at zero.
                match value_ty {
                    Type::Number => {
                        f.asm().mov_imm(X(0), 0);
                        f.asm().fmov_d_from_x(D(0), X(0));
                    }
                    Type::String => {
                        f.asm().mov_imm(X(0), 0);
                        f.asm().mov_imm(X(1), 0);
                    }
                    _ => f.asm().mov_imm(X(0), 0),
                }
            }
            f.declare_local(name, value_ty, *mutable)?;
            // The value is in the register its type calls for; put it away.
            let offset = f.lookup(name)?.offset;
            let mem = Mem { base: FP, offset };
            match value_ty {
                Type::Number => f.asm().store_double(D(0), mem),
                Type::String => {
                    f.asm().store(X(0), mem);
                    f.asm().store(X(1), Mem { base: FP, offset: offset - 8 });
                }
                _ => f.asm().store(X(0), mem),
            }
            Ok(false)
        }
        Stmt::Func { .. } => Err("nested function declarations are not supported".into()),
        Stmt::If { cond, then, els, .. } => {
            let (v, ct) = super::expr::gen_expr(f, cond)?;
            super::expr::as_bool(f.asm(), v, ct)?;

            let then_label = f.label();
            let merge_label = f.label();
            let else_label = if els.is_empty() {
                merge_label.clone()
            } else {
                f.label()
            };

            f.asm().cmp_imm(X(0), 0);
            f.asm().branch_if(Cond::Eq, &else_label);

            f.asm().place(&then_label);
            f.push_scope();
            let then_term = gen_stmts(f, then)?;
            f.pop_scope();
            if !then_term {
                f.asm().branch(&merge_label);
            }

            let else_term = if els.is_empty() {
                false
            } else {
                f.asm().place(&else_label);
                f.push_scope();
                let t = gen_stmts(f, els)?;
                f.pop_scope();
                if !t {
                    f.asm().branch(&merge_label);
                }
                t
            };

            f.asm().place(&merge_label);
            if then_term && else_term {
                // Both branches returned, so this point is unreachable; it
                // still has to be well-formed.
                default_return(f);
                Ok(true)
            } else {
                Ok(false)
            }
        }
        Stmt::While { cond, body, .. } => {
            let header_label = f.label();
            let body_label = f.label();
            let exit_label = f.label();

            f.asm().branch(&header_label);
            f.asm().place(&header_label);
            let (v, ct) = super::expr::gen_expr(f, cond)?;
            super::expr::as_bool(f.asm(), v, ct)?;
            f.asm().cmp_imm(X(0), 0);
            f.asm().branch_if(Cond::Eq, &exit_label);

            f.asm().place(&body_label);
            f.push_scope();
            let body_term = gen_stmts(f, body)?;
            f.pop_scope();
            if !body_term {
                f.asm().branch(&header_label);
            }

            f.asm().place(&exit_label);
            Ok(false)
        }
        Stmt::Block { stmts, .. } => {
            f.push_scope();
            let term = gen_stmts(f, stmts)?;
            f.pop_scope();
            Ok(term)
        }
        Stmt::Return { value, .. } => match (value, f.ret()) {
            (None, Type::Void) => {
                let epilogue = f.epilogue().clone();
                f.asm().branch(&epilogue);
                Ok(true)
            }
            (None, rt) => Err(format!("must return a value of type {}", rt)),
            (Some(_), Type::Void) => {
                Err("cannot return a value from a void function".into())
            }
            (Some(e), rt) => {
                // The return type is the context: an integer literal
                // becomes an int here.
                let (_, t) = super::expr::gen_expr_as(f, e, Some(rt))?;
                if t != rt {
                    return Err(format!(
                        "cannot return {} from a function returning {}",
                        t, rt
                    ));
                }
                let epilogue = f.epilogue().clone();
                f.asm().branch(&epilogue);
                Ok(true)
            }
        },
        Stmt::Expr { expr, .. } => {
            super::expr::gen_expr(f, expr)?;
            Ok(false)
        }
    }
}

/// The return of a path that cannot be reached: a zero, so the code is
/// well-formed.
fn default_return(f: &mut Fn) {
    match f.ret() {
        Type::Number => {
            f.asm().mov_imm(X(0), 0);
            f.asm().fmov_d_from_x(D(0), X(0));
        }
        Type::Void => {}
        _ => f.asm().mov_imm(X(0), 0),
    }
    let epilogue = f.epilogue().clone();
    f.asm().branch(&epilogue);
}
