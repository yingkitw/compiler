//! The recurrence tests: every shape the rewrite accepts,
//! and every shape it must refuse.

use super::*;
use crate::rewrite::common::testing::*;

#[test]
fn a_recurrence_becomes_a_loop() {
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    // Only the two starting values are still calls.
    assert_eq!(
        calls_to(body, "f"),
        2,
        "the recursion should be gone: {body:?}"
    );
}

#[test]
fn a_recurrence_on_a_wider_lattice_keeps_a_narrower_window() {
    // The offsets 2 and 4 are both multiples of 2, so the arguments the
    // recursion reaches lie on one lattice — the loop steps by 2 and keeps
    // one accumulator per lattice position: two of them, not four.
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 2) + f(n - 4);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(
        calls_to(body, "f"),
        2,
        "the window should be two lattice positions, not four: {body:?}"
    );
}

#[test]
fn a_recurrence_with_coprime_offsets_keeps_every_value() {
    // The offsets 1 and 17 share no divisor, so the arguments the
    // recursion reaches lie on no narrower lattice: the loop keeps one
    // accumulator per argument in the range — seventeen of them.
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 17);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(
        calls_to(body, "f"),
        17,
        "every value in the range has to be kept: {body:?}"
    );
}

#[test]
fn a_number_recurrence_becomes_a_loop() {
    // A `number` lattice is only known at run time, so the rewrite finds it
    // by walking the argument down — but the recursion is gone all the same.
    let prog = optimised(
        "function f(n: number): number {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(
        calls_to(body, "f"),
        2,
        "only the starting values should still be calls: {body:?}"
    );
}

#[test]
fn a_number_recurrence_walks_the_argument_down_and_counts_back_up() {
    // The `number` scheme has two loops: one that walks the argument down
    // to the top base point, and one that replays that descent as a
    // counter — replaying it is what makes the iteration count exact.
    let prog = optimised(
        "function f(n: number): number {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    let loops = body
        .iter()
        .filter(|s| matches!(s, Stmt::While { .. }))
        .count();
    assert_eq!(loops, 2, "expected a descent and a counter: {body:?}");
}

#[test]
fn an_int_recurrence_does_not_walk_the_argument_down() {
    // The `int` lattice is known at compile time; adding a run-time walk to
    // it would double the loop's work for nothing.
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    let loops = body
        .iter()
        .filter(|s| matches!(s, Stmt::While { .. }))
        .count();
    assert_eq!(loops, 1, "the int path should be one loop: {body:?}");
}

#[test]
fn a_number_recurrence_that_reads_the_parameter_is_left_alone() {
    // A term that reads the parameter must see its own level's value. The
    // `int` loop can give it one; the `number` counter counts down from the
    // argument and is never the level's value, so the rewrite refuses.
    let prog = optimised(
        "function f(n: number): number {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 2) + n;\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_number_recurrence_with_an_inclusive_base_becomes_a_loop() {
    let prog = optimised(
        "function f(n: number): number {\n\
         \x20  if (n <= 2) { return 1; }\n\
         \x20  return f(n - 1) + f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(calls_to(body, "f"), 2);
}

#[test]
fn a_recurrence_with_swapped_calls_becomes_a_loop() {
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 2) + f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(calls_to(body, "f"), 2);
}

#[test]
fn a_recurrence_with_a_side_effect_in_the_base_is_left_alone() {
    // The source program evaluates the base expression once per leaf of
    // the call tree; the rewrite evaluates it exactly twice.
    let prog = optimised(
        "function g(): number { return 1; }\n\
         function f(n: int): int {\n\
         \x20  if (n < 2) { return g(); }\n\
         \x20  return f(n - 1) + f(n - 2);\n\
         }\n",
    );
    let body = body_of(&prog, "f");
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_recurrence_with_a_wider_gap_becomes_a_loop() {
    // The loop keeps as many values as the widest offset needs.
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 3) { return n; }\n\
         \x20  return f(n - 1) + f(n - 3);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    // Three values are kept: the ones from one, two and three steps back.
    let kept = body
        .iter()
        .filter(|s| matches!(s, Stmt::Let { name, .. } if name.starts_with("__ts_rec_v")))
        .count();
    assert_eq!(kept, 3, "the loop keeps three values: {body:?}");
}

#[test]
fn a_recurrence_with_an_extra_term_becomes_a_loop() {
    // A term that does not call the function is carried along the loop.
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 2) + 1;\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
}

#[test]
fn a_recurrence_with_three_calls_becomes_a_loop() {
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 3) { return 1; }\n\
         \x20  return f(n - 1) + f(n - 2) + f(n - 3);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
}

#[test]
fn a_recurrence_that_calls_another_function_is_left_alone() {
    // The rewrite would evaluate the call in the opposite order.
    let prog = optimised(
        "function g(x: number): number { return x; }\n\
         function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + g(n);\n\
         }\n",
    );
    let body = body_of(&prog, "f");
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_recurrence_that_assigns_is_left_alone() {
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  let x = 0;\n\
         \x20  return f(n - 1) + f(n - 2) + (x = 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_recurrence_that_counts_up_is_left_alone() {
    // The offsets have to step down, or the recursion never ends.
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n + 1) + f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_recurrence_with_a_statement_before_the_base_case_is_left_alone() {
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  let x = n * 2;\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_chain_recursion_is_not_a_recurrence() {
    let prog = optimised(
        "function f(n: int): int {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    // The tail-call rewrite handles this one, not the recurrence one.
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(calls_to(body, "f"), 0, "the call should be gone");
}

#[test]
fn a_recurrence_in_a_boolean_function_is_left_alone() {
    // There is nothing to add up.
    let prog = optimised(
        "function f(n: number): boolean {\n\
         \x20  if (n < 2) { return true; }\n\
         \x20  return f(n - 1) && f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn the_recurrence_accumulators_avoid_a_collision() {
    let prog = optimised(
        "function f(n: int, __ts_rec_v0: int): int {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1, __ts_rec_v0) + f(n - 2, __ts_rec_v0);\n\
         }\n",
    );
    let body = first_body(&prog);
    let declared: Vec<&str> = body
        .iter()
        .filter_map(|s| match s {
            Stmt::Let { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        declared
            .iter()
            .any(|n| n.starts_with("__ts_rec_v0") && *n != "__ts_rec_v0"),
        "the accumulator must not take the parameter's name: {declared:?}"
    );
}
