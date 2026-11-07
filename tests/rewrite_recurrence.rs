//! End-to-end tests for the recurrence rewrite: a linear recurrence becomes
//! a loop, and the loop must mean the same thing as the program it replaces.

mod common;

use common::{compile_and_run, run_source};

#[test]
fn recurrence() {
    assert_eq!(
        compile_and_run("recurrence"),
        "2880067194370816120\n\
         2\n\
         1505174.5\n\
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
        (
            "fibonacci_number",
            // A `number` parameter is rewritten too. The differential spells
            // the same program with two lets, which no rewrite fires on, at
            // a fractional argument — where the two lattices differ most.
            "function f(n: number): number {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 return f(n - 1) + f(n - 2);\n\
             }\n\
             print(f(12.5));\n",
            "function f(n: number): number {\n\
             \x20 if (n < 2) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(12.5));\n",
        ),
        (
            "inclusive_base_case_number",
            "function f(n: number): number {\n\
             \x20 if (n <= 1) { return 1; }\n\
             \x20 return f(n - 1) + f(n - 2);\n\
             }\n\
             print(f(12.5));\n",
            "function f(n: number): number {\n\
             \x20 if (n <= 1) { return 1; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 2);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(12.5));\n",
        ),
        (
            "a_wider_gap_number",
            "function f(n: number): number {\n\
             \x20 if (n < 3) { return n; }\n\
             \x20 return f(n - 1) + f(n - 3);\n\
             }\n\
             print(f(12.5));\n",
            "function f(n: number): number {\n\
             \x20 if (n < 3) { return n; }\n\
             \x20 let a = f(n - 1);\n\
             \x20 let b = f(n - 3);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(12.5));\n",
        ),
        (
            "a_second_parameter_number",
            "function f(n: number, m: number): number {\n\
             \x20 if (n < 2) { return m; }\n\
             \x20 return f(n - 1, m) + f(n - 2, m);\n\
             }\n\
             print(f(12.5, 3));\n",
            "function f(n: number, m: number): number {\n\
             \x20 if (n < 2) { return m; }\n\
             \x20 let a = f(n - 1, m);\n\
             \x20 let b = f(n - 2, m);\n\
             \x20 return a + b;\n\
             }\n\
             print(f(12.5, 3));\n",
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
fn a_number_recurrence_computes_a_fractional_argument_exactly() {
    // Regression test. The first version of the rewrite counted its own
    // integer index, and a fractional argument made that count disagree
    // with the recursion's depth: f(2.5) printed 1 where the program
    // computes 2. A `number` parameter was therefore left as written. The
    // rewrite now derives its lattice from the argument — the descent, the
    // starting calls and the counter all replay the recursion's own
    // subtractions — so the rewritten program prints what the source
    // program computes, at fractional arguments included.
    let source = "function f(n: number): number {\n\
                  \x20 if (n < 2) { return n; }\n\
                  \x20 return f(n - 1) + f(n - 2);\n\
                  }\n\
                  print(f(2.5));\n\
                  print(f(0.5));\n\
                  print(f(1.9));\n";
    assert_eq!(
        run_source("rec_float_arg", source),
        "2\n\
         0.5\n\
         1.9\n"
    );
}

#[test]
fn a_recursion_can_be_deeper_than_the_stack_once_it_becomes_a_loop() {
    // fib(90) is an astronomical number of calls in the source program: it
    // could never finish, and it would exhaust the stack long before that.
    // The rewrite turns the recurrence into a loop, so the program computes
    // the answer at once.
    let source = "function f(n: int): int {\n\
                  \x20 if (n < 2) { return n; }\n\
                  \x20 return f(n - 1) + f(n - 2);\n\
                  }\n\
                  print(f(90));\n";
    assert_eq!(run_source("rec_deep", source), "2880067194370816120\n");
}
