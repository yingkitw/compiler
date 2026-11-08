//! The code generator: a program becomes assembly text.
//!
//! There is no Cranelift here and no LLVM: the compiler emits the target's
//! assembly and lets the system C compiler assemble and link it. The
//! generated code is ordinary native code — the C runtime is linked
//! alongside it, and the calls into it follow the platform's calling
//! convention.
//!
//! # The value model
//!
//! Every expression leaves its value in a register — `x0` for an `int` or a
//! `boolean`, `d0` for a `number` — and the frame holds everything that
//! must outlive the next expression: the locals, and a pool of
//! temporaries for the two operands of a binary operation.
//!
//! The frame is laid out from the frame pointer down:
//!
//! ```text
//!   fp + 0    the saved frame pointer and link register
//!   fp - 8    the locals, one slot each
//!             the temporaries
//!   the outgoing arguments of a call, at the bottom
//! ```
//!
//! The assembly itself is written by the `asm` module: this one asks it
//! for instructions rather than writing text.

use std::collections::HashMap;
use std::path::Path;

use crate::ast::*;

pub mod asm;
pub mod expr;
mod function;
pub mod runtime;
pub mod stmt;

#[cfg(test)]
mod tests;

pub use function::Fn;

use asm::Asm;

/// The value an expression left behind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Val {
    /// An `int` or a `boolean`, in `x0`.
    Int,
    /// A `number`, in `d0`.
    Num,
    /// A `string`: pointer in `x0`, byte length in `x1`.
    Str,
}

impl Val {
    /// The value a type is held in.
    pub fn of(t: Type) -> Val {
        match t {
            Type::Number => Val::Num,
            Type::String => Val::Str,
            _ => Val::Int,
        }
    }
}

/// How many 8-byte frame slots a type occupies.
pub(super) fn type_slots(ty: Type) -> i32 {
    if ty == Type::String { 2 } else { 1 }
}

/// A local variable: where it lives, and what it may be used for.
#[derive(Clone, Copy)]
pub struct Local {
    /// The offset from the frame pointer, in bytes, below it.
    pub offset: i32,
    pub ty: Type,
    pub mutable: bool,
}

/// A function's signature, as the compiler needs it.
#[derive(Clone)]
pub struct FnSig {
    pub params: Vec<Type>,
    pub ret: Type,
}

/// Compile a program into assembly text.
pub fn emit(prog: &Program, source: &Path) -> Result<String, String> {
    let mut out = String::new();
    out.push_str(&Asm::debug_file_directive(source));
    out.push('\n');

    // Pass 1: every function, so a call can appear before its definition.
    let mut funcs: HashMap<String, FnSig> = HashMap::new();
    for stmt in &prog.stmts {
        if let Stmt::Func {
            name, params, ret, ..
        } = stmt
        {
            if name == "print" {
                return Err("'print' is a built-in and cannot be redefined".into());
            }
            if funcs.contains_key(name) {
                return Err(format!("duplicate function '{}'", name));
            }
            funcs.insert(
                name.clone(),
                FnSig {
                    params: params.iter().map(|p| p.1).collect(),
                    ret: ret.unwrap_or(Type::Void),
                },
            );
        }
    }

    let user_main = match funcs.get("main") {
        Some(f) => {
            if !f.params.is_empty() || f.ret != Type::Void {
                return Err("`main` must be declared as `function main(): void`".into());
            }
            Some(f.clone())
        }
        None => None,
    };

    // The top-level statements, in a function of their own.
    let top_level: Vec<Stmt> = prog
        .stmts
        .iter()
        .filter(|s| !matches!(s, Stmt::Func { .. }))
        .cloned()
        .collect();
    let mut start = Fn::new("__ts_start", &[], Type::Void, &funcs);
    start.emit_body(&top_level)?;
    out.push_str(&start.finish());

    // The user functions.
    for stmt in &prog.stmts {
        if let Stmt::Func {
            name,
            params,
            body,
            ..
        } = stmt
        {
            let sig = funcs.get(name).expect("the function was collected above");
            let mut f = Fn::new(&format!("tsfn_{}", name), params, sig.ret, &funcs);
            f.emit_body(body)?;
            out.push_str(&f.finish());
        }
    }

    // The C entry point: the runtime's setup first, then the top-level
    // statements, then the user's `main` if there is one.
    out.push_str(&wrapper(user_main.is_some()));
    Ok(out)
}

/// The C `main`: it runs the runtime's setup, the top-level statements and
/// the user's `main`, and returns zero.
fn wrapper(has_main: bool) -> String {
    let mut asm = Asm::new("main");
    asm.line(".text");
    asm.line(&format!(".globl {}", Fn::symbol("main")));
    asm.place_symbol(&Fn::symbol("main"));
    asm.push_frame(16);
    asm.call(&Fn::symbol("ts_runtime_init"));
    asm.call(&Fn::symbol("__ts_start"));
    if has_main {
        asm.call(&Fn::symbol("tsfn_main"));
    }
    asm.mov_imm(asm::reg::X0, 0);
    asm.pop_frame(16);
    asm.ret();
    asm.text().to_string()
}
