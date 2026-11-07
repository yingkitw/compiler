//! Statement parsing: `let`, `const`, `function`, `if`, `while`, `for`,
//! `return` and blocks.

use crate::ast::*;
use crate::lexer::Tok;

use super::Parser;

/// The statement `++x` and `x++` desugar to: `x = x + 1` (or `x = x - 1`).
///
/// The desugar is in the parser, so the AST, the code generator and the
/// type rules need no new case at all — the assignment's own rules apply
/// unchanged: the target must be a variable, it must not be a `const`, and
/// the arithmetic must type-check.
fn step_op(name: String, op: BinOp) -> Expr {
    Expr::Assign {
        name: name.clone(),
        value: Box::new(Expr::Bin {
            op,
            lhs: Box::new(Expr::Var(name)),
            rhs: Box::new(Expr::Num(NumLit::new(1.0, Some(1)))),
        }),
    }
}

impl Parser {
    pub(super) fn stmt(&mut self) -> Result<Stmt, String> {
        // Each statement starts a new expression, and so a new tree.
        self.reset_depth();
        match self.peek() {
            Tok::Let | Tok::Const => self.let_decl(),
            Tok::Function => self.func_decl(),
            Tok::If => self.if_stmt(),
            Tok::While => self.while_stmt(),
            Tok::For => self.for_stmt(),
            Tok::Return => self.return_stmt(),
            Tok::PlusPlus | Tok::MinusMinus => self.step_stmt(),
            Tok::LBrace => {
                self.enter()?;
                let block = self.block();
                self.leave();
                Ok(Stmt::Block(block?))
            }
            _ => {
                let e = self.expr()?;
                // The postfix form of `++` and `--`.
                let step = match self.peek() {
                    Tok::PlusPlus => Some(BinOp::Add),
                    Tok::MinusMinus => Some(BinOp::Sub),
                    _ => None,
                };
                if let Some(op) = step {
                    self.bump();
                    let e = match e {
                        Expr::Var(name) => step_op(name, op),
                        _ => {
                            return Err(format!(
                                "line {}: invalid assignment target",
                                self.line()
                            ))
                        }
                    };
                    self.expect(&Tok::Semi, "';'")?;
                    return Ok(Stmt::Expr(e));
                }
                self.expect(&Tok::Semi, "';'")?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    /// `++x` — the prefix form of the increment and decrement operators.
    ///
    /// Both forms are statements rather than expressions: TypeScript's
    /// postfix `x++` yields the *old* value of `x`, which this subset does
    /// not model, so they are parsed only where a statement is expected.
    /// Their meaning is the assignment they desugar to — `x = x + 1`.
    fn step_stmt(&mut self) -> Result<Stmt, String> {
        let op = match self.peek() {
            Tok::PlusPlus => BinOp::Add,
            _ => BinOp::Sub,
        };
        self.bump(); // '++' or '--'
        let name = self.expect_ident("a variable")?;
        self.expect(&Tok::Semi, "';'")?;
        Ok(Stmt::Expr(step_op(name, op)))
    }

    pub(super) fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Tok::LBrace, "'{'")?;
        let mut stmts = Vec::new();
        while *self.peek() != Tok::RBrace {
            if *self.peek() == Tok::Eof {
                return Err(format!("line {}: unterminated block", self.line()));
            }
            stmts.push(self.stmt()?);
        }
        self.bump(); // '}'
        Ok(stmts)
    }

    /// A braced block, or a single statement wrapped in a block.
    pub(super) fn stmt_or_block(&mut self) -> Result<Vec<Stmt>, String> {
        if *self.peek() == Tok::LBrace {
            self.block()
        } else {
            Ok(vec![self.stmt()?])
        }
    }

    pub(super) fn let_decl(&mut self) -> Result<Stmt, String> {
        let mutable = *self.peek() == Tok::Let;
        self.bump();
        let name = self.expect_ident("variable name")?;
        let ty = if self.eat(&Tok::Colon) {
            Some(self.value_type()?)
        } else {
            None
        };
        let init = if self.eat(&Tok::Assign) {
            Some(self.expr()?)
        } else {
            None
        };
        self.expect(&Tok::Semi, "';'")?;
        if ty.is_none() && init.is_none() {
            return Err(format!(
                "line {}: '{}' needs a type annotation or an initializer",
                self.line(),
                name
            ));
        }
        Ok(Stmt::Let { name, ty, init, mutable })
    }

    pub(super) fn func_decl(&mut self) -> Result<Stmt, String> {
        self.bump(); // 'function'
        let name = self.expect_ident("function name")?;
        self.expect(&Tok::LParen, "'('")?;
        let mut params = Vec::new();
        while *self.peek() != Tok::RParen {
            let pname = self.expect_ident("parameter name")?;
            self.expect(&Tok::Colon, "':'")?;
            let pty = self.value_type()?;
            params.push((pname, pty));
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(&Tok::RParen, "')'")?;
        let ret = if self.eat(&Tok::Colon) {
            Some(self.type_annotation()?)
        } else {
            None
        };
        let body = self.block()?;
        Ok(Stmt::Func { name, params, ret, body })
    }

    pub(super) fn if_stmt(&mut self) -> Result<Stmt, String> {
        self.bump(); // 'if'
        self.expect(&Tok::LParen, "'('")?;
        let cond = self.expr()?;
        self.expect(&Tok::RParen, "')'")?;
        let then = self.stmt_or_block()?;
        let els = if self.eat(&Tok::Else) {
            self.stmt_or_block()?
        } else {
            Vec::new()
        };
        Ok(Stmt::If { cond, then, els })
    }

    pub(super) fn while_stmt(&mut self) -> Result<Stmt, String> {
        self.bump(); // 'while'
        self.expect(&Tok::LParen, "'('")?;
        let cond = self.expr()?;
        self.expect(&Tok::RParen, "')'")?;
        let body = self.stmt_or_block()?;
        Ok(Stmt::While { cond, body })
    }

    /// A C-style `for` loop, desugared into the statements the language
    /// already has: the init runs once, then a `while` over the condition
    /// whose body is the loop body followed by the step.
    ///
    /// ```text
    /// for (let i = 0; i < n; i = i + 1) { body }
    /// ```
    ///
    /// becomes
    ///
    /// ```text
    /// { let i = 0; while (i < n) { body i = i + 1; } }
    /// ```
    ///
    /// The desugar is why the init lives in its own block: two sequential
    /// loops may each declare `let i`, and shadowing in the loop body stays
    /// legal. Desugaring in the parser means the code generator needs no
    /// knowledge of `for` at all — there is no `Stmt::For`.
    ///
    /// `break` and `continue` do not exist yet, so nothing in a loop body
    /// can target the loop rather than the `while` it becomes. If they are
    /// ever added, they must be taught about this desugar: a `continue`
    /// in the body still has to run the step.
    pub(super) fn for_stmt(&mut self) -> Result<Stmt, String> {
        self.bump(); // 'for'
        self.expect(&Tok::LParen, "'('")?;

        // The init: a `let`/`const` declaration (which consumes its own
        // `;`), an assignment expression, or nothing.
        let init = if *self.peek() == Tok::Let || *self.peek() == Tok::Const {
            Some(self.let_decl()?)
        } else if self.eat(&Tok::Semi) {
            None
        } else {
            let e = self.expr()?;
            self.expect(&Tok::Semi, "';'")?;
            Some(Stmt::Expr(e))
        };

        // The condition: absent means `true`, the same as `while (true)`.
        let cond = if self.eat(&Tok::Semi) {
            Expr::Bool(true)
        } else {
            let e = self.expr()?;
            self.expect(&Tok::Semi, "';'")?;
            e
        };

        // The step: an expression, or nothing.
        let step = if *self.peek() == Tok::RParen {
            None
        } else {
            Some(self.expr()?)
        };
        self.expect(&Tok::RParen, "')'")?;

        let body = self.stmt_or_block()?;

        let mut loop_body = body;
        if let Some(step) = step {
            loop_body.push(Stmt::Expr(step));
        }

        match init {
            // No init: the desugar is the `while` on its own, without a
            // block wrapper around nothing.
            None => Ok(Stmt::While { cond, body: loop_body }),
            Some(init) => Ok(Stmt::Block(vec![init, Stmt::While { cond, body: loop_body }])),
        }
    }

    pub(super) fn return_stmt(&mut self) -> Result<Stmt, String> {
        self.bump(); // 'return'
        if self.eat(&Tok::Semi) {
            Ok(Stmt::Return(None))
        } else {
            let e = self.expr()?;
            self.expect(&Tok::Semi, "';'")?;
            Ok(Stmt::Return(Some(e)))
        }
    }

    /// A type for something that holds a value.
    ///
    /// `void` is the type of "no value", and a variable or a parameter
    /// always holds one, so it is not allowed here.
    pub(super) fn value_type(&mut self) -> Result<Type, String> {
        let ty = self.type_annotation()?;
        if ty == Type::Void {
            return Err(format!(
                "line {}: a variable or a parameter cannot have type 'void'",
                self.line()
            ));
        }
        Ok(ty)
    }

    pub(super) fn type_annotation(&mut self) -> Result<Type, String> {
        let name = self.expect_ident("type")?;
        match name.as_str() {
            "number" => Ok(Type::Number),
            "int" => Ok(Type::Int),
            "boolean" => Ok(Type::Boolean),
            "void" => Ok(Type::Void),
            _ => Err(format!(
                "line {}: unsupported type '{}' (supported: number, int, boolean, void)",
                self.line(),
                name
            )),
        }
    }

    // Expressions, lowest precedence first.
}
