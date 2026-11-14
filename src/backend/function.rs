//! Per-function emission state: frame, locals, temporaries, and the prologue/epilogue.

use std::collections::HashMap;

use crate::ast::*;

use super::asm::reg::{D, FP, X};
use super::asm::{Asm, Label, Mem};
use super::regs::{self, Cache};
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
    /// The locals cached in callee-saved registers.
    cache: Cache,
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
            cache: Cache::new(),
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

    /// The frame's size in bytes: the locals, the temporaries, room for
    /// the outgoing arguments of a call, and the callee-saved registers
    /// the function's register cache uses.
    fn frame_size(&self) -> i32 {
        let locals = self.locals * 8;
        let temps = self.temps * 8;
        // The runtime's widest call takes two numbers.
        let outgoing = 16;
        let (fp, gp) = self.cache.used();
        let saves = (fp.len() + gp.len()) as i32 * 8;
        ((locals + temps + outgoing + saves) + 15) / 16 * 16
    }

    /// The whole function: the label, the prologue, the body, the epilogue.
    ///
    /// The body is assembled separately, because the prologue's size is
    /// only known once the body is written — and the prologue has to come
    /// first, or the body would run with the caller's frame pointer.
    pub fn finish(&mut self) -> String {
        let frame = self.frame_size();
        // The bytes below the frame pointer the locals and temporaries
        // occupy; the callee-save area starts under them.
        let used_bytes = (self.locals + self.temps) * 8;
        let body = self.asm.take();

        let mut head = Asm::new("head");
        head.line(".text");
        head.line(&format!(".globl {}", Fn::symbol(&self.name)));
        head.place_symbol(&Fn::symbol(&self.name));
        head.push_frame(frame);
        save_callee_saved(&mut head, &self.cache, used_bytes);
        let mut text = head.take();
        text.push_str(&body);

        let mut tail = Asm::new("tail");
        tail.place(&self.epilogue);
        restore_callee_saved(&mut tail, &self.cache, used_bytes);
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

    /// Copy the incoming arguments into the parameters' slots — or into a
    /// callee-saved register, when the parameter is promoted.
    fn bind_parameters(&mut self) -> Result<(), String> {
        let params = self.params.clone();
        let mut int_reg = 0;
        let mut num_reg = 0;
        for (name, ty) in params {
            self.declare_local(&name, ty, true)?;
            let local = self.lookup(&name)?;
            // A parameter is assigned exactly once, by the caller, so it
            // is a natural resident of the register cache.
            self.promote(&local);
            match self.cached(&local) {
                Some((reg, true)) => {
                    self.asm.fmov_d_from_d(D(reg), D(num_reg));
                    num_reg += 1;
                }
                Some((reg, false)) => {
                    self.asm.mov_reg(X(reg), X(int_reg));
                    int_reg += 1;
                }
                None => match ty {
                    Type::Number => {
                        // On AArch64 the argument is in `vN`, which aliases `dN`.
                        self.asm
                            .store_double(D(num_reg), Mem { base: FP, offset: local.offset });
                        num_reg += 1;
                    }
                    Type::String => {
                        self.asm
                            .store(X(int_reg), Mem { base: FP, offset: local.offset });
                        self.asm
                            .store(X(int_reg + 1), Mem { base: FP, offset: local.offset - 8 });
                        int_reg += 2;
                    }
                    _ => {
                        self.asm
                            .store(X(int_reg), Mem { base: FP, offset: local.offset });
                        int_reg += 1;
                    }
                },
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

    /// Pop a scope. The locals it declared can no longer be named, so any
    /// cached values of them are dead and their registers are freed.
    pub fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            let slots: Vec<i32> = scope.values().map(|l| l.offset).collect();
            self.cache.drop_slots(&slots);
        }
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

    /// Promote a local and load its slot's value into the register — how
    /// a loop makes an assigned local register-resident before its first
    /// test. A local that is already a resident keeps its register: the
    /// value in it is current, and the slot may never have been written.
    /// A name that does not resolve (it is declared inside the loop's
    /// body) is skipped: its `let` promotes it itself.
    pub fn promote_and_load(&mut self, name: &str) {
        let Ok(local) = self.lookup(name) else { return };
        if self.cached(&local).is_some() {
            return;
        }
        self.promote(&local);
        let Some(reg) = self.promotion_of(&local) else {
            return;
        };
        let mem = Mem {
            base: FP,
            offset: local.offset,
        };
        if local.ty == Type::Number {
            self.asm.load_double(D(reg), mem);
        } else {
            self.asm.load(X(reg), mem);
        }
    }

    // --- the register cache ---------------------------------------------

    /// The register a local is cached in, if any: `(reg, is_fp)`.
    pub fn cached(&self, local: &Local) -> Option<(u8, bool)> {
        self.cache.get(local.offset).map(|e| {
            let is_fp = e.reg < regs::GP_BASE;
            (e.reg, is_fp)
        })
    }

    /// Promote a local into a callee-saved register if one is free and the
    /// type has one — numbers use the floating pool, ints and booleans the
    /// general one; strings and arrays stay in their slots.
    pub fn promote(&mut self, local: &Local) {
        let fp = match local.ty {
            Type::Number => true,
            Type::Int | Type::Boolean => false,
            _ => return,
        };
        self.cache.promote(local.offset, fp);
    }

    /// The register a local was promoted to, if it was.
    pub fn promotion_of(&mut self, local: &Local) -> Option<u8> {
        self.cached(local).map(|(reg, _)| reg)
    }

    /// Record that a local's cached value is newer than its slot.
    pub fn mark_dirty(&mut self, local: &Local) {
        self.cache.mark_dirty(local.offset);
    }

    /// Open a loop region: locals promoted while it is open are flushed
    /// and dropped when it closes.
    pub fn open_region(&mut self) -> u32 {
        self.cache.open_region()
    }

    /// Close a loop region, flushing its dirty entries to their slots.
    pub fn close_region(&mut self, region: u32) {
        let dropped = self.cache.close_region(region);
        for e in dropped {
            if !e.dirty {
                continue;
            }
            let mem = Mem {
                base: FP,
                offset: e.slot,
            };
            if e.reg < regs::GP_BASE {
                self.asm.store_double(D(e.reg), mem);
            } else {
                self.asm.store(X(e.reg), mem);
            }
        }
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

/// Save the callee-saved registers the cache uses. They go below the
/// locals and temporaries and above the saved `x29`/`x30` pair, which the
/// frame's pre-decrement store places at its very bottom. Pairs go
/// through `stp`; a dangling single through `str`.
fn save_callee_saved(asm: &mut Asm, cache: &Cache, used_bytes: i32) {
    let (fp, gp) = cache.used();
    place_list(asm, &fp, true, true, used_bytes, 0);
    place_list(asm, &gp, false, true, used_bytes, fp.len());
}

/// Restore what `save_callee_saved` saved.
fn restore_callee_saved(asm: &mut Asm, cache: &Cache, used_bytes: i32) {
    let (fp, gp) = cache.used();
    place_list(asm, &fp, true, false, used_bytes, 0);
    place_list(asm, &gp, false, false, used_bytes, fp.len());
}

/// Place one pool's registers into the save area, `stp` for pairs and
/// `str` for a dangling single.
fn place_list(
    asm: &mut Asm,
    regs: &[u8],
    fp: bool,
    save: bool,
    used_bytes: i32,
    start: usize,
) {
    let mut i = 0;
    while i + 1 < regs.len() {
        let mem = save_mem(used_bytes, start + i);
        let (a, b) = (regs[i], regs[i + 1]);
        if fp {
            if save {
                asm.store_pair_d(D(a), D(b), mem);
            } else {
                asm.load_pair_d(D(a), D(b), mem);
            }
        } else if save {
            asm.store_pair_x(X(a), X(b), mem);
        } else {
            asm.load_pair_x(X(a), X(b), mem);
        }
        i += 2;
    }
    if i < regs.len() {
        let mem = save_mem(used_bytes, start + i);
        if fp {
            if save {
                asm.store_double(D(regs[i]), mem);
            } else {
                asm.load_double(D(regs[i]), mem);
            }
        } else if save {
            asm.store(X(regs[i]), mem);
        } else {
            asm.load(X(regs[i]), mem);
        }
    }
}

/// The memory of entry `i` in the save area.
fn save_mem(used_bytes: i32, i: usize) -> Mem {
    Mem {
        base: FP,
        offset: -(used_bytes + 8 * (i as i32 + 1)),
    }
}
