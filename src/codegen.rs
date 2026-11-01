//! Type-checking code generator: walks the AST, checks types, and emits
//! native machine code through Cranelift's object backend.

use std::collections::HashMap;

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Signature, Value};
use cranelift_codegen::isa::{self, CallConv, TargetFrontendConfig};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;

use crate::ast::*;

/// Tiny C runtime linked against the generated object. Kept minimal so the
/// generated code only needs two well-defined (non-variadic) C ABI calls.
pub const RUNTIME_C: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>

/* Print a double the way JavaScript's Number::toString does: the shortest
 * decimal string that round-trips back to the same f64, in fixed notation for
 * decimal exponents in [-6, 21] and exponential notation otherwise.
 * Printing "%g" instead would silently drop precision beyond six
 * significant digits. */
void ts_print_number(double x) {
    if (isnan(x)) { puts("NaN"); return; }
    if (isinf(x)) { puts(x < 0 ? "-Infinity" : "Infinity"); return; }
    if (x == 0.0) { puts("0"); return; }

    int neg = x < 0;
    double a = neg ? -x : x;

    /* Shortest round-tripping representation: try 1..17 significant digits. */
    char buf[64];
    int prec, exp10 = 0;
    for (prec = 1; prec <= 17; prec++) {
        snprintf(buf, sizeof buf, "%.*e", prec - 1, a);
        if (strtod(buf, NULL) == a) break;
    }

    /* Split into digits and exponent: value == 0.<digits> * 10^n */
    char digits[32];
    int k = 0, i = 0;
    while (buf[i] && buf[i] != 'e') {
        if (buf[i] != '.') digits[k++] = buf[i];
        i++;
    }
    if (buf[i] == 'e') exp10 = atoi(buf + i + 1);
    while (k > 1 && digits[k - 1] == '0') k--;
    digits[k] = '\0';
    int n = exp10 + 1;

    char out[80];
    char *p = out;
    if (neg) *p++ = '-';
    if (k <= n && n <= 21) {
        memcpy(p, digits, (size_t)k); p += k;
        for (int j = 0; j < n - k; j++) *p++ = '0';
    } else if (0 < n && n <= 21) {
        memcpy(p, digits, (size_t)n); p += n;
        *p++ = '.';
        memcpy(p, digits + n, (size_t)(k - n)); p += k - n;
    } else if (-6 < n && n <= 0) {
        *p++ = '0'; *p++ = '.';
        for (int j = 0; j < -n; j++) *p++ = '0';
        memcpy(p, digits, (size_t)k); p += k;
    } else {
        *p++ = digits[0];
        if (k > 1) {
            *p++ = '.';
            memcpy(p, digits + 1, (size_t)(k - 1)); p += k - 1;
        }
        *p++ = 'e';
        int e = n - 1;
        *p++ = e < 0 ? '-' : '+';
        if (e < 0) e = -e;
        p += sprintf(p, "%d", e);
    }
    *p = '\0';
    puts(out);
}

void ts_print_bool(signed char x) { puts(x ? "true" : "false"); }

void ts_print_int(long long x) { printf("%lld\n", x); }
"#;

/// A generated value and its static type. `None` means the expression has no
/// value (`void`).
type TypedValue = (Option<Value>, Type);

/// The type a literal should adopt given the other operand's type. Only
/// `int` is contagious: a `number` literal in an `int` context stays a
/// `number`, because widening is not implicit.
fn int_hint(t: Type) -> Option<Type> {
    if t == Type::Int { Some(Type::Int) } else { None }
}

fn cl_ty(t: Type) -> types::Type {
    match t {
        Type::Number => types::F64,
        Type::Int => types::I64,
        Type::Boolean => types::I8,
        Type::Void => panic!("void has no machine representation"),
    }
}

fn sig_for(call_conv: CallConv, params: &[Type], ret: Type) -> Signature {
    let mut sig = Signature::new(call_conv);
    for t in params {
        sig.params.push(AbiParam::new(cl_ty(*t)));
    }
    if ret != Type::Void {
        sig.returns.push(AbiParam::new(cl_ty(ret)));
    }
    sig
}

#[derive(Clone)]
struct FnSig {
    id: FuncId,
    params: Vec<Type>,
    ret: Type,
}

#[derive(Clone, Copy)]
struct Slot {
    var: Variable,
    ty: Type,
    mutable: bool,
}

pub fn compile(prog: &Program) -> Result<Vec<u8>, String> {
    let triple = Triple::host();
    let mut flag_builder = settings::builder();
    flag_builder
        .set("opt_level", "speed")
        .map_err(|e| e.to_string())?;
    // Position-independent code: required so calls to imported symbols
    // (the C runtime) link cleanly, especially on macOS.
    flag_builder
        .set("is_pic", "true")
        .map_err(|e| e.to_string())?;
    let isa = isa::lookup(triple.clone())
        .map_err(|e| e.to_string())?
        .finish(settings::Flags::new(flag_builder))
        .map_err(|e| e.to_string())?;
    let frontend_config = isa.frontend_config();
    let call_conv = CallConv::triple_default(&triple);
    let builder = ObjectBuilder::new(
        isa,
        "tsc_module".to_string(),
        cranelift_module::default_libcall_names(),
    )
    .map_err(|e| e.to_string())?;
    let mut module = ObjectModule::new(builder);

    // Runtime imports.
    let print_num = module
        .declare_function(
            "ts_print_number",
            Linkage::Import,
            &sig_for(call_conv, &[Type::Number], Type::Void),
        )
        .map_err(|e| e.to_string())?;
    let print_bool = module
        .declare_function(
            "ts_print_bool",
            Linkage::Import,
            &sig_for(call_conv, &[Type::Boolean], Type::Void),
        )
        .map_err(|e| e.to_string())?;
    let print_int = module
        .declare_function(
            "ts_print_int",
            Linkage::Import,
            &sig_for(call_conv, &[Type::Int], Type::Void),
        )
        .map_err(|e| e.to_string())?;

    // Pass 1: declare every top-level function so calls can appear before
    // definitions (and recursion works).
    let mut funcs: HashMap<String, FnSig> = HashMap::new();
    for stmt in &prog.stmts {
        if let Stmt::Func { name, params, ret, .. } = stmt {
            if name == "print" {
                return Err("'print' is a built-in and cannot be redefined".into());
            }
            if funcs.contains_key(name) {
                return Err(format!("duplicate function '{}'", name));
            }
            let ret = ret.unwrap_or(Type::Void);
            let param_tys: Vec<Type> = params.iter().map(|p| p.1).collect();
            let sig = sig_for(call_conv, &param_tys, ret);
            let id = module
                .declare_function(&format!("tsfn_{}", name), Linkage::Export, &sig)
                .map_err(|e| e.to_string())?;
            funcs.insert(
                name.clone(),
                FnSig { id, params: param_tys, ret },
            );
        }
    }

    // `main`, if present, is the program entry point.
    let user_main = match funcs.get("main") {
        Some(f) => {
            if !f.params.is_empty() || f.ret != Type::Void {
                return Err("`main` must be declared as `function main(): void`".into());
            }
            Some(f.id)
        }
        None => None,
    };

    let mut fbcx = FunctionBuilderContext::new();

    // Pass 2: compile function bodies.
    for stmt in &prog.stmts {
        if let Stmt::Func { name, params, body, .. } = stmt {
            let fs = funcs[name].clone();
            let sig = sig_for(call_conv, &fs.params, fs.ret);
            define_body(
                &mut module, &mut fbcx, frontend_config, &funcs, print_num, print_bool, print_int,
                fs.id, sig, name, params, fs.ret, body,
            )?;
        }
    }

    // Top-level statements become `__ts_start`, run before `main`.
    let top: Vec<Stmt> = prog
        .stmts
        .iter()
        .filter(|s| !matches!(s, Stmt::Func { .. }))
        .cloned()
        .collect();
    let start_sig = sig_for(call_conv, &[], Type::Void);
    let start_id = module
        .declare_function("__ts_start", Linkage::Export, &start_sig)
        .map_err(|e| e.to_string())?;
    define_body(
        &mut module, &mut fbcx, frontend_config, &funcs, print_num, print_bool, print_int,
        start_id, start_sig, "__ts_start", &[], Type::Void, &top,
    )?;

    // C `main` wrapper: call __ts_start, then the user's main if any.
    emit_main_wrapper(&mut module, &mut fbcx, frontend_config, call_conv, start_id, user_main)?;

    let product = module.finish();
    product.emit().map_err(|e| e.to_string())
}

#[allow(clippy::too_many_arguments)]
fn define_body(
    module: &mut ObjectModule,
    fbcx: &mut FunctionBuilderContext,
    frontend_config: TargetFrontendConfig,
    funcs: &HashMap<String, FnSig>,
    print_num: FuncId,
    print_bool: FuncId,
    print_int: FuncId,
    id: FuncId,
    sig: Signature,
    name: &str,
    params: &[(String, Type)],
    ret: Type,
    body: &[Stmt],
) -> Result<(), String> {
    let mut ctx = Context::new();
    ctx.func.signature = sig;
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, fbcx);
        let entry = b.create_block();
        for (_, t) in params {
            b.append_block_param(entry, cl_ty(*t));
        }
        b.switch_to_block(entry);
        b.seal_block(entry);

        let mut fg = FnGen {
            b,
            module,
            funcs,
            print_num,
            print_bool,
            print_int,
            scopes: Vec::new(),
            ret,
        };

        let mut scope = HashMap::new();
        for (i, (pname, pty)) in params.iter().enumerate() {
            if scope.contains_key(pname) {
                return Err(format!("duplicate parameter '{}' in '{}'", pname, name));
            }
            let var = fg.b.declare_var(cl_ty(*pty));
            let pv = fg.b.block_params(entry)[i];
            fg.b.def_var(var, pv);
            scope.insert(
                pname.clone(),
                Slot { var, ty: *pty, mutable: true },
            );
        }
        fg.scopes.push(scope);

        let terminated = fg.gen_stmts(body)?;
        if !terminated {
            if ret == Type::Void {
                fg.b.ins().return_(&[]);
            } else {
                return Err(format!(
                    "function '{}' must return {} on all paths",
                    name, ret
                ));
            }
        }
        fg.b.finalize(frontend_config);
    }
    module
        .define_function(id, &mut ctx)
        .map_err(|e| e.to_string())
}

fn emit_main_wrapper(
    module: &mut ObjectModule,
    fbcx: &mut FunctionBuilderContext,
    frontend_config: TargetFrontendConfig,
    call_conv: CallConv,
    start: FuncId,
    user_main: Option<FuncId>,
) -> Result<(), String> {
    let mut sig = Signature::new(call_conv);
    sig.returns.push(AbiParam::new(types::I32));
    let id = module
        .declare_function("main", Linkage::Export, &sig)
        .map_err(|e| e.to_string())?;
    let mut ctx = Context::new();
    ctx.func.signature = sig;
    {
        let mut b = FunctionBuilder::new(&mut ctx.func, fbcx);
        let entry = b.create_block();
        b.switch_to_block(entry);
        b.seal_block(entry);
        let start_ref = module.declare_func_in_func(start, b.func);
        b.ins().call(start_ref, &[]);
        if let Some(m) = user_main {
            let mref = module.declare_func_in_func(m, b.func);
            b.ins().call(mref, &[]);
        }
        let zero = b.ins().iconst(types::I32, 0);
        b.ins().return_(&[zero]);
        b.finalize(frontend_config);
    }
    module
        .define_function(id, &mut ctx)
        .map_err(|e| e.to_string())
}

/// Per-function code generator. `gen_*` methods also type-check; expressions
/// return their machine value (or `None` for `void`) plus their `Type`.
struct FnGen<'a> {
    b: FunctionBuilder<'a>,
    module: &'a mut ObjectModule,
    funcs: &'a HashMap<String, FnSig>,
    print_num: FuncId,
    print_bool: FuncId,
    print_int: FuncId,
    scopes: Vec<HashMap<String, Slot>>,
    ret: Type,
}

impl<'a> FnGen<'a> {
    fn lookup(&self, name: &str) -> Result<Slot, String> {
        self.scopes
            .iter()
            .rev()
            .find_map(|s| s.get(name).copied())
            .ok_or_else(|| format!("undefined variable '{}'", name))
    }

    /// Generate a statement list; returns true if control flow is guaranteed
    /// terminated (a `return` on every path) after the list.
    fn gen_stmts(&mut self, stmts: &[Stmt]) -> Result<bool, String> {
        for s in stmts {
            if self.gen_stmt(s)? {
                return Ok(true); // rest is unreachable
            }
        }
        Ok(false)
    }

    fn gen_stmt(&mut self, s: &Stmt) -> Result<bool, String> {
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

    fn gen_expr(&mut self, e: &Expr) -> Result<(Option<Value>, Type), String> {
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
        }
    }

    fn gen_binop(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(Option<Value>, Type), String> {
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
                        if op == BinOp::Rem {
                            return Err(
                                "the remainder operator requires integer operands".into()
                            );
                        }
                        let v = match op {
                            BinOp::Add => self.b.ins().fadd(lv, rv),
                            BinOp::Sub => self.b.ins().fsub(lv, rv),
                            BinOp::Mul => self.b.ins().fmul(lv, rv),
                            BinOp::Div => self.b.ins().fdiv(lv, rv),
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
    fn gen_operands(
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
    fn gen_lit_or_expr(
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
    fn gen_expr_as(
        &mut self,
        e: &Expr,
        expected: Option<Type>,
    ) -> Result<(Option<Value>, Type), String> {
        if let (Expr::Num(lit), Some(Type::Int)) = (e, expected) {
            // Use the literal's exact integer value when it has one. A large
            // literal such as 9007199254740993 does not fit in an f64, so the
            // double must not be the source of truth here.
            let exact = match lit.int {
                Some(i) => i,
                None => {
                    if lit.value.fract() != 0.0 {
                        return Err(format!(
                            "cannot use the non-integer value {} as an int",
                            lit.value
                        ));
                    }
                    if lit.value < i64::MIN as f64 || lit.value > i64::MAX as f64 {
                        return Err(format!("the value {} does not fit in an int", lit.value));
                    }
                    lit.value as i64
                }
            };
            let v = self.b.ins().iconst(types::I64, exact);
            return Ok((Some(v), Type::Int));
        }
        self.gen_expr(e)
    }

    fn gen_short_circuit(&mut self, op: BinOp, lhs: &Expr, rhs: &Expr) -> Result<(Option<Value>, Type), String> {
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

    fn gen_call(&mut self, name: &str, args: &[Expr]) -> Result<(Option<Value>, Type), String> {
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
    fn as_bool(&mut self, v: Value, t: Type) -> Result<Value, String> {
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

    fn val(&self, v: Option<Value>, t: Type) -> Result<Value, String> {
        v.ok_or_else(|| format!("void value of type {} used where a value is required", t))
    }

    fn default_value(&mut self, t: Type) -> Value {
        match t {
            Type::Number => self.b.ins().f64const(0.0),
            Type::Int => self.b.ins().iconst(types::I64, 0),
            Type::Boolean => self.b.ins().iconst(types::I8, 0),
            Type::Void => unreachable!(),
        }
    }

    /// Terminator for a provably-unreachable block, matching the fn's return.
    fn emit_default_return(&mut self) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;

    /// Compile a source snippet, expecting it to be rejected, and return the
    /// diagnostic. These cover the type rules that live inside `FnGen`.
    fn type_error(src: &str) -> String {
        let prog = parser::parse(lexer::lex(src).expect("lex failed")).expect("parse failed");
        compile(&prog).expect_err("expected a type error, but compilation succeeded")
    }

    /// Compile a source snippet, expecting it to succeed.
    fn compiles(src: &str) -> Vec<u8> {
        let prog = parser::parse(lexer::lex(src).expect("lex failed")).expect("parse failed");
        compile(&prog).expect("expected compilation to succeed")
    }

    /// Compile a source snippet, returning either the object bytes or the
    /// diagnostic. Never panics.
    fn try_compile(src: &str) -> Result<Vec<u8>, String> {
        let prog = parser::parse(lexer::lex(src).expect("lex failed")).expect("parse failed");
        compile(&prog)
    }

    /// Every AST variant must be handled by the code generator. Rust's
    /// exhaustiveness checking means a missing `match` arm is a compile error,
    /// but a variant that is *constructed* and then mishandled — or a codegen
    /// path that reaches an `unreachable!()` — would panic at compile time.
    /// This test drives one program per variant and asserts the compiler
    /// never panics, whatever the outcome.
    #[test]
    fn no_ast_variant_panics_the_compiler() {
        let cases: &[(&str, &str)] = &[
            ("let", "let x = 1;"),
            ("let with type", "let x: number = 1;"),
            ("const", "const x = 1;"),
            ("function", "function f(): void { }"),
            ("function with params", "function f(a: number, b: boolean): void { }"),
            ("function with return", "function f(): number { return 1; }"),
            ("if", "if (true) { print(1); }"),
            ("if else", "if (true) { print(1); } else { print(2); }"),
            ("while", "while (false) { print(1); }"),
            ("block", "{ print(1); }"),
            ("return value", "function f(): number { return 1; }"),
            ("return bare", "function f(): void { return; }"),
            ("expr stmt", "print(1);"),
            ("number literal", "print(1.5);"),
            ("bool literal", "print(true);"),
            ("var read", "let x = 1; print(x);"),
            ("assignment", "let x = 1; x = 2;"),
            ("binary", "print(1 + 2);"),
            ("unary minus", "print(-1);"),
            ("unary not", "print(!true);"),
            ("call", "function f(): void { } f();"),
            ("nested call", "function f(): number { return 1; } print(f());"),
            ("main", "function main(): void { print(1); }"),
            ("top level", "print(1);"),
            ("shadowing", "let x = 1; { let x = 2; print(x); }"),
            ("forward reference", "f(); function f(): void { }"),
            ("recursion", "function f(n: number): number { if (n < 1) { return n; } return f(n - 1); }"),
        ];
        for (name, src) in cases {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                try_compile(src)
            }));
            assert!(
                outcome.is_ok(),
                "the compiler panicked while handling the `{name}` case: {src}"
            );
        }
    }

    /// A rejected program must produce a diagnostic, never a panic and never
    /// a successful compile.
    #[test]
    fn rejected_programs_report_cleanly() {
        let cases: &[(&str, &str)] = &[
            ("bad type", "let x: number = true;"),
            ("bad arity", "function f(a: number): void { } f();"),
            ("undefined name", "print(nope);"),
            ("nested function", "function o(): void { function i(): void { } }"),
            ("bad main", "function main(): number { return 1; }"),
        ];
        for (name, src) in cases {
            let err = try_compile(src).expect_err("expected this program to be rejected");
            assert!(!err.is_empty(), "the `{name}` case produced an empty diagnostic");
        }
    }

    #[test]
    fn compiles_a_minimal_program() {
        let obj = compiles("print(1);");
        assert!(!obj.is_empty(), "expected a non-empty object file");
    }

    // --- the `int` type ----------------------------------------------------

    #[test]
    fn int_literal_adopts_the_annotated_type() {
        // `let x: int = 42` makes the literal an int, not a number.
        compiles("let x: int = 42;");
    }

    #[test]
    fn int_literal_stays_a_number_without_a_context() {
        // No annotation means the default type, which is still `number`.
        compiles("let x = 42;");
    }

    #[test]
    fn int_arithmetic_yields_an_int() {
        compiles("let a: int = 2; let b: int = 3; let c: int = a + b;");
    }

    #[test]
    fn int_literal_adapts_to_an_int_operand() {
        // The literal takes the type of the other operand.
        compiles("let a: int = 2; let b = a + 1;");
        compiles("let a: int = 2; let b = 1 + a;");
    }

    #[test]
    fn int_division_truncates_toward_zero() {
        compiles("let a: int = 7; let b: int = 2; let c: int = a / b;");
    }

    #[test]
    fn int_remainder_is_supported() {
        compiles("let a: int = 7; let b: int = 3; let c: int = a % b;");
    }

    #[test]
    fn rejects_a_non_integer_literal_as_an_int() {
        let err = type_error("let x: int = 4.5;");
        assert!(err.contains("non-integer value"), "got: {err}");
    }

    #[test]
    fn rejects_mixing_int_and_number() {
        let err = type_error("let a: int = 1; let b = a + 1.5;");
        assert!(
            err.contains("arithmetic operators require two numbers or two ints"),
            "got: {err}"
        );
    }

    #[test]
    fn rejects_a_float_remainder() {
        let err = type_error("print(5.0 % 2.0);");
        assert!(err.contains("remainder operator requires integer operands"), "got: {err}");
    }

    #[test]
    fn rejects_relational_operators_across_types() {
        let err = type_error("let a: int = 1; let b = a < 1.5;");
        assert!(err.contains("relational operators require"), "got: {err}");
    }

    #[test]
    fn accepts_int_unary_operations() {
        compiles("let a: int = 1; let b: int = -a; let c: boolean = !a;");
    }

    #[test]
    fn accepts_an_int_parameter_and_argument() {
        compiles("function f(n: int): int { return n; } let x: int = f(3);");
    }

    #[test]
    fn an_int_literal_adapts_to_the_return_type() {
        // Regression test: `return 0;` in an int function used to be
        // rejected, because the return statement did not pass the expected
        // type to the expression.
        compiles("function f(): int { return 0; }");
        compiles("function f(): int { return 42; }");
        compiles("function f(): int { if (true) { return 1; } return 0; }");
    }

    #[test]
    fn compiles_a_program_with_no_statements() {
        // An empty program is valid and produces a do-nothing binary.
        let obj = compiles("");
        assert!(!obj.is_empty());
    }

    #[test]
    fn rejects_arithmetic_on_booleans() {
        let err = type_error("let x = 1 + true;");
        assert!(
            err.contains("arithmetic operators require two numbers or two ints"),
            "got: {err}"
        );
    }

    #[test]
    fn rejects_relational_operators_on_booleans() {
        let err = type_error("let x = true < false;");
        assert!(
            err.contains("relational operators require two numbers or two ints"),
            "got: {err}"
        );
    }

    #[test]
    fn rejects_comparing_mismatched_types() {
        let err = type_error("let x = 1 == true;");
        assert!(err.contains("cannot compare number with boolean"), "got: {err}");
    }

    #[test]
    fn rejects_initializer_type_mismatch() {
        let err = type_error("let x: number = true;");
        assert!(err.contains("cannot initialize 'x: number'"), "got: {err}");
    }

    #[test]
    fn rejects_assignment_type_mismatch() {
        let err = type_error("let x: number = 1; x = true;");
        assert!(err.contains("cannot assign boolean to 'x: number'"), "got: {err}");
    }

    #[test]
    fn rejects_assignment_to_const() {
        let err = type_error("const c = 1; c = 2;");
        assert!(err.contains("cannot assign to const 'c'"), "got: {err}");
    }

    #[test]
    fn rejects_undefined_variable() {
        let err = type_error("print(nope);");
        assert!(err.contains("undefined variable 'nope'"), "got: {err}");
    }

    #[test]
    fn rejects_undefined_function() {
        let err = type_error("nope();");
        assert!(err.contains("undefined function 'nope'"), "got: {err}");
    }

    #[test]
    fn rejects_wrong_argument_count() {
        let err = type_error("function f(a: number): void { }\nf(1, 2);");
        assert!(err.contains("'f' takes 1 arguments, got 2"), "got: {err}");
    }

    #[test]
    fn rejects_wrong_argument_type() {
        let err = type_error("function f(a: number): void { }\nf(true);");
        assert!(err.contains("argument of 'f' must be number, got boolean"), "got: {err}");
    }

    #[test]
    fn rejects_missing_return() {
        let err = type_error("function f(): number { let a = 1; }");
        assert!(err.contains("must return number on all paths"), "got: {err}");
    }

    #[test]
    fn rejects_returning_a_value_from_a_void_function() {
        let err = type_error("function f(): void { return 1; }");
        assert!(err.contains("cannot return a value from a void function"), "got: {err}");
    }

    #[test]
    fn rejects_returning_the_wrong_type() {
        let err = type_error("function f(): number { return true; }");
        assert!(err.contains("cannot return boolean from a function returning number"), "got: {err}");
    }

    #[test]
    fn rejects_bare_return_from_a_value_function() {
        let err = type_error("function f(): number { return; }");
        assert!(err.contains("must return a value of type number"), "got: {err}");
    }

    #[test]
    fn rejects_printing_void() {
        let err = type_error("function f(): void { }\nprint(f());");
        assert!(err.contains("cannot print void"), "got: {err}");
    }

    #[test]
    fn rejects_redefining_print() {
        let err = type_error("function print(x: number): void { }");
        assert!(err.contains("'print' is a built-in"), "got: {err}");
    }

    #[test]
    fn rejects_a_main_with_parameters() {
        let err = type_error("function main(a: number): void { }");
        assert!(err.contains("`main` must be declared as `function main(): void`"), "got: {err}");
    }

    #[test]
    fn rejects_a_main_returning_a_value() {
        let err = type_error("function main(): number { return 1; }");
        assert!(err.contains("`main` must be declared as `function main(): void`"), "got: {err}");
    }

    #[test]
    fn rejects_nested_function_declarations() {
        let err = type_error("function outer(): void { function inner(): void { } }");
        assert!(err.contains("nested function declarations are not supported"), "got: {err}");
    }

    #[test]
    fn rejects_redeclaring_a_variable_in_the_same_scope() {
        let err = type_error("let x = 1; let x = 2;");
        assert!(err.contains("already declared in this scope"), "got: {err}");
    }

    #[test]
    fn accepts_shadowing_in_a_nested_scope() {
        // A nested block may reuse a name; only same-scope reuse is an error.
        compiles("let x = 1; { let x = 2; }");
    }

    #[test]
    fn accepts_short_circuit_operands_of_either_type() {
        compiles("let a = true && 1;");
        compiles("let b = 0 || false;");
    }

    #[test]
    fn accepts_a_forward_reference_to_a_later_function() {
        compiles("f();\nfunction f(): void { }");
    }

    #[test]
    fn accepts_recursion() {
        compiles("function f(n: number): number { if (n < 1) { return n; } return f(n - 1); }");
    }

    #[test]
    fn accepts_an_if_without_an_else() {
        compiles("let x = 1; if (x > 0) { print(x); }");
    }

    #[test]
    fn accepts_a_while_loop() {
        compiles("let i = 0; while (i < 3) { i = i + 1; }");
    }

    #[test]
    fn accepts_top_level_statements_alongside_main() {
        compiles("print(1);\nfunction main(): void { print(2); }");
    }

    #[test]
    fn accepts_a_void_function_without_a_return() {
        compiles("function f(): void { }");
    }
}
