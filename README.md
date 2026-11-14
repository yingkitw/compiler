# compiler

[![crates.io](https://img.shields.io/crates/v/compiler.svg)](https://crates.io/crates/compiler)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A small Rust project that compiles a **statically typed subset** of TypeScript to
a native binary: hand-written lexer and parser, a type-checking backend that
emits assembly for the host (AArch64 on Apple silicon), then `cc` assembles
and links it with a tiny C runtime. No LLVM, no JS engine, no third-party
dependencies at all.

```sh
cargo install compiler   # installs `tsc` and `tscc` (same binary; prefer `tscc` if Microsoft's `tsc` is on PATH)
```

You need Rust 1.96+ and a C compiler on `PATH` (it assembles the emitted
`.s` and links the runtime — both `-c` and the default mode use it).

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
- Built-ins: `print(number | int | boolean | string)`.
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
| `runtime.c` | `print`, the string/array bump arena, stack-overflow + SIGFPE guards, float remainder, saturating `number→int` |

[`ARCHITECTURE.md`](ARCHITECTURE.md) and [`SPEC.md`](SPEC.md) have the detail.
Emitted objects can carry a DWARF line table back to the `.ts` file (see SPEC).

## Benchmark

Same programs, several toolchains; the harness requires matching output. Figures
below come from the committed [`bench/RESULTS.md`](bench/RESULTS.md) (regenerate
with `python3 scripts/bench.py`). That run had load average of 198.9 and a 20.07 ms
spawn floor — the host was busy, but the numbers moved together, so the ratios
are what this table shows.

| | compiler | scriptc | compiler v0.1.0 | deno compile | bun --compile | tsrust + node | tsc + node |
|---|---|---|---|---|---|---|---|
| **Artifact size** | **35.0 KiB** | 108.9 KiB | 32.9 KiB | 65.1 MiB | 60.5 MiB | 133 B JS | 119 B JS |
| **Startup** | 33.04ms | **23.77ms** | 26.49ms | 82.69ms | 47.60ms | 153.54ms | 143.98ms |
| **Compile time** | 0.27s | 0.27s | 0.20s | 2.24s | 2.86s | 1.21s | 0.69s |
| **fib(35) runtime** | **28.87ms** | 58.41ms | 72.09ms | 216.54ms | 136.07ms | 292.95ms | 260.70ms |
| **fib(35) unrewritten** | 122.47ms | **49.50ms** | 70.18ms | 139.23ms | 94.33ms | 259.11ms | 250.34ms |
| **loop(1e8) runtime** | 138.42ms | **123.71ms** | 134.11ms | 157.54ms | 157.46ms | 280.46ms | 260.61ms |

We are still small and quick to start — no embedded VM — and the recurrence
rewrite keeps the natural-shape fibonacci the fastest in the set. The assembly
backend that replaced Cranelift in v0.1.3 was a measured regression in raw code
quality (the tight loop ran ~5x slower than our own published v0.1.0); the
register cache closed it: the loop is now at the engines' level — level with
our own v0.1.0 and faster than `deno compile` and `bun --compile` — and
unrewritten recursion is within 1.75x of v0.1.0 (it was 3.4x), still behind
`scriptc`, whose own recursion rewrite cuts the number of calls. The `int` type
is **1.88x** faster on a pure arithmetic loop and **2.21x** faster on an
unrewritten recursive fib in that report. Treat the table as a snapshot, not a
verdict.

## Testing

```sh
cargo test   # unit tests + compile/run every example under examples/
```

See also `tests/rewrite_*.rs`, `tests/abi.rs`, `tests/sanitize.rs`,
`tests/debug_line.rs`, and `tests/fuzz.rs`.

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
