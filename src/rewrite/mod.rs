//! Source-to-source optimisations.
//!
//! Two rewrites run between the parser and the code generator. They change
//! the program on the AST, so the code generator never learns that they
//! exist, and neither of them can change what a program computes.
//!
//! # Tail-call elimination
//!
//! A function that calls itself in tail position grows the stack once per
//! level, so a recursion of a million levels dies with "maximum recursion
//! depth exceeded". That is a property of the *call*, not of the algorithm:
//! the recursive call never needs the frame it was made in, so it can be a
//! jump instead. The rewrite turns the call into a loop.
//!
//! # Linear recurrences
//!
//! A *linear recurrence* is a function defined in terms of its own previous
//! values. The source program computes it exponentially — the tree of calls
//! has 2^depth leaves — but the values satisfy a recurrence that can be
//! iterated, so the rewrite turns it into a loop as well.
//!
//! # The rule both rewrites obey
//!
//! A rewrite changes *when* things run, so every statement it moves has to
//! run the same number of times before and after. That is why the base case
//! must be the first statement of the body, and why the terms a rewrite
//! evaluates itself must be free of side effects. The module that
//! implements each rewrite states its own rules; `common.rs` holds the
//! predicates they share.

mod common;
mod recurrence;
mod tail_call;

use crate::ast::*;

/// Run every source-to-source optimisation over the program.
pub fn optimise(prog: &mut Program) {
    tail_call::eliminate_tail_calls(prog);
    recurrence::eliminate_recurrences(prog);
}
