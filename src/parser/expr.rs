//! Expression parsing: one method per precedence level, from the loosest
//! (`||`) to the tightest (a primary).

use crate::ast::*;
use crate::lexer::Tok;

use super::Parser;

impl Parser {
    pub(super) fn expr(&mut self) -> Result<Expr, String> {
        self.assignment()
    }

    pub(super) fn assignment(&mut self) -> Result<Expr, String> {
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

    pub(super) fn logic_or(&mut self) -> Result<Expr, String> {
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

    pub(super) fn logic_and(&mut self) -> Result<Expr, String> {
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

    pub(super) fn equality(&mut self) -> Result<Expr, String> {
        let mut lhs = self.comparison()?;
        loop {
            let op = match self.peek() {
                Tok::EqEq => BinOp::Eq,
                Tok::BangEq => BinOp::Ne,
                _ => break,
            };
            self.bump();
            self.deepen()?;
            let rhs = self.comparison()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn comparison(&mut self) -> Result<Expr, String> {
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
            self.deepen()?;
            let rhs = self.term()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn term(&mut self) -> Result<Expr, String> {
        let mut lhs = self.factor()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.bump();
            self.deepen()?;
            let rhs = self.factor()?;
            lhs = Expr::Bin {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            };
        }
        Ok(lhs)
    }

    pub(super) fn factor(&mut self) -> Result<Expr, String> {
        let mut lhs = self.unary()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Rem,
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

    pub(super) fn unary(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Tok::Bang => {
                self.bump();
                self.enter()?;
                self.deepen()?;
                    let e = self.unary()?;
                    self.leave();
                Ok(Expr::Un {
                    op: UnOp::Not,
                    expr: Box::new(e),
                })
            }
            Tok::Minus => {
                self.bump();
                self.enter()?;
                self.deepen()?;
                    let e = self.unary()?;
                    self.leave();
                Ok(Expr::Un {
                    op: UnOp::Neg,
                    expr: Box::new(e),
                })
            }
            _ => self.call(),
        }
    }

    pub(super) fn call(&mut self) -> Result<Expr, String> {
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

    pub(super) fn primary(&mut self) -> Result<Expr, String> {
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
                self.enter()?;
                let e = self.expr()?;
                self.leave();
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
