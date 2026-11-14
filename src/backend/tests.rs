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
fn number_remainder_calls_ts_fmod() {
    let asm = emit_asm("let x = 5.5;\nlet y = -2.0;\nprint(x % y);");
    assert!(asm.contains("bl\t_ts_fmod"));
    // Both locals are register residents, so the operands are staged into
    // `d0`/`d1` through scratch registers — writing them directly would
    // clobber the operands, since `d0` aliases `v0`.
    assert!(asm.contains("fmov\td2, d8"));
    assert!(asm.contains("fmov\td3, d9"));
    assert!(asm.contains("fmov\td0, d2"));
    assert!(asm.contains("fmov\td1, d3"));
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

#[test]
fn a_loop_keeps_its_counter_in_a_callee_saved_register() {
    let asm = emit_asm("let i = 0;\nwhile (i < 10) { i = i + 1; }\nprint(i);");
    // The counter lives in a register across the back edge, so the
    // addition writes the register itself.
    assert!(
        asm.contains("fadd\td8, d8, d0"),
        "expected the counter to be updated in place:\n{asm}"
    );
    // ... and the function saves and restores it, because the callee-
    // saved registers are the only ones a call may not clobber.
    assert!(asm.contains("str\td8,"), "expected a save:\n{asm}");
    assert!(asm.contains("ldr\td8,"), "expected a restore:\n{asm}");
}

#[test]
fn a_binary_operation_takes_its_operands_where_they_are() {
    let asm = emit_asm("let a = 1;\nlet b = 2;\nprint(a + b);");
    // Both operands are register residents: the addition reads them in
    // place, with no trip through the stack.
    assert!(
        asm.contains("fadd\td0, d8, d9"),
        "expected the operands to be used in place:\n{asm}"
    );
}

#[test]
fn a_comparison_branches_on_its_own_flags() {
    let asm = emit_asm("let i = 0;\nwhile (i < 10) { i = i + 1; }");
    // The comparison's flags branch directly — no boolean is
    // materialised and then tested.
    assert!(asm.contains("fcmp\td8, d0"), "expected a compare:\n{asm}");
    assert!(
        !asm.contains("cset"),
        "expected no materialised boolean:\n{asm}"
    );
}

#[test]
fn an_operand_the_other_side_assigns_is_copied_first() {
    let asm = emit_asm("let x = 1;\nprint(x + (x = 5));");
    // A cached register is live storage, not a snapshot: the operand's
    // value is copied aside before the assignment replaces it.
    let copy = asm.find("fmov\td1, d8").expect("the copy");
    let after = &asm[copy..];
    assert!(
        after.contains("fmov\td8, d0"),
        "expected the assignment after the copy:\n{asm}"
    );
}

#[test]
fn a_number_literal_the_hardware_can_encode_needs_no_literal_pool() {
    let asm = emit_asm("print(1.5);");
    assert!(asm.contains("fmov\td0, #1.5"), "expected an immediate:\n{asm}");
    assert!(
        !asm.contains("ldr\tx0, ="),
        "expected no literal pool load:\n{asm}"
    );
}

#[test]
fn a_number_literal_the_hardware_cannot_encode_uses_the_literal_pool() {
    let asm = emit_asm("print(100000000.0);");
    assert!(
        asm.contains("ldr\tx0, ="),
        "expected a literal pool load:\n{asm}"
    );
}
