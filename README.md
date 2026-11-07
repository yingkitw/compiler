# compiler

[![crates.io](https://img.shields.io/crates/v/compiler.svg)](https://crates.io/crates/compiler)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A tiny TypeScript-subset compiler that emits **native binaries**, written in Rust.

Hand-written lexer and recursive-descent parser, type-checking code generation on
top of [Cranelift](https://cranelift.dev/), and a small C runtime linked
with the system C compiler. No LLVM, no JavaScript engine, no runtime
dependency beyond libc and libm.

```sh
cargo install compiler     # installs a binary named `tsc`
```

Requires Rust 1.96+ and a C compiler (`cc`) on `PATH` for linking.

---

## Quick start

```ts
// hello.ts
function fib(n: number): number {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}

print(fib(20));
```

```sh
$ tsc hello.ts -o hello
$ ./hello
6765
```

`hello` is a real native executable — no VM, no runtime, no interpreter in the
loop.

## Usage

```sh
tsc <input.ts> [-o <output>] [-c]
```

| Flag | Meaning |
|---|---|
| `-o`, `--output <path>` | Output path. Default: the input path with its extension replaced |
| `-c`, `--compile-only` | Emit an object file only, don't link. Default: `<stem>.o` |
| `-V`, `--version` | Print the compiler's name and version |
| `-h`, `--help` | Print usage |

The link step uses the C compiler named by `CC` (`cc` by default) with the
flags from `CFLAGS` — the conventional ones, and how you link the runtime with
the sanitizers.

Exit codes: `0` success · `1` compile or link error · `2` usage error.

## The language

A deliberately small, statically-typed subset of TypeScript.

```ts
// Types: number (IEEE-754 double), int (64-bit, wrapping), boolean, void.
// Annotations are optional wherever the type is inferrable.
let x: number = 40;
let y = 2;                    // inferred: number
const pi: number = 3.14159;
let k: int = -5;              // negative int literals work

// Functions — recursion and forward references both work.
// A self-recursive call in tail position is turned into a loop, so a tail
// recursion runs in constant stack space.
function double(n: number): number { return n * 2; }
// A linear recurrence — a function defined in terms of its own previous
// values — is rewritten into a loop as well, so fib(90) is an astronomical
// number of calls as written and ninety after the rewrite.
//
// The rewrite needs an `int` parameter: the loop counts the values it
// computes with its own index, and a fractional argument would make the
// loop's count and the recursion's depth disagree. A `number` fibonacci is
// left exactly as it was written.
function fib(n: int): int {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}

// Control flow.
let i = 0;
let sum = 0;
while (i < 10) {
    sum = sum + i;
    i = i + 1;
}
print(sum);                   // 45

// A C-style `for` loop. The init runs once, then it is a `while` whose body
// is the loop body followed by the step — so `break`/`continue` are the only
// things missing, and each init has its own scope.
let total = 0;
for (let j = 0; j < 5; j = j + 1) {
    total = total + j * j;
}
print(total);                 // 0 + 1 + 4 + 9 + 16 = 30

// Operators:  + - * / %   == != < <= > >=   && || (short-circuit)   !   unary -
// Truthiness: a number is false iff it equals 0.

// Entry points: top-level statements run first, then main() if you define one.
function main(): void {
    print(fib(20));           // 6765
    print(double(21));        // 42
    print(!false);            // true
}
```

### Type rules

The checker is strict — there are no implicit conversions.

- Arithmetic (`+ - * / %`) and relational operators (`< <= > >=`) require both
  operands to have the *same* numeric type — two `number`s or two `int`s.
  There is no implicit conversion between them.
- `int` arithmetic wraps (there is no overflow check) and division by zero
  reports `error: integer division by zero or overflow` at runtime.
- Equality (`== !=`) requires both operands to have the *same* type.
- `&&`, `||`, and `!` accept `boolean`, `number` or `int` and always yield
  `boolean`. They **short-circuit**: in `false && (1 / 0 > 0)`, the right side
  is never evaluated.
- Assignment requires the value's type to match the variable's type exactly.
- A function whose return type isn't `void` must return a value on every path.
  `void` cannot be the type of a variable or a parameter.

Type errors are reported at compile time, not at runtime:

```
$ tsc bad.ts
error: cannot initialize 'x: number' with a value of type boolean
```

### Built-ins

| Function | Behaviour |
|---|---|
| `print(number)` | Prints the value and a newline (`42`, `12.56636`) — formatted exactly like JavaScript's `Number.prototype.toString` |
| `print(int)` | Prints the value and a newline (`9007199254740993` stays exact) |
| `print(boolean)` | Prints `true` or `false` |

## How it works

```
 source ──lex──▶ tokens ──parse──▶ AST ──rewrite──▶ AST ──codegen──▶ object ──cc──▶ binary
  lexer.rs      parser/    ast.rs   rewrite/    ast.rs   codegen/   (Cranelift)  runtime.c
```

| Stage | What happens |
|---|---|
| `src/lexer.rs` | Tokenizer. Every token carries its source line, so diagnostics point at a line |
| `src/parser/` | Recursive descent — `stmt.rs` for the statement forms, `expr.rs` for one method per precedence level |
| `src/ast.rs` | Shared AST types — the contract between parser and codegen |
| `src/rewrite/` | Source-to-source optimisations: a self-recursive call in tail position becomes a loop, and a linear recurrence becomes a loop as well |
| `src/codegen/` | One pass that type-checks **and** emits Cranelift IR: SSA variables, basic blocks for `if`/`while`, and for short-circuit `&&` / `\|\|` |

Five design points worth knowing:

- **A tiny C runtime instead of variadic `printf`.** The generated code imports
  `ts_print_number(double)`, `ts_print_bool(signed char)` and
  `ts_print_int(long long)` — fixed-arity functions with an unambiguous ABI,
  linked alongside the object file.
- **Position-independent code is always on.** Without it, external calls fail to
  link on macOS arm64 with "Illegal text-relocations".
- **A stack overflow is an error, not a crash.** Unbounded recursion used to
  kill the process with `SIGSEGV`. The runtime now installs a handler on an
  alternate signal stack, so the program reports
  `error: maximum recursion depth exceeded` and exits with status 1 — and
  whatever it printed before the overflow is preserved.
- **A tail call costs no stack.** A self-recursive call in tail position is
  rewritten into a loop before code generation, so the recursion runs in
  constant stack space — ten million levels deep, where it used to overflow.
  The rewrite only fires on a shape it can recognise exactly (see
  [SPEC.md](SPEC.md)); anything else is compiled as written.
- **A linear recurrence is not exponential.** A function defined in terms of
  its own previous values — `fib(n) = fib(n-1) + fib(n-2)` — is rewritten
  into a loop with two accumulators, so the same program computes the same
  answer in linear time and in constant stack space. `fib(90)` is an
  astronomical number of calls as written; after the rewrite it is ninety.
  The rewrite needs an `int` parameter — the loop counts integers, so a
  fractional argument would make its count and the recursion's depth
  disagree — so a `number` fibonacci is left exactly as it was written.

Read [ARCHITECTURE.md](ARCHITECTURE.md) for the full design rationale, and
[SPEC.md](SPEC.md) for the complete grammar and CLI contract.

## Not supported yet

Strings, arrays, objects, classes, closures, generics, modules, `do`/
`switch`, `continue`/`break`, `++`/`--`, compound assignment, `? :`,
`throw`/`try`, and arrow functions. `void` is a return type only — a
variable or a parameter cannot have it. The full list is in
[SPEC.md](SPEC.md).

`number` is an IEEE-754 double, so it is exact only up to 2^53. Use the `int`
type (64-bit, wrapping) when the value has to be exact; an `int` division by
zero is a runtime error rather than an `Infinity`.

This is a compiler for a *subset*, not a TypeScript implementation. It is meant
as a readable reference for how a native code generator is put together — and as
a fast way to turn small numeric programs into fast binaries.

## Benchmark

The same program, compiled by every toolchain that can turn TypeScript into
something runnable. Every toolchain must print the same result — the harness
fails if they disagree.

| | compiler | scriptc | compiler v0.1.0 | deno compile | bun --compile | tsrust + node | tsc + node |
|---|---|---|---|---|---|---|---|
| **Artifact size** | **33.8 KiB** | 109 KiB | 32.9 KiB | 65.1 MiB | 60.5 MiB | 133 B JS | 119 B JS |
| **Startup** | **22.9 ms** | 21.8 ms | 21.7 ms | 32.0 ms | 34.1 ms | 153 ms | 150 ms |
| **Compile time** | **0.21 s** | 0.12 s | 0.17 s | 1.15 s | 1.07 s | 0.39 s | 0.43 s |
| **fib(35) runtime** | 68.2 ms | **48.3 ms** | 67.4 ms | 130 ms | 81.0 ms | 242 ms | 241 ms |
| **fib(35) unrewritten** | 69.4 ms | **46.0 ms** | 68.0 ms | 131 ms | 81.2 ms | 240 ms | 239 ms |
| **loop(1e8) runtime** | **122 ms** | 123 ms | 126 ms | 134 ms | 130 ms | 248 ms | 255 ms |

The **Startup** and **Compile time** rows are from the startup workload; the
other three are from the workload they name.

Full numbers, methodology, and the raw report: [`bench/RESULTS.md`](bench/RESULTS.md).
Regenerate with `python3 scripts/bench.py`.

The **Startup** row is the one sensitive to the host: spawning a process costs
~5 ms on an idle machine and ~20 ms on a busy one, which changes every ratio
without changing the differences. The committed report was taken with a load
average of 16.8 and a 17.7 ms spawn floor, so the startup row above is a
busy-host number; on an idle host it reads about a third of that. The report
records the load average of the run it came from, so you can tell which
numbers to trust.

**Where we win.** `deno compile` and `bun build --compile` embed a JavaScript
engine (V8 / JavaScriptCore) in the binary. We compile to machine code, so
there is no engine to ship and nothing to initialise at startup — a 33 KiB
binary instead of 60 MiB, ~2000x smaller, and ~1.4x faster to start.

**Where we lose.** The honest comparison is [`scriptc`](https://scriptc.dev)
(Vercel Labs), the only other engine-free native TypeScript compiler. It beats
us on the recursive workloads: **48 ms against our 68 ms** on the fibonacci, and
**46 ms against our 69 ms** on the version written so no rewrite can fire.
Profiling both binaries shows this is **not** a difference in the cost of a
call — it is a difference in **how many calls are made**: scriptc rewrites a
self-recursive function into a loop around a single recursive call, which cuts
the number of function entries by 5.24x. We do the same for the *chain* shape
(`f(n) = f(n-1)` is a loop on our side too) and for a *linear recurrence*
(`f(n) = f(n-1) + f(n-2)` is a loop with two accumulators) — but scriptc
recognises more shapes than we do, and a shape we do not recognise is left
exactly as it was written. Our tight loop already ties scriptc, and switching
that loop to 64-bit integers makes it **1.52x** faster while the same change
does not help recursive calls — the arithmetic was never the issue.

**A linear recurrence is not exponential.** This is the rewrite at work, and
it is the biggest thing the compiler does that a JIT cannot do. The same
fibonacci, twice, both using `int`; the only difference is the shape:

| | runtime |
|---|---|
| rewritten into a loop | **25.3 ms** — about the cost of starting the process |
| left as a recursion | 69.8 ms |

The rewrite needs an `int` parameter — the loop counts integers, so a
fractional argument would make its count and the recursion's depth disagree —
so a `number` fibonacci is left exactly as written. That is why the two
fibonacci rows above are both exponential.

**Where nobody wins.** On a tight numeric counting loop, V8 and
JavaScriptCore are excellent at that shape, and Cranelift's optimiser does
not beat a mature JIT — we are within noise of all of them (~1.0x).

**A note on `tsrust`.** [`tsrust`](https://github.com/ferrotype/tsr) is
TypeScript 7's Go compiler ported to Rust. It emits the same JavaScript as
`tsc` and performs identically (242 ms vs 241 ms on fib) — being written in
Rust does not make the *output* faster, only the compiler. It is the sharpest
illustration of what this project is doing differently: we do not emit
JavaScript at all.

**The `int` type.** `int` (64-bit) keeps arithmetic exact past 2^53 and is
measurably faster where arithmetic is the work: **1.52x** on an arithmetic-only
loop. It does not help on recursion — the `int` version of the unrewritten
fibonacci is **1.01x slower** — which is the evidence that
the gap to scriptc is not about arithmetic. It is not a cross-toolchain
comparison — the JavaScript toolchains have no integer type.

## Testing

```sh
cargo test    # compiles and runs every example, plus type-error cases
```

The test suite compiles each program in `examples/` to a native binary, runs it,
and asserts its exact output — so the tests check *generated code*, not just
generated IR. Beyond that:

- `tests/rewrite.rs` — the rewrites are compiled both as written and as the
  equivalent hand-written program, and the two must agree exactly. Differential
  testing is how two floating-point miscompiles were caught.
- `tests/abi.rs` — a C driver calls the generated code, pinning the calling
  convention against the platform's.
- `tests/sanitize.rs` — every example is linked with the address and
  undefined-behaviour sanitizers and run (skipped when the sanitizers are
  unavailable).
- `tests/fuzz.rs` — 300 random token sequences must not panic the compiler
  and must exit with a documented code.

## Documentation

| Doc | Purpose |
|---|---|
| [SPEC.md](SPEC.md) | CLI contract, grammar, type rules, runtime ABI |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module relationships, data flow, design decisions |
| [TODO.md](TODO.md) | Roadmap, completed work, scorecard |
| [bench/RESULTS.md](bench/RESULTS.md) | Competitive benchmark against scriptc, deno, bun, tsrust, and tsc+node |

`AGENTS.md` and `MEMORY.md` are agent-local working documents — the development
loop and the harvested knowledge base. They are gitignored and not shipped with
the crate.

## License

Apache-2.0 — see [LICENSE](LICENSE).
