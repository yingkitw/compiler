//! Statement parsing: `let`, `const`, `function`, `if`, `while`, `return`
//! and blocks.

use crate::ast::*;
use crate::lexer::Tok;

use super::Parser;

impl Parser {
    pub(super) fn stmt(&mut self) -> Result<Stmt, String> {
        // Each statement starts a new expression, and so a new tree.
        self.reset_depth();
        match self.peek() {
            Tok::Let | Tok::Const => self.let_decl(),
            Tok::Function => self.func_decl(),
            Tok::If => self.if_stmt(),
            Tok::While => self.while_stmt(),
            Tok::Return => self.return_stmt(),
            Tok::LBrace => {
                self.enter()?;
                let block = self.block();
                self.leave();
                Ok(Stmt::Block(block?))
            }
            _ => {
                let e = self.expr()?;
                self.expect(&Tok::Semi, "';'")?;
                Ok(Stmt::Expr(e))
            }
        }
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
