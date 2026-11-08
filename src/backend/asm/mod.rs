//! A small assembler for the target.
//!
//! The code generator does not write assembly text. It asks this module
//! for instructions — a move, an add, a branch — and this writes them.
//! Making the operands types rather than strings is the point: a register
//! cannot be written where an immediate belongs, a label cannot be used as
//! an address, and the mistakes the assembler would only report after the
//! program had been compiled become compile errors here.
//!
//! The output is the target's assembly, which the system C compiler
//! assembles and links.

use std::fmt;
use std::fmt::Write as _;
use std::path::Path;

pub mod inst;
pub mod reg;

use reg::X;

/// A branch condition.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cond {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    /// Strictly below: the floating-point "less than".
    Mi,
    /// Below or equal: the floating-point "less than or equal".
    Ls,
}

impl fmt::Display for Cond {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Cond::Eq => "eq",
            Cond::Ne => "ne",
            Cond::Lt => "lt",
            Cond::Le => "le",
            Cond::Gt => "gt",
            Cond::Ge => "ge",
            Cond::Mi => "mi",
            Cond::Ls => "ls",
        };
        f.write_str(name)
    }
}

/// A label in the generated code.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Label(pub String);

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A memory operand: a register plus an offset.
#[derive(Clone, Copy, Debug)]
pub struct Mem {
    pub base: X,
    pub offset: i32,
}

impl fmt::Display for Mem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.offset == 0 {
            write!(f, "[{}]", self.base)
        } else {
            write!(f, "[{}, #{}]", self.base, self.offset)
        }
    }
}

/// The assembler: the text of a function, and the labels in it.
pub struct Asm {
    text: String,
    labels: u32,
    /// The prefix every label of this function carries, so that two
    /// functions never define the same one.
    prefix: String,
}

impl Asm {
    /// A fresh assembler for a function.
    pub fn new(prefix: &str) -> Asm {
        Asm {
            text: String::new(),
            labels: 0,
            prefix: prefix.to_string(),
        }
    }

    /// The text written so far.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Take the text.
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.text)
    }

    /// A fresh label.
    pub fn label(&mut self) -> Label {
        self.labels += 1;
        Label(format!(".L{}_{}", self.prefix, self.labels))
    }

    /// A label with a name of its own.
    pub fn named_label(&mut self, name: &str) -> Label {
        Label(format!(".L{}", name))
    }

    /// Write a line of assembly — for the directives, which are not
    /// instructions.
    pub fn line(&mut self, text: &str) {
        let _ = writeln!(self.text, "\t{}", text);
    }

    /// A `.file` directive for DWARF line tables (file index 1).
    pub fn debug_file_directive(source: &Path) -> String {
        let name = source
            .to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"");
        format!("\t.file 1 \"{name}\"")
    }

    /// Map the following instructions to a source line (1-based).
    pub fn loc(&mut self, line: u32) {
        if line == 0 {
            return;
        }
        let _ = writeln!(self.text, "\t.loc 1 {line} 0");
    }

    /// Define a label.
    pub fn place(&mut self, label: &Label) {
        let _ = writeln!(self.text, "{}:", label);
    }

    /// Define a symbol — the name of a function, which is not a label.
    pub fn place_symbol(&mut self, name: &str) {
        let _ = writeln!(self.text, "{}:", name);
    }
}
