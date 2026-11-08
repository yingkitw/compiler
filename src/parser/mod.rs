//! Recursive-descent parser for the TypeScript subset.

use std::collections::HashSet;

use crate::ast::*;
use crate::lexer::{source_loc, Tok, Token};

pub fn parse(toks: Vec<Token>) -> Result<Program, String> {
    let mut p = Parser {
        toks,
        pos: 0,
        nesting: 0,
        depth: 0,
        scopes: vec![HashSet::new()],
        warnings: Vec::new(),
    };
    let mut stmts = Vec::new();
    while *p.peek() != Tok::Eof {
        stmts.push(p.stmt()?);
    }
    Ok(Program {
        stmts,
        warnings: p.warnings,
    })
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
    /// How deeply the expression or statement being parsed is nested. This
    /// is the parser's own recursion depth: parentheses nest the parse
    /// without nesting the tree.
    nesting: usize,
    /// How deep the expression tree being built is.
    ///
    /// The tree grows without the parser recursing: `a + b + c + ...` is
    /// built left-associated, so each term makes the tree one level deeper.
    /// Every stage that walks the tree — the rewrites, the code generator,
    /// and the tree's own destructor — recurses over it, so its depth has to
    /// be bounded.
    depth: usize,
    /// Names in scope, innermost last — used for shadowing warnings.
    scopes: Vec<HashSet<String>>,
    warnings: Vec<String>,
}

impl Parser {
    pub(super) fn push_bind_scope(&mut self) {
        self.scopes.push(HashSet::new());
    }

    pub(super) fn pop_bind_scope(&mut self) {
        self.scopes.pop();
    }

    /// Record a binding: same-scope redeclaration is an error; hiding an
    /// outer name is a warning.
    pub(super) fn bind_name(&mut self, name: &str, line: u32, col: u32) -> Result<(), String> {
        if self
            .scopes
            .last()
            .expect("a scope is open")
            .contains(name)
        {
            return Err(format!(
                "{}: '{name}' is already declared in this scope",
                source_loc(line, col)
            ));
        }
        let shadows = self
            .scopes
            .iter()
            .rev()
            .skip(1)
            .any(|outer| outer.contains(name));
        if shadows {
            self.warnings.push(format!(
                "{}: '{name}' shadows an outer declaration",
                source_loc(line, col)
            ));
        }
        self.scopes
            .last_mut()
            .expect("a scope is open")
            .insert(name.to_string());
        Ok(())
    }


    /// Enter a nested expression or statement.
    ///
    /// The parser recurses, so a program with a hundred thousand nested
    /// parentheses would take the compiler down with it. The limit turns
    /// that into a diagnostic.
    fn enter(&mut self) -> Result<(), String> {
        self.nesting += 1;
        if self.nesting > MAX_DEPTH {
            return Err("expression or statement nested too deeply".to_string());
        }
        Ok(())
    }

    /// Leave a nested expression or statement.
    fn leave(&mut self) {
        self.nesting -= 1;
    }

    /// Grow the expression tree by one level.
    ///
    /// The level stays: the tree keeps it, and the stages that walk the tree
    /// recurse through it.
    fn deepen(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_AST_DEPTH {
            return Err("expression nested too deeply".to_string());
        }
        Ok(())
    }

    /// Start a new expression, at the top level of the tree.
    fn reset_depth(&mut self) {
        self.depth = 0;
    }

    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn line(&self) -> u32 {
        self.toks[self.pos].line
    }

    fn col(&self) -> u32 {
        self.toks[self.pos].col
    }

    fn loc(&self) -> String {
        format!("{}:", source_loc(self.line(), self.col()))
    }

    pub(super) fn prev_span(&self) -> (u32, u32) {
        let i = self.pos.saturating_sub(1);
        (self.toks[i].line, self.toks[i].col)
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
                "{}: expected {}, found {:?}",
                source_loc(self.line(), self.col()),
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
                "{}: expected {}, found {:?}",
                source_loc(self.line(), self.col()),
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
