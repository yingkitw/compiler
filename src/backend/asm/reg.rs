//! The registers of the target.
//!
//! They are a type, not a number, so that a register cannot be written
//! where an immediate belongs — the mistakes the assembler would only
//! report after the program had been compiled.

use std::fmt;

/// A general-purpose register (`x0`–`x30`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct X(pub u8);

/// A floating-point register (`d0`–`d31`), holding a double.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct D(pub u8);

/// The frame pointer (`x29`).
pub const FP: X = X(29);
/// The first register of the argument sequence.
pub const X0: X = X(0);

impl fmt::Display for X {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "x{}", self.0)
    }
}

impl fmt::Display for D {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "d{}", self.0)
    }
}
