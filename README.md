# tsc — a tiny TypeScript-to-native compiler, written in Rust

Compiles a statically-typed subset of TypeScript directly to native binaries.
Hand-written lexer and recursive-descent parser, type-checking code generation
on top of [Cranelift](https://cranelift.dev/), and a ~3-line C runtime linked
with the system C compiler.

## Usage

```sh
cargo build --release
./target/release/tsc examples/fib.ts        # writes ./examples/fib
./examples/fib                              # prints 6765

tsc input.ts -o mybin                       # choose output path
tsc input.ts -c                             # emit an object file only
```

## Language subset

```ts
// Types: number (f64), boolean, void. Annotations optional when inferrable.
let x: number = 40;
let y = 2;              // inferred number
const pi: number = 3.14159;

// Functions (recursion and forward references work).
function fib(n: number): number {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}

// Control flow: if / else, while, return, blocks.
let i = 0;
while (i < 10) { i = i + 1; }

// Operators: + - * /  == != < <= > >=  && || (short-circuit)  !  unary -
// Truthiness: numbers are false iff == 0.

// Entry points: top-level statements run first, then main() if defined.
function main(): void {
    print(fib(20));     // built-in print(number | boolean)
}
```

Type errors (mismatched operands, assigning to `const`, calling undefined
functions, missing returns, ...) are reported at compile time.

## How it works

```
 source ──lex──▶ tokens ──parse──▶ AST ──codegen──▶ object file ──cc──▶ binary
   lexer.rs       parser.rs   ast.rs   codegen.rs      (Cranelift)   runtime.c
```

- `src/lexer.rs` — tokenizer (line-tracked, comments supported)
- `src/parser.rs` — recursive descent with standard precedence
- `src/codegen.rs` — single pass that type-checks and emits Cranelift IR
  (SSA variables, short-circuit `&&`/`||` via basic blocks), plus a C-ABI
  `main` wrapper that runs top-level statements and then `main()`
- Runtime (`ts_print_number` / `ts_print_bool`) is embedded and linked with `cc`

## Tests

```sh
cargo test    # compiles and runs every example, plus type-error cases
```

## Not supported (yet)

Strings, arrays, objects, classes, closures, generics, modules, `for` loops,
`%`, integer types. `number` is always an IEEE-754 double.
