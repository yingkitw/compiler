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
    is_pure_at(expr, 0)
}

/// The recursive form of `is_pure`, with the depth of the expression so far.
///
/// An expression that is nested deeper than the limit is not pure as far as
/// the rewrite is concerned: the rewrite simply leaves the program alone,
/// and the code generator reports the depth.
pub(crate) fn is_pure_at(expr: &Expr, depth: usize) -> bool {
    if depth > MAX_AST_DEPTH {
        return false;
    }
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Var(_) => true,
        Expr::Array(elems) => elems.iter().all(|e| is_pure_at(e, depth + 1)),
        Expr::Index { base, index } => {
            is_pure_at(base, depth + 1) && is_pure_at(index, depth + 1)
        }
        Expr::Member { base, .. } => is_pure_at(base, depth + 1),
        Expr::Bin { lhs, rhs, .. } => is_pure_at(lhs, depth + 1) && is_pure_at(rhs, depth + 1),
        Expr::Un { expr, .. } => is_pure_at(expr, depth + 1),
        // A cast is a pure conversion: it writes nothing and calls nothing.
        Expr::Cast { expr, .. } => is_pure_at(expr, depth + 1),
        Expr::Cond {
            cond,
            then_expr,
            else_expr,
        } => {
            is_pure_at(cond, depth + 1)
                && is_pure_at(then_expr, depth + 1)
                && is_pure_at(else_expr, depth + 1)
        }
        Expr::BlockExpr { .. } => false,
        Expr::Call { .. } => false,
        Expr::Assign { .. } => false,
    }
}

/// Does the expression assign to anything?
pub(crate) fn contains_assign(expr: &Expr) -> bool {
    contains_assign_at(expr, 0)
}

/// The recursive form of `contains_assign`, with the depth so far.
pub(crate) fn contains_assign_at(expr: &Expr, depth: usize) -> bool {
    if depth > MAX_AST_DEPTH {
        // Too deep to look at: assume it does assign, so the rewrite leaves
        // the program alone.
        return true;
    }
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Var(_) | Expr::Array(_) => false,
        Expr::Index { base, index } => {
            contains_assign_at(base, depth + 1) || contains_assign_at(index, depth + 1)
        }
        Expr::Member { base, .. } => contains_assign_at(base, depth + 1),
        Expr::Bin { lhs, rhs, .. } => {
            contains_assign_at(lhs, depth + 1) || contains_assign_at(rhs, depth + 1)
        }
        Expr::Un { expr, .. } => contains_assign_at(expr, depth + 1),
        Expr::Cast { expr, .. } => contains_assign_at(expr, depth + 1),
        Expr::Cond {
            cond,
            then_expr,
            else_expr,
        } => {
            contains_assign_at(cond, depth + 1)
                || contains_assign_at(then_expr, depth + 1)
                || contains_assign_at(else_expr, depth + 1)
        }
        Expr::BlockExpr { stmts, value } => {
            contains_assign_at(value, depth + 1)
                || stmts.iter().any(|s| {
                    matches!(s, Stmt::Expr { expr: e, .. } if contains_assign_at(e, depth + 1))
                })
        }
        Expr::Call { receiver, args, .. } => {
            receiver
                .as_ref()
                .is_some_and(|r| contains_assign_at(r, depth + 1))
                || args.iter().any(|a| contains_assign_at(a, depth + 1))
        }
        Expr::Assign { .. } => true,
    }
}

/// Does the expression call the named function, directly or indirectly?
pub(crate) fn contains_call_to(expr: &Expr, name: &str) -> bool {
    contains_call_to_at(expr, name, 0)
}

/// The recursive form of `contains_call_to`, with the depth so far.
pub(crate) fn contains_call_to_at(expr: &Expr, name: &str, depth: usize) -> bool {
    if depth > MAX_AST_DEPTH {
        // Too deep to look at: assume it calls the function, so the rewrite
        // leaves the program alone.
        return true;
    }
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::Str(_) | Expr::Var(_) | Expr::Array(_) => false,
        Expr::Index { base, index } => {
            contains_call_to_at(base, name, depth + 1)
                || contains_call_to_at(index, name, depth + 1)
        }
        Expr::Member { base, .. } => contains_call_to_at(base, name, depth + 1),
        Expr::Bin { lhs, rhs, .. } => {
            contains_call_to_at(lhs, name, depth + 1) || contains_call_to_at(rhs, name, depth + 1)
        }
        Expr::Un { expr, .. } => contains_call_to_at(expr, name, depth + 1),
        Expr::Cast { expr, .. } => contains_call_to_at(expr, name, depth + 1),
        Expr::Cond {
            cond,
            then_expr,
            else_expr,
        } => {
            contains_call_to_at(cond, name, depth + 1)
                || contains_call_to_at(then_expr, name, depth + 1)
                || contains_call_to_at(else_expr, name, depth + 1)
        }
        Expr::BlockExpr { value, .. } => contains_call_to_at(value, name, depth + 1),
        Expr::Call {
            name: callee,
            receiver,
            args,
        } => {
            callee == name
                || receiver
                    .as_ref()
                    .is_some_and(|r| contains_call_to_at(r, name, depth + 1))
                || args.iter().any(|a| contains_call_to_at(a, name, depth + 1))
        }
        Expr::Assign { value, .. } => contains_call_to_at(value, name, depth + 1),
    }
}

pub(crate) fn collect_stmt_names(stmt: &Stmt, out: &mut Vec<String>) {
    collect_stmt_names_at(stmt, out, 0)
}

/// The recursive form of `collect_stmt_names`, with the depth so far.
///
/// A program nested deeper than the limit is left alone: the rewrite skips
/// it, and the code generator reports the depth.
pub(crate) fn collect_stmt_names_at(stmt: &Stmt, out: &mut Vec<String>, depth: usize) {
    if depth > MAX_AST_DEPTH {
        return;
    }
    match stmt {
        Stmt::Let { name, init, .. } => {
            out.push(name.clone());
            if let Some(init) = init {
                collect_expr_names_at(init, out, depth + 1);
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
                collect_stmt_names_at(stmt, out, depth + 1);
            }
        }
        Stmt::If {
            cond,
            then,
            els,
            ..
        } => {
            collect_expr_names_at(cond, out, depth + 1);
            for stmt in then.iter().chain(els.iter()) {
                collect_stmt_names_at(stmt, out, depth + 1);
            }
        }
        Stmt::While { cond, body, .. } => {
            collect_expr_names_at(cond, out, depth + 1);
            for stmt in body {
                collect_stmt_names_at(stmt, out, depth + 1);
            }
        }
        Stmt::Block { stmts, .. } => {
            for stmt in stmts {
                collect_stmt_names_at(stmt, out, depth + 1);
            }
        }
        Stmt::Return { value: Some(expr), .. } => {
            collect_expr_names_at(expr, out, depth + 1)
        }
        Stmt::Return { value: None, .. } => {}
        Stmt::Expr { expr, .. } => collect_expr_names_at(expr, out, depth + 1),
    }
}

pub(crate) fn collect_expr_names_at(expr: &Expr, out: &mut Vec<String>, depth: usize) {
    if depth > MAX_AST_DEPTH {
        return;
    }
    match expr {
        Expr::Num(_) | Expr::Bool(_) | Expr::Str(_) => {}
        Expr::Var(name) => out.push(name.clone()),
        Expr::Assign { name, value } => {
            out.push(name.clone());
            collect_expr_names_at(value, out, depth + 1);
        }
        Expr::Bin { lhs, rhs, .. } => {
            collect_expr_names_at(lhs, out, depth + 1);
            collect_expr_names_at(rhs, out, depth + 1);
        }
        Expr::Un { expr, .. } => collect_expr_names_at(expr, out, depth + 1),
        Expr::Cast { expr, .. } => collect_expr_names_at(expr, out, depth + 1),
        Expr::Cond {
            cond,
            then_expr,
            else_expr,
        } => {
            collect_expr_names_at(cond, out, depth + 1);
            collect_expr_names_at(then_expr, out, depth + 1);
            collect_expr_names_at(else_expr, out, depth + 1);
        }
        Expr::BlockExpr { stmts, value } => {
            for s in stmts {
                collect_stmt_names_at(s, out, depth + 1);
            }
            collect_expr_names_at(value, out, depth + 1);
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_expr_names_at(e, out, depth + 1);
            }
        }
        Expr::Index { base, index } => {
            collect_expr_names_at(base, out, depth + 1);
            collect_expr_names_at(index, out, depth + 1);
        }
        Expr::Member { base, name } => {
            collect_expr_names_at(base, out, depth + 1);
            out.push(name.clone());
        }
        Expr::Call { name, receiver, args } => {
            out.push(name.clone());
            if let Some(rcv) = receiver {
                collect_expr_names_at(rcv, out, depth + 1);
            }
            for arg in args {
                collect_expr_names_at(arg, out, depth + 1);
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

/// Pick `count` names that the program does not already use, numbered from
/// zero.
pub(crate) fn fresh_numbered(base: &str, count: usize, used: &[String]) -> Vec<String> {
    (0..count)
        .map(|i| fresh_name(&format!("{base}{i}"), used))
        .collect()
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
