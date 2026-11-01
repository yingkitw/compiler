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
void ts_print_number(double x) { printf("%g\n", x); }
void ts_print_bool(signed char x) { puts(x ? "true" : "false"); }
"#;

fn cl_ty(t: Type) -> types::Type {
    match t {
        Type::Number => types::F64,
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
                &mut module, &mut fbcx, frontend_config, &funcs, print_num, print_bool,
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
        &mut module, &mut fbcx, frontend_config, &funcs, print_num, print_bool,
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
                        let (v, t) = self.gen_expr(init)?;
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
                    let (v, t) = self.gen_expr(e)?;
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
                let v = self.b.ins().f64const(*n);
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
                        if t != Type::Number {
                            return Err(format!("cannot negate {}", t));
                        }
                        Ok((Some(self.b.ins().fneg(v)), Type::Number))
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
                let (lv, lt) = self.gen_expr(lhs)?;
                let (rv, rt) = self.gen_expr(rhs)?;
                if lt != rt {
                    return Err(format!("cannot compare {} with {}", lt, rt));
                }
                let lv = self.val(lv, lt)?;
                let rv = self.val(rv, rt)?;
                let v = match (lt, op) {
                    (Type::Number, BinOp::Eq) => self.b.ins().fcmp(FloatCC::Equal, lv, rv),
                    (Type::Number, BinOp::Ne) => self.b.ins().fcmp(FloatCC::NotEqual, lv, rv),
                    (Type::Boolean, BinOp::Eq) => self.b.ins().icmp(IntCC::Equal, lv, rv),
                    (Type::Boolean, BinOp::Ne) => self.b.ins().icmp(IntCC::NotEqual, lv, rv),
                    _ => unreachable!(),
                };
                Ok((Some(v), Type::Boolean))
            }
            BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let (lv, lt) = self.gen_expr(lhs)?;
                let (rv, rt) = self.gen_expr(rhs)?;
                if lt != Type::Number || rt != Type::Number {
                    return Err("relational operators require numbers".into());
                }
                let lv = self.val(lv, lt)?;
                let rv = self.val(rv, rt)?;
                let cc = match op {
                    BinOp::Lt => FloatCC::LessThan,
                    BinOp::Le => FloatCC::LessThanOrEqual,
                    BinOp::Gt => FloatCC::GreaterThan,
                    BinOp::Ge => FloatCC::GreaterThanOrEqual,
                    _ => unreachable!(),
                };
                let v = self.b.ins().fcmp(cc, lv, rv);
                Ok((Some(v), Type::Boolean))
            }
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => {
                let (lv, lt) = self.gen_expr(lhs)?;
                let (rv, rt) = self.gen_expr(rhs)?;
                if lt != Type::Number || rt != Type::Number {
                    return Err("arithmetic operators require numbers".into());
                }
                let lv = self.val(lv, lt)?;
                let rv = self.val(rv, rt)?;
                let v = match op {
                    BinOp::Add => self.b.ins().fadd(lv, rv),
                    BinOp::Sub => self.b.ins().fsub(lv, rv),
                    BinOp::Mul => self.b.ins().fmul(lv, rv),
                    BinOp::Div => self.b.ins().fdiv(lv, rv),
                    _ => unreachable!(),
                };
                Ok((Some(v), Type::Number))
            }
        }
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
            let (v, t) = self.gen_expr(arg)?;
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
            Type::Void => Err("void expression has no truth value".into()),
        }
    }

    fn val(&self, v: Option<Value>, t: Type) -> Result<Value, String> {
        v.ok_or_else(|| format!("void value of type {} used where a value is required", t))
    }

    fn default_value(&mut self, t: Type) -> Value {
        match t {
            Type::Number => self.b.ins().f64const(0.0),
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
            Type::Boolean => {
                let z = self.b.ins().iconst(types::I8, 0);
                self.b.ins().return_(&[z]);
            }
        }
    }
}
