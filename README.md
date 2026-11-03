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
| `-h`, `--help` | Print usage |

```sh
tsc app.ts -o build/app     # choose the output path
tsc app.ts -c               # emit app.o, skip linking
tsc app.ts                  # writes ./app
```

Exit codes: `0` success · `1` compile or link error · `2` usage error.

## The language

A deliberately small, statically-typed subset of TypeScript.

```ts
// Types: number (IEEE-754 double), boolean, void.
// Annotations are optional wherever the type is inferrable.
let x: number = 40;
let y = 2;                    // inferred: number
const pi: number = 3.14159;

// Functions — recursion and forward references both work.
// A self-recursive call in tail position is turned into a loop, so a tail
// recursion runs in constant stack space.
function double(n: number): number { return n * 2; }
// A linear recurrence — a function defined in terms of its own previous
// values — is rewritten into a loop as well: fib(90) is 2^90 calls as
// written, and ninety after the rewrite.
function fib(n: number): number {
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

// Operators:  + - * /   == != < <= > >=   && || (short-circuit)   !   unary -
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
- Equality (`== !=`) requires both operands to have the *same* type.
- `&&`, `||`, and `!` accept `boolean` or `number` and always yield `boolean`.
  They **short-circuit**: in `false && (1 / 0 > 0)`, the right side is never
  evaluated.
- Assignment requires the value's type to match the variable's type exactly.
- A function whose return type isn't `void` must return a value on every path.

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

Four design points worth knowing:

- **A tiny C runtime instead of variadic `printf`.** The generated code imports
  `ts_print_number(double)` and `ts_print_bool(signed char)` — fixed-arity
  functions with an unambiguous ABI, linked alongside the object file.
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
  answer in linear time and in constant stack space. `fib(90)` is 2^90 calls
  as written; after the rewrite it is ninety.

Read [ARCHITECTURE.md](ARCHITECTURE.md) for the full design rationale, and
[SPEC.md](SPEC.md) for the complete grammar and CLI contract.

## Not supported yet

Strings, arrays, objects, classes, closures, generics, modules, `for`/`switch`,
`continue`/`break`, `++`/`--`, and compound assignment.

`number` is an IEEE-754 double, so it is exact only up to 2^53. Use the `int`
type (64-bit) when the value has to be exact.

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
| **Startup** | **24.4 ms** | 25.9 ms | 22.2 ms | 32.1 ms | 34.3 ms | 149 ms | 157 ms |
| **Compile time** | **0.19 s** | 0.11 s | 0.19 s | 1.00 s | 0.98 s | 0.45 s | 0.39 s |
| **fib(35) runtime** | **28.2 ms** | 49.9 ms | 69.7 ms | 130 ms | 89 ms | 246 ms | 248 ms |
| **fib(35) unrewritten** | 69.9 ms | **48.3 ms** | 70.7 ms | 130 ms | 87 ms | 244 ms | 251 ms |
| **loop(1e8) runtime** | 129 ms | **125 ms** | 129 ms | 139 ms | 142 ms | 259 ms | 275 ms |

Full numbers, methodology, and the raw report: [`bench/RESULTS.md`](bench/RESULTS.md).
Regenerate with `python3 scripts/bench.py`.

The **Startup** row is the one sensitive to the host: spawning a process costs
~5 ms on an idle machine and ~20 ms on a busy one, which changes every ratio
without changing the differences. The committed report was taken with a load
average of 22.5 and a 21.5 ms spawn floor, so the startup row above is a
busy-host number; on an idle host it reads about a third of that. The report
records the load average of the run it came from, so you can tell which
numbers to trust.

The two fibonacci rows are the same recursion. In the first, it is written the
way a programmer writes it and the compiler rewrites the recurrence into a
loop, so the whole program finishes in the time it takes to start. In the
second it is written so that no rewrite can fire on it, which is the
comparison of what the recursion itself costs.

**Where we win.** `deno compile` and `bun build --compile` embed a JavaScript
engine (V8 / JavaScriptCore) in the binary. We compile to machine code, so
there is no engine to ship and nothing to initialise at startup — a 33 KiB
binary instead of 60 MiB, ~2000x smaller, and ~1.4x faster to start.

**A linear recurrence is not exponential.** The fibonacci workload is the
same program twice: written the way a programmer writes it, and written so
that no rewrite can fire on it. In the first form our compiler turns the
recurrence into a loop, and the whole program — 2^35 calls as written —
finishes in the time it takes to start the process: **28 ms against
scriptc's 50 ms**. In the second form, where the recursion survives, the
gap is the other way round: **70 ms against scriptc's 48 ms**, because
scriptc rewrites more shapes than we do.

**Where we lose.** The honest comparison is [`scriptc`](https://scriptc.dev)
(Vercel Labs), the only other engine-free native TypeScript compiler. On the
recursion that survives the rewrite it beats us (49 ms against our 68 ms).
Profiling both binaries shows this is **not** a difference in the cost of a
call — our calls are the cheaper of the two (1.48 ns per entry against
scriptc's effective 1.05 ns). It is a difference in **how many calls are
made**: scriptc rewrites a self-recursive function into a loop around a
single recursive call, which cuts the number of function entries by 5.24x.
We now do the same for the *chain* shape (`f(n) = f(n-1)` is a loop on our
side too) and for a *linear recurrence* (`f(n) = f(n-1) + f(n-2)` is a loop
with two accumulators), but scriptc recognises more shapes than we do — and
a shape we do not recognise is left exactly as it was written. Our tight
loop already ties scriptc, and switching that loop to 64-bit integers makes
it 1.5x faster while the same change does nothing for recursive calls
(1.01x) — the arithmetic was never the issue.

**Where nobody wins.** On a tight numeric counting loop, V8 and
JavaScriptCore are excellent at that shape, and Cranelift's optimiser does
not beat a mature JIT — we are within noise of all of them (~1.0x).

**A note on `tsrust`.** [`tsrust`](https://github.com/ferrotype/tsr) is
TypeScript 7's Go compiler ported to Rust. It emits the same JavaScript as
`tsc` and performs identically (106 ms vs 105 ms on fib) — being written in
Rust does not make the *output* faster, only the compiler. It is the sharpest
illustration of what this project is doing differently: we do not emit
JavaScript at all.

**The `int` type.** `int` (64-bit) keeps arithmetic exact past 2^53 and is
measurably faster where arithmetic is the work: **1.57x** on an arithmetic-only
loop and **1.94x** when a function call happens per iteration. It does not help
on recursion (1.07x on the unrewritten fibonacci), which is the evidence that
the gap to scriptc is not about arithmetic. It is not a cross-toolchain
comparison — the JavaScript toolchains have no integer type.

## Testing

```sh
cargo test    # compiles and runs every example, plus type-error cases
```

The test suite compiles each program in `examples/` to a native binary, runs it,
and asserts its exact output — so the tests check *generated code*, not just
generated IR.

## Documentation

| Doc | Purpose |
|---|---|
| [SPEC.md](SPEC.md) | CLI contract, grammar, type rules, runtime ABI |
| [ARCHITECTURE.md](ARCHITECTURE.md) | Module relationships, data flow, design decisions |
| [TODO.md](TODO.md) | Roadmap, completed work, scorecard |
| [bench/RESULTS.md](bench/RESULTS.md) | Competitive benchmark against deno, bun, and tsc+node |

`AGENTS.md` and `MEMORY.md` are agent-local working documents — the development
loop and the harvested knowledge base. They are gitignored and not shipped with
the crate.

## License

Apache-2.0 — see [LICENSE](LICENSE).
