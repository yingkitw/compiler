//! The instructions the code generator uses.
//!
//! Each one writes the target's syntax for a single operation, with the
//! operands typed: a register where a register belongs, an immediate where
//! an immediate belongs. A call that does not type-check does not compile,
//! which is worth more than the assembler catching it later.

use std::fmt::Write as _;

use super::reg::{D, X};
use super::{Asm, Cond, Label, Mem};

impl Asm {
    // --- moving values -------------------------------------------------

    /// `mov` a register from an immediate that fits in a single `mov`.
    pub fn mov_imm(&mut self, dst: X, imm: i64) {
        let _ = writeln!(self.text, "\tmov\t{}, #{}", dst, imm);
    }

    /// Put an integer immediate into a general register — `mov` when the
    /// value fits, otherwise a literal-pool load.
    pub fn mov_x_imm(&mut self, dst: X, imm: i64) {
        if (0..=0xFFFF).contains(&imm) {
            self.mov_imm(dst, imm);
        } else {
            self.load_imm(dst, imm);
        }
    }

    /// `mov` a register from another.
    pub fn mov_reg(&mut self, dst: X, src: X) {
        let _ = writeln!(self.text, "\tmov\t{}, {}", dst, src);
    }

    /// Load an immediate into a register through the literal pool — for a
    /// value too large for a `mov` immediate.
    pub fn load_imm(&mut self, dst: X, imm: i64) {
        let _ = writeln!(self.text, "\tldr\t{}, ={}", dst, imm);
    }

    /// Move a double into another double register.
    pub fn fmov_d_from_d(&mut self, dst: D, src: D) {
        let _ = writeln!(self.text, "\tfmov\t{}, {}", dst, src);
    }

    /// `fmov` a double from an immediate the hardware can encode:
    /// ±(1.mmmm)₂ × 2ᴱ, four mantissa bits and E in −3…4 (plus ±0.0).
    /// Returns false when the value needs the literal pool instead.
    pub fn fmov_d_imm(&mut self, dst: D, value: f64) -> bool {
        let bits = value.to_bits();
        let exp = ((bits >> 52) & 0x7FF) as i32 - 1023;
        let encodable = bits == 0
            || bits == 0x8000_0000_0000_0000
            || (bits & 0x0000_FFFF_FFFF_FFFF == 0 && (-3..=4).contains(&exp));
        if !encodable {
            return false;
        }
        let _ = writeln!(self.text, "\tfmov\t{}, #{}", dst, value);
        true
    }

    /// Move the bits of a general register into a double.
    pub fn fmov_d_from_x(&mut self, dst: D, src: X) {
        let _ = writeln!(self.text, "\tfmov\t{}, {}", dst, src);
    }

    // --- the stack -----------------------------------------------------

    /// Allocate a frame: the frame pointer and the link register are saved.
    /// The frame pointer ends up at the high end of the frame so locals at
    /// negative offsets from it stay inside the allocated block.
    pub fn push_frame(&mut self, size: i32) {
        let _ = writeln!(self.text, "\tstp\tx29, x30, [sp, #-{}]!", size);
        let _ = writeln!(self.text, "\tadd\tx29, sp, #{}", size);
    }

    /// Release a frame.
    pub fn pop_frame(&mut self, size: i32) {
        let _ = writeln!(self.text, "\tldp\tx29, x30, [sp], #{}", size);
    }

    /// Store a pair of general registers.
    pub fn store_pair_x(&mut self, a: X, b: X, mem: Mem) {
        let _ = writeln!(self.text, "\tstp\t{}, {}, {}", a, b, mem);
    }

    /// Load a pair of general registers.
    pub fn load_pair_x(&mut self, a: X, b: X, mem: Mem) {
        let _ = writeln!(self.text, "\tldp\t{}, {}, {}", a, b, mem);
    }

    /// Store a pair of floating-point registers.
    pub fn store_pair_d(&mut self, a: D, b: D, mem: Mem) {
        let _ = writeln!(self.text, "\tstp\t{}, {}, {}", a, b, mem);
    }

    /// Load a pair of floating-point registers.
    pub fn load_pair_d(&mut self, a: D, b: D, mem: Mem) {
        let _ = writeln!(self.text, "\tldp\t{}, {}, {}", a, b, mem);
    }

    /// Return from a function.
    pub fn ret(&mut self) {
        let _ = writeln!(self.text, "\tret");
    }

    // --- loading and storing -------------------------------------------

    /// Load a general register from memory.
    pub fn load(&mut self, dst: X, mem: Mem) {
        let _ = writeln!(self.text, "\tldr\t{}, {}", dst, mem);
    }

    /// Load a double from memory.
    pub fn load_double(&mut self, dst: D, mem: Mem) {
        let _ = writeln!(self.text, "\tldr\t{}, {}", dst, mem);
    }

    /// Store a general register to memory.
    pub fn store(&mut self, src: X, mem: Mem) {
        let _ = writeln!(self.text, "\tstr\t{}, {}", src, mem);
    }

    /// Store a double to memory.
    pub fn store_double(&mut self, src: D, mem: Mem) {
        let _ = writeln!(self.text, "\tstr\t{}, {}", src, mem);
    }

    // --- arithmetic ----------------------------------------------------

    /// Add two general registers. The instruction takes three operands: the
    /// destination is not a source.
    pub fn add(&mut self, dst: X, lhs: X, rhs: X) {
        let _ = writeln!(self.text, "\tadd\t{}, {}, {}", dst, lhs, rhs);
    }

    /// Subtract two general registers.
    pub fn sub(&mut self, dst: X, lhs: X, rhs: X) {
        let _ = writeln!(self.text, "\tsub\t{}, {}, {}", dst, lhs, rhs);
    }

    /// Multiply two general registers.
    pub fn mul(&mut self, dst: X, lhs: X, rhs: X) {
        let _ = writeln!(self.text, "\tmul\t{}, {}, {}", dst, lhs, rhs);
    }

    /// Signed division — an integer division by zero traps, as it should.
    pub fn sdiv(&mut self, dst: X, lhs: X, rhs: X) {
        let _ = writeln!(self.text, "\tsdiv\t{}, {}, {}", dst, lhs, rhs);
    }

    /// Signed remainder: `dst = dividend % divisor`. The quotient goes
    /// through `x2`, so the dividend and the divisor may live in any
    /// registers — including `dst` itself.
    pub fn srem(&mut self, dst: X, dividend: X, divisor: X) {
        let q = X(2);
        let _ = writeln!(self.text, "\tsdiv\t{}, {}, {}", q, dividend, divisor);
        // dst = dividend − quotient × divisor
        let _ = writeln!(
            self.text,
            "\tmsub\t{}, {}, {}, {}",
            dst, q, divisor, dividend
        );
    }

    /// Negate a general register.
    pub fn neg(&mut self, dst: X) {
        let _ = writeln!(self.text, "\tneg\t{}, {}", dst, dst);
    }

    /// Add, subtract, multiply or divide two doubles. These take three
    /// operands as well.
    pub fn fadd(&mut self, dst: D, lhs: D, rhs: D) {
        let _ = writeln!(self.text, "\tfadd\t{}, {}, {}", dst, lhs, rhs);
    }

    pub fn fsub(&mut self, dst: D, lhs: D, rhs: D) {
        let _ = writeln!(self.text, "\tfsub\t{}, {}, {}", dst, lhs, rhs);
    }

    pub fn fmul(&mut self, dst: D, lhs: D, rhs: D) {
        let _ = writeln!(self.text, "\tfmul\t{}, {}, {}", dst, lhs, rhs);
    }

    pub fn fdiv(&mut self, dst: D, lhs: D, rhs: D) {
        let _ = writeln!(self.text, "\tfdiv\t{}, {}, {}", dst, lhs, rhs);
    }

    /// Negate a double.
    pub fn fneg(&mut self, dst: D) {
        let _ = writeln!(self.text, "\tfneg\t{}, {}", dst, dst);
    }

    /// Convert an integer to a double.
    pub fn int_to_double(&mut self, dst: D, src: X) {
        let _ = writeln!(self.text, "\tscvtf\t{}, {}", dst, src);
    }

    // --- comparing and branching ----------------------------------------

    /// Compare two general registers.
    pub fn cmp(&mut self, lhs: X, rhs: X) {
        let _ = writeln!(self.text, "\tcmp\t{}, {}", lhs, rhs);
    }

    /// Compare a register with an immediate.
    pub fn cmp_imm(&mut self, lhs: X, imm: i64) {
        let _ = writeln!(self.text, "\tcmp\t{}, #{}", lhs, imm);
    }

    /// Compare two doubles.
    pub fn fcmp(&mut self, lhs: D, rhs: D) {
        let _ = writeln!(self.text, "\tfcmp\t{}, {}", lhs, rhs);
    }

    /// Leave the result of a comparison in a register: true is 1, false 0.
    pub fn set(&mut self, dst: X, cond: Cond) {
        let _ = writeln!(self.text, "\tcset\t{}, {}", dst, cond);
    }

    /// Branch if the last comparison holds.
    pub fn branch_if(&mut self, cond: Cond, label: &Label) {
        let _ = writeln!(self.text, "\tb.{}\t{}", cond, label);
    }

    /// Branch to a label.
    pub fn branch(&mut self, label: &Label) {
        let _ = writeln!(self.text, "\tb\t{}", label);
    }

    /// Call a function.
    pub fn call(&mut self, symbol: &str) {
        let _ = writeln!(self.text, "\tbl\t{}", symbol);
    }

    /// Load the address of a symbol (Mach-O PIC: `@PAGE` / `@PAGEOFF`).
    pub fn load_pie_addr(&mut self, dst: X, label: &str) {
        let _ = writeln!(self.text, "\tadrp\t{}, {}@PAGE", dst, label);
        let _ = writeln!(self.text, "\tadd\t{}, {}, {}@PAGEOFF", dst, dst, label);
    }

    /// Emit read-only bytes for a string literal, then return to `.text`.
    pub fn emit_string_rodata(&mut self, label: &str, bytes: &[u8]) {
        let _ = writeln!(self.text, "\t.section __TEXT,__const");
        let _ = writeln!(self.text, "{label}:");
        if bytes.is_empty() {
            let _ = writeln!(self.text, "\t.byte 0");
        } else {
            let mut line = String::from("\t.byte ");
            for (i, b) in bytes.iter().enumerate() {
                if i > 0 {
                    line.push_str(", ");
                }
                let _ = write!(line, "{b}");
            }
            let _ = writeln!(self.text, "{line}");
        }
        let _ = writeln!(self.text, "\t.text");
    }
}
