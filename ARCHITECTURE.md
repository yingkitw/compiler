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
                    │      backend/        │  Program → assembly text
                    │   (type checks too)   │  (for the host)
                    └───────────┬──────────┘
                                │  assembly text
                    ┌───────────▼──────────┐
                    │  main.rs (link step)  │  cc a.s + runtime.c → binary
                    └──────────────────────┘

                    ┌────────────┐
                    │  ast.rs    │  shared data types: Type, Expr, Stmt
                    └────────────┘  (depends on nothing; used by parser + backend)
```

**Dependency direction is strictly one-way**: `main` → `lexer` → `parser` →
`rewrite` → `backend` → `ast`. `ast.rs` depends on nothing inside the crate.
No cycles, no back-references.

**The three biggest modules are directories**, each split so that no file is
over about 450 lines:

| Module | Files | What is in them |
|---|---|---|
| `parser/` | `mod.rs`, `stmt.rs`, `expr.rs`, `tests.rs` + `tests/` | the token helpers and the statement dispatch; the statement forms (including the `for` and `for-of` desugars); one method per precedence level; the shared test helpers, with the tests in `tests/` (`stmts.rs`, `exprs.rs`, `errors.rs`) |
| `rewrite/` | `mod.rs`, `common.rs`, `tail_call/`, `recurrence/` | the entry point and the shared predicates; each rewrite with its own tests; the recurrence split into recognising the shape (`recognize.rs`) and building the loop (`build.rs`) |
| `backend/` | `mod.rs`, `function.rs`, `expr/`, `stmt.rs`, `regs.rs`, `runtime.rs`, `tests.rs`, `asm/` | `emit` and the C wrapper; per-function frame; `expr/` (`mod.rs`, `binop.rs`, `call.rs`, `array.rs`); statements; the register cache; C shim; regressions; `asm/` |

### `main.rs` — CLI entry point
- Parses arguments by hand: `-o` / `--output`, `-c` / `--compile-only`,
  `-V` / `--version`, `-h` / `--help`.
- Honours the conventional `CC` and `CFLAGS` environment variables for the
  link step.
- Runs the compilation on a thread with a 64 MiB stack: every stage recurses
  over the AST, and the depth limits decide how deep that may go — the limits
  should be what fires, not the operating system.
- Reads the source file, then drives the pipeline: `lexer::lex` → `parser::parse`
  → `backend::emit`.
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
- `Token { tok, line, col }` carries 1-based line and column on every token
  for lexer/parser diagnostics.
- Handles `//` and `/* */` comments, whitespace, and numeric literals.
- Knows nothing about types or grammar — it only classifies characters.

### `ast.rs` — shared data types
- `Type` (`Number` | `Int` | `Boolean` | `Void`), `BinOp`, `UnOp`, `Expr`,
  `Stmt`, `Program`, and the depth limits the walking stages share.
- Deliberately has no behaviour and no dependencies: it is the contract between
  the parser and the code generator.
- Because both sides pattern-match on these types, **adding a variant requires
  a parser arm and a code generator arm** — see MEMORY.md → *AST / Parser Sync*.

### `parser/` — recursive-descent parser
- `parse(tokens: Vec<Token>) -> Result<Program, String>`.
- `mod.rs` holds the parser state and the token helpers; `stmt.rs` the
  statement forms (`let_decl`, `func_decl`, `if_stmt`, `while_stmt`,
  `for_stmt` — which desugars the C-style `for` *and* `for-of` —
  `return_stmt`, `block`, `block_expr`, expression statements and the
  increment/decrement sugar); `expr.rs` one method per precedence level
  (`assignment`, `conditional`, `logic_or`, `logic_and`, `equality`,
  `comparison`, `term`, `factor`, `unary`, `call`, `primary`).
  `comparison` also handles `as` casts — their
  precedence is relational, wrapping the whole expression to the left — and
  its `tight_tail` continues the tighter operators after a cast, so
  `x as int + 1` is `(x as int) + 1`.
- The parser performs only *syntactic* checks plus a handful of
  well-formedness rules: a `let` needs a type or an initializer, a variable
  or parameter cannot have type `void`, an expression or statement may not
  nest deeper than `MAX_DEPTH`, a name may not be redeclared in the same
  scope, and an inner binding that hides an outer one produces a warning
  (`Program.warnings`, printed by `main`). The `main` shape is checked by
  the backend, and all type checking happens there too.
- `for_stmt` is a desugar, not a statement form: the parser emits a block
  holding the init and a `while` whose body is the loop body followed by the
  step, so there is no `Stmt::For` anywhere. The AST, the rewrites and the
  code generator gained nothing and changed not at all.
- The assignment sugars are desugars too: `x += e` is parsed as `x = x + e`,
  and `x++` (in either form) as `x = x + 1`. They cost the AST, the code
  generator and the type rules nothing — the assignment's own rules apply
  unchanged. The increment operators are parsed only where a statement is
  expected, because TypeScript's postfix form yields the *old* value,
  which the subset does not model.

### `rewrite/` — source-to-source optimisations
- `optimise(prog: &mut Program)` runs every rewrite over the program:
  `tail_call::eliminate_tail_calls` and then `recurrence::eliminate_recurrences`.
- It runs **between parsing and code generation**, so the backend sees only
  ordinary statements and needs no knowledge of the optimisations.
- `common.rs` holds what both rewrites share: the predicates ("is this
  expression free of side effects?") and the name helpers ("which names does
  this program already use?").
- The recurrence rewrite is split in two: `recognize.rs` decides whether a
  function has the recurrence shape (and gives up if it does not), and
  `build.rs` rewrites the function it recognised. The seam is the one the
  module already had — a rewrite that fires on the wrong program is a
  miscompile, so the half that says no is worth its own file.
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
  program — the tree of calls has `F(depth)` leaves. The rewrite turns it into a
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
- **The recurrence rewrite keeps the narrowest window the shape allows.** The
  loop needs the values the expression reads, and the widest offset says how
  far back it must reach. When every offset is a multiple of the same number
  — the *stride* — the arguments the recursion reaches lie on one lattice,
  so the loop steps by the stride and keeps one value per lattice position:
  `f(n - 2) + f(n - 4)` steps by 2 and keeps two values, not four. The
  first value on the lattice is only known at run time, so the `int` scheme
  computes it and the `number` scheme finds it by walking down the lattice. An `int` loop counts a fixed lattice with its own index, but
  a `number` argument's lattice is not known until run time: the recursion's
  arguments are `n`, `n − 1`, `n − 2`, … at whatever fraction `n` carries.
  The rewrite therefore walks the argument down to the base case first, with
  the same subtractions the recursion performs, and then counts *down* from
  the argument, replaying that descent exactly — the values the loop computes
  sit on the recursion's own lattice, so the answer is the same, bit for bit.
  The one shape a `number` rewrite refuses is a term that reads the
  parameter: counting down gives such a term no level's value to see, so
  `f(n - 1) + f(n - 2) + n` is rewritten only for an `int` parameter.
- **Why it matters**: a tail call does not need the frame it was made in, so
  the recursion costs one frame instead of one per level. A recursion of ten
  million levels now runs in constant stack space; before, it died with
  "maximum recursion depth exceeded". A recurrence that costs an astronomical
  number of calls now costs ninety.
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
  evaluates it once per starting value — one per accumulator, so two for a
  fibonacci and three for a recurrence with a gap of three — so the base
  expression must have no side effects either.

### `backend/` — type checking + native code
- `emit(program: &Program, source: &Path) -> Result<String, String>` returns
  the program's assembly text (with `.file` / `.loc` for the source path and
  each statement's `line`), which the system C compiler assembles and links with
  the runtime.
- There is no Cranelift and no LLVM: the code generator writes the target's
  assembly itself, through the `asm` module.
- Structure:
  - `mod.rs` — `emit()` collects the functions, then emits each one: the
    top-level statements become `__ts_start`, the user's functions become
    `tsfn_<name>`, and a C `main` calls the runtime's setup, the top-level
    statements and the user's `main`. `Fn` holds the state of one function
    being written: its assembler, the scopes, the frame's locals and the
    pool of temporaries.
  - `expr/` and `stmt.rs` — the type rules and the code they emit. Their
    methods (`gen_expr`, `gen_binop`, `gen_short_circuit`, `gen_call`,
    `gen_stmt`) do type checking *and* emit instructions in one pass;
    `expr/array.rs` holds the array literal/index/`length`/`push` rules.
  - `asm/` — the assembler the code is written through: `mod.rs` (the
    builder, the labels, the branch conditions and the memory operands),
    `inst.rs` (the instructions) and `reg.rs` (the registers). The operands
    are types, not text, so a register cannot be written where an immediate
    belongs — the mistakes the assembler would only report after the program
    had been compiled become compile errors here.
  - `regs.rs` — the register cache: the locals that live in callee-saved
    registers (`d8`–`d13`, `x19`–`x24`) instead of in frame slots. The
    frame stays the authority — a load consults the cache, a store updates
    it — so a local that is never promoted simply keeps its slot. A `while`
    promotes the locals its body assigns, so they stay in registers across
    its back edge; the cache's entries belong to the region (the function,
    or a loop) that promoted them, and a closing region flushes its dirty
    entries back to their slots.
  - `runtime.rs` — `RUNTIME_C`, the embedded C runtime shim, written to disk at
    link time.
- The value model: every expression leaves its value in a register — `x0`
  for an `int` or a `boolean` (and the data pointer of an array), `d0` for
  a `number`, `x0` + `x1` (data + byte length) for a `string` — and the
  frame holds what must outlive it: the locals, and a pool of temporaries
  for the two operands of a binary operation. A *cached* local is the
  exception: its value lives in a callee-saved register, and because
  expression evaluation never touches those registers, it can be used as
  an operand where it is — `fadd d0, d8, d9`, with no move at all.
- The operand scheme: the operand evaluated last is one the other's value
  can wait for — a literal (so it can adopt the other operand's type), or
  one whose evaluation touches nothing but the result register. Its value
  waits in `x1`/`d1`; only when neither operand qualifies does a value take
  a frame slot.

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
   │  backend::emit
   ├─ pass 1: collect Stmt::Func → name + signature      (enables recursion
   │                                                        and forward refs)
   ├─ pass 2: for each function → emit_body
   │            └─ expr::gen_expr / stmt::gen_stmt walk the AST:
   │                 expression → (register, static type)
   │                 type error → Err(String) → abort
   ├─ __ts_start: top-level statements as a void function
   ├─ main: C-ABI wrapper calling ts_runtime_init, __ts_start, then user
   │        main, returning 0
   ▼
assembly text (.s)             typed operands via backend/asm/
   │  cc <asm> <runtime.c> -o <out>
   ▼
native binary
```

### Runtime

```
main (C ABI, returns i32)
   └── ts_runtime_init     installs the stack-overflow and SIGFPE handlers
   └── __ts_start          top-level statements
         └── user main()   if declared
               └── tsfn_*  user functions
                     └── ts_print_number / ts_print_bool / ts_print_int / ts_print_string
                     └── ts_fmod, ts_double_to_int, ts_int_div_zero
                     └── ts_str_equal / ts_str_concat
                     └── ts_*_array_new / push / get, ts_array_len
                           (all allocated from the runtime's bump arena)
```

### Type checking

There is no separate checking pass. `expr::gen_expr` returns `(Val, Type)` —
which register holds the value and its static type — and every rule is
enforced where the value is produced:

| Rule | Where |
|---|---|
| Initializer type matches annotation | `Stmt::Let` in `gen_stmt` |
| Assignment type matches the variable | `Expr::Assign` in `gen_expr` |
| `const` is not reassigned | `Expr::Assign` in `gen_expr` |
| Arithmetic needs numbers | `gen_binop` |
| Relational needs numbers | `gen_binop` |
| Equality needs matching types | `gen_binop` |
| Argument count and types | `gen_call` |
| Function returns on all paths | `emit_body` |
| Undefined variable / function | `lookup` / `gen_call` |

## Design Decisions

### Type checking is fused with code generation
**Decision**: one pass that checks types and emits code, instead of a separate
checking pass over the AST.
**Why**: a separate pass would duplicate the type lattice and traverse the AST
twice. The fused version has one source of truth for types.
**Trade-off**: type errors surface in *code generation order* rather than source order,
and the first error aborts compilation. This is acceptable while the language is
small; if error ordering becomes important (e.g. an IDE-style language server),
splitting the checker into its own module is the natural refactor.

### Assembly text + the system C compiler, not an embedded codegen IR
**Decision**: the `backend/` emits host assembly through `asm/`, then `cc`
assembles and links it with the embedded C runtime.
**Why**: the compiler crate has **no third-party dependencies**; the only
platform-specific piece is the assembly syntax the `asm` module writes and the
`cc` invocation in `main.rs`. Type checking stays fused with emission in
`expr.rs` / `stmt.rs`.
**Trade-off**: optimisations are whatever the system assembler and linker
provide; there is no SSA-level optimiser in the compiler itself.

### A fixed-arity C runtime shim instead of calling `printf`
**Decision**: the generated code imports `ts_print_number(double)`,
`ts_print_bool(signed char)`, `ts_print_int(long long)`, `ts_runtime_init(void)`
and `ts_fmod(double, double)`, all defined in a small C file linked alongside
the object.
**Why**: `printf` is variadic, and calling it from generated code requires
care with the platform's vector-argument count — fragile and
platform-specific. Fixed-arity functions have an unambiguous ABI.
**Trade-off**: the runtime must be written in C and linked with `cc`, so a C
compiler is required. The alternative — emitting the print logic in assembly
itself — would need a string-conversion routine and more runtime code.

### Number printing follows JavaScript, not `printf("%g")`
**Decision**: `ts_print_number` implements JavaScript's
`Number.prototype.toString` — the shortest decimal that round-trips back to the
same `f64`, in fixed notation for decimal exponents in [-6, 20] and exponential
notation otherwise.
**Why**: `%g` keeps only six significant digits, so `print(333333283333717100000)`
printed `3.33333e+20` — a *different number* from the one computed. A
TypeScript compiler whose output disagrees with TypeScript's own semantics is
wrong, not merely imprecise. The competitive benchmark caught this: the
harness requires every toolchain to print identical output, and only the
JavaScript toolchains matched each other.
**Trade-off**: `ts_print_number` grew from one line to about sixty, and the
whole runtime is about 150 lines of C, and
`print(3.14159 * 2 * 2)` changed from `12.5664` to `12.56636` — a visible
behaviour change, but the old value was lossy.

### A rewrite pass between parsing and code generation
**Decision**: `rewrite/` is a separate pass that runs on the AST between
`parser::parse` and `backend::emit`, rather than an optimisation inside
the code generator.
**Why**: the code generator is the largest and most delicate module in the
project, and an optimisation does not belong in the middle of it. Keeping the
rewrite on the AST means the backend needs no knowledge of it at all, and
the rewrite can be tested on its own — by inspecting the AST it produces.
**Trade-off**: the compiler has one more stage, and the rewrite can only use
what the AST knows. In particular it cannot see types, so it has to be
conservative about which shapes it accepts: it rewrites only a call it can
see is self-recursive, in tail position, with a decreasing argument.

### `for` is desugared in the parser, not added to the AST
**Decision**: a C-style `for` loop is rewritten *while parsing* into a block
containing the init and a `while` whose body is the loop body followed by the
step. There is no `Stmt::For`.
**Why**: adding an AST variant forces a parser arm *and* a code generator
arm (see
MEMORY.md → *AST / Parser Sync*), and the language already has the loop the
`for` abbreviates. Desugaring in the parser gives the feature with zero
changes to the AST, the rewrites, or the code generator — and the rewrites
see a shape they already know how to walk. Wrapping the init in its own block
gives the scoping for free: the loop variable cannot leak, and two
sequential loops may declare the same name.
**Trade-off**: the `for` is invisible from the AST onward, so anything that
needs to know where the loop's step lives — a future `continue`, which must
still run it — has to understand the desugar. The step being the *last*
statement of the desugared body is what makes `continue` correct if loops
ever grow one.

### The three biggest modules are directories
**Decision**: `parser/`, `rewrite/` and `backend/` are directories of files
rather than single files, and no file is over about 450 lines.
**Why**: a 1,400-line file is hard to read, hard to review, and hard to
change safely. The split is along the seams the modules already had — the
statement forms and the precedence levels in the parser, the two rewrites,
and (in the assembly backend) the expressions, the statements, the frame and
the assembler — so no new abstraction was introduced and no behaviour
changed.
**Trade-off**: a directory module needs `mod.rs` (or a `foo/mod.rs`), the
methods that cross a file boundary need `pub(super)`, and the tests move into
their own file. The Mature Module Policy applies here too: the `parser/`
and `rewrite/` splits were pure moves proven by their unit tests, and the
`backend/` directory — a wholesale replacement of the earlier Cranelift
code generator, not a split — was written with its own regression tests
(`backend/tests.rs`) from the start.

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

### Strings and arrays allocate from a bump arena — no collector yet
**Decision**: the first strings and arrays allocate from a single
program-lifetime arena — a bump allocator in the C runtime — and nothing is
ever reclaimed. String literals live in the object's static data; only
concatenation and array growth allocate.
**Why**: the language has no pointers a program can see, no destructors and
no way to express a lifetime. There is nothing for a reference count or a
tracing collector to *do*: a program cannot build a cycle it cannot break,
and cannot observe a dangling value. A collector would also need the code
generator to say which machine values are pointers — a shadow stack, or a
pointer map per function — which is a large change to the backend for a
feature that does not exist yet.
**Trade-off**: a program that allocates in a loop grows without bound — the
arena is never reset, so its high-water mark is the program's *total*
allocation, not its live set. That is acceptable for the small numeric
programs this subset is aimed at, and it is the honest reason a collector is
a later item rather than a mistake in this one: the fix is a real collector
with real backend support, not a tweak to the arena.

### Linking with the system C compiler
**Decision**: emit an object file and run `cc` on it, rather than writing an
ELF/Mach-O linker.
**Why**: a native linker is a large, platform-specific component. `cc` is
present on any machine that can build Rust projects and handles the object
format, symbol resolution, and the C runtime in one step.
**Trade-off**: `cc` must be on `PATH`.

### External calls use platform linkage names
**Decision**: runtime and `bl` targets use `Fn::symbol`, which prefixes `_` on
Mach-O and not on ELF, and every runtime entry is a normal externally linked C
function.
**Why**: the generated `.s` is assembled by the system `cc`, which applies the
platform's relocation rules. Keeping calls at the C ABI layer avoids fighting
the assembler over GOT/PLT details in handwritten instructions.
**Trade-off**: the backend must match the host calling convention (AAPCS64 on
macOS arm64 today).

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

### Casts are real conversions, and saturating ones
**Decision**: `x as int` and `x as number` emit real conversions (`ts_double_to_int`
for saturation / `scvtf` for widening), and the awkward inputs are *defined*:
`NaN` casts to `0`, and an out-of-range `number` saturates to the ends of the
`i64` range — Rust's `as` semantics.
**Why**: TypeScript's `as` is erased at compile time because its types share
one representation; ours are `f64` and `i64`, so the conversion must run.
The saturating instruction is chosen over the trapping `fcvt_to_sint` because
a trap would surface through the runtime's signal handler as "integer
division by zero or overflow" — a lie about what the program did.
**Trade-off**: a trap would flag more suspicious programs, but saturation is
defined, testable, and the semantics a Rust user already expects. A cast is
also *value* semantics, deliberately distinct from a literal's adaptation:
`4.5 as int` is `4`, where `let x: int = 4.5;` is an error.

### `&&` and `||` are implemented with basic blocks
**Decision**: short-circuit evaluation uses three blocks and a dedicated
temporary slot rather than eager evaluation plus masking.
**Why**: eager evaluation would evaluate the right-hand side even when the
result is already determined, so `false && (1/0 > 0)` would trap.
**Trade-off**: more complex code generation than a bitwise `and`/`or`.

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
