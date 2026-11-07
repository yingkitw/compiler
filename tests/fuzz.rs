//! The compiler must not crash on a program it cannot parse.
//!
//! A compiler that panics on malformed input is a compiler with a bug: the
//! input is not the programmer's fault, and the process must not die with a
//! backtrace. The exit status has to be one of the documented ones — 0 for
//! success, 1 for a compile error, 2 for a usage error — and never the 101
//! a Rust panic exits with.
//!
//! The input is random *tokens* rather than random bytes, because random
//! bytes almost always fail in the lexer, and the interesting failures are
//! further down: in the parser, and in the type checker.

use std::path::PathBuf;
use std::process::Command;

/// The vocabulary the fuzzer draws from. It is the language's own, so that
/// a random sequence of these is often *almost* a program.
const TOKENS: &[&str] = &[
    "let", "const", "function", "if", "else", "while", "for", "as", "return",
    "true", "false", "number", "int", "boolean", "void", "print", "x", "y", "n",
    "f", "42", "3.5", "(", ")", "{", "}", ";", ",", ":", "=", "==", "!=", "<",
    "<=", ">", ">=", "+", "-", "*", "/", "%", "&&", "||", "!", "+=", "-=", "*=",
    "/=", "%=", "++", "--",
];

/// A tiny linear congruential generator, so that a failing case can be
/// reproduced from its seed.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        // The constants are from the "Numerical Recipes" LCG.
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn out_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("tsc-tests");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_compiler_never_panics_on_malformed_input() {
    let dir = out_dir();
    let src = dir.join("fuzz.ts");
    let bin = dir.join("fuzz.bin");

    for seed in 0..300u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(1));
        let count = 1 + rng.below(24);
        let mut text = String::new();
        for _ in 0..count {
            text.push_str(TOKENS[rng.below(TOKENS.len())]);
            text.push(' ');
        }

        std::fs::write(&src, &text).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_tsc"))
            .arg(&src)
            .arg("-o")
            .arg(&bin)
            .output()
            .expect("failed to run tsc");

        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("panicked"),
            "seed {seed} panicked on {text:?}:\n{stderr}"
        );
        let code = output.status.code().unwrap_or(-1);
        assert!(
            matches!(code, 0..=2),
            "seed {seed} exited with {code} on {text:?}:\n{stderr}"
        );
    }
}
