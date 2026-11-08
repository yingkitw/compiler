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
/// Names the `for (const x of arr)` desugar uses for temporaries. Each
/// desugared loop is wrapped in its own block, so these may be reused.
const FOR_OF_ARR: &str = "__ts_of_arr";
const FOR_OF_I: &str = "__ts_of_i";
const FOR_OF_LEN: &str = "__ts_of_len";

/// `for (const x of expr) body` → a block that binds the iterable once, then
/// walks it with an `int` index and a `while`.
fn desugar_for_of(
    line: u32,
    name: String,
    mutable: bool,
    iterable: Expr,
    body: Vec<Stmt>,
) -> Stmt {
    let arr = Expr::Var(FOR_OF_ARR.into());
    let i = Expr::Var(FOR_OF_I.into());
    let len = Expr::Var(FOR_OF_LEN.into());
    let step = Expr::Assign {
        name: FOR_OF_I.into(),
        value: Box::new(Expr::Bin {
            op: BinOp::Add,
            lhs: Box::new(i.clone()),
            rhs: Box::new(Expr::Num(NumLit::new(1.0, Some(1)))),
        }),
    };
    let mut loop_body = vec![Stmt::Let {
        line,
        name,
        ty: None,
        init: Some(Expr::Index {
            base: Box::new(arr.clone()),
            index: Box::new(i),
        }),
        mutable,
    }];
    loop_body.extend(body);
    loop_body.push(Stmt::Expr {
        line,
        expr: step,
    });
    Stmt::Block {
        line,
        stmts: vec![
            Stmt::Let {
                line,
                name: FOR_OF_ARR.into(),
                ty: None,
                init: Some(iterable),
                mutable: false,
            },
            Stmt::Let {
                line,
                name: FOR_OF_LEN.into(),
                ty: Some(Type::Int),
                init: Some(Expr::Member {
                    base: Box::new(arr.clone()),
                    name: "length".into(),
                }),
                mutable: false,
            },
            Stmt::Let {
                line,
                name: FOR_OF_I.into(),
                ty: Some(Type::Int),
                init: Some(Expr::Num(NumLit::new(0.0, Some(0)))),
                mutable: true,
            },
            Stmt::While {
                line,
                cond: Expr::Bin {
                    op: BinOp::Lt,
                    lhs: Box::new(Expr::Var(FOR_OF_I.into())),
                    rhs: Box::new(len),
                },
                body: loop_body,
            },
        ],
    }
}

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
                let line = self.line();
                self.enter()?;
                let block = self.block();
                self.leave();
                Ok(Stmt::Block {
                    line,
                    stmts: block?,
                })
            }
            _ => {
                let line = self.line();
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
                                "{} invalid assignment target",
                                self.loc()
                            ))
                        }
                    };
                    self.expect(&Tok::Semi, "';'")?;
                    return Ok(Stmt::Expr { line, expr: e });
                }
                self.expect(&Tok::Semi, "';'")?;
                Ok(Stmt::Expr { line, expr: e })
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
        let line = self.line();
        let op = match self.peek() {
            Tok::PlusPlus => BinOp::Add,
            _ => BinOp::Sub,
        };
        self.bump(); // '++' or '--'
        let name = self.expect_ident("a variable")?;
        self.expect(&Tok::Semi, "';'")?;
        Ok(Stmt::Expr {
            line,
            expr: step_op(name, op),
        })
    }

    pub(super) fn block(&mut self) -> Result<Vec<Stmt>, String> {
        self.expect(&Tok::LBrace, "'{'")?;
        self.push_bind_scope();
        let stmts = self.block_stmts()?;
        self.pop_bind_scope();
        Ok(stmts)
    }

    pub(super) fn block_stmts(&mut self) -> Result<Vec<Stmt>, String> {
        let mut stmts = Vec::new();
        while *self.peek() != Tok::RBrace {
            if *self.peek() == Tok::Eof {
                return Err(format!("{} unterminated block", self.loc()));
            }
            stmts.push(self.stmt()?);
        }
        self.bump(); // '}'
        Ok(stmts)
    }

    /// A braced block in expression position: statements plus a final value.
    pub(super) fn block_expr(&mut self) -> Result<Expr, String> {
        let mut stmts = self.block()?;
        let value = match stmts.pop() {
            Some(Stmt::Expr { expr: e, .. }) => e,
            Some(_) => {
                return Err(format!(
                    "{} block expression must end with an expression statement",
                    self.loc()
                ));
            }
            None => {
                return Err(format!(
                    "{} block expression cannot be empty",
                    self.loc()
                ));
            }
        };
        Ok(Expr::BlockExpr {
            stmts,
            value: Box::new(value),
        })
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
        let start_line = self.line();
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
                "{} '{}' needs a type annotation or an initializer",
                self.loc(),
                name
            ));
        }
        let (_, col) = self.prev_span();
        self.bind_name(&name, start_line, col)?;
        Ok(Stmt::Let {
            line: start_line,
            name,
            ty,
            init,
            mutable,
        })
    }

    pub(super) fn func_decl(&mut self) -> Result<Stmt, String> {
        let start_line = self.line();
        self.bump(); // 'function'
        let name = self.expect_ident("function name")?;
        let (_, col) = self.prev_span();
        self.bind_name(&name, start_line, col)?;
        self.expect(&Tok::LParen, "'('")?;
        let mut params = Vec::new();
        let mut param_bindings = Vec::new();
        while *self.peek() != Tok::RParen {
            let pname = self.expect_ident("parameter name")?;
            let (pline, pcol) = self.prev_span();
            self.expect(&Tok::Colon, "':'")?;
            let pty = self.value_type()?;
            param_bindings.push((pname.clone(), pline, pcol));
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
        self.push_bind_scope();
        for (pname, pline, pcol) in param_bindings {
            self.bind_name(&pname, pline, pcol)?;
        }
        self.expect(&Tok::LBrace, "'{'")?;
        let body = self.block_stmts()?;
        self.pop_bind_scope();
        Ok(Stmt::Func {
            line: start_line,
            name,
            params,
            ret,
            body,
        })
    }

    pub(super) fn if_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
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
        Ok(Stmt::If {
            line,
            cond,
            then,
            els,
        })
    }

    pub(super) fn while_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.bump(); // 'while'
        self.expect(&Tok::LParen, "'('")?;
        let cond = self.expr()?;
        self.expect(&Tok::RParen, "')'")?;
        let body = self.stmt_or_block()?;
        Ok(Stmt::While { line, cond, body })
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
        let line = self.line();
        self.bump(); // 'for'
        self.expect(&Tok::LParen, "'('")?;

        // The init: `let`/`const` (C-style or `for-of`), an expression, or none.
        let init = if matches!(*self.peek(), Tok::Let | Tok::Const) {
            let mutable = *self.peek() == Tok::Let;
            self.bump();
            let name = self.expect_ident("variable name")?;
            if self.eat(&Tok::Of) {
                let iterable = self.expr()?;
                self.expect(&Tok::RParen, "')'")?;
                let body = self.stmt_or_block()?;
                return Ok(desugar_for_of(line, name, mutable, iterable, body));
            }
            let ty = if self.eat(&Tok::Colon) {
                Some(self.value_type()?)
            } else {
                None
            };
            let init_expr = if self.eat(&Tok::Assign) {
                Some(self.expr()?)
            } else {
                None
            };
            self.expect(&Tok::Semi, "';'")?;
            if ty.is_none() && init_expr.is_none() {
                return Err(format!(
                    "{} '{}' needs a type annotation or an initializer",
                    self.loc(),
                    name
                ));
            }
            Some(Stmt::Let {
                line,
                name,
                ty,
                init: init_expr,
                mutable,
            })
        } else if self.eat(&Tok::Semi) {
            None
        } else {
            let e = self.expr()?;
            self.expect(&Tok::Semi, "';'")?;
            Some(Stmt::Expr { line, expr: e })
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
            loop_body.push(Stmt::Expr {
                line,
                expr: step,
            });
        }

        match init {
            // No init: the desugar is the `while` on its own, without a
            // block wrapper around nothing.
            None => Ok(Stmt::While {
                line,
                cond,
                body: loop_body,
            }),
            Some(init) => Ok(Stmt::Block {
                line,
                stmts: vec![
                    init,
                    Stmt::While {
                        line,
                        cond,
                        body: loop_body,
                    },
                ],
            }),
        }
    }

    pub(super) fn return_stmt(&mut self) -> Result<Stmt, String> {
        let line = self.line();
        self.bump(); // 'return'
        if self.eat(&Tok::Semi) {
            Ok(Stmt::Return { line, value: None })
        } else {
            let e = self.expr()?;
            self.expect(&Tok::Semi, "';'")?;
            Ok(Stmt::Return {
                line,
                value: Some(e),
            })
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
                "{} a variable or a parameter cannot have type 'void'",
                self.loc()
            ));
        }
        Ok(ty)
    }

    pub(super) fn type_annotation(&mut self) -> Result<Type, String> {
        let name = self.expect_ident("type")?;
        let base = match name.as_str() {
            "number" => Type::Number,
            "int" => Type::Int,
            "boolean" => Type::Boolean,
            "string" => Type::String,
            "void" => Type::Void,
            _ => {
                return Err(format!(
                    "{} unsupported type '{}' (supported: number, int, boolean, string, void)",
                    self.loc(),
                    name
                ));
            }
        };
        if self.eat(&Tok::LBracket) {
            self.expect(&Tok::RBracket, "']'")?;
            Ok(match base {
                Type::Number => Type::NumberArray,
                Type::Boolean => Type::BooleanArray,
                other => {
                    return Err(format!(
                        "{} only number[] and boolean[] are supported, not {}",
                        self.loc(),
                        other
                    ));
                }
            })
        } else {
            Ok(base)
        }
    }

    // Expressions, lowest precedence first.
}
