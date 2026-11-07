//! End-to-end tests for the tail-call rewrite: a self-recursive call in
//! tail position becomes a loop, and the loop must mean the same thing as
//! the program it replaces.

mod common;

use common::run_source;

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

