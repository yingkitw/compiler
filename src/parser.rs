//! Recursive-descent parser for the TypeScript subset.

use crate::ast::*;
use crate::lexer::{Tok, Token};

pub fn parse(toks: Vec<Token>) -> Result<Program, String> {
    let mut p = Parser { toks, pos: 0 };
    let mut stmts = Vec::new();
    while *p.peek() != Tok::Eof {
        stmts.push(p.stmt()?);
    }
    Ok(Program { stmts })
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn line(&self) -> u32 {
        self.toks[self.pos].line
    }

    fn bump(&mut self) -> Tok {
        let t = self.toks[self.pos].tok.clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.bump();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, t: &Tok, what: &str) -> Result<(), String> {
        if self.eat(t) {
            Ok(())
        } else {
            Err(format!(
                "line {}: expected {}, found {:?}",
                self.line(),
                what,
                self.peek()
            ))
        }
    }

    fn expect_ident(&mut self, what: &str) -> Result<String, String> {
        match self.peek().clone() {
            Tok::Ident(name) => {
                self.bump();
                Ok(name)
            }
            other => Err(format!(
                "line {}: expected {}, found {:?}",
                self.line(),
                what,
                other
            )),
        }
    }

    fn stmt(&mut self) -> Result<Stmt, String> {
        match self.peek() {
            Tok::Let | Tok::Const => self.let_decl(),
            Tok::Function => self.func_decl(),
            Tok::If => self.if_stmt(),
            Tok::While => self.while_stmt(),
            Tok::Return => self.return_stmt(),
            Tok::LBrace => Ok(Stmt::Block(self.block()?)),
            _ => {
                let e = self.expr()?;
                self.expect(&Tok::Semi, "';'")?;
                Ok(Stmt::Expr(e))
            }
        }
    }

    fn block(&mut self) -> Result<Vec<Stmt>, String> {
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
    fn stmt_or_block(&mut self) -> Result<Vec<Stmt>, String> {
        if *self.peek() == Tok::LBrace {
            self.block()
        } else {
            Ok(vec![self.stmt()?])
        }
    }

    fn let_decl(&mut self) -> Result<Stmt, String> {
        let mutable = *self.peek() == Tok::Let;
        self.bump();
        let name = self.expect_ident("variable name")?;
        let ty = if self.eat(&Tok::Colon) {
            Some(self.type_annotation()?)
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

    fn func_decl(&mut self) -> Result<Stmt, String> {
        self.bump(); // 'function'
        let name = self.expect_ident("function name")?;
        self.expect(&Tok::LParen, "'('")?;
        let mut params = Vec::new();
        while *self.peek() != Tok::RParen {
            let pname = self.expect_ident("parameter name")?;
            self.expect(&Tok::Colon, "':'")?;
            let pty = self.type_annotation()?;
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

    fn if_stmt(&mut self) -> Result<Stmt, String> {
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

    fn while_stmt(&mut self) -> Result<Stmt, String> {
        self.bump(); // 'while'
        self.expect(&Tok::LParen, "'('")?;
        let cond = self.expr()?;
        self.expect(&Tok::RParen, "')'")?;
        let body = self.stmt_or_block()?;
        Ok(Stmt::While { cond, body })
    }

    fn return_stmt(&mut self) -> Result<Stmt, String> {
        self.bump(); // 'return'
        if self.eat(&Tok::Semi) {
            Ok(Stmt::Return(None))
        } else {
            let e = self.expr()?;
            self.expect(&Tok::Semi, "';'")?;
            Ok(Stmt::Return(Some(e)))
        }
    }

    fn type_annotation(&mut self) -> Result<Type, String> {
        let name = self.expect_ident("type")?;
        match name.as_str() {
            "number" => Ok(Type::Number),
            "boolean" => Ok(Type::Boolean),
            "void" => Ok(Type::Void),
            _ => Err(format!(
                "line {}: unsupported type '{}' (supported: number, boolean, void)",
                self.line(),
                name
            )),
        }
    }

    // Expressions, lowest precedence first.

    fn expr(&mut self) -> Result<Expr, String> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<Expr, String> {
        let lhs = self.logic_or()?;
        if self.eat(&Tok::Assign) {
            let value = self.assignment()?;
            return match lhs {
                Expr::Var(name) => Ok(Expr::Assign {
                    name,
                    value: Box::new(value),
                }),
                _ => Err(format!(
                    "line {}: invalid assignment target",
                    self.line()
                )),
            };
        }
        Ok(lhs)
    }

    fn logic_or(&mut self) -> Result<Expr, String> {
        let mut lhs = self.logic_and()?;
        while self.eat(&Tok::OrOr) {
            let rhs = self.logic_and()?;
            lhs = Expr::Bin {
                op: BinOp::Or,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn logic_and(&mut self) -> Result<Expr, String> {
        let mut lhs = self.equality()?;
        while self.eat(&Tok::AndAnd) {
            let rhs = self.equality()?;
            lhs = Expr::Bin {
                op: BinOp::And,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn equality(&mut self) -> Result<Expr, String> {
        let mut lhs = self.comparison()?;
        loop {
            let op = match self.peek() {
                Tok::EqEq => BinOp::Eq,
                Tok::BangEq => BinOp::Ne,
                _ => break,
            };
            self.bump();
            let rhs = self.comparison()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn comparison(&mut self) -> Result<Expr, String> {
        let mut lhs = self.term()?;
        loop {
            let op = match self.peek() {
                Tok::Lt => BinOp::Lt,
                Tok::LtEq => BinOp::Le,
                Tok::Gt => BinOp::Gt,
                Tok::GtEq => BinOp::Ge,
                _ => break,
            };
            self.bump();
            let rhs = self.term()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn term(&mut self) -> Result<Expr, String> {
        let mut lhs = self.factor()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            let rhs = self.factor()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn factor(&mut self) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                _ => break,
            };
            self.bump();
            let rhs = self.unary()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Tok::Bang => {
                self.bump();
                let e = self.unary()?;
                Ok(Expr::Un {
                    op: UnOp::Not,
                    expr: Box::new(e),
                })
            }
            Tok::Minus => {
                self.bump();
                let e = self.unary()?;
                Ok(Expr::Un {
                    op: UnOp::Neg,
                    expr: Box::new(e),
                })
            }
            _ => self.call(),
        }
    }

    fn call(&mut self) -> Result<Expr, String> {
        let mut e = self.primary()?;
        while self.eat(&Tok::LParen) {
            let mut args = Vec::new();
            while *self.peek() != Tok::RParen {
                args.push(self.expr()?);
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(&Tok::RParen, "')'")?;
            e = match e {
                Expr::Var(name) => Expr::Call { name, args },
                _ => {
                    return Err(format!(
                        "line {}: can only call named functions",
                        self.line()
                    ))
                }
            };
        }
        Ok(e)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Tok::Num(n) => {
                self.bump();
                Ok(Expr::Num(n))
            }
            Tok::True => {
                self.bump();
                Ok(Expr::Bool(true))
            }
            Tok::False => {
                self.bump();
                Ok(Expr::Bool(false))
            }
            Tok::Ident(name) => {
                self.bump();
                Ok(Expr::Var(name))
            }
            Tok::LParen => {
                self.bump();
                let e = self.expr()?;
                self.expect(&Tok::RParen, "')'")?;
                Ok(e)
            }
            other => Err(format!(
                "line {}: expected an expression, found {:?}",
                self.line(),
                other
            )),
        }
    }
}
