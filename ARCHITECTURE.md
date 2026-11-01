# Architecture

How the modules of **tsc** fit together, what flows between them, and why the
design is the way it is.

## Module Relationships

```
                    ┌────────────┐
                    │  main.rs   │  CLI: args, file IO, cc invocation
                    └─────┬──────┘
          reads source  │  │  module graph
                    ┌───▼──┴──────────────┐
                    │      lexer.rs        │  source text → Vec<Token>
                    └───────────┬──────────┘
                                │  Vec<Token>
                    ┌───────────▼──────────┐
                    │      parser.rs        │  tokens → ast::Program
                    └───────────┬──────────┘
                                │  ast::Program
                    ┌───────────▼──────────┐
                    │      codegen.rs       │  Program → Cranelift IR
                    │   (type checks too)   │  → object file bytes
                    └───────────┬──────────┘
                                │  Vec<u8> (object)
                    ┌───────────▼──────────┐
                    │  main.rs (link step)  │  cc obj + runtime.c → binary
                    └──────────────────────┘

                    ┌────────────┐
                    │  ast.rs    │  shared data types: Type, Expr, Stmt
                    └────────────┘  (depends on nothing; used by parser + codegen)
```

**Dependency direction is strictly one-way**: `main` → `lexer` → `parser` →
`codegen` → `ast`. `ast.rs` depends on nothing inside the crate. No cycles, no
back-references.

### `main.rs` — CLI entry point
- Parses arguments by hand: `-o` / `--output`, `-c` / `--compile-only`,
  `-h` / `--help`.
- Reads the source file, then drives the pipeline: `lexer::lex` → `parser::parse`
  → `codegen::compile`.
- On success with linking: writes the object and the embedded C runtime to a
  temp directory, invokes `cc <obj> <runtime.c> -o <out>`, and removes the temp
  directory.
- Owns all user-visible formatting: the `error: ` prefix, the usage text, and
  the exit codes (`0` success, `1` compile/link failure, `2` usage error).
  Library-level code returns bare messages.

### `lexer.rs` — tokenizer
- `lex(src: &str) -> Result<Vec<Token>, String>`.
- `Tok` is a flat enum of keywords, `Ident(String)`, `Num(f64)` and punctuation.
- `Token { tok, line }` carries the source line on every token so parser
  diagnostics can point at a line.
- Handles `//` and `/* */` comments, whitespace, and numeric literals.
- Knows nothing about types or grammar — it only classifies characters.

### `ast.rs` — shared data types
- `Type` (`Number` | `Boolean` | `Void`), `BinOp`, `UnOp`, `Expr`, `Stmt`,
  `Program`.
- Deliberately has no behaviour and no dependencies: it is the contract between
  the parser and the codegen.
- Because both sides pattern-match on these types, **adding a variant requires
  a parser arm and a codegen arm** — see MEMORY.md → *AST / Parser Sync*.

### `parser.rs` — recursive-descent parser
- `parse(tokens: Vec<Token>) -> Result<Program, String>`.
- One method per precedence level (`logic_or`, `logic_and`, `equality`,
  `comparison`, `term`, `factor`, `unary`, `call`, `primary`), plus `assignment`
  at the top.
- Statement parsing: `let_decl`, `func_decl`, `if_stmt`, `while_stmt`,
  `return_stmt`, `block`, expression statements.
- The parser performs only *syntactic* checks plus two well-formedness rules:
  a `let` needs a type or an initializer, and `main` must have the right shape.
  All type checking happens later.

### `codegen.rs` — type checking + machine code
- `compile(program: &Program) -> Result<Vec<u8>, String>` returns the bytes of a
  relocatable object file.
- Structure:
  - `compile()` — sets up the ISA, creates the `ObjectModule`, declares runtime
    imports, then runs two passes over the function declarations, then emits
    `__ts_start` and the C `main` wrapper.
  - `define_body()` — builds one function: creates the entry block, binds
    parameters to SSA variables, runs the statement generator, and adds the
    implicit return if needed.
  - `FnGen` — the per-function generator. Holds the `FunctionBuilder`, a
    `&mut ObjectModule` (needed because `declare_func_in_func` takes `&mut self`
    in Cranelift 0.136), the function signature table, and the scope stack.
    Its methods (`gen_stmt`, `gen_expr`, `gen_binop`, `gen_short_circuit`,
    `gen_call`) do type checking *and* emit instructions in one pass.
- `RUNTIME_C` — the embedded C runtime shim, written to disk at link time.
- `cl_ty()` maps the language's types onto Cranelift's `F64` / `I8`.

## Data Flow

### Compilation

```
source text
   │  lexer::lex
   ▼
Vec<Token>                     every token carries its line number
   │  parser::parse
   ▼
ast::Program                   Type / Expr / Stmt trees
   │  codegen::compile
   ├─ pass 1: collect Stmt::Func → FuncId + signature   (enables recursion
   │                                                        and forward refs)
   ├─ pass 2: for each function → define_body
   │            └─ FnGen walks the body:
   │                 expression → (machine value, static type)
   │                 type error → Err(String) → abort
   ├─ __ts_start: top-level statements as a void function
   ├─ main: C-ABI wrapper calling __ts_start, then user main, returning 0
   ▼
Object file bytes              Mach-O / ELF, position-independent
   │  cc <obj> <runtime.c> -o <out>
   ▼
native binary
```

### Runtime

```
main (C ABI, returns i32)
   └── __ts_start          top-level statements
         └── user main()   if declared
               └── tsfn_*  user functions
                     └── ts_print_number / ts_print_bool   (imported from the C runtime)
```

### Type checking

There is no separate checking pass. `FnGen::gen_expr` returns
`(Option<Value>, Type)` — the emitted machine value (or `None` for `void`) and
its static type — and every rule is enforced where the value is produced:

| Rule | Where |
|---|---|
| Initializer type matches annotation | `Stmt::Let` in `gen_stmt` |
| Assignment type matches the variable | `Expr::Assign` in `gen_expr` |
| `const` is not reassigned | `Expr::Assign` in `gen_expr` |
| Arithmetic needs numbers | `gen_binop` |
| Relational needs numbers | `gen_binop` |
| Equality needs matching types | `gen_binop` |
| Argument count and types | `gen_call` |
| Function returns on all paths | `define_body` |
| Undefined variable / function | `lookup` / `gen_call` |

## Design Decisions

### Type checking is fused with code generation
**Decision**: one pass that checks types and emits code, instead of a separate
checking pass over the AST.
**Why**: a separate pass would duplicate the type lattice and traverse the AST
twice. The fused version has one source of truth for types.
**Trade-off**: type errors surface in *codegen order* rather than source order,
and the first error aborts compilation. This is acceptable while the language is
small; if error ordering becomes important (e.g. an IDE-style language server),
splitting the checker into its own module is the natural refactor.

### Cranelift, not LLVM
**Decision**: Cranelift as the code generator, via `cranelift-object`.
**Why**: it is pure Rust — no LLVM installation, no C++ toolchain, no
`inkwell` binding layer. The dependency tree stays small (five crates) and the
build is a single `cargo build`.
**Trade-off**: Cranelift's IR is less flexible than LLVM's for aggressive
optimisation, and its API changes between releases (see MEMORY.md → *Cranelift
0.136 API Differences*).

### A fixed-arity C runtime shim instead of calling `printf`
**Decision**: the generated code imports `ts_print_number(double)` and
`ts_print_bool(signed char)`, both defined in a tiny C file linked alongside the
object.
**Why**: `printf` is variadic, and calling a variadic function from Cranelift
requires setting the vector-register count for the SysV ABI — fragile and
platform-specific. Two fixed-arity functions have an unambiguous ABI.
**Trade-off**: the runtime must be written in C and linked with `cc`, so a C
compiler is required. The alternative — emitting the print logic in Cranelift
itself — would need a string-conversion routine and more runtime code.

### Number printing follows JavaScript, not `printf("%g")`
**Decision**: `ts_print_number` implements JavaScript's
`Number.prototype.toString` — the shortest decimal that round-trips back to the
same `f64`, in fixed notation for decimal exponents in [-6, 21] and exponential
notation otherwise.
**Why**: `%g` keeps only six significant digits, so `print(333333283333717100000)`
printed `3.33333e+20` — a *different number* from the one computed. A
TypeScript compiler whose output disagrees with TypeScript's own semantics is
wrong, not merely imprecise. The competitive benchmark caught this: the
harness requires every toolchain to print identical output, and only the
JavaScript toolchains matched each other.
**Trade-off**: the runtime grew from one line to roughly ninety, and
`print(3.14159 * 2 * 2)` changed from `12.5664` to `12.56636` — a visible
behaviour change, but the old value was lossy.

### A competitive benchmark in the loop
**Decision**: `scripts/bench.py` compiles the same programs with every
toolchain that can turn TypeScript into something runnable — including
`scriptc`, the only other engine-free native compiler — and the report is
committed to `bench/RESULTS.md`.
**Why**: the advantage has to be measured. Comparing only against `deno` and
`bun` would have shown a 2000x size win while missing the competitor that
actually matters.
**Trade-off**: the harness depends on other toolchains being installed, and
needs disk space for their (very large) artifacts. It skips whatever is
missing and reports that it skipped.

### Linking with the system C compiler
**Decision**: emit an object file and run `cc` on it, rather than writing an
ELF/Mach-O linker.
**Why**: a native linker is a large, platform-specific component. `cc` is
present on any machine that can build Rust projects and handles the object
format, symbol resolution, and the C runtime in one step.
**Trade-off**: `cc` must be on `PATH`.

### Position-independent code is always on
**Decision**: set `is_pic` unconditionally.
**Why**: without it, Cranelift emits absolute address references to imported
symbols, which Mach-O rejects with "Illegal text-relocations" — a hard link
failure on macOS arm64 that does not reproduce on Linux ELF. Enabling PIC
unconditionally is simpler than detecting the platform.
**Trade-off**: a small performance cost on targets where it is not strictly
required.

### `number` is always `f64`
**Decision**: one numeric type, no separate integer type.
**Why**: it keeps the type lattice at three types, avoids implicit numeric
conversions in the checker, and matches the "small typed core" scope of the
project.
**Trade-off**: integer arithmetic is exact only up to 2^53, and there is no
integer division or bitwise operation. Adding an `int` type is in the TODO
backlog.

### `&&` and `||` are implemented with basic blocks
**Decision**: short-circuit evaluation uses three blocks and a dedicated SSA
variable rather than eager evaluation plus masking.
**Why**: eager evaluation would evaluate the right-hand side even when the
result is already determined, so `false && (1/0 > 0)` would trap.
**Trade-off**: more complex codegen than a bitwise `and`/`or`.

### Synthetic `__ts_start` plus a C `main` wrapper
**Decision**: top-level statements compile to a synthetic `void` function; a
generated C-ABI `main` calls it, then the user's `main` if present, and returns
`0`.
**Why**: real programs have an entry point, and C requires the symbol to be
called `main`. Funnelling both top-level code and the user's `main` through one
wrapper keeps the C-ABI surface to exactly one function.
**Trade-off**: user functions are exported as `tsfn_<name>` rather than their
bare names, to avoid colliding with C runtime symbols.

### Documentation drift is a test failure
**Decision**: `tests/docs_sync.rs` asserts that the AGENTS.md module tree, the
ARCHITECTURE.md module mentions, the TODO.md test counts, and the README doc
links all match the code.
**Why**: in a project this small the documentation *is* the specification — the
language surface is defined in SPEC.md, not by the code alone. Drift is
inevitable otherwise.
**Trade-off**: the counts must be updated whenever tests are added. That is the
intended friction: it makes the update explicit rather than forgotten.
**Note**: the AGENTS.md tree check is skipped when `AGENTS.md` is absent, since
that file is gitignored by default and may not exist in a fresh clone.
