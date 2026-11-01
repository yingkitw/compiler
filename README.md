# compiler

[![crates.io](https://img.shields.io/crates/v/compiler.svg)](https://crates.io/crates/compiler)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)

A tiny TypeScript-subset compiler that emits **native binaries**, written in Rust.

Hand-written lexer and recursive-descent parser, type-checking code generation on
top of [Cranelift](https://cranelift.dev/), and a two-function C runtime linked
with the system C compiler. No LLVM, no JavaScript engine, no runtime
dependency beyond libc.

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
function double(n: number): number { return n * 2; }
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

- Arithmetic (`+ - * /`) and relational operators (`< <= > >=`) require
  `number` operands.
- Equality (`== !=`) requires both operands to have the *same* type.
- `&&`, `||`, and `!` accept `boolean` or `number` and always yield `boolean`.
  They **short-circuit**: in `false && (1 / 0 > 0)`, the right side is never
  evaluated.
- Assignment requires the value's type to match the variable's type exactly.
- A function whose return type isn't `void` must return a value on every path.

Errors are reported at compile time, not at runtime:

```
$ tsc bad.ts
error: cannot initialize 'x: number' with a value of type boolean
```

### Built-ins

| Function | Behaviour |
|---|---|
| `print(number)` | Prints the value and a newline (`42`, `12.5664`) |
| `print(boolean)` | Prints `true` or `false` |

## How it works

```
 source ──lex──▶ tokens ──parse──▶ AST ──codegen──▶ object file ──cc──▶ binary
   lexer.rs       parser.rs   ast.rs   codegen.rs      (Cranelift)   runtime.c
```

| Stage | What happens |
|---|---|
| `src/lexer.rs` | Tokenizer. Every token carries its source line, so diagnostics point at a line |
| `src/parser.rs` | Recursive descent, one method per precedence level |
| `src/ast.rs` | Shared AST types — the contract between parser and codegen |
| `src/codegen.rs` | One pass that type-checks **and** emits Cranelift IR: SSA variables, basic blocks for `if`/`while`, and for short-circuit `&&` / `\|\|` |

Two design points worth knowing:

- **A tiny C runtime instead of variadic `printf`.** The generated code imports
  `ts_print_number(double)` and `ts_print_bool(signed char)` — fixed-arity
  functions with an unambiguous ABI, linked alongside the object file.
- **Position-independent code is always on.** Without it, external calls fail to
  link on macOS arm64 with "Illegal text-relocations".

Read [ARCHITECTURE.md](ARCHITECTURE.md) for the full design rationale, and
[SPEC.md](SPEC.md) for the complete grammar and CLI contract.

## Not supported yet

Strings, arrays, objects, classes, closures, generics, modules, `for`/`switch`,
`continue`/`break`, `++`/`--`, compound assignment, `%`, and integer types.
`number` is always an IEEE-754 double, so integer arithmetic is exact only up to
2^53.

This is a compiler for a *subset*, not a TypeScript implementation. It is meant
as a readable reference for how a native code generator is put together — and as
a fast way to turn small numeric programs into fast binaries.

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

`AGENTS.md` and `MEMORY.md` are agent-local working documents — the development
loop and the harvested knowledge base. They are gitignored and not shipped with
the crate.

## License

Apache-2.0 — see [LICENSE](LICENSE).
