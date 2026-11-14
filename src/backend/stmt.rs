//! Statement translation: the statements of a function, and the blocks they
//! need.

use crate::ast::*;

use super::asm::reg::{D, X};
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
            // The value is in the register its type calls for. A numeric
            // local is promoted into the register cache here, so a loop
            // that assigns it keeps it in a register across its back edge.
            let local = f.lookup(name)?;
            f.promote(&local);
            super::expr::store_local(f, &local);
            Ok(false)
        }
        Stmt::Func { .. } => Err("nested function declarations are not supported".into()),
        Stmt::If { cond, then, els, .. } => {
            let then_label = f.label();
            let merge_label = f.label();
            let else_label = if els.is_empty() {
                merge_label.clone()
            } else {
                f.label()
            };

            super::expr::branch_when(f, cond, &else_label, false)?;

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
            // The locals the loop assigns become register residents, so
            // the back edge does not round-trip them through memory. The
            // scan is a hint, not a rule: anything it misses simply keeps
            // its slot, because every load and store consults the cache.
            let region = f.open_region();
            for name in assigned_names(cond, body) {
                f.promote_and_load(&name);
            }

            let header_label = f.label();
            let body_label = f.label();
            let exit_label = f.label();

            f.asm().branch(&header_label);
            f.asm().place(&header_label);
            super::expr::branch_when(f, cond, &exit_label, false)?;

            f.asm().place(&body_label);
            f.push_scope();
            let body_term = gen_stmts(f, body)?;
            f.pop_scope();
            if !body_term {
                f.asm().branch(&header_label);
            }

            f.asm().place(&exit_label);
            // The loop's registers go back to their slots, so the code
            // after the loop reads what the last iteration left.
            f.close_region(region);
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
            // An assignment statement's value is dead, so when its target
            // is a register resident the arithmetic can write the
            // register directly.
            if let Expr::Assign { name, value } = expr {
                super::expr::gen_assign_stmt(f, name, value)?;
            } else {
                super::expr::gen_expr(f, expr)?;
            }
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

/// The names assigned anywhere in a loop's condition or body — the locals
/// a `while` may want to keep in registers across its back edge. The
/// result over-approximates: a name shadowed by a `let` inside the loop
/// is included even though its assignments target the inner variable,
/// which only costs a register.
fn assigned_names(cond: &Expr, body: &[Stmt]) -> Vec<String> {
    let mut names = Vec::new();
    collect_assigned_expr(cond, &mut names);
    for s in body {
        collect_assigned_stmt(s, &mut names);
    }
    names.sort();
    names.dedup();
    names
}

fn collect_assigned_expr(e: &Expr, names: &mut Vec<String>) {
    match e {
        Expr::Assign { name, value } => {
            names.push(name.clone());
            collect_assigned_expr(value, names);
        }
        Expr::Bin { lhs, rhs, .. } => {
            collect_assigned_expr(lhs, names);
            collect_assigned_expr(rhs, names);
        }
        Expr::Un { expr, .. } | Expr::Cast { expr, .. } => collect_assigned_expr(expr, names),
        Expr::Cond {
            cond,
            then_expr,
            else_expr,
        } => {
            collect_assigned_expr(cond, names);
            collect_assigned_expr(then_expr, names);
            collect_assigned_expr(else_expr, names);
        }
        Expr::BlockExpr { stmts, value } => {
            for s in stmts {
                collect_assigned_stmt(s, names);
            }
            collect_assigned_expr(value, names);
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_assigned_expr(e, names);
            }
        }
        Expr::Index { base, index } => {
            collect_assigned_expr(base, names);
            collect_assigned_expr(index, names);
        }
        Expr::Member { base, .. } => collect_assigned_expr(base, names),
        Expr::Call {
            receiver, args, ..
        } => {
            if let Some(r) = receiver {
                collect_assigned_expr(r, names);
            }
            for a in args {
                collect_assigned_expr(a, names);
            }
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Var(_) => {}
    }
}

fn collect_assigned_stmt(s: &Stmt, names: &mut Vec<String>) {
    match s {
        Stmt::Let { init, .. } => {
            if let Some(e) = init {
                collect_assigned_expr(e, names);
            }
        }
        Stmt::If { cond, then, els, .. } => {
            collect_assigned_expr(cond, names);
            for s in then.iter().chain(els.iter()) {
                collect_assigned_stmt(s, names);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_assigned_expr(cond, names);
            for s in body {
                collect_assigned_stmt(s, names);
            }
        }
        Stmt::Block { stmts, .. } => {
            for s in stmts {
                collect_assigned_stmt(s, names);
            }
        }
        Stmt::Expr { expr, .. } => collect_assigned_expr(expr, names),
        Stmt::Return { value, .. } => {
            if let Some(e) = value {
                collect_assigned_expr(e, names);
            }
        }
        Stmt::Func { .. } => {}
    }
}
