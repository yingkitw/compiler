//! Linear recurrences: a function defined in terms of its own previous
//! values becomes a loop.
//!
//! A *linear recurrence* is a function defined in terms of its own previous
//! values:
//!
//! ```text
//! function f(p0) {
//!     if (p0 < 2) { return p0; }
//!     return f(p0 - 1) + f(p0 - 2);
//! }
//! ```
//!
//! That is fibonacci, and the source program computes it exponentially: the
//! tree of calls has `F(depth)` leaves. The values, however, satisfy a
//! recurrence that can be *iterated*:
//!
//! ```text
//! function f(p0) {
//!     if (p0 < 2) { return p0; }
//!     let a = f(0);                       // f(p0 - 2)
//!     let b = f(1);                       // f(p0 - 1)
//!     while (p0 > 1) {
//!         let c = b + a;
//!         a = b;
//!         b = c;
//!         p0 = p0 - 1;
//!     }
//!     return b;
//! }
//! ```
//!
//! — linear time, constant stack, and the same value. The loop performs the
//! same additions, in the same order, as the recursion does on its way back
//! up, so the result is bit for bit the same. This is the classical
//! transform (Kapur, Kogge & Palem, *Efficient compilation of linear
//! recursive functions into object level loops*, 1986).
//!
//! For a `number` parameter the loop cannot count a fixed lattice — the
//! recursion's arguments are `n`, `n-1`, `n-2`, … at whatever fraction `n`
//! carries, and the loop has to visit exactly those values. So it walks the
//! argument down to the base case first, with the same subtractions the
//! recursion performs, and then counts *down* from the argument, replaying
//! that same descent — the counter becomes the top base point bit for bit,
//! so the number of iterations is exact, and the values the loop computes
//! sit on the recursion's own lattice:
//!
//! ```text
//! function f(p0) {
//!     if (p0 < 2) { return p0; }
//!     let b = p0;
//!     while (b >= 2) { b = b - 1; }      // the top base point
//!     let a = f(b - 1);                  // f(p0 - 2) on the same lattice
//!     let bb = f(b);                     // f(p0 - 1)
//!     let i = p0;
//!     while (i > b) {                    // replays the descent exactly
//!         let c = bb + a;
//!         a = bb;
//!         bb = c;
//!         i = i - 1;
//!     }
//!     return bb;
//! }
//! ```
//!
//! It is a much bigger hammer than tail-call elimination: it changes a
//! program from exponential to linear, so the shape it accepts has to be
//! recognised exactly. It fires only when
//!
//! * the function returns a `number` or an `int`, and its first parameter is
//!   an `int` — or a `number` whose expression does not read the parameter
//!   outside the self-calls. An `int` loop can count a fixed lattice, and a
//!   term that reads the parameter can be given the level's own value. A
//!   `number` loop cannot: an upward index would leave the recursion's
//!   lattice (adding one does not undo subtracting one, in floating point),
//!   so a `number` rewrite counts down from the argument instead — and a
//!   term that reads the parameter has no value it could see, so such a term
//!   refuses the rewrite,
//! * the base case is the first statement of the body, with the same rules
//!   as the tail-call rewrite — a statement before it would run once per
//!   recursion level in the source program and once in the rewritten one,
//! * the base expression has no side effects. The source program evaluates
//!   it once per leaf of the call tree; the rewrite evaluates it once per
//!   starting value,
//! * the rest of the body is a single `return` whose expression calls the
//!   function at least once, always at a constant offset — `p0 - 1`,
//!   `p0 - 3`, … — and whose every other part is free of side effects, and
//! * every other argument of those calls is a parameter passed on unchanged.
//!
//! The loop keeps as many values as the widest offset needs, so a
//! recurrence with a gap (`f(p0 - 1) + f(p0 - 3)`) is rewritten too. The
//! expression's shape is preserved exactly — the loop performs the same
//! operations, in the same order, on the same values — so the answer is the
//! same, bit for bit.
//!
//! The loop counts with an induction variable of its own rather than with
//! the parameter: the recursion's level and the loop's index are not the
//! same number, and a term that reads the parameter must see the level's
//! own value.
//!
//! Both schemes change how a program that never terminates *fails*. The
//! source program exhausts the stack and is reported; the loop keeps
//! counting. When the descent cannot make progress — an `int` argument near
//! `i64::MAX`, a `number` argument at or above 2^53, or an infinity — the
//! rewritten program loops where the source program was reported as a stack
//! overflow. Every finite argument below those bounds computes the same
//! answer, bit for bit.

use crate::ast::*;

use build::rewrite_recurrence;

// --- Linear recurrences -----------------------------------------------------

/// The base name of the generated accumulators — one per value the loop has
/// to keep. The names are checked against the names already in use, so they
/// cannot collide with a program's own variables.
const REC_ACC: &str = "__ts_rec_v";

/// The name of the generated induction variable.
const REC_INDEX: &str = "__ts_rec_i";

/// The name of the generated base pointer — the largest argument, on the
/// argument's own lattice, that reaches the base case. Only a `number`
/// parameter needs one: its lattice is not known until run time.
const REC_BASE: &str = "__ts_rec_b";

/// The name of the generated value the loop computes.
const REC_NEXT: &str = "__ts_rec_c";

/// A self-recursive function whose values satisfy a recurrence.
struct Recurrence {
    /// The largest argument that reaches the base case — what an `int`
    /// parameter's fixed lattice needs.
    threshold: i64,
    /// The comparison the descent loop uses to find the top base point —
    /// what a `number` parameter's run-time lattice needs. `n < L` descends
    /// while `b >= L`; `n <= L` while `b > L`.
    descent: BinOp,
    /// The literal the base case compares the parameter against.
    limit: i64,
    /// How many values the loop has to keep: the largest offset the
    /// expression calls the function at.
    width: i64,
    /// The largest number every offset is a multiple of. When it is greater
    /// than one, the arguments the recursion reaches lie on one lattice, so
    /// the loop can step by the stride and keep one accumulator per lattice
    /// position instead of one per argument.
    stride: i64,
    /// The expression that computes the next value.
    tail: Expr,
}

/// Rewrite every linear recurrence in the program into a loop.
pub fn eliminate_recurrences(prog: &mut Program) {
    for stmt in prog.stmts.iter_mut() {
        if let Stmt::Func {
            name,
            params,
            ret,
            body,
        } = stmt
        {
            rewrite_recurrence(name, params, *ret, body);
        }
    }
}


mod build;
mod recognize;

#[cfg(test)]
mod tests;
