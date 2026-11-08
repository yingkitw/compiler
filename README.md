# compiler

[![crates.io](https://img.shields.io/crates/v/compiler.svg)](https://crates.io/crates/compiler)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A small Rust project that compiles a **statically typed subset** of TypeScript to
a native binary: hand-written lexer and parser, a type-checking backend that
emits assembly for the host (AArch64 on Apple silicon), then `cc` links a tiny
C runtime. No LLVM, no JS engine, no third-party codegen crates.

```sh
cargo install compiler   # installs `tsc` and `tscc` (same binary; prefer `tscc` if Microsoft's `tsc` is on PATH)
```

You need Rust 1.96+ and a C compiler on `PATH` for linking.

## Quick start

```ts
// hello.ts
let x: number = 40;
let y = 2.0;
print(x + y);
```

```sh
$ tsc hello.ts -o hello && ./hello
42
```

More examples live in [`examples/`](examples/).

## Usage

```sh
tsc <input.ts> [-o <output>] [-c]
```

| Flag | Meaning |
|---|---|
| `-o`, `--output <path>` | Output path (default: input stem) |
| `-c`, `--compile-only` | Object file only (default: `<stem>.o`) |
| `-V`, `--version` | Version |
| `-h`, `--help` | Usage |

Linking honours `CC` and `CFLAGS`. Exit codes: `0` ok · `1` compile/link error · `2` usage error.

## What the language covers

This is **not** a full TypeScript implementation. See [SPEC.md](SPEC.md) for the
grammar and type rules. In short:

- Types: `number`, `int`, `boolean`, `string`, `number[]` / `boolean[]`, `void`
  (return type only).
- Functions, recursion, `if` / `while` / C-style `for` / `for…of`, `as` casts,
  compound assignment, `++`/`--`, `&&` / `||` (short-circuit), `?:`, block
  expressions, and a few logical assignment forms.
- Built-ins: `print(number | int | boolean)`.
- Optional `function main(): void` runs after top-level statements.

The checker is strict: no implicit numeric conversions; mismatches are compile
errors. Parse errors are reported as `line N, column M:`.

Two source rewrites run before codegen when the shape matches exactly: tail calls
and simple linear recurrences become loops. Unrecognised shapes are compiled as
written — performance is not guaranteed.

Still missing (among other things): modules, classes, closures, generics,
`throw`/`try`, `break`/`continue`, and most of the rest of TypeScript.

## How it fits together

```
source → lexer → parser → AST → rewrite → backend → .s → cc → binary
```

| Piece | Role |
|---|---|
| `lexer.rs` | Tokens (with line/column for diagnostics) |
| `parser/` | AST |
| `rewrite/` | Tail-call and recurrence → loop when safe |
| `backend/` | Type-check + emit assembly |
| `runtime.c` | `print`, stack-overflow guard, float remainder |

[`ARCHITECTURE.md`](ARCHITECTURE.md) and [`SPEC.md`](SPEC.md) have the detail.
Emitted objects can carry a DWARF line table back to the `.ts` file (see SPEC).

## Benchmark

Same programs, several toolchains; the harness requires matching output. Figures
below come from the committed [`bench/RESULTS.md`](bench/RESULTS.md) (regenerate
with `python3 scripts/bench.py`). That run had load average of 4.4 and a 17.2 ms
spawn floor — startup times move with host load.

| | compiler | scriptc | compiler v0.1.0 | deno compile | bun --compile | tsrust + node | tsc + node |
|---|---|---|---|---|---|---|---|
| **Artifact size** | **33.8 KiB** | 109 KiB | 32.9 KiB | 65.1 MiB | 60.5 MiB | 133 B JS | 119 B JS |
| **Startup** | **24.7 ms** | 23.2 ms | 24.0 ms | 34.1 ms | 35.4 ms | 149 ms | 145 ms |
| **Compile time** | **0.19 s** | 0.10 s | 0.17 s | 1.01 s | 0.70 s | 0.40 s | 0.32 s |
| **fib(35) runtime** | **23.4 ms** | 49.8 ms | 66.4 ms | 124 ms | 82.0 ms | 239 ms | 236 ms |
| **fib(35) unrewritten** | 66.8 ms | **45.8 ms** | 64.9 ms | 124 ms | 81.7 ms | 237 ms | 236 ms |
| **loop(1e8) runtime** | **121 ms** | 135 ms | 133 ms | 135 ms | 139 ms | 250 ms | 245 ms |

We are small and quick to start because there is no embedded VM. On numeric loops
we are roughly even with mature JITs; on some fibonacci shapes
[`scriptc`](https://scriptc.dev) is faster than us, and on others our recurrence
rewrite helps. The `int` type is **1.55x** faster on a pure arithmetic loop in
that report; it does **1.03x slower** on an unrewritten recursive fib — the
gaps are as much about call patterns as about arithmetic. Treat the table as a
snapshot, not a verdict.

## Testing

```sh
cargo test   # unit tests + compile/run every example under examples/
```

See also `tests/rewrite_*.rs`, `tests/abi.rs`, `tests/sanitize.rs`, and
`tests/fuzz.rs`.

## Documentation

| Doc | Purpose |
|---|---|
| [SPEC.md](SPEC.md) | Grammar, types, CLI, runtime ABI |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Modules and data flow |
| [TODO.md](TODO.md) | Roadmap and scorecard |
| [bench/RESULTS.md](bench/RESULTS.md) | Full benchmark report |

`AGENTS.md` and `MEMORY.md` are agent-local notes (gitignored).

## License

Apache-2.0 — see [LICENSE](LICENSE).
