//! The tail call tests: every shape the rewrite accepts,
//! and every shape it must refuse.

use super::*;
use crate::rewrite::common::testing::*;

#[test]
fn a_tail_call_becomes_a_loop() {
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected the tail call to become a loop");
    assert_eq!(
        calls_to(body, "f"),
        0,
        "the recursive call should be gone: {body:?}"
    );
}

#[test]
fn an_accumulator_tail_call_becomes_a_loop() {
    // An accumulator needs an integer return: the rewrite folds the terms in
    // a different order than the source program does, which is exact for
    // integers and not for floating point.
    let prog = rewrite(
        "function f(n: int): int {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n - 1) + 1;\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(calls_to(body, "f"), 0, "the call should be gone");
    // The accumulator is introduced, and it is mutable.
    let acc = body.iter().find_map(|s| match s {
        Stmt::Let { name, mutable, .. } if *mutable => Some(name.clone()),
        _ => None,
    });
    assert!(acc.is_some(), "expected an accumulator: {body:?}");
}

#[test]
fn mutual_recursion_is_left_alone() {
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return g(n - 1);\n\
         }\n\
         function g(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "mutual recursion must not be rewritten");
    assert_eq!(calls_to(body, "g"), 1);
}

#[test]
fn a_call_that_is_not_in_tail_position_is_left_alone() {
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  let x = f(n - 1);\n\
         \x20  return x + 1;\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
    assert_eq!(calls_to(body, "f"), 1);
}

#[test]
fn a_recursion_that_grows_is_left_alone() {
    // The argument increases, so a loop would never end.
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n + 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
    assert_eq!(calls_to(body, "f"), 1);
}

#[test]
fn a_computed_argument_is_left_alone() {
    // The loop would have to know what to do with `n - 1 + 1`.
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n - 1 + 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
    assert_eq!(calls_to(body, "f"), 1);
}

#[test]
fn a_call_to_another_function_is_left_alone() {
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return g(n - 1);\n\
         }\n\
         function g(n: number): number {\n\
         \x20  return n;\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_function_without_a_base_case_is_left_alone() {
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  return f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn an_if_with_an_else_is_left_alone() {
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; } else { return 1; }\n\
         \x20  return f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_statement_before_the_base_case_is_left_alone() {
    // The statement would run once per level in the source program and
    // once in the rewritten one.
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  let x = n * 2;\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n - 1) + x;\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
    assert_eq!(calls_to(body, "f"), 1);
}

#[test]
fn a_call_in_the_accumulator_is_left_alone() {
    // The accumulator term would be evaluated in the opposite order.
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n - 1) + g(n);\n\
         }\n\
         function g(n: number): number { return n; }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
    assert_eq!(calls_to(body, "f"), 1);
}

#[test]
fn a_second_recursive_call_is_left_alone() {
    // `f(n - 1) + f(n - 2)` is not a tail call: the second call's
    // result is used after the first one returns.
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 2) { return n; }\n\
         \x20  return f(n - 1) + f(n - 2);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
    assert_eq!(calls_to(body, "f"), 2);
}

#[test]
fn a_void_tail_call_becomes_a_loop() {
    let prog = rewrite(
        "function f(n: number): void {\n\
         \x20  if (n < 1) { return; }\n\
         \x20  f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(calls_to(body, "f"), 0, "the call should be gone");
}

#[test]
fn a_void_tail_call_keeps_its_per_level_statements() {
    let prog = rewrite(
        "function f(n: number): void {\n\
         \x20  if (n < 1) { return; }\n\
         \x20  print(n);\n\
         \x20  f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    // The print is now inside the loop.
    let Some(Stmt::While { body, .. }) = body.iter().find(|s| matches!(s, Stmt::While { .. }))
    else {
        panic!("expected a loop: {body:?}");
    };
    assert_eq!(body.len(), 2, "the print and the decrement: {body:?}");
}

#[test]
fn a_per_level_statement_that_calls_the_function_is_left_alone() {
    let prog = rewrite(
        "function f(n: number): void {\n\
         \x20  if (n < 1) { return; }\n\
         \x20  print(f(n - 1));\n\
         \x20  f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_per_level_statement_that_assigns_is_left_alone() {
    let prog = rewrite(
        "function f(n: number): void {\n\
         \x20  if (n < 1) { return; }\n\
         \x20  n = n - 1;\n\
         \x20  f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_value_returned_from_a_void_function_is_left_alone() {
    // The program is invalid ("cannot return a value from a void
    // function"), and the rewrite must not turn it into a valid one.
    let prog = rewrite(
        "function f(n: number): void {\n\
         \x20  if (n < 1) { return; }\n\
         \x20  return f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn a_void_function_without_a_return_type_is_left_alone() {
    // `return 0;` in a function with no return type is an error, and the
    // rewrite must not hide it.
    let prog = rewrite(
        "function f(n: number) {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n - 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
}

#[test]
fn the_accumulator_name_avoids_a_parameter() {
    let prog = rewrite(
        "function f(n: int, __ts_tco_acc: int): int {\n\
         \x20  if (n < 1) { return __ts_tco_acc; }\n\
         \x20  return f(n - 1, __ts_tco_acc) + 1;\n\
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
    assert_eq!(declared.len(), 1, "one accumulator: {declared:?}");
    assert!(
        declared.iter().any(|n| n.starts_with("__ts_tco_acc_")),
        "the accumulator must not take the parameter's name: {declared:?}"
    );
}

#[test]
fn a_second_parameter_may_stay_unchanged() {
    let prog = rewrite(
        "function f(n: number, m: number): number {\n\
         \x20  if (n < 1) { return m; }\n\
         \x20  return f(n - 1, m);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(calls_to(body, "f"), 0);
}

#[test]
fn a_second_parameter_may_step_up() {
    // The accumulator-parameter shape: the second argument carries the
    // running total.
    let prog = rewrite(
        "function f(n: number, m: number): number {\n\
         \x20  if (n < 1) { return m; }\n\
         \x20  return f(n - 1, m + 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(has_loop(body), "expected a loop: {body:?}");
    assert_eq!(calls_to(body, "f"), 0);
}

#[test]
fn a_first_parameter_that_steps_up_is_left_alone() {
    // The loop would never end.
    let prog = rewrite(
        "function f(n: number): number {\n\
         \x20  if (n < 1) { return 0; }\n\
         \x20  return f(n + 1);\n\
         }\n",
    );
    let body = first_body(&prog);
    assert!(!has_loop(body), "expected no rewrite: {body:?}");
    assert_eq!(calls_to(body, "f"), 1);
}
