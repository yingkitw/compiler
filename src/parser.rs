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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;

    fn parse_src(src: &str) -> Program {
        parse(lexer::lex(src).expect("lex failed")).expect("parse failed")
    }

    fn parse_err(src: &str) -> String {
        parse(lexer::lex(src).expect("lex failed")).expect_err("expected a parse error")
    }

    /// The single top-level expression statement of a program, if there is one.
    fn only_expr(src: &str) -> Expr {
        let prog = parse_src(src);
        assert_eq!(prog.stmts.len(), 1, "expected exactly one statement");
        match &prog.stmts[0] {
            Stmt::Expr(e) => e.clone(),
            other => panic!("expected an expression statement, got {other:?}"),
        }
    }

    #[test]
    fn parses_let_with_type_annotation() {
        let prog = parse_src("let x: number = 1;");
        match &prog.stmts[0] {
            Stmt::Let { name, ty, init, mutable } => {
                assert_eq!(name, "x");
                assert_eq!(ty, &Some(Type::Number));
                assert!(init.is_some());
                assert!(mutable);
            }
            other => panic!("expected a let, got {other:?}"),
        }
    }

    #[test]
    fn parses_const_as_immutable() {
        let prog = parse_src("const x = 1;");
        match &prog.stmts[0] {
            Stmt::Let { mutable, ty, init, .. } => {
                assert!(!mutable);
                assert_eq!(ty, &None);
                assert!(init.is_some());
            }
            other => panic!("expected a let, got {other:?}"),
        }
    }

    #[test]
    fn parses_function_declaration() {
        let prog = parse_src("function f(a: number, b: boolean): void { }");
        match &prog.stmts[0] {
            Stmt::Func { name, params, ret, .. } => {
                assert_eq!(name, "f");
                assert_eq!(params.len(), 2);
                assert_eq!(params[0], ("a".to_string(), Type::Number));
                assert_eq!(params[1], ("b".to_string(), Type::Boolean));
                assert_eq!(ret, &Some(Type::Void));
            }
            other => panic!("expected a function, got {other:?}"),
        }
    }

    #[test]
    fn function_without_return_type_defaults_to_void() {
        let prog = parse_src("function f() { }");
        match &prog.stmts[0] {
            Stmt::Func { ret, params, .. } => {
                assert_eq!(ret, &None);
                assert!(params.is_empty());
            }
            other => panic!("expected a function, got {other:?}"),
        }
    }

    #[test]
    fn parses_if_else() {
        let prog = parse_src("if (x) { y(); } else { z(); }");
        match &prog.stmts[0] {
            Stmt::If { then, els, .. } => {
                assert_eq!(then.len(), 1);
                assert_eq!(els.len(), 1);
            }
            other => panic!("expected an if, got {other:?}"),
        }
    }

    #[test]
    fn parses_if_without_else() {
        let prog = parse_src("if (x) { y(); }");
        match &prog.stmts[0] {
            Stmt::If { then, els, .. } => {
                assert_eq!(then.len(), 1);
                assert!(els.is_empty(), "an absent else must be empty");
            }
            other => panic!("expected an if, got {other:?}"),
        }
    }

    #[test]
    fn parses_while() {
        let prog = parse_src("while (x) { y(); }");
        match &prog.stmts[0] {
            Stmt::While { body, .. } => assert_eq!(body.len(), 1),
            other => panic!("expected a while, got {other:?}"),
        }
    }

    #[test]
    fn parses_return_with_and_without_value() {
        let prog = parse_src("function f(): number { return 1; }");
        match &prog.stmts[0] {
            Stmt::Func { body, .. } => match &body[0] {
                Stmt::Return(Some(_)) => {}
                other => panic!("expected a return with a value, got {other:?}"),
            },
            other => panic!("expected a function, got {other:?}"),
        }
        let prog = parse_src("function f(): void { return; }");
        match &prog.stmts[0] {
            Stmt::Func { body, .. } => match &body[0] {
                Stmt::Return(None) => {}
                other => panic!("expected a bare return, got {other:?}"),
            },
            other => panic!("expected a function, got {other:?}"),
        }
    }

    #[test]
    fn multiplication_binds_tighter_than_addition() {
        // 1 + 2 * 3 must parse as Add(1, Mul(2, 3)).
        match only_expr("1 + 2 * 3;") {
            Expr::Bin { op: BinOp::Add, rhs, .. } => match *rhs {
                Expr::Bin { op: BinOp::Mul, .. } => {}
                other => panic!("expected a multiplication on the right, got {other:?}"),
            },
            other => panic!("expected an addition, got {other:?}"),
        }
    }

    #[test]
    fn comparison_binds_tighter_than_equality() {
        // a == b < c must parse as Eq(a, Lt(b, c)).
        match only_expr("a == b < c;") {
            Expr::Bin { op: BinOp::Eq, rhs, .. } => match *rhs {
                Expr::Bin { op: BinOp::Lt, .. } => {}
                other => panic!("expected a comparison on the right, got {other:?}"),
            },
            other => panic!("expected an equality, got {other:?}"),
        }
    }

    #[test]
    fn and_binds_tighter_than_or() {
        match only_expr("a || b && c;") {
            Expr::Bin { op: BinOp::Or, rhs, .. } => match *rhs {
                Expr::Bin { op: BinOp::And, .. } => {}
                other => panic!("expected an and on the right, got {other:?}"),
            },
            other => panic!("expected an or, got {other:?}"),
        }
    }

    #[test]
    fn arithmetic_is_left_associative() {
        // 1 - 2 - 3 must parse as Sub(Sub(1, 2), 3), not Sub(1, Sub(2, 3)).
        match only_expr("1 - 2 - 3;") {
            Expr::Bin { op: BinOp::Sub, lhs, rhs } => {
                assert!(matches!(*lhs, Expr::Bin { op: BinOp::Sub, .. }));
                assert!(matches!(*rhs, Expr::Num(_)));
            }
            other => panic!("expected a subtraction, got {other:?}"),
        }
    }

    #[test]
    fn assignment_is_right_associative() {
        // a = b = c must parse as Assign(a, Assign(b, c)).
        match only_expr("a = b = c;") {
            Expr::Assign { name, value } => {
                assert_eq!(name, "a");
                match *value {
                    Expr::Assign { name, .. } => assert_eq!(name, "b"),
                    other => panic!("expected a nested assignment, got {other:?}"),
                }
            }
            other => panic!("expected an assignment, got {other:?}"),
        }
    }

    #[test]
    fn unary_minus_binds_tighter_than_multiplication() {
        // -2 * 3 must parse as Mul(Neg(2), 3).
        match only_expr("-2 * 3;") {
            Expr::Bin { op: BinOp::Mul, lhs, .. } => match *lhs {
                Expr::Un { op: UnOp::Neg, .. } => {}
                other => panic!("expected a negation on the left, got {other:?}"),
            },
            other => panic!("expected a multiplication, got {other:?}"),
        }
    }

    #[test]
    fn parses_call_arguments() {
        match only_expr("f(1, true);") {
            Expr::Call { name, args } => {
                assert_eq!(name, "f");
                assert_eq!(args.len(), 2);
            }
            other => panic!("expected a call, got {other:?}"),
        }
    }

    #[test]
    fn parses_parenthesised_expression() {
        // (1 + 2) * 3 must parse as Mul(Add(1, 2), 3).
        match only_expr("(1 + 2) * 3;") {
            Expr::Bin { op: BinOp::Mul, lhs, .. } => match *lhs {
                Expr::Bin { op: BinOp::Add, .. } => {}
                other => panic!("expected an addition on the left, got {other:?}"),
            },
            other => panic!("expected a multiplication, got {other:?}"),
        }
    }

    #[test]
    fn rejects_missing_semicolon() {
        let err = parse_err("let x = 1");
        assert!(err.contains("expected ';'"), "got: {err}");
    }

    #[test]
    fn rejects_unknown_type() {
        let err = parse_err("let x: string = 1;");
        assert!(err.contains("unsupported type 'string'"), "got: {err}");
    }

    #[test]
    fn rejects_let_without_type_or_initializer() {
        let err = parse_err("let x;");
        assert!(err.contains("needs a type annotation or an initializer"), "got: {err}");
    }

    #[test]
    fn rejects_assignment_to_non_variable() {
        let err = parse_err("1 = 2;");
        assert!(err.contains("invalid assignment target"), "got: {err}");
    }

    #[test]
    fn rejects_call_to_non_function() {
        // A call target must be a bare name; `(1)(2)` is not callable.
        let err = parse_err("(1)(2);");
        assert!(err.contains("can only call named functions"), "got: {err}");
    }

    #[test]
    fn parses_call_on_a_named_variable() {
        // `x(1)` parses fine — whether `x` is a function is a codegen concern.
        match only_expr("x(1);") {
            Expr::Call { name, args } => {
                assert_eq!(name, "x");
                assert_eq!(args.len(), 1);
            }
            other => panic!("expected a call, got {other:?}"),
        }
    }

    #[test]
    fn reports_the_line_of_the_error() {
        let err = parse_err("let a = 1;\nlet b = 2;\nlet c;");
        assert!(err.starts_with("line 3:"), "got: {err}");
    }
}
