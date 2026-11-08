//! Regression tests on the shape of emitted assembly and on type rules at
//! emit time — without running `cc` or executing code.

use std::path::Path;

use crate::{backend, lexer, parser, rewrite};

fn emit_asm(src: &str) -> String {
    let toks = lexer::lex(src).expect("lex");
    let mut prog = parser::parse(toks).expect("parse");
    rewrite::optimise(&mut prog);
    backend::emit(&prog, Path::new("test.ts")).expect("emit")
}

fn emit_err(src: &str) -> String {
    let toks = lexer::lex(src).expect("lex");
    let prog = parser::parse(toks).expect("parse");
    backend::emit(&prog, Path::new("test.ts")).unwrap_err()
}

#[test]
fn the_frame_pointer_sits_at_the_high_end_of_the_frame() {
    let asm = emit_asm("print(1);");
    assert!(
        asm.contains("add\tx29, sp, #"),
        "prologue should place x29 above the allocated frame"
    );
}

#[test]
fn float_negation_uses_a_two_operand_fneg() {
    let asm = emit_asm("print(-1.0);");
    assert!(asm.contains("fneg\td0, d0"));
}

#[test]
fn integer_division_branches_to_the_runtime_on_a_zero_divisor() {
    let asm = emit_asm("let a: int = 1; let b: int = 0; print(a / b);");
    assert!(asm.contains("ts_int_div_zero"));
    assert!(asm.contains("b\t"));
}

#[test]
fn integer_remainder_uses_srem_with_scratch() {
    let asm = emit_asm("let a: int = 7; let b: int = 3; print(a % b);");
    assert!(asm.contains("sdiv"));
    assert!(asm.contains("msub"));
    assert!(asm.contains("mov\tx2"));
}

#[test]
fn number_truthiness_compares_against_zero() {
    let asm = emit_asm("if (1.0) { print(1); }");
    assert!(asm.contains("fmov\td1, x1") || asm.contains("fmov\td1"));
    assert!(asm.contains("fcmp\td0, d1"));
}

#[test]
fn a_multi_argument_call_places_both_integers_in_the_abi_registers() {
    let asm = emit_asm(
        "function f(x: int, y: int): int { return x + y; }\n\
         print(f(4, 1));",
    );
    let call_idx = asm.find("bl\t_tsfn_f").expect("call");
    let site = &asm[..call_idx];
    assert!(site.contains("mov\tx1,"));
}

#[test]
fn number_remainder_calls_ts_fmod_with_vector_arguments() {
    let asm = emit_asm("print(5.5 % -2.0);");
    assert!(asm.contains("bl\t_ts_fmod"));
    assert!(asm.contains("mov\tv0.d[0]"));
    assert!(asm.contains("mov\tv1.d[0]"));
}

#[test]
fn number_to_int_cast_calls_the_runtime() {
    let asm = emit_asm("let x: int = 3.9 as int;");
    assert!(asm.contains("ts_double_to_int"));
}

#[test]
fn mixed_int_and_number_addition_is_rejected() {
    let err = emit_err("let a: int = 1; let b: int = a + 1.0;");
    assert!(err.contains("arithmetic operators require"));
}

#[test]
fn a_fraction_point_literal_cannot_initialize_int() {
    let err = emit_err("let x: int = 1.0;");
    assert!(err.contains("cannot assign") || err.contains("number"));
}

#[test]
fn assigning_to_a_const_is_rejected() {
    let err = emit_err("const x = 1; x = 2;");
    assert!(err.contains("cannot assign to const"));
}

#[test]
fn an_undefined_variable_is_rejected() {
    let err = emit_err("print(nope);");
    assert!(err.contains("undefined variable"));
}

#[test]
fn comparing_int_and_number_is_rejected() {
    let err = emit_err("let a: int = 1; print(a == 1.0);");
    assert!(err.contains("cannot compare"));
}

#[test]
fn emitted_asm_carries_source_locations() {
    let asm = emit_asm("let x = 1;\nprint(x);");
    assert!(
        asm.contains(".file 1 \"test.ts\""),
        "expected a .file directive in:\n{asm}"
    );
    assert!(asm.contains(".loc 1 "), "expected .loc directives in:\n{asm}");
}
