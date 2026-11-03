//! The predicates and helpers that the rewrites share.
//!
//! Nothing here knows about a particular rewrite: these are questions about
//! an expression ("is this free of side effects?") and about a program
//! ("which names does it already use?").

use crate::ast::*;

pub(crate) fn is_var(expr: &Expr, name: &str) -> bool {
    matches!(expr, Expr::Var(v) if v == name)
}

pub(crate) fn is_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Num(_))
}

/// Is the expression free of side effects? Only reading a variable and
/// arithmetic on such reads qualify.
pub(crate) fn is_pure(expr: &Expr) -> bool {
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::Var(_) => true,
        Expr::Bin { lhs, rhs, .. } => is_pure(lhs) && is_pure(rhs),
        Expr::Un { expr, .. } => is_pure(expr),
        Expr::Call { .. } => false,
        Expr::Assign { .. } => false,
    }
}

/// Does the expression assign to anything?
pub(crate) fn contains_assign(expr: &Expr) -> bool {
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::Var(_) => false,
        Expr::Bin { lhs, rhs, .. } => contains_assign(lhs) || contains_assign(rhs),
        Expr::Un { expr, .. } => contains_assign(expr),
        Expr::Call { args, .. } => args.iter().any(contains_assign),
        Expr::Assign { .. } => true,
    }
}

/// Does the expression call the named function, directly or indirectly?
pub(crate) fn contains_call_to(expr: &Expr, name: &str) -> bool {
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::Var(_) => false,
        Expr::Bin { lhs, rhs, .. } => contains_call_to(lhs, name) || contains_call_to(rhs, name),
        Expr::Un { expr, .. } => contains_call_to(expr, name),
        Expr::Call {
            name: callee,
            args,
        } => callee == name || args.iter().any(|a| contains_call_to(a, name)),
        Expr::Assign { value, .. } => contains_call_to(value, name),
    }
}

pub(crate) fn collect_stmt_names(stmt: &Stmt, out: &mut Vec<String>) {
    match stmt {
        Stmt::Let { name, init, .. } => {
            out.push(name.clone());
            if let Some(init) = init {
                collect_expr_names(init, out);
            }
        }
        Stmt::Func {
            name,
            params,
            body,
            ..
        } => {
            out.push(name.clone());
            out.extend(params.iter().map(|(p, _)| p.clone()));
            for stmt in body {
                collect_stmt_names(stmt, out);
            }
        }
        Stmt::If {
            cond,
            then,
            els,
        } => {
            collect_expr_names(cond, out);
            for stmt in then.iter().chain(els.iter()) {
                collect_stmt_names(stmt, out);
            }
        }
        Stmt::While { cond, body } => {
            collect_expr_names(cond, out);
            for stmt in body {
                collect_stmt_names(stmt, out);
            }
        }
        Stmt::Block(stmts) => {
            for stmt in stmts {
                collect_stmt_names(stmt, out);
            }
        }
        Stmt::Return(Some(expr)) => collect_expr_names(expr, out),
        Stmt::Return(None) => {}
        Stmt::Expr(expr) => collect_expr_names(expr, out),
    }
}

pub(crate) fn collect_expr_names(expr: &Expr, out: &mut Vec<String>) {
    match expr {
        Expr::Num(_) | Expr::Bool(_) => {}
        Expr::Var(name) => out.push(name.clone()),
        Expr::Assign { name, value } => {
            out.push(name.clone());
            collect_expr_names(value, out);
        }
        Expr::Bin { lhs, rhs, .. } => {
            collect_expr_names(lhs, out);
            collect_expr_names(rhs, out);
        }
        Expr::Un { expr, .. } => collect_expr_names(expr, out),
        Expr::Call { name, args } => {
            out.push(name.clone());
            for arg in args {
                collect_expr_names(arg, out);
            }
        }
    }
}

/// The names a program already uses: its parameters, and everything its body
/// mentions.
pub(crate) fn names_in_use(params: &[(String, Type)], body: &[Stmt]) -> Vec<String> {
    let mut used: Vec<String> = params.iter().map(|(n, _)| n.clone()).collect();
    for stmt in body {
        collect_stmt_names(stmt, &mut used);
    }
    used
}

/// Pick a name that the program does not already use, by appending a counter
/// when it does.
pub(crate) fn fresh_name(base: &str, used: &[String]) -> String {
    if !used.iter().any(|n| n == base) {
        return base.to_string();
    }
    for suffix in 1.. {
        let candidate = format!("{base}_{suffix}");
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    unreachable!("an unused name always exists")
}

/// Pick several names that the program does not already use.
pub(crate) fn fresh_names(bases: &[&str], used: &[String]) -> Vec<String> {
    bases.iter().map(|base| fresh_name(base, used)).collect()
}

/// The helpers the rewrite modules' tests share.
#[cfg(test)]
pub(crate) mod testing {
    use crate::ast::*;
    use crate::lexer;
    use crate::parser;

    use super::collect_stmt_names;

    /// Parse a program and run the tail-call rewrite over it.
    pub(crate) fn rewrite(src: &str) -> Program {
        let prog = parser::parse(lexer::lex(src).expect("lex failed")).expect("parse failed");
        let mut prog = prog;
        crate::rewrite::tail_call::eliminate_tail_calls(&mut prog);
        prog
    }

    /// Parse a program and run every rewrite over it.
    pub(crate) fn optimised(src: &str) -> Program {
        let prog = parser::parse(lexer::lex(src).expect("lex failed")).expect("parse failed");
        let mut prog = prog;
        crate::rewrite::optimise(&mut prog);
        prog
    }

    /// The body of the first function in a program.
    pub(crate) fn first_body(prog: &Program) -> &[Stmt] {
        match &prog.stmts[0] {
            Stmt::Func { body, .. } => body,
            other => panic!("expected a function, got {other:?}"),
        }
    }

    /// The body of the function with the given name.
    pub(crate) fn body_of<'a>(prog: &'a Program, name: &str) -> &'a [Stmt] {
        for stmt in &prog.stmts {
            if let Stmt::Func {
                name: fname,
                body,
                ..
            } = stmt
                && fname == name
            {
                return body;
            }
        }
        panic!("no function named {name}");
    }

    /// How many times the body still calls `name`.
    pub(crate) fn calls_to(body: &[Stmt], name: &str) -> usize {
        let mut names = Vec::new();
        for stmt in body {
            collect_stmt_names(stmt, &mut names);
        }
        names.iter().filter(|n| *n == name).count()
    }

    /// Does the body contain a `while` loop?
    pub(crate) fn has_loop(body: &[Stmt]) -> bool {
        body.iter()
            .any(|s| matches!(s, Stmt::While { .. }))
    }
}
