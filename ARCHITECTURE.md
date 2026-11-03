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
                    │      parser/         │  tokens → ast::Program
                    └───────────┬──────────┘
                                │  ast::Program
                    ┌───────────▼──────────┐
                    │      rewrite/        │  tail calls and recurrences → loops
                    └───────────┬──────────┘
                                │  ast::Program
                    ┌───────────▼──────────┐
                    │      codegen/        │  Program → Cranelift IR
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
`rewrite` → `codegen` → `ast`. `ast.rs` depends on nothing inside the crate.
No cycles, no back-references.

**The three biggest modules are directories**, each split so that no file is
over about 450 lines:

| Module | Files | What is in them |
|---|---|---|
| `parser/` | `mod.rs`, `stmt.rs`, `expr.rs`, `tests.rs` | the token helpers and the statement dispatch; the statement forms; one method per precedence level |
| `rewrite/` | `mod.rs`, `common.rs`, `tail_call/`, `recurrence/` | the entry point and the shared predicates; each rewrite with its own tests |
| `codegen/` | `mod.rs`, `ty.rs`, `runtime.rs`, `stmt.rs`, `expr.rs`, `tests.rs` | `compile()` and the function builder; the type lattice; the C shim; statement and expression generation |

### `main.rs` — CLI entry point
- Parses arguments by hand: `-o` / `--output`, `-c` / `--compile-only`,
  `-V` / `--version`, `-h` / `--help`.
- Honours the conventional `CC` and `CFLAGS` environment variables for the
  link step.
- Runs the compilation on a thread with a 64 MiB stack: every stage recurses
  over the AST, and the depth limits decide how deep that may go — the limits
  should be what fires, not the operating system.
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
- `Tok` is a flat enum of keywords, `Ident(String)`, `Num(NumLit)` and
  punctuation. A `NumLit` keeps the literal's exact `i64` value alongside its
  `f64`, because a literal such as 2^53 + 1 does not survive the round trip
  through a double.
- `Token { tok, line }` carries the source line on every token so parser
  diagnostics can point at a line.
- Handles `//` and `/* */` comments, whitespace, and numeric literals.
- Knows nothing about types or grammar — it only classifies characters.

### `ast.rs` — shared data types
- `Type` (`Number` | `Int` | `Boolean` | `Void`), `BinOp`, `UnOp`, `Expr`,
  `Stmt`, `Program`, and the depth limits the walking stages share.
- Deliberately has no behaviour and no dependencies: it is the contract between
  the parser and the codegen.
- Because both sides pattern-match on these types, **adding a variant requires
  a parser arm and a codegen arm** — see MEMORY.md → *AST / Parser Sync*.

### `parser/` — recursive-descent parser
- `parse(tokens: Vec<Token>) -> Result<Program, String>`.
- `mod.rs` holds the parser state and the token helpers; `stmt.rs` the
  statement forms (`let_decl`, `func_decl`, `if_stmt`, `while_stmt`,
  `return_stmt`, `block`, expression statements); `expr.rs` one method per
  precedence level (`logic_or`, `logic_and`, `equality`, `comparison`, `term`,
  `factor`, `unary`, `call`, `primary`), plus `assignment` at the top.
- The parser performs only *syntactic* checks plus three well-formedness rules:
  a `let` needs a type or an initializer, a variable or parameter cannot have
  type `void`, and an expression or statement may not nest deeper than
  `MAX_DEPTH`. The `main` shape is checked by the code generator, and all type
  checking happens there too.

### `rewrite/` — source-to-source optimisations
- `optimise(prog: &mut Program)` runs every rewrite over the program:
  `tail_call::eliminate_tail_calls` and then `recurrence::eliminate_recurrences`.
- It runs **between parsing and code generation**, so `codegen/` sees only
  ordinary statements and needs no knowledge of the optimisations.
- `common.rs` holds what both rewrites share: the predicates ("is this
  expression free of side effects?") and the name helpers ("which names does
  this program already use?").
- **Tail-call elimination**: a self-recursive call in tail position becomes a
  loop. A function whose body is
  ```
  if (p0 <= limit) { return base; }
  return f(p0 - k, p1, ...);
  ```
  becomes
  ```
  while (p0 > limit) { p0 = p0 - k; }
  return base;
  ```
  and the accumulator form (`return f(p0 - k, ...) + e;`) carries the
  accumulator along the loop instead of on the stack. A void function whose
  tail call is a statement (`f(p0 - k);`) is rewritten the same way, with the
  statements that run on every level moved into the loop.
- **Linear recurrences**: a function defined in terms of its own previous
  values (`return f(p0 - 1) + f(p0 - 2);`) is exponential in the source
  program — the tree of calls has 2^depth leaves. The rewrite turns it into a
  loop with one accumulator per value the recurrence needs:
  ```
  if (p0 < 2) { return base; }        // the base case, unchanged
  let a = f(0);                        // the starting values, one per offset
  let b = f(1);
  let i = 2;                           // the induction variable
  while (i <= p0) {
      let c = b + a;
      a = b;
      b = c;
      i = i + 1;
  }
  return b;
  ```
  The loop performs the same operations, in the same order, on the same
  values, so the answer is bit for bit the same — in linear time, and in
  constant stack space. The expression's shape is preserved exactly, so a
  recurrence with a third term, a coefficient or a wider gap is rewritten
  the same way.
- **Why it matters**: a tail call does not need the frame it was made in, so
  the recursion costs one frame instead of one per level. A recursion of ten
  million levels now runs in constant stack space; before, it died with
  "maximum recursion depth exceeded". A recurrence that costs 2^90 calls now
  costs 90.
- **What it deliberately does not do**: the rewrites only fire on a narrow,
  recognisable shape (see the module's own documentation). Anything they do
  not recognise is left exactly as it was — a rewrite that fires on the wrong
  program is a miscompile, and a missed optimisation is always the cheaper
  mistake.
- **The rule that keeps it honest**: the rewrite changes *when* things run, so
  every statement it moves has to run the same number of times. Three
  consequences, each of which was a miscompile before the rule was added:
  - A statement *before* the base case runs once per recursion level in the
    source program and once in the rewritten one, so the base case has to be
    the body's first statement.
  - An accumulator term is evaluated while the recursion unwinds — from the
    innermost level outwards — and the rewrite evaluates it from the
    outermost level inwards, so the term must have no side effects.
  - The statements of a void tail call run in a fresh frame per level, so
    they may not assign, and they may not call the function again.
- **The recurrence rewrite has one more rule**: the source program evaluates
  the base expression once per leaf of the call tree, and the rewrite
  evaluates it exactly twice — for the two starting values — so the base
  expression must have no side effects either.

### `codegen/` — type checking + machine code
- `compile(program: &Program) -> Result<Vec<u8>, String>` returns the bytes of a
  relocatable object file.
- Structure:
  - `mod.rs` — `compile()` sets up the ISA, creates the `ObjectModule`, declares
    runtime imports, then runs two passes over the function declarations, then
    emits `__ts_start` and the C `main` wrapper. `define_body()` builds one
    function: creates the entry block, binds parameters to SSA variables, runs
    the statement generator, and adds the implicit return if needed.
  - `ty.rs` — the type lattice: `cl_ty()` maps the language's types onto
    Cranelift's `F64` / `I8` / `I64`, `sig_for()` builds a function's
    signature, and `FnSig` / `Slot` are the signature table and a variable's
    slot.
  - `runtime.rs` — `RUNTIME_C`, the embedded C runtime shim, written to disk at
    link time.
  - `stmt.rs` and `expr.rs` — `FnGen`, the per-function generator. It holds the
    `FunctionBuilder`, a `&mut ObjectModule` (needed because
    `declare_func_in_func` takes `&mut self` in Cranelift 0.136), the function
    signature table, and the scope stack. Its methods (`gen_stmt`, `gen_expr`,
    `gen_binop`, `gen_short_circuit`, `gen_call`) do type checking *and* emit
    instructions in one pass. The two files are two `impl FnGen` blocks: one
    for the statements, one for the expressions.

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
   │  rewrite::optimise
   ▼
ast::Program                   tail calls and linear recurrences turned into loops
   │  codegen::compile
   ├─ pass 1: collect Stmt::Func → FuncId + signature   (enables recursion
   │                                                        and forward refs)
   ├─ pass 2: for each function → define_body
   │            └─ FnGen walks the body:
   │                 expression → (machine value, static type)
   │                 type error → Err(String) → abort
   ├─ __ts_start: top-level statements as a void function
   ├─ main: C-ABI wrapper calling ts_runtime_init, __ts_start, then user
   │        main, returning 0
   ▼
Object file bytes              Mach-O / ELF, position-independent
   │  cc <obj> <runtime.c> -o <out>
   ▼
native binary
```

### Runtime

```
main (C ABI, returns i32)
   └── ts_runtime_init     installs the stack-overflow handler
   └── __ts_start          top-level statements
         └── user main()   if declared
               └── tsfn_*  user functions
                     └── ts_print_number / ts_print_bool / ts_print_int
                     └── ts_fmod             (imported from the C runtime)
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

### A rewrite pass between parsing and code generation
**Decision**: `rewrite/` is a separate pass that runs on the AST between
`parser::parse` and `codegen::compile`, rather than an optimisation inside
the code generator.
**Why**: the code generator is the largest and most delicate module in the
project, and an optimisation does not belong in the middle of it. Keeping the
rewrite on the AST means `codegen/` needs no knowledge of it at all, and
the rewrite can be tested on its own — by inspecting the AST it produces.
**Trade-off**: the compiler has one more stage, and the rewrite can only use
what the AST knows. In particular it cannot see types, so it has to be
conservative about which shapes it accepts: it rewrites only a call it can
see is self-recursive, in tail position, with a decreasing argument.

### The three biggest modules are directories
**Decision**: `parser/`, `rewrite/` and `codegen/` are directories of files
rather than single files, and no file is over about 450 lines.
**Why**: a 1,400-line file is hard to read, hard to review, and hard to
change safely. The split is along the seams the modules already had — the
statement forms and the precedence levels in the parser, the two rewrites,
the runtime shim and the two halves of the code generator — so no new
abstraction was introduced and no behaviour changed.
**Trade-off**: a directory module needs `mod.rs` (or a `foo/mod.rs`), the
methods that cross a file boundary need `pub(super)`, and the tests move into
their own file. The Mature Module Policy applies here too: `codegen/` was
covered by 46 unit tests before the split, and the split is a *move*, so the
tests are what proves it changed nothing.

### Depth limits instead of a stack overflow
**Decision**: the parser bounds how deeply expressions and statements may
nest (`MAX_DEPTH`), the stages that walk the tree bound how deep the tree
itself may be (`MAX_AST_DEPTH`), and the compilation runs on a thread with a
64 MiB stack.
**Why**: every stage recurses over the AST, and a program with a hundred
thousand nested parentheses — or a long sum, which the parser builds
left-associated — would take the compiler down with a stack overflow. A
diagnostic ("expression or statement nested too deeply") is the honest
answer.
**Trade-off**: a program whose expressions really are deeper than the limit
is rejected rather than compiled. The limits are far beyond what a person
writes, and the 64 MiB stack means reaching the limit is the compiler's
decision rather than the operating system's.

### Tail-call elimination is narrow on purpose
**Decision**: the rewrite fires only on a shape it can recognise exactly — a
self-recursive call in tail position, whose arguments are parameters plus or
minus constants, preceded by an `if` with no `else` whose only action is to
return.
**Why**: a rewrite that fires on the wrong program is a miscompile, and a
missed optimisation is only a missed optimisation. The narrow shape is the
one that covers the common cases (a tail-recursive loop, a tail-recursive
accumulator) and it is easy to check by reading the module.
**Trade-off**: mutual recursion, a call whose result is used after it
returns, and a recursion whose argument is computed rather than stepped are
all left alone. Each of those could be handled by widening the shape, at
the cost of a rule that is harder to be sure about.

### The rewrite is checked by differential tests, not by inspection
**Decision**: the end-to-end test for tail-call elimination compiles each
shape twice — once with the tail call, once as the equivalent program written
with a loop — and asserts that the two print exactly the same thing.
**Why**: the rewrite changes *when* statements run, and reading the module
does not prove that it does not change *what* they compute. Two real
miscompiles got through code review and were caught only by running the
programs: an accumulator term that called a function was evaluated in the
opposite order, and a local declared before the base case kept the outermost
level's value. A differential test catches both classes by construction,
because the reference program is written the way a human would write it.
**Trade-off**: the reference versions have to be written by hand, so each
shape costs a second program to maintain.

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

### Two numeric types: `number` and `int`
**Decision**: `number` is an IEEE-754 double and `int` is a 64-bit signed
integer, with no implicit conversion between them.
**Why**: JavaScript's `number` cannot represent every integer beyond 2^53, and
a compiler that emits native code can do better. The two types are distinct in
the checker, so nothing is silently widened or narrowed.
**Trade-off**: every operator has to type-check both combinations, and a
`number`/`int` boundary in a program is a compile error the programmer has to
resolve. Integer arithmetic wraps — there is no overflow check — and integer
division by zero is a runtime error reported by the runtime's signal handler.

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
