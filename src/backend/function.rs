//! Per-function emission state: frame, locals, temporaries, and the prologue/epilogue.

use std::collections::HashMap;

use crate::ast::*;

use super::asm::reg::{D, FP, X};
use super::asm::{Asm, Label, Mem};
use super::stmt;
use super::{type_slots, FnSig, Local};

/// The state of one function being emitted.
pub struct Fn<'a> {
    /// The function's name, as the C runtime would spell it.
    name: String,
    /// Its parameters: the names and the types.
    params: Vec<(String, Type)>,
    /// Its return type.
    ret: Type,
    /// The functions the program defines, for the calls.
    funcs: &'a HashMap<String, FnSig>,
    /// The assembler the code goes into.
    asm: Asm,
    /// The locals of the scopes, innermost last.
    scopes: Vec<HashMap<String, Local>>,
    /// How many local slots the frame has: the locals and the parameters.
    locals: i32,
    /// How many local slots have been handed out.
    next_local: i32,
    /// The highest temporary slot used.
    temps: i32,
    /// The next free temporary.
    next_temp: i32,
    /// The label of the function's epilogue, where its returns go.
    epilogue: Label,
    /// The next rodata label for a string literal in this function.
    next_string: u32,
}

impl<'a> Fn<'a> {
    pub fn new(
        name: &str,
        params: &[(String, Type)],
        ret: Type,
        funcs: &'a HashMap<String, FnSig>,
    ) -> Fn<'a> {
        let mut asm = Asm::new(name);
        let epilogue = asm.named_label(&format!("{}_e", name));
        Fn {
            name: name.to_string(),
            params: params.to_vec(),
            ret,
            funcs,
            asm,
            scopes: vec![HashMap::new()],
            locals: 0,
            next_local: 0,
            temps: 0,
            next_temp: 0,
            epilogue,
            next_string: 0,
        }
    }

    /// A fresh label for a string literal's bytes in this function.
    pub fn string_label(&mut self) -> String {
        let id = self.next_string;
        self.next_string += 1;
        format!(".L{}_s{id}", self.name)
    }

    /// The symbol the assembler sees. On Mach-O every symbol carries a
    /// leading underscore; on ELF it does not.
    pub fn symbol(name: &str) -> String {
        if cfg!(target_os = "macos") {
            format!("_{}", name)
        } else {
            name.to_string()
        }
    }

    /// The frame's size in bytes: the locals, the temporaries, and room for
    /// the outgoing arguments of a call.
    fn frame_size(&self) -> i32 {
        let locals = self.locals * 8;
        let temps = self.temps * 8;
        // The runtime's widest call takes two numbers.
        let outgoing = 16;
        ((locals + temps + outgoing) + 15) / 16 * 16
    }

    /// The whole function: the label, the prologue, the body, the epilogue.
    ///
    /// The body is assembled separately, because the prologue's size is
    /// only known once the body is written — and the prologue has to come
    /// first, or the body would run with the caller's frame pointer.
    pub fn finish(&mut self) -> String {
        let frame = self.frame_size();
        let body = self.asm.take();

        let mut head = Asm::new("head");
        head.line(".text");
        head.line(&format!(".globl {}", Fn::symbol(&self.name)));
        head.place_symbol(&Fn::symbol(&self.name));
        head.push_frame(frame);
        let mut text = head.take();
        text.push_str(&body);

        let mut tail = Asm::new("tail");
        tail.place(&self.epilogue);
        if frame > 0 {
            tail.pop_frame(frame);
        }
        tail.ret();
        text.push_str(tail.text());
        text
    }

    /// Emit the body's statements. The parameters are bound first: they
    /// arrive in the argument registers, and they are the frame's first
    /// locals.
    pub fn emit_body(&mut self, body: &[Stmt]) -> Result<(), String> {
        // The locals are counted first: the temporaries live below them, so
        // their slots must be known before the body is emitted.
        let param_slots: i32 = self
            .params
            .iter()
            .map(|(_, ty)| type_slots(*ty))
            .sum();
        self.locals = count_locals(body) as i32 + param_slots;
        self.bind_parameters()?;
        let terminated = stmt::gen_stmts(self, body)?;
        if !terminated {
            if self.ret == Type::Void {
                self.asm.branch(&self.epilogue);
            } else {
                return Err(format!(
                    "function '{}' must return {} on all paths",
                    self.name, self.ret
                ));
            }
        }
        Ok(())
    }

    /// Copy the incoming arguments into the parameters' slots.
    fn bind_parameters(&mut self) -> Result<(), String> {
        let params = self.params.clone();
        let mut int_reg = 0;
        let mut num_reg = 0;
        for (name, ty) in params {
            self.declare_local(&name, ty, true)?;
            let offset = self.lookup(&name)?.offset;
            match ty {
                Type::Number => {
                    // On AArch64 the argument is in `vN`, which aliases `dN`.
                    self.asm
                        .store_double(D(num_reg), Mem { base: FP, offset });
                    num_reg += 1;
                }
                Type::String => {
                    self.asm
                        .store(X(int_reg), Mem { base: FP, offset });
                    self.asm
                        .store(X(int_reg + 1), Mem { base: FP, offset: offset - 8 });
                    int_reg += 2;
                }
                Type::NumberArray | Type::BooleanArray => {
                    self.asm.store(X(int_reg), Mem { base: FP, offset });
                    int_reg += 1;
                }
                _ => {
                    self.asm.store(X(int_reg), Mem { base: FP, offset });
                    int_reg += 1;
                }
            }
        }
        Ok(())
    }

    /// The functions the program defines.
    pub fn funcs(&self) -> &HashMap<String, FnSig> {
        self.funcs
    }

    /// The function's return type.
    pub fn ret(&self) -> Type {
        self.ret
    }

    /// The assembler the code goes into.
    pub fn asm(&mut self) -> &mut Asm {
        &mut self.asm
    }

    /// The label of the function's epilogue.
    pub fn epilogue(&self) -> &Label {
        &self.epilogue
    }

    /// A fresh label.
    pub fn label(&mut self) -> Label {
        self.asm.label()
    }

    /// Push a scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop a scope.
    pub fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    /// Declare a local: it gets a frame slot.
    pub fn declare_local(&mut self, name: &str, ty: Type, mutable: bool) -> Result<(), String> {
        if self.scopes.last().expect("a scope is open").contains_key(name) {
            return Err(format!("'{}' is already declared in this scope", name));
        }
        let slots = type_slots(ty);
        let start = self.next_local;
        self.next_local += slots;
        let offset = -(start + 1) * 8;
        self.scopes
            .last_mut()
            .expect("a scope is open")
            .insert(name.to_string(), Local { offset, ty, mutable });
        Ok(())
    }

    /// Look a local up: the innermost scope that has it.
    pub fn lookup(&self, name: &str) -> Result<Local, String> {
        for scope in self.scopes.iter().rev() {
            if let Some(local) = scope.get(name) {
                return Ok(*local);
            }
        }
        Err(format!("undefined variable '{}'", name))
    }

    /// Allocate a temporary slot, and mark the stack so it can be released.
    ///
    /// The temporaries live below the locals, so a temporary's slot never
    /// collides with a local's — the locals' count is known before the body
    /// is emitted.
    pub fn temp(&mut self) -> i32 {
        self.temp_for(Type::Int)
    }

    /// A temporary large enough for `ty` (`string` uses two slots).
    pub fn temp_for(&mut self, ty: Type) -> i32 {
        let slots = type_slots(ty);
        let start = self.next_local + self.next_temp;
        self.next_temp += slots;
        if self.next_temp > self.temps {
            self.temps = self.next_temp;
        }
        -(start + 1) * 8
    }

    /// The mark of the temporary stack.
    pub fn temp_mark(&self) -> i32 {
        self.next_temp
    }

    /// Release the temporaries allocated since `mark`.
    pub fn release_temps(&mut self, mark: i32) {
        self.next_temp = mark;
    }
}

/// How many local slots an expression's nested block expressions may declare.
fn count_expr_locals(expr: &Expr) -> usize {
    match expr {
        Expr::BlockExpr { stmts, value } => count_locals(stmts) + count_expr_locals(value),
        Expr::Cond {
            cond,
            then_expr,
            else_expr,
        } => {
            count_expr_locals(cond) + count_expr_locals(then_expr) + count_expr_locals(else_expr)
        }
        Expr::Bin { lhs, rhs, .. } => count_expr_locals(lhs) + count_expr_locals(rhs),
        Expr::Un { expr, .. } => count_expr_locals(expr),
        Expr::Cast { expr, .. } => count_expr_locals(expr),
        Expr::Assign { value, .. } => count_expr_locals(value),
        Expr::Array(elems) => elems.iter().map(count_expr_locals).sum(),
        Expr::Index { base, index } => count_expr_locals(base) + count_expr_locals(index),
        Expr::Member { base, .. } => count_expr_locals(base),
        Expr::Call { receiver, args, .. } => {
            receiver.as_deref().map(count_expr_locals).unwrap_or(0)
                + args.iter().map(count_expr_locals).sum::<usize>()
        }
        Expr::Num(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Var(_) => 0,
    }
}

/// How many local slots a statement list needs: one for each `let`, and any
/// `let` inside a block expression the list may evaluate.
fn count_locals(stmts: &[Stmt]) -> usize {
    let mut n = 0;
    for s in stmts {
        match s {
            Stmt::Let { ty, init, .. } => {
                n += match ty {
                    Some(t) => type_slots(*t) as usize,
                    None => 1,
                };
                if let Some(init) = init {
                    n += count_expr_locals(init);
                }
            }
            Stmt::If { cond, then, els, .. } => {
                n += count_expr_locals(cond) + count_locals(then) + count_locals(els);
            }
            Stmt::While { cond, body, .. } => {
                n += count_expr_locals(cond) + count_locals(body);
            }
            Stmt::Block { stmts, .. } => n += count_locals(stmts),
            Stmt::Expr { expr, .. } | Stmt::Return { value: Some(expr), .. } => {
                n += count_expr_locals(expr);
            }
            _ => {}
        }
    }
    n
}
