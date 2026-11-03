//! Statement generation: the part of the code generator that walks a
//! function's statements and emits the blocks for them.

use cranelift_codegen::ir::InstBuilder;

use std::collections::HashMap;

use crate::ast::*;

use super::ty::{cl_ty, Slot};
use super::FnGen;

impl<'a> FnGen<'a> {
    pub(super) fn lookup(&self, name: &str) -> Result<Slot, String> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name).copied())
            .ok_or_else(|| format!("undefined variable '{}'", name))
    }

    /// Generate a statement list; returns true if control flow is guaranteed
    /// terminated (a `return` on every path) after the list.
    pub(super) fn gen_stmts(&mut self, stmts: &[Stmt]) -> Result<bool, String> {
        for s in stmts {
            if self.gen_stmt(s)? {
                return Ok(true); // rest is unreachable
            }
        }
        Ok(false)
    }

    pub(super) fn gen_stmt(&mut self, s: &Stmt) -> Result<bool, String> {
        match s {
            Stmt::Let { name, ty, init, mutable } => {
                let (v, t) = match (ty, init) {
                    (Some(ant), Some(init)) => {
                        // The annotation is the context: an integer literal
                        // becomes an int here.
                        let (v, t) = self.gen_expr_as(init, Some(*ant))?;
                        if t != *ant {
                            return Err(format!(
                                "cannot initialize '{}: {}' with a value of type {}",
                                name, ant, t
                            ));
                        }
                        (self.val(v, t)?, *ant)
                    }
                    (Some(ant), None) => (self.default_value(*ant), *ant),
                    (None, Some(init)) => {
                        let (v, t) = self.gen_expr(init)?;
                        if t == Type::Void {
                            return Err(format!(
                                "cannot infer a type for '{}' from a void expression",
                                name
                            ));
                        }
                        (self.val(v, t)?, t)
                    }
                    (None, None) => unreachable!("parser rejects untyped uninit let"),
                };
                let var = self.b.declare_var(cl_ty(t));
                self.b.def_var(var, v);
                let scope = self.scopes.last_mut().unwrap();
                if scope.contains_key(name) {
                    return Err(format!("'{}' is already declared in this scope", name));
                }
                scope.insert(name.clone(), Slot { var, ty: t, mutable: *mutable });
                Ok(false)
            }
            Stmt::Func { .. } => Err("nested function declarations are not supported".into()),
            Stmt::If { cond, then, els } => {
                let (cv, ct) = self.gen_expr(cond)?;
                let cv = self.as_bool(self.val(cv, ct)?, ct)?;

                let then_b = self.b.create_block();
                let merge_b = self.b.create_block();
                let els_b = if els.is_empty() { merge_b } else { self.b.create_block() };
                self.b.ins().brif(cv, then_b, &[], els_b, &[]);

                self.b.switch_to_block(then_b);
                self.b.seal_block(then_b);
                self.scopes.push(HashMap::new());
                let then_term = self.gen_stmts(then)?;
                self.scopes.pop();
                if !then_term {
                    self.b.ins().jump(merge_b, &[]);
                }

                let else_term = if els.is_empty() {
                    false
                } else {
                    self.b.switch_to_block(els_b);
                    self.b.seal_block(els_b);
                    self.scopes.push(HashMap::new());
                    let t = self.gen_stmts(els)?;
                    self.scopes.pop();
                    if !t {
                        self.b.ins().jump(merge_b, &[]);
                    }
                    t
                };

                if then_term && else_term {
                    // Unreachable merge block: keep it well-formed and report
                    // that control is terminated.
                    self.b.switch_to_block(merge_b);
                    self.b.seal_block(merge_b);
                    self.emit_default_return();
                    Ok(true)
                } else {
                    self.b.switch_to_block(merge_b);
                    self.b.seal_block(merge_b);
                    Ok(false)
                }
            }
            Stmt::While { cond, body } => {
                let header_b = self.b.create_block();
                let body_b = self.b.create_block();
                let exit_b = self.b.create_block();

                self.b.ins().jump(header_b, &[]);
                self.b.switch_to_block(header_b);
                let (cv, ct) = self.gen_expr(cond)?;
                let cv = self.as_bool(self.val(cv, ct)?, ct)?;
                self.b.ins().brif(cv, body_b, &[], exit_b, &[]);

                self.b.switch_to_block(body_b);
                self.b.seal_block(body_b);
                self.scopes.push(HashMap::new());
                let body_term = self.gen_stmts(body)?;
                self.scopes.pop();
                if !body_term {
                    self.b.ins().jump(header_b, &[]);
                }
                self.b.seal_block(header_b);

                self.b.switch_to_block(exit_b);
                self.b.seal_block(exit_b);
                Ok(false)
            }
            Stmt::Block(stmts) => {
                self.scopes.push(HashMap::new());
                let term = self.gen_stmts(stmts)?;
                self.scopes.pop();
                Ok(term)
            }
            Stmt::Return(e) => match (e, self.ret) {
                (None, Type::Void) => {
                    self.b.ins().return_(&[]);
                    Ok(true)
                }
                (None, rt) => Err(format!("must return a value of type {}", rt)),
                (Some(_), Type::Void) => {
                    Err("cannot return a value from a void function".into())
                }
                (Some(e), rt) => {
                    // The return type is the context: an integer literal
                    // becomes an int here.
                    let (v, t) = self.gen_expr_as(e, Some(rt))?;
                    if t != rt {
                        return Err(format!("cannot return {} from a function returning {}", t, rt));
                    }
                    let v = self.val(v, t)?;
                    self.b.ins().return_(&[v]);
                    Ok(true)
                }
            },
            Stmt::Expr(e) => {
                self.gen_expr(e)?;
                Ok(false)
            }
        }
    }
}
