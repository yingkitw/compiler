//! Type-checking code generator: walks the AST, checks types, and emits
//! native machine code through Cranelift's object backend.

use std::collections::HashMap;

use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Signature};
use cranelift_codegen::isa::{self, CallConv, TargetFrontendConfig};
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::Context;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{FuncId, Linkage, Module};
use cranelift_object::{ObjectBuilder, ObjectModule};
use target_lexicon::Triple;

use crate::ast::*;

mod expr;
mod runtime;
mod stmt;
mod ty;

#[cfg(test)]
mod tests;

use ty::{cl_ty, sig_for, FnSig, Slot};

pub use runtime::RUNTIME_C;

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
    // The verifier re-checks the IR Cranelift built. It is a debugging aid,
    // and a costly one: it is worth several percent of the compile time.
    // Debug builds keep it — the whole test suite runs there, so a code
    // generation bug is still caught — and release builds drop it.
    flag_builder
        .set("enable_verifier", if cfg!(debug_assertions) { "true" } else { "false" })
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
    // Installed before anything else runs: it turns a stack overflow into a
    // clean error instead of a SIGSEGV that kills the program.
    let runtime_init = module
        .declare_function(
            "ts_runtime_init",
            Linkage::Import,
            &sig_for(call_conv, &[], Type::Void),
        )
        .map_err(|e| e.to_string())?;
    // Float remainder: Cranelift has no `frem`, so it goes through libm.
    let fmod = module
        .declare_function(
            "ts_fmod",
            Linkage::Import,
            &sig_for(call_conv, &[Type::Number, Type::Number], Type::Number),
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
                fmod, fs.id, sig, name, params, fs.ret, body,
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
        fmod, start_id, start_sig, "__ts_start", &[], Type::Void, &top,
    )?;

    // C `main` wrapper: initialise the runtime, call __ts_start, then the
    // user's main if any.
    emit_main_wrapper(
        &mut module,
        &mut fbcx,
        frontend_config,
        call_conv,
        runtime_init,
        start_id,
        user_main,
    )?;

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
    fmod: FuncId,
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
            depth: 0,
            module,
            funcs,
            print_num,
            print_bool,
            print_int,
            fmod,
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
    runtime_init: FuncId,
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
        let init_ref = module.declare_func_in_func(runtime_init, b.func);
        b.ins().call(init_ref, &[]);
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
    /// How deeply the expression or statement being generated is nested.
    depth: usize,
    module: &'a mut ObjectModule,
    funcs: &'a HashMap<String, FnSig>,
    print_num: FuncId,
    print_bool: FuncId,
    print_int: FuncId,
    fmod: FuncId,
    scopes: Vec<HashMap<String, Slot>>,
    ret: Type,
}
