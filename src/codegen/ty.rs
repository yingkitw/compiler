//! The type lattice: how a source type maps to a machine type, and how a
//! function's signature is built.

use cranelift_codegen::ir::types;
use cranelift_codegen::ir::{AbiParam, Signature, Value};
use cranelift_codegen::isa::CallConv;
use cranelift_frontend::Variable;
use cranelift_module::FuncId;

use crate::ast::*;

/// A generated value and its static type. `None` means the expression has no
/// value (`void`).
pub(crate) type TypedValue = (Option<Value>, Type);

/// The type a literal should adopt given the other operand's type. Only
/// `int` is contagious: a `number` literal in an `int` context stays a
/// `number`, because widening is not implicit.
pub(crate) fn int_hint(t: Type) -> Option<Type> {
    if t == Type::Int { Some(Type::Int) } else { None }
}

pub(crate) fn cl_ty(t: Type) -> types::Type {
    match t {
        Type::Number => types::F64,
        Type::Int => types::I64,
        Type::Boolean => types::I8,
        Type::Void => panic!("void has no machine representation"),
    }
}

pub(crate) fn sig_for(call_conv: CallConv, params: &[Type], ret: Type) -> Signature {
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
pub(crate) struct FnSig {
    pub(crate) id: FuncId,
    pub(crate) params: Vec<Type>,
    pub(crate) ret: Type,
}

#[derive(Clone, Copy)]
pub(crate) struct Slot {
    pub(crate) var: Variable,
    pub(crate) ty: Type,
    pub(crate) mutable: bool,
}
