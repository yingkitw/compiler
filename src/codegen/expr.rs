//! Expression generation: the part of the code generator that type-checks
//! an expression and emits its machine value.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{InstBuilder, Value};
use cranelift_module::Module;

use crate::ast::*;

use super::ty::{int_hint, TypedValue};
use super::FnGen;

impl<'a> FnGen<'a> {
    pub(super) fn gen_expr(&mut self, e: &Expr) -> Result<(Option<Value>, Type), String> {
        self.depth += 1;
        if self.depth > MAX_AST_DEPTH {
            self.depth -= 1;
            return Err("expression or statement nested too deeply".to_string());
        }
        let result = self.gen_expr_inner(e);
        self.depth -= 1;
        result
    }

    fn gen_expr_inner(&mut self, e: &Expr) -> Result<(Option<Value>, Type), String> {
        match e {
            Expr::Num(n) => {
                let v = self.b.ins().f64const(n.value);
                Ok((Some(v), Type::Number))
            }
            Expr::Bool(x) => {
                let v = self.b.ins().iconst(types::I8, i64::from(*x));
                Ok((Some(v), Type::Boolean))
            }
            Expr::Var(name) => {
                let slot = self.lookup(name)?;
                let v = self.b.use_var(slot.var);
                Ok((Some(v), slot.ty))
            }
            Expr::Assign { name, value } => {
                let slot = self.lookup(name)?;
                if !slot.mutable {
                    return Err(format!("cannot assign to const '{}'", name));
                }
                let (v, t) = self.gen_expr(value)?;
                if t != slot.ty {
                    return Err(format!(
                        "cannot assign {} to '{}: {}'",
                        t, name, slot.ty
                    ));
                }
                let v = self.val(v, t)?;
                self.b.def_var(slot.var, v);
                Ok((Some(v), t))
            }
            Expr::Bin { op, lhs, rhs } => self.gen_binop(*op, lhs, rhs),
            Expr::Un { op, expr } => {
                let (v, t) = self.gen_expr(expr)?;
                let v = self.val(v, t)?;
                match op {
                    UnOp::Neg => {
                        let v = match t {
                            Type::Number => self.b.ins().fneg(v),
                            Type::Int => self.b.ins().ineg(v),
                            _ => return Err(format!("cannot negate {}", t)),
                        };
                        Ok((Some(v), t))
                    }
                    UnOp::Not => {
                        let v = self.as_bool(v, t)?;
                        let zero = self.b.ins().iconst(types::I8, 0);
                        let r = self.b.ins().icmp(IntCC::Equal, v, zero);
                        Ok((Some(r), Type::Boolean))
                    }
                }
            }
            Expr::Call { name, args } => self.gen_call(name, args),
            Expr::Cast { expr, ty } => {
                let (v, t) = self.gen_expr(expr)?;
                let v = self.val(v, t)?;
                // A cast to the value's own type is a no-op, like
                // TypeScript's no-op assertion.
                if *ty == t {
                    return Ok((Some(v), t));
                }
                match (*ty, t) {
                    // number → int: truncate toward zero; NaN becomes 0 and
                    // an out-of-range value saturates to the end of the i64
                    // range — Rust's `as` semantics, and unlike the trapping
                    // `fcvt_to_sint`, defined for every input.
                    (Type::Int, Type::Number) => {
                        let v = self.b.ins().fcvt_to_sint_sat(types::I64, v);
                        Ok((Some(v), Type::Int))
                    }
                    // int → number: exact up to 2^53, rounded beyond it —
                    // the honest conversion, since a double cannot hold more.
                    (Type::Number, Type::Int) => {
                        let v = self.b.ins().fcvt_from_sint(types::F64, v);
                        Ok((Some(v), Type::Number))
                    }
                    // The parser rejects `as void` with a line number; this
                    // arm is defence in depth.
                    (Type::Void, _) => Err("cannot cast to 'void'".to_string()),
                    _ => Err(format!("cannot cast {} to {}", t, ty)),
                }
            }
        }
    }

    pub(super) fn gen_binop(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(Option<Value>, Type), String> {
        match op {
            BinOp::And | BinOp::Or => self.gen_short_circuit(op, lhs, rhs),
            BinOp::Eq | BinOp::Ne => {
                let ((lv, lt), (rv, rt)) = self.gen_operands(lhs, rhs)?;
                if lt != rt {
                    return Err(format!("cannot compare {} with {}", lt, rt));
                }
                let lv = self.val(lv, lt)?;
                let rv = self.val(rv, rt)?;
                let v = match (lt, op) {
                    (Type::Number, BinOp::Eq) => self.b.ins().fcmp(FloatCC::Equal, lv, rv),
                    (Type::Number, BinOp::Ne) => self.b.ins().fcmp(FloatCC::NotEqual, lv, rv),
                    (Type::Int, BinOp::Eq) => self.b.ins().icmp(IntCC::Equal, lv, rv),
                    (Type::Int, BinOp::Ne) => self.b.ins().icmp(IntCC::NotEqual, lv, rv),
                    (Type::Boolean, BinOp::Eq) => self.b.ins().icmp(IntCC::Equal, lv, rv),
                    (Type::Boolean, BinOp::Ne) => self.b.ins().icmp(IntCC::NotEqual, lv, rv),
                    _ => unreachable!(),
                };
                Ok((Some(v), Type::Boolean))
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let ((lv, lt), (rv, rt)) = self.gen_operands(lhs, rhs)?;
                let lv = self.val(lv, lt)?;
                let rv = self.val(rv, rt)?;
                let v = match (lt, rt) {
                    (Type::Number, Type::Number) => {
                        let cc = match op {
                            BinOp::Lt => FloatCC::LessThan,
                            BinOp::Le => FloatCC::LessThanOrEqual,
                            BinOp::Gt => FloatCC::GreaterThan,
                            BinOp::Ge => FloatCC::GreaterThanOrEqual,
                            _ => unreachable!(),
                        };
                        self.b.ins().fcmp(cc, lv, rv)
                    }
                    (Type::Int, Type::Int) => {
                        let cc = match op {
                            BinOp::Lt => IntCC::SignedLessThan,
                            BinOp::Le => IntCC::SignedLessThanOrEqual,
                            BinOp::Gt => IntCC::SignedGreaterThan,
                            BinOp::Ge => IntCC::SignedGreaterThanOrEqual,
                            _ => unreachable!(),
                        };
                        self.b.ins().icmp(cc, lv, rv)
                    }
                    _ => {
                        return Err(format!(
                            "relational operators require two numbers or two ints, got {} and {}",
                            lt, rt
                        ))
                    }
                };
                Ok((Some(v), Type::Boolean))
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                let ((lv, lt), (rv, rt)) = self.gen_operands(lhs, rhs)?;
                let lv = self.val(lv, lt)?;
                let rv = self.val(rv, rt)?;
                match (lt, rt) {
                    (Type::Int, Type::Int) => {
                        let v = match op {
                            BinOp::Add => self.b.ins().iadd(lv, rv),
                            BinOp::Sub => self.b.ins().isub(lv, rv),
                            BinOp::Mul => self.b.ins().imul(lv, rv),
                            // Integer division truncates toward zero, like Go.
                            BinOp::Div => self.b.ins().sdiv(lv, rv),
                            BinOp::Rem => self.b.ins().srem(lv, rv),
                            _ => unreachable!(),
                        };
                        Ok((Some(v), Type::Int))
                    }
                    (Type::Number, Type::Number) => {
                        let v = match op {
                            BinOp::Add => self.b.ins().fadd(lv, rv),
                            BinOp::Sub => self.b.ins().fsub(lv, rv),
                            BinOp::Mul => self.b.ins().fmul(lv, rv),
                            BinOp::Div => self.b.ins().fdiv(lv, rv),
                            // No float remainder instruction exists, so this
                            // goes through libm. C's fmod truncates toward
                            // zero and takes the sign of the dividend, which
                            // is what JavaScript's % does.
                            BinOp::Rem => {
                                let fref =
                                    self.module.declare_func_in_func(self.fmod, self.b.func);
                                let call = self.b.ins().call(fref, &[lv, rv]);
                                self.b.func.dfg.inst_results(call)[0]
                            }
                            _ => unreachable!(),
                        };
                        Ok((Some(v), Type::Number))
                    }
                    _ => Err(format!(
                        "arithmetic operators require two numbers or two ints, got {} and {}",
                        lt, rt
                    )),
                }
            }
        }
    }

    /// Generate both operands of a binary operator, adapting integer literals
    /// to the other operand's type.
    ///
    /// A literal has no fixed type of its own: `1` is `number` by default but
    /// becomes an `int` when the other side of the operation is an `int`. If
    /// one side is a literal and the other is not, the non-literal side is
    /// evaluated first so the literal can adopt its type. Literals are
    /// side-effect free, so the reordering is safe.
    pub(super) fn gen_operands(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
    ) -> Result<(TypedValue, TypedValue), String> {
        let literal = |e: &Expr| matches!(e, Expr::Num(_));
        if literal(lhs) && !literal(rhs) {
            let (rv, rt) = self.gen_expr(rhs)?;
            let (lv, lt) = self.gen_lit_or_expr(lhs, int_hint(rt))?;
            Ok(((lv, lt), (rv, rt)))
        } else {
            let (lv, lt) = self.gen_expr(lhs)?;
            let (rv, rt) = self.gen_lit_or_expr(rhs, int_hint(lt))?;
            Ok(((lv, lt), (rv, rt)))
        }
    }

    /// Generate a literal, adapting it to the expected type when it can.
    ///
    /// If a literal cannot take the expected type — `4.5` where an `int` is
    /// wanted — it is generated as a plain `number` instead, so the caller
    /// reports the real problem ("these two types do not match") rather than
    /// a conversion failure the programmer did not ask for.
    pub(super) fn gen_lit_or_expr(
        &mut self,
        e: &Expr,
        expected: Option<Type>,
    ) -> Result<(Option<Value>, Type), String> {
        if matches!(e, Expr::Num(_)) && let Ok(v) = self.gen_expr_as(e, expected) {
            return Ok(v);
        }
        self.gen_expr(e)    }

    /// Generate an expression, optionally with the type its context expects.
    /// Only integer literals are affected: in an `int` context they become
    /// `int` (they must be integral and in range); everywhere else they stay
    /// `number`.
    pub(super) fn gen_expr_as(
        &mut self,
        e: &Expr,
        expected: Option<Type>,
    ) -> Result<(Option<Value>, Type), String> {
        if expected == Some(Type::Int) {
            // A negative literal is a unary minus over a literal, and it
            // should adopt the context's type the way a positive one does:
            // `let x: int = -5;` is an int, not a number.
            let (lit, negative) = match e {
                Expr::Num(lit) => (lit, false),
                Expr::Un {
                    op: UnOp::Neg,
                    expr,
                } => match &**expr {
                    Expr::Num(lit) => (lit, true),
                    _ => return self.gen_expr(e),
                },
                _ => return self.gen_expr(e),
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
            let v = self.b.ins().iconst(types::I64, exact);
            return Ok((Some(v), Type::Int));
        }
        self.gen_expr(e)
    }

    pub(super) fn gen_short_circuit(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(Option<Value>, Type), String> {
        let result = self.b.declare_var(types::I8);
        let (lv, lt) = self.gen_expr(lhs)?;
        let lv = self.as_bool(self.val(lv, lt)?, lt)?;

        let rhs_b = self.b.create_block();
        let short_b = self.b.create_block();
        let merge_b = self.b.create_block();
        match op {
            BinOp::And => self.b.ins().brif(lv, rhs_b, &[], short_b, &[]),
            BinOp::Or => self.b.ins().brif(lv, short_b, &[], rhs_b, &[]),
            _ => unreachable!(),
        };

        self.b.switch_to_block(rhs_b);
        self.b.seal_block(rhs_b);
        let (rv, rt) = self.gen_expr(rhs)?;
        let rv = self.as_bool(self.val(rv, rt)?, rt)?;
        self.b.def_var(result, rv);
        self.b.ins().jump(merge_b, &[]);

        self.b.switch_to_block(short_b);
        self.b.seal_block(short_b);
        self.b.def_var(result, lv);
        self.b.ins().jump(merge_b, &[]);

        self.b.switch_to_block(merge_b);
        self.b.seal_block(merge_b);
        let v = self.b.use_var(result);
        Ok((Some(v), Type::Boolean))
    }

    pub(super) fn gen_call(&mut self, name: &str, args: &[Expr]) -> Result<(Option<Value>, Type), String> {
        if name == "print" {
            if args.len() != 1 {
                return Err(format!("print takes 1 argument, got {}", args.len()));
            }
            let (v, t) = self.gen_expr(&args[0])?;
            let fid = match t {
                Type::Number => self.print_num,
                Type::Int => self.print_int,
                Type::Boolean => self.print_bool,
                Type::Void => return Err("cannot print void".into()),
            };
            let v = self.val(v, t)?;
            let fref = self.module.declare_func_in_func(fid, self.b.func);
            self.b.ins().call(fref, &[v]);
            return Ok((None, Type::Void));
        }

        let fs = self
            .funcs
            .get(name)
            .cloned()
            .ok_or_else(|| format!("undefined function '{}'", name))?;
        if args.len() != fs.params.len() {
            return Err(format!(
                "'{}' takes {} arguments, got {}",
                name,
                fs.params.len(),
                args.len()
            ));
        }
        let mut argv = Vec::with_capacity(args.len());
        for (arg, want) in args.iter().zip(&fs.params) {
            // The parameter type is the context for an integer literal.
            let (v, t) = self.gen_expr_as(arg, Some(*want))?;
            if t != *want {
                return Err(format!(
                    "argument of '{}' must be {}, got {}",
                    name, want, t
                ));
            }
            argv.push(self.val(v, t)?);
        }
        let fref = self.module.declare_func_in_func(fs.id, self.b.func);
        let call = self.b.ins().call(fref, &argv);
        if fs.ret == Type::Void {
            Ok((None, Type::Void))
        } else {
            let res = self.b.func.dfg.inst_results(call)[0];
            Ok((Some(res), fs.ret))
        }
    }

    /// Convert a value to a boolean machine value (numbers compare `!= 0`).
    pub(super) fn as_bool(&mut self, v: Value, t: Type) -> Result<Value, String> {
        match t {
            Type::Boolean => Ok(v),
            Type::Number => {
                let zero = self.b.ins().f64const(0.0);
                Ok(self.b.ins().fcmp(FloatCC::NotEqual, v, zero))
            }
            Type::Int => {
                let zero = self.b.ins().iconst(types::I64, 0);
                Ok(self.b.ins().icmp(IntCC::NotEqual, v, zero))
            }
            Type::Void => Err("void expression has no truth value".into()),
        }
    }

    pub(super) fn val(&self, v: Option<Value>, t: Type) -> Result<Value, String> {
        v.ok_or_else(|| format!("void value of type {} used where a value is required", t))
    }

    pub(super) fn default_value(&mut self, t: Type) -> Value {
        match t {
            Type::Number => self.b.ins().f64const(0.0),
            Type::Int => self.b.ins().iconst(types::I64, 0),
            Type::Boolean => self.b.ins().iconst(types::I8, 0),
            Type::Void => unreachable!(),
        }
    }

    /// Terminator for a provably-unreachable block, matching the fn's return.
    pub(super) fn emit_default_return(&mut self) {
        match self.ret {
            Type::Void => {
                self.b.ins().return_(&[]);
            }
            Type::Number => {
                let z = self.b.ins().f64const(0.0);
                self.b.ins().return_(&[z]);
            }
            Type::Int => {
                let z = self.b.ins().iconst(types::I64, 0);
                self.b.ins().return_(&[z]);
            }
            Type::Boolean => {
                let z = self.b.ins().iconst(types::I8, 0);
                self.b.ins().return_(&[z]);
            }
        }
    }
}
