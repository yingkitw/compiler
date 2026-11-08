//! Expression parsing: one method per precedence level, from the loosest
//! (assignment) to the tightest (a primary). The conditional `?:` sits between
//! assignment and `||`, as in TypeScript.

use crate::ast::*;
use crate::lexer::Tok;

use super::Parser;

impl Parser {
    pub(super) fn expr(&mut self) -> Result<Expr, String> {
        self.assignment()
    }

    pub(super) fn assignment(&mut self) -> Result<Expr, String> {
        let lhs = self.conditional()?;
        // A compound assignment is sugar: `x += e` is `x = x + e`. The
        // desugar is here rather than in the code generator, so the AST and
        // the type rules need no new case at all — the assignment's own
        // rules (the target must be a variable, the types must match) apply
        // unchanged.
        let op = match self.peek() {
            Tok::PlusEq => Some(BinOp::Add),
            Tok::MinusEq => Some(BinOp::Sub),
            Tok::StarEq => Some(BinOp::Mul),
            Tok::SlashEq => Some(BinOp::Div),
            Tok::PercentEq => Some(BinOp::Rem),
            Tok::AndEq => Some(BinOp::And),
            Tok::OrEq => Some(BinOp::Or),
            _ => None,
        };
        if let Some(op) = op {
            self.bump();
            self.enter()?;
            self.deepen()?;
            let value = self.assignment()?;
            self.leave();
            return match lhs {
                Expr::Var(name) => Ok(Expr::Assign {
                    name: name.clone(),
                    value: Box::new(Expr::Bin {
                        op,
                        lhs: Box::new(Expr::Var(name)),
                        rhs: Box::new(value),
                    }),
                }),
                _ => Err(format!(
                    "{} invalid assignment target",
                    self.loc()
                )),
            };
        }
        if self.eat(&Tok::Assign) {
            let value = self.assignment()?;
            return match lhs {
                Expr::Var(name) => Ok(Expr::Assign {
                    name,
                    value: Box::new(value),
                }),
                _ => Err(format!(
                    "{} invalid assignment target",
                    self.loc()
                )),
            };
        }
        Ok(lhs)
    }

    /// `logicalOr ? assignment : conditional` — the else branch is
    /// right-associative for nested ternaries.
    pub(super) fn conditional(&mut self) -> Result<Expr, String> {
        let cond = self.logic_or()?;
        if self.eat(&Tok::Question) {
            self.enter()?;
            self.deepen()?;
            let then_expr = self.assignment()?;
            self.expect(&Tok::Colon, "':'")?;
            let else_expr = self.conditional()?;
            self.leave();
            return Ok(Expr::Cond {
                cond: Box::new(cond),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });
        }
        Ok(cond)
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
            // An `as` cast has the precedence of the relational operators
            // and wraps the whole expression accumulated to its left —
            // `1 + 2 as int` is `(1 + 2) as int` — while a tighter operator
            // may still follow the cast, so `x as int + 1` is
            // `(x as int) + 1`. Both are TypeScript's behaviour.
            if self.eat(&Tok::As) {
                let ty = self.type_annotation()?;
                if ty == Type::Void {
                    return Err(format!("{} cannot cast to 'void'", self.loc()));
                }
                lhs = Expr::Cast { expr: Box::new(lhs), ty };
                lhs = self.tight_tail(lhs)?;
                continue;
            }
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

    /// The tighter operators a cast may be followed by: multiplication and
    /// division before addition and subtraction, so `x as int + 1 * 2` is
    /// `(x as int) + (1 * 2)`.
    fn tight_tail(&mut self, mut lhs: Expr) -> Result<Expr, String> {
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
        loop {
            if self.eat(&Tok::LParen) {
                let mut args = Vec::new();
                while *self.peek() != Tok::RParen {
                    args.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(&Tok::RParen, "')'")?;
                e = match e {
                    Expr::Var(name) => Expr::Call {
                        name,
                        receiver: None,
                        args,
                    },
                    Expr::Member { base, name } => Expr::Call {
                        name,
                        receiver: Some(base),
                        args,
                    },
                    _ => {
                        return Err(format!(
                            "{} can only call named functions or methods",
                            self.loc()
                        ));
                    }
                };
            } else if self.eat(&Tok::LBracket) {
                self.enter()?;
                let index = self.expr()?;
                self.leave();
                self.expect(&Tok::RBracket, "']'")?;
                e = Expr::Index {
                    base: Box::new(e),
                    index: Box::new(index),
                };
            } else if self.eat(&Tok::Dot) {
                let name = self.expect_ident("field name")?;
                e = Expr::Member {
                    base: Box::new(e),
                    name,
                };
            } else {
                break;
            }
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
            Tok::Str(s) => {
                self.bump();
                Ok(Expr::Str(s))
            }
            Tok::Ident(name) => {
                self.bump();
                Ok(Expr::Var(name))
            }
            Tok::LParen => {
                let mut opens = 0usize;
                while *self.peek() == Tok::LParen {
                    self.bump();
                    opens += 1;
                    if opens > MAX_DEPTH {
                        return Err("expression or statement nested too deeply".to_string());
                    }
                }
                let e = self.expr()?;
                for _ in 0..opens {
                    self.expect(&Tok::RParen, "')'")?;
                }
                Ok(e)
            }
            Tok::LBracket => {
                self.bump();
                let mut elems = Vec::new();
                if !self.eat(&Tok::RBracket) {
                    loop {
                        elems.push(self.expr()?);
                        if self.eat(&Tok::RBracket) {
                            break;
                        }
                        self.expect(&Tok::Comma, "','")?;
                    }
                }
                Ok(Expr::Array(elems))
            }
            Tok::LBrace => self.block_expr(),
            other => Err(format!(
                "{} expected an expression, found {:?}",
                self.loc(),
                other
            )),
        }
    }
}
