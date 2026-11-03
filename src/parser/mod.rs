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
}

mod expr;
mod stmt;

#[cfg(test)]
mod tests;
