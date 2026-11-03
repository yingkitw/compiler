//! End-to-end tests for the rewrites: a self-recursive call in tail position
//! and a linear recurrence both become loops, and a loop must mean the same
//! thing as the program it replaces.

mod common;

use common::{compile_and_run, run_source};

#[test]
fn a_tail_call_means_the_same_program_as_the_loop_it_replaces() {
    // The rewrite turns a self-recursive call in tail position into a loop.
    // The one thing that must not change is what the program prints, so each
    // shape is compiled twice: once with the tail call, and once as the
    // equivalent program written with a loop. The two must agree exactly.
    let cases: &[(&str, &str, &str)] = &[
        (
            "chain",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 7; }\n\
             \x20 return f(n - 1);\n\
             }\n\
             print(f(9));\n",
            "function f(n: number): number {\n\
             \x20 let r = 7;\n\
             \x20 while (n >= 1) { n = n - 1; }\n\
             \x20 return r;\n\
             }\n\
             print(f(9));\n",
        ),
        (
            "accumulator",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 0; }\n\
             \x20 return f(n - 1) + n;\n\
             }\n\
             print(f(10));\n",
            "function f(n: number): number {\n\
             \x20 let acc = 0;\n\
             \x20 while (n >= 1) { acc = acc + n; n = n - 1; }\n\
             \x20 return acc;\n\
             }\n\
             print(f(10));\n",
        ),
        (
            "accumulator_parameter",
            "function f(n: number, m: number): number {\n\
             \x20 if (n < 1) { return m; }\n\
             \x20 return f(n - 1, m + 2);\n\
             }\n\
             print(f(4, 1));\n",
            "function f(n: number, m: number): number {\n\
             \x20 while (n >= 1) { m = m + 2; n = n - 1; }\n\
             \x20 return m;\n\
             }\n\
             print(f(4, 1));\n",
        ),
        (
            "subtracting_accumulator",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 100; }\n\
             \x20 return f(n - 1) - n;\n\
             }\n\
             print(f(5));\n",
            "function f(n: number): number {\n\
             \x20 let acc = 0;\n\
             \x20 while (n >= 1) { acc = acc + n; n = n - 1; }\n\
             \x20 return 100 - acc;\n\
             }\n\
             print(f(5));\n",
        ),
        (
            "void_loop",
            "function f(n: number): void {\n\
             \x20 if (n < 1) { return; }\n\
             \x20 print(n);\n\
             \x20 f(n - 1);\n\
             }\n\
             f(4);\n",
            "function f(n: number): void {\n\
             \x20 while (n >= 1) { print(n); n = n - 1; }\n\
             }\n\
             f(4);\n",
        ),
        (
            "int_accumulator",
            "function f(n: int): int {\n\
             \x20 if (n < 1) { return 0; }\n\
             \x20 return f(n - 1) + n;\n\
             }\n\
             print(f(1000));\n",
            "function f(n: int): int {\n\
             \x20 let acc: int = 0;\n\
             \x20 while (n >= 1) { acc = acc + n; n = n - 1; }\n\
             \x20 return acc;\n\
             }\n\
             print(f(1000));\n",
        ),
        (
            "base_case_is_a_variable",
            "function f(n: number): number {\n\
             \x20 let base = 3;\n\
             \x20 if (n < 1) { return base; }\n\
             \x20 return f(n - 1);\n\
             }\n\
             print(f(4));\n",
            "function f(n: number): number {\n\
             \x20 let base = 3;\n\
             \x20 while (n >= 1) { n = n - 1; }\n\
             \x20 return base;\n\
             }\n\
             print(f(4));\n",
        ),
        (
            "two_stepping_parameters",
            "function f(n: number, m: number): number {\n\
             \x20 if (n < 1) { return m; }\n\
             \x20 return f(n - 1, m - 1);\n\
             }\n\
             print(f(4, 10));\n",
            "function f(n: number, m: number): number {\n\
             \x20 while (n >= 1) { n = n - 1; m = m - 1; }\n\
             \x20 return m;\n\
             }\n\
             print(f(4, 10));\n",
        ),
        (
            "step_of_two",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 0; }\n\
             \x20 return f(n - 2) + 1;\n\
             }\n\
             print(f(7));\n",
            "function f(n: number): number {\n\
             \x20 let acc = 0;\n\
             \x20 while (n >= 1) { acc = acc + 1; n = n - 2; }\n\
             \x20 return acc;\n\
             }\n\
             print(f(7));\n",
        ),
        (
            "reversed_condition",
            "function f(n: number): number {\n\
             \x20 if (0 >= n) { return 5; }\n\
             \x20 return f(n - 1);\n\
             }\n\
             print(f(3));\n",
            "function f(n: number): number {\n\
             \x20 while (0 < n) { n = n - 1; }\n\
             \x20 return 5;\n\
             }\n\
             print(f(3));\n",
        ),
        (
            "accumulator_on_the_right",
            "function f(n: number): number {\n\
             \x20 if (n < 1) { return 0; }\n\
             \x20 return n + f(n - 1);\n\
             }\n\
             print(f(4));\n",
            "function f(n: number): number {\n\
             \x20 let acc = 0;\n\
             \x20 while (n >= 1) { acc = acc + n; n = n - 1; }\n\
             \x20 return acc;\n\
             }\n\
             print(f(4));\n",
        ),
        (
            "boolean_result",
            "function f(n: number): boolean {\n\
             \x20 if (n < 1) { return true; }\n\
             \x20 return f(n - 1);\n\
             }\n\
             print(f(3));\n",
            "function f(n: number): boolean {\n\
             \x20 while (n >= 1) { n = n - 1; }\n\
             \x20 return true;\n\
             }\n\
             print(f(3));\n",
        ),
    ];
    for (name, tail_call, loop_version) in cases {
        assert_eq!(
            run_source(&format!("tco_{name}"), tail_call),
            run_source(&format!("loop_{name}"), loop_version),
            "the {name} shape must mean the same thing either way"
        );
    }
}

#[test]
fn a_call_in_the_accumulator_keeps_its_order() {
    // Regression test. The accumulator term of a tail call is evaluated while
    // the recursion unwinds — from the innermost level outwards. A rewrite
    // that folded the term into a loop would evaluate it the other way
    // round, and a program that prints would show it.
    let tail_call = "function g(x: number): number {\n\
                     \x20 print(x);\n\
                     \x20 return x;\n\
                     }\n\
                     function f(n: number): number {\n\
                     \x20 if (n < 1) { return 0; }\n\
                     \x20 return f(n - 1) + g(n);\n\
                     }\n\
                     print(f(3));\n";
    let expected = "1\n2\n3\n6\n";
    assert_eq!(run_source("tco_order", tail_call), expected);
}

#[test]
fn a_local_before_the_base_case_keeps_its_value() {
    // Regression test. A statement before the base case runs once per
    // recursion level in the source program, so folding the recursion into a
    // loop would leave the local with the outermost level's value.
    let tail_call = "function f(n: number): number {\n\
                     \x20 let x = n * 2;\n\
                     \x20 if (n < 1) { return 0; }\n\
                     \x20 return f(n - 1) + x;\n\
                     }\n\
                     print(f(4));\n";
    // 0 + 2 + 4 + 6 + 8
    assert_eq!(run_source("tco_local", tail_call), "20\n");
}

#[test]
fn a_void_tail_recursion_can_be_much_deeper_than_the_stack() {
    // The void shape takes the same route: the call in statement position
    // becomes a loop, so a million levels cost one frame.
    let source = "function f(n: number): void {\n\
                  \x20 if (n < 1) { return; }\n\
                  \x20 f(n - 1);\n\
                  }\n\
                  f(10000000);\n\
                  print(1);\n";
    assert_eq!(run_source("tco_deep_void", source), "1\n");
}

#[test]
fn recurrence() {
    assert_eq!(
        compile_and_run("recurrence"),
        "2880067194370816120\n\
         -1\n\
         1\n\
         324\n"
    );
}

#[test]
fn a_recurrence_means_the_same_program_as_the_loop_it_replaces() {
    // A linear recurrence — a function defined in terms of its own previous
    // values — is turned into a loop. The only thing that may not change is
    // what the program prints, so each shape is compiled twice: once with
    // the recurrence, and once as the same program written so that no
    // rewrite can fire on it.
    let cases: &[(&str, &str, &str)] = &[
        (
            "fibonacci",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 return f(n - 1) + f(n - 2);\n\
             }\n\
             print(f(20));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(20));\n",
        ),
        (
            "swapped_calls",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 return f(n - 2) + f(n - 1);\n\
             }\n\
             print(f(20));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(20));\n",
        ),
        (
            "inclusive_base_case",
            "function f(n: int): int {\n\
             \x20 if (n <= 1) { return 1; }\n\
             \x20 return f(n - 1) + f(n - 2);\n\
             }\n\
             print(f(20));\n",
            "function f(n: int): int {\n\
             \x20 if (n <= 1) { return 1; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(20));\n",
        ),
        (
            "base_case_uses_the_parameter",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n * 10; }\n\
             \x20 return f(n - 1) + f(n - 2);\n\
             }\n\
             print(f(12));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n * 10; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(12));\n",
        ),
        (
            "multiplication",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return 1; }\n\
             \x20 return f(n - 1) * f(n - 2);\n\
             }\n\
             print(f(10));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return 1; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a * b;\n\
             }\n\
             print(f(10));\n",
        ),
        (
            "a_second_parameter",
            "function f(n: int, m: int): int {\n\
             \x20 if (n < 2) { return m; }\n\
             \x20 return f(n - 1, m) + f(n - 2, m);\n\
             }\n\
             print(f(15, 3));\n",
            "function f(n: int, m: int): int {\n\
             \x20 if (n < 2) { return m; }\n\
             \x20 let a = f(n - 1, m);\n\
             \x20 let b = f(n - 2, m);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(15, 3));\n",
        ),
        (
            "an_extra_term",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 return f(n - 1) + f(n - 2) + 1;\n\
             }\n\
             print(f(15));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b + 1;\n\
             }\n\
             print(f(15));\n",
        ),
        (
            "a_term_that_reads_the_parameter",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 return f(n - 1) + f(n - 2) + n;\n\
             }\n\
             print(f(12));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b + n;\n\
             }\n\
             print(f(12));\n",
        ),
        (
            "three_calls",
            "function f(n: int): int {\n\
             \x20 if (n < 3) { return 1; }\n\
             \x20 return f(n - 1) + f(n - 2) + f(n - 3);\n\
             }\n\
             print(f(12));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 3) { return 1; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 let c = f(n - 3);\n\
             \x20 return a + b + c;\n\
             }\n\
             print(f(12));\n",
        ),
        (
            "a_wider_gap",
            "function f(n: int): int {\n\
             \x20 if (n < 3) { return n; }\n\
             \x20 return f(n - 1) + f(n - 3);\n\
             }\n\
             print(f(12));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 3) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 3);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(12));\n",
        ),
        (
            "sparse_offsets",
            "function f(n: int): int {\n\
             \x20 if (n < 4) { return n; }\n\
             \x20 return f(n - 2) + f(n - 3);\n\
             }\n\
             print(f(14));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 4) { return n; }\n\
             \x20 let a = f(n - 2);\n\
             \x20 let b = f(n - 3);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(14));\n",
        ),
        (
            "coefficients",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return 1; }\n\
             \x20 return 2 * f(n - 1) + 3 * f(n - 2);\n\
             }\n\
             print(f(10));\n",
            "function f(n: int): int {\n\
             \x20 if (n < 2) { return 1; }\n\
             \x20 let a = 2 * f(n - 1);\n\
             \x20 let b = 3 * f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(10));\n",
        ),
    ];
    for (name, recurrence, spelled_out) in cases {
        assert_eq!(
            run_source(&format!("rec_{name}"), recurrence),
            run_source(&format!("plain_{name}"), spelled_out),
            "the {name} recurrence must mean the same thing either way"
        );
    }
}

#[test]
fn a_number_accumulator_is_left_alone_so_the_rounding_survives() {
    // Regression test. The accumulator rewrite folds the terms in a
    // different order than the source program does: the source folds from
    // the innermost level outwards, the loop from the outermost. That is
    // exact for integers, whose wrapping arithmetic is associative, but not
    // for floating point — with a base of 1e16 and three terms of 1, the
    // rewrite printed 10000000000000004 where the program computes
    // 10000000000000000. A `number` accumulator is therefore left exactly as
    // it was written.
    let source = "function f(n: number): number {\n\
                  \x20 if (n < 1) { return 10000000000000000; }\n\
                  \x20 return f(n - 1) + 1;\n\
                  }\n\
                  print(f(3));\n";
    assert_eq!(
        run_source("tco_float_rounding", source),
        "10000000000000000\n"
    );
}

#[test]
fn a_recurrence_with_a_number_parameter_is_left_alone() {
    // Regression test. The recurrence rewrite counts the values it computes
    // with its own index; a fractional argument makes that count disagree
    // with the recursion's depth, and f(2.5) printed 1 where the program
    // computes 2. A `number` parameter is therefore left as written.
    let source = "function f(n: number): number {\n\
                  \x20 if (n < 2) { return n; }\n\
                  \x20 return f(n - 1) + f(n - 2);\n\
                  }\n\
                  print(f(2.5));\n";
    assert_eq!(run_source("rec_float_arg", source), "2\n");
}

#[test]
fn a_recursion_can_be_deeper_than_the_stack_once_it_becomes_a_loop() {
    // fib(90) is 2^90 calls in the source program: it could never finish,
    // and it would exhaust the stack long before that. The rewrite turns the
    // recurrence into a loop, so the program computes the answer at once.
    let source = "function f(n: int): int {\n\
                  \x20 if (n < 2) { return n; }\n\
                  \x20 return f(n - 1) + f(n - 2);\n\
                  }\n\
                  print(f(90));\n";
    assert_eq!(run_source("rec_deep", source), "2880067194370816120\n");
}
