# TODO

## Done

### Core
- [x] **The audit's findings are fixed** — an independent review of the whole
      codebase found five real bugs, all fixed with tests:
      - **The tail-call accumulator miscompiled `number` programs.** The
        rewrite folds the terms in a different order than the source program
        does, which changes the floating-point rounding: `f(3)` with a base of
        10^16 printed `10000000000000004` where the program computes
        `10000000000000000`. The accumulator form is now applied only to
        `int`, whose wrapping arithmetic is associative, so the order does not
        matter; a `number` program is left exactly as it was written.
      - **The recurrence rewrite miscompiled fractional arguments.** The loop
        counts the values it computes with its own index, and a fractional
        argument makes that count disagree with the recursion's depth:
        `fib(2.5)` printed `1` where the program computes `2`. The rewrite now
        requires the parameter to be an `int`.
      - **`void` as a variable or parameter type panicked the compiler**
        (`unreachable!` and a `panic!` in the code generator, both reachable
        from a program a user could write). Both now produce a diagnostic.
      - **A negative `int` literal could not be written.** `-5` parsed as a
        unary minus over a `number` literal, so `let x: int = -5;` was
        rejected. The literal now adopts the context the same way a positive
        one does, including `i64::MIN`.
      - **An `int` division by zero killed the process with SIGFPE.** The
        runtime now also handles the signal and reports
        `error: integer division by zero or overflow`, the same way it already
        reported a stack overflow.
- [x] **Lexer** — tokens, line tracking, `//` and `/* */` comments, numeric literals
- [x] **Parser** — recursive descent, full operator precedence, `let`/`const`,
      functions, `if`/`else`, `while`, C-style `for`, `return`, blocks, type
      annotations
- [x] **Type system** — `number` / `int` / `boolean` / `void`, fused type checking
      in codegen, diagnostics for mismatches, const reassignment, undefined names,
      missing returns
- [x] **`int` type (i64)** — exact integer arithmetic, integer division that
      truncates toward zero, `%` for integers, integer comparisons. A literal
      adopts the type of its context, and the lexer keeps a literal's *exact*
      integer value so `9007199254740993` (2^53 + 1) survives — a double would
      round it. No implicit conversion between `int` and `number`.
- [x] **`for` loops** — C-style `for (init; condition; step) body`, desugared
      in the parser into the statements the language already has: the init
      runs once, then a `while` whose body is the loop body followed by the
      step. No new AST variant and no codegen arm, so the wiring guard needed
      nothing. The init lives in its own desugared block, so two sequential
      loops may each declare `let i` and the variable does not leak. Each of
      the three parts is optional — a missing condition is `true`.
      The `for (const x of arr)` form waits for arrays. `break`/`continue`
      do not exist yet; when they do, a `continue` in a `for` body must run
      the step, which the desugar has to be taught about
- [x] **Explicit `int`/`number` conversion with `as`** — `x as int` and
      `x as number` are real conversions, not TypeScript's erased assertion:
      `number` → `int` truncates toward zero, `NaN` becomes 0, out-of-range
      saturates (Rust's `as` semantics, via `fcvt_to_sint_sat`); `int` →
      `number` is exact up to 2^53. `boolean` and `void` casts are errors.
      The precedence is TypeScript's — `as` wraps the whole expression to
      its left at the relational level, and a tighter operator may follow
      (`x as int + 1` is `(x as int) + 1`) — which took a `tight_tail` in
      `comparison` after a first cut bound the cast too tightly. The new
      `Expr::Cast` variant wired into the codegen and every rewrite walker;
      a first cut's precedence bug was caught by an AST-shape unit test
      before it shipped
- [x] **A recurrence on a wider lattice keeps a narrower window** — the item
      this replaces said the loop could "hop backwards over the offsets" and
      keep two values for `f(n - 1) + f(n - 17)`. **That premise was wrong**:
      the offsets 1 and 17 share no divisor, so the arguments the recursion
      reaches lie on no narrower lattice, and that shape is already optimal
      — the loop's seventeen values are all of them, and no refinement
      exists for it.
      The rule that does hold is the **stride**: the largest number every
      offset is a multiple of. When it is greater than one, the arguments
      lie on one lattice (the ones congruent to the initial argument), so
      the loop steps by the stride and keeps one accumulator per lattice
      position instead of one per argument — `f(n - 2) + f(n - 4)` steps by 2
      and keeps two accumulators, not four.
      The first value on the lattice is only known at run time — it is the
      first argument above the base point that is congruent to the argument
      itself — so the `int` scheme computes it (`threshold + 1 + (n -
      threshold - 1) % stride`) and the `number` scheme finds it by walking
      down the lattice. The values do not change: the differential tests
      compile each shape twice, and every program whose offsets share no
      divisor compiles to a byte-identical object file.
      Evidence and method: MEMORY.md → *Replaying the Descent* and *The Stride
      Is the GCD, Not the Difference*.
- [x] **The compiler is a quarter faster, and no file is over 450 lines** —
      measured, not asserted:
      - **Cranelift's verifier is off in release builds** (it stays on in
        debug, so the test suite still has it): the code generator is **23%
        faster**.
      - **Cranelift's `timing` feature is dropped** — it is in the crate's
        default features, and it costs a `clock_gettime` per pass: another
        **4%**.
      - `src/parser/tests.rs` (516 lines) and `src/codegen/tests.rs` (483)
        are split into `stmts`/`exprs`/`errors` and `types`/`errors`/
        `programs`; the lexer's operator arms moved into a helper, so `lex`
        is 125 lines rather than 203. No file in the tree is now over the
        450-line convention.
      Every one of these is a pure move, and the proof is the same in all
      four cases: all twelve examples compile to byte-identical object files
      before and after, so no generated code changed — which is why no
      benchmark needed re-running.
- [x] **The assignment sugars** — `x += e`, `x -= e`, `x *= e`, `x /= e`,
      `x %= e`, and the increment operators `x++`, `++x`, `x--`, `--x`. They
      are desugared *in the parser*: `x += e` becomes `x = x + e` and `x++`
      becomes `x = x + 1`, so the AST, the code generator and the type rules
      needed no new case at all — the assignment's own rules apply unchanged
      (the target must be a variable, it must not be a `const`, the
      arithmetic must type-check). The increment operators are statements
      rather than expressions, because TypeScript's postfix `x++` yields the
      *old* value, which this subset does not model; `print(x++)` is a parse
      error rather than a silently different program
- [x] **No file exceeds ~450 lines** — the audit convention (AGENTS.md Step
      6a) is now checked by hand and by the scorecard, and the two files
      that had crossed the line are split:
      - `src/rewrite/recurrence/mod.rs` (629) became `mod.rs` (the entry
        point and the shared types), `recognize.rs` (the half that decides
        whether a function has the recurrence shape) and `build.rs` (the half
        that rewrites it) — the seam the module already had;
      - `tests/rewrite.rs` (570) became `tests/rewrite_tail_call.rs` and
        `tests/rewrite_recurrence.rs`, one file per rewrite.
      Both were pure *moves*: the same unit tests before and after, the
      code extracted with `sed` rather than retyped, and `pub(super)` only
      on the calls that cross the new boundary. The proof is the object
      files: all twelve examples compile to byte-identical SHA-256s before
      and after, so the split changed no generated code.
- [x] **Measured what the `int` type is worth** (`scripts/bench_int.py`,
      interleaved rounds, min-of-N): **1.20x** on arithmetic-only code, **2.44x**
      on a function call per iteration, and **0.83x** — a small loss — on
      recursion.
      The recursion result is the interesting one: it says the scriptc gap is
      not about arithmetic.
      The fib case deliberately uses the shape the recurrence rewrite cannot
      fire on. Comparing the shape it *can* fire on would measure the rewrite,
      not the type: the `int` version becomes a loop and the `number` version
      stays exponential. That was a real bug in this script — see MEMORY.md →
      *A Benchmark Can Compare Two Different Programs*
- [x] **`return` adapts a literal to the function's return type** — `return 0;`
      in an `int` function used to be rejected, because the return statement did
      not pass the expected type to the expression. Found while writing a
      benchmark program, fixed, and covered by a regression test
- [x] **Linear recurrences become loops** — a function defined in terms of its
      own previous values (`fib(n) = fib(n-1) + fib(n-2)`) is exponential in
      the source program: the tree of calls has `F(depth)` leaves. The rewrite
      turns it into a loop with one accumulator per value the recurrence
      needs, so the same program computes the same answer — bit for bit, the
      loop performs the same operations in the same order on the same values —
      in linear time and in constant stack space. `fib(90)` is an astronomical
      number of calls as written and ninety after the rewrite. This is the
      classical transform
      (Kapur, Kogge & Palem, 1986); the recipe and the scoping are in the
      brainstorm below. It fires only on a shape it can recognise exactly: the
      calls must be at constant offsets, everything else in the expression
      must be free of side effects (the rewrite evaluates it in the opposite
      order), and the base expression must have no side effects (the source
      program evaluates it once per leaf, the rewrite once per starting
      value)
- [x] **The recurrence rewrite requires an `int` parameter** — the loop counts
      the values it computes with its own index, so a fractional argument made
      the loop's count and the recursion's depth disagree: `fib(2.5)` printed
      1 where the program computes 2. The rewrite now fires only on an `int`
      parameter, so a `number` fibonacci is left exactly as it was written. The
      `int` type and the rewrite are therefore entangled: there is no way to
      rewrite a `number` recurrence
- [x] **The recurrence rewrite covers the wider shapes** — the first version
      handled exactly two calls at offsets one and two. It now handles any
      number of terms, any constant offsets (`f(n-1) + f(n-3)`), a third
      term that reads the parameter (`f(n-1) + f(n-2) + n`), and a
      coefficient (`2 * f(n-1) + 3 * f(n-2)`) — the expression's shape is
      preserved exactly, so the answer stays bit for bit the same. The loop
      keeps one accumulator per value the recurrence needs, and counts with
      an induction variable of its own, because the recursion's level and
      the loop's index are not the same number
- [x] **Tail-call elimination** — a self-recursive call in tail position
      becomes a loop, so the recursion costs one frame instead of one per
      level. A recursion of ten million levels now runs in constant stack
      space; before, it died with "maximum recursion depth exceeded". The
      rewrite runs between parsing and code generation (`src/rewrite/`),
      so the code generator knows nothing about it. It fires only on a shape
      it can recognise exactly — the base case must be the body's first
      statement, the arguments must be parameters stepped by constants, and
      an accumulator term must have no side effects — because a rewrite that
      fires on the wrong program is a miscompile. Two miscompiles were found
      by the audit and fixed: an accumulator term that called a function was
      evaluated in the opposite order, and a local declared before the base
      case kept the outermost level's value. The end-to-end test now
      compiles every shape twice — with the tail call and as the equivalent
      loop — and asserts the two print the same thing
- [x] **Stack-overflow detection** — unbounded recursion used to exhaust the
      stack and die with SIGSEGV, killing the program and losing whatever it
      had printed. The runtime now installs a SIGSEGV handler on an alternate
      signal stack and turns the fault into "error: maximum recursion depth
      exceeded" with exit status 1; output printed before the overflow is
      preserved (stdout is flushed first). The language has no pointers, so
      the only way a compiled program can fault is by running out of stack
- [x] **Float remainder `%`** — `5.5 % 2.0` used to be rejected because
      Cranelift has no `frem` instruction. The runtime now provides
      `ts_fmod`, which is C's `fmod`: it truncates toward zero and takes the
      sign of the dividend, exactly like JavaScript's `%` (verified against
      Node). Integer `%` is unchanged — it is still a single `srem`
- [x] **Cranelift backend** — SSA variables, `if`/`else`, `while`, short-circuit
      `&&` / `||`, recursion, forward references, two-pass function collection
- [x] **Native binary output** — Mach-O/ELF object via `cranelift-object`, linked
      with `cc` against an embedded C runtime shim (`ts_print_number` /
      `ts_print_bool` / `ts_print_int` / `ts_runtime_init`)
- [x] **Entry points** — top-level statements (`__ts_start`) run before a user
      `function main(): void`; a C-ABI `main` wrapper drives both
- [x] **CLI** — `tsc <input.ts> [-o <output>] [-c]`, `--help`, exit codes 0/1/2
- [x] **PIC** — `is_pic` enabled so external calls link on macOS arm64
- [x] **Examples** — `hello.ts`, `fib.ts`, `control.ts`, `main.ts`, `numbers.ts`,
      `integers.ts`, `remainder.ts`, `tail_call.ts`, `recurrence.ts`,
      `stack_overflow.ts`, `for_loop.ts`, `cast.ts`
- [x] **Doc-sync enforcement** — `tests/docs_sync.rs` checks the AGENTS.md module
      tree, ARCHITECTURE.md mentions, TODO.md test counts, and README doc links
- [x] **Profiled the recursion gap to scriptc — it is algorithmic, not codegen**
      — `sample` on both binaries plus an exact count of function entries
      (instrument the function, count the printed lines) shows scriptc rewrites
      a self-recursive call into a double loop around one recursive call, which
      cuts the entry count by 5.24x. Our per-entry cost (~5 cycles, ~21
      instructions) is the better of the two; the gap is the *number* of
      entries. `scripts/bench_calls.py` was rewritten to measure entries and
      ns/entry separately — the old version assumed `2**depth` calls, which is
      wrong (it is `2*F(d+1)-1`), and duly reported 0.04 ns/call

### Quality
- [x] **The three biggest modules are directories** — `parser/` (693 lines),
      `rewrite/` (1459) and `codegen/` (1375) were single files; they are now
      split along the seams they already had, and no file is over about 450
      lines. The split was a *move*, verified by the test suite — the object
      files it produces are byte-for-byte identical to the ones the single
      files produced
- [x] **158 inline unit tests + 49 integration tests** — all passing; `cargo clippy
      --all-targets` exits 0; doc-sync enforced by `tests/docs_sync.rs`
- [x] **Inline unit tests for the lexer, parser, and type rules** — 14 lexer tests
      (token kinds, comments, line tracking, exact integer literals, lex errors),
      41 parser tests (precedence, associativity, statement forms including
      `for`, `as` casts and the assignment sugars, parse errors), 61 codegen
      tests (every type rule in `FnGen`, the `int` type, the cast rules, the
      runtime entry points, plus acceptance cases for scoping, shadowing,
      recursion, and forward references), and 42 rewrite tests (every shape
      the tail-call and recurrence rewrites accept, and every shape they must
      refuse)
- [x] **Sanitizer, ABI and fuzz gates** — `tests/sanitize.rs` compiles every
      example with the address and undefined-behaviour sanitizers and runs it
      (skipping itself when the sanitizers are unavailable);
      `tests/abi.rs` calls the generated code from a C driver, so a calling
      convention that disagrees with the platform's fails the test;
      `tests/fuzz.rs` feeds the compiler 300 random token sequences and
      asserts it never panics and never exits with an undocumented code —
      which is how the depth limits were found: before them, the compiler
      crashed with a stack overflow on deeply nested input
- [x] **Wiring guard** — a test that drives one program per AST variant and
      asserts the compiler never panics, plus a test that rejected programs
      always produce a diagnostic and never a successful compile
- [x] **Competitive benchmark** — `scripts/bench.py` compares the same programs
      compiled by our compiler, `scriptc`, `deno compile`, `bun build
      --compile`, `tsrust + node`, `tsc + node`, and our own published build;
      output agreement is a gate, and `bench/RESULTS.md` is committed with each
      change. Result: ~2000x smaller artifacts, ~1.4x faster startup, fastest
      compile time, the natural-shape fibonacci now *won* (the `number`
      recurrence rewrite), and scriptc ahead only on the recursion shape no
      rewrite recognises (reported honestly)
- [x] **A `number` recurrence is rewritten too** — the rewrite used to require
      an `int` parameter, because the loop counted a fixed integer lattice and
      a fractional argument would make the loop's count and the recursion's
      depth disagree. The `number` scheme has no fixed lattice, so the loop
      first walks the argument down to the base case with the same
      subtractions the recursion performs, then counts *down* from the
      argument, replaying that descent exactly — the values sit on the
      recursion's own lattice, so the answer is bit for bit the same
      (`fib(2.5)` is 2 rewritten and not). The one refusal: a term that reads
      the parameter, because a downward-counting loop has no level's value to
      give it. Verified by differential tests in `tests/rewrite.rs` and by the
      benchmark's criterion: the `fib` workload flipped from 68 ms against
      scriptc's 48 ms to **23 ms against scriptc's 50 ms** — the whole program
      now costs about as much as starting the process. The only workload
      scriptc still wins is `fib_recursive`, the shape no rewrite recognises
      (46 ms against 67 ms)
- [x] **JS-compatible number printing** — the runtime emits the shortest
      decimal that round-trips, exactly like `Number.prototype.toString`,
      instead of `%g`'s lossy six significant digits. Found by the benchmark;
      verified byte-identical against Node and locked in by `examples/numbers.ts`
      plus an e2e test
- [x] **End-to-end test harness** — `tests/e2e.rs` compiles each example, runs the
      binary, asserts exact stdout
- [x] **Negative tests** — type errors, const reassignment, undefined variables,
      missing returns, bad arithmetic, wrong arity
- [x] **Publishable package metadata** — description, Apache-2.0 `LICENSE`,
      repository/homepage, keywords, categories, `rust-version = "1.96"` (the
      dependency MSRV), `[[bin]] name = "tsc"` so the installed binary keeps its
      name despite the `compiler` package name
- [x] **Published to crates.io as `compiler` v0.1.0** — `cargo publish --dry-run`
      green; `cargo package --list` reviewed (no local-only files shipped);
      the earlier `tsc-rs` v0.1.0 publish was yanked (names are permanent, so a
      rename means a new crate)
- [x] **Published as `compiler` v0.3.0** — the tail-call rewrite, the
      recurrence rewrite, the module split, the depth limits, the audit fixes,
      and the new gates. Dry run green, `cargo package --list` reviewed, the
      poisoned build fingerprint cleaned and the release binary verified
      (its symbols present) before the upload
- [x] **Published as `compiler` v0.4.0** — the language grew: C-style `for`
      loops (desugared in the parser), `as` conversions between `number` and
      `int`, the compound assignments and the increment operators (sugar for
      assignments), and the recurrence rewrite now fires on a `number`
      parameter too — which flipped the benchmark's fibonacci from a loss
      against scriptc into a win. The compiler is also a quarter faster
      (Cranelift's verifier off in release builds, its `timing` feature
      dropped), and the tree is split so that no file is over the 450-line
      convention. Dry run green, `cargo package --list` reviewed, the build
      fingerprint cleaned and the release binary verified before the upload.
- [x] **Published as `compiler` v0.3.1** — the documentation audit and the
      benchmark-script fixes: three scripts had been silently measuring the
      recurrence rewrite instead of what they claimed, `bench/RESULTS.md` was
      regenerated, the README now quotes the generated report and is checked
      against it by three new doc-sync gates, and the honest competitive
      picture (scriptc ahead on recursive code) is recorded in README, TODO
      and MEMORY. Dry run green, `cargo package --list` reviewed, the build
      fingerprint cleaned and the release binary verified before the upload;
      the published artifact installed and run (its `fib(90)` matches
      `examples/recurrence.ts`)

## Next

### High priority
- [ ] **Strings** — add a `string` type (UTF-8 bytes + length, not a Rust
      `String`), string literals in the lexer, `+` concatenation, `print`
      support, comparison operators. The allocation strategy is decided —
      a program-lifetime bump arena, no collector (ARCHITECTURE.md → *Strings
      and arrays allocate from a bump arena*) — so the work is the lexer,
      the type and the runtime's string helpers
- [ ] **Arrays** — `number[]` and `boolean[]` with bounds-checked indexing, `push`,
      `length`. The same arena decides their allocation (ARCHITECTURE.md)
- [ ] **`for (const x of arr)`** — needs arrays first (the C-style `for` is
      done); a `for-of` desugars into an index loop over `arr[i]`, which needs
      `arr.length` and bounds-checked indexing

### Medium priority
- [ ] **Decide the binary name** — `cargo install compiler` currently installs a
      command called `tsc`, the same name as TypeScript's official compiler, and
      ours shadows it when earlier on `PATH`. Options: keep `tsc` as the primary
      name and also install a second, unambiguous name (`tscc`); or rename
      outright. `--version` now answers sanely, which removes the worst of the
      clash, but the shadowing itself remains
- [ ] **Logical assignment** — `&&=`, `||=`
- [ ] **Blocks as expressions** — `const x = { let y = 1; y + 1; }`
- [ ] **Conditional expression** — `cond ? a : b`
- [ ] **Shadowing diagnostics** — warn on redeclaration in nested scopes
- [ ] **Better error spans** — include the column, not just the line
- [ ] **Source maps** — map generated code back to `.ts` line numbers for
      debuggers (`debug info` section in the object file)

### Low priority
- [ ] **Modules** — `import` / `export` across files
- [ ] **Closures** — capturing lexical scope in a first-class function value
- [ ] **Interfaces and type annotations** — structural typing, compile-time only
- [ ] **Generics** — monomorphization for a small set of type parameters
- [ ] **Classes** — with fields, methods, and `this`
- [ ] **Objects and records** — `{ x: number }` literals and property access
- [ ] **Cross-compilation** — target triples other than the host
- [ ] **Emit assembly / LLVM IR** — `--emit-asm` for inspection and debugging
- [ ] **REPL** — interactive mode evaluating snippets against a persistent context

## Brainstorming

Ideas captured from competitive intelligence (AGENTS.md Step 8), not yet committed
to the roadmap:

- **scriptc (Vercel Labs)** — the only other engine-free native TypeScript
  compiler, and therefore our real competitor. Three ideas worth stealing:
  1. **`scriptc coverage`** — a command that reports *how much of a program
     compiled statically and, for the parts that did not, why*. That is
     user-visible value we have no equivalent of, and it would also guide our
     own "what can this subset compile?" story.
  2. **A three-tier compilation model** — static by default, with an escape
     hatch for the parts that cannot be statically compiled.
  3. **`comptime` (compile-time evaluation) and FFI** — both are large
     language features, but FFI in particular is what would make this usable
     for real programs (calling into C libraries) rather than demos.
  4. **Its recursion-rewriting pass** — the thing it does that we did not,
     and the reason it wins on recursive code. Measured: it rewrites a
     self-recursive function into a double loop around a single recursive
     call, cutting the number of function entries by 5.24x. Its own entries
     are ~2.4x more expensive than ours, and the net is ~2x on fib. It
     applies to every self-recursive shape we tried, so it is a general
     optimiser, not a fib special case. **We now do both halves of it**
     (`src/rewrite/`): the chain shape is a loop on our side too, and a
     linear recurrence is a loop with two accumulators — so `fib(90)`, which
     is an astronomical number of calls as written, costs ninety.
     **The caveat is closed (2026-11-07)**: scriptc's rewrite worked on
     `number` where ours needed an `int` parameter — the descent-replay
     scheme lifted that, and the cross-toolchain `fib` workload flipped:
     ours 23 ms against scriptc's 50 ms. What remains is scriptc's *wider*
     set of shapes: on the recursion written so no rewrite can fire, it
     still wins (46 ms against our 67 ms).
- **AssemblyScript** — compiles a TS-like language to WebAssembly with an
  explicit, GC-free memory model (`--runtime stub` gives 1-3 KiB runtimes).
  Worth studying their managed-object layout before designing tsc's runtime,
  and their `asc` binary shows how small a TS-like toolchain can be.
- **swc / oxc** — Rust-based TS toolchains with a much larger language surface.
  Their AST and swc's `common` crate show how to structure a parser that stays
  fast and how to keep a type-checking layer separate from codegen — a
  candidate refactor for tsc's fused checker if error ordering starts to matter.
- **`deno compile`** — bundles a TS program into a single self-contained binary.
  Relevant to a future "compile a whole program" mode with multiple files. Its
  `--bundle` flag (esbuild + tree-shaking) is the interesting part: it shrinks
  a 65 MiB binary by dropping unused npm payloads.
- **Cranelift's `cranelift-wasm`** — not applicable now, but shows how the same
  frontend can target multiple backends. Relevant if tsc ever needs a wasm target.
- **Selective SSA vs. stack slots** — Cranelift's SSA variables handle shadowing
  for free, but a future optimizer might prefer explicit stack slots for
  address-taken locals (once arrays/structs arrive and locals become
  addressable).
- **Benchmark honesty** — the `scriptc` result showed that a benchmark against
  only the engine-bundling toolchains would have looked like a landslide win
  while missing the real competitor. Keep adding toolchains as they appear.
- **scriptc's loop-optimisation suite** (from its changelog) — worth copying
  the *ideas* even where we cannot copy the implementation: "running short
  constant-count loops together, versioning `for` and `while` loops on the
  ranges their locals enter with, unrolling counted loops by the period of a
  guard, and inlining small leaf functions called from loops". Two of those
  are cheap for us and would show up in the `loop` workload: **loop
  versioning** (hoist the loop-invariant part of the condition) and
  **inlining a small leaf function called from a loop**.
- **The post-audit nits, deliberately not fixed** — the independent review
  found these and they are recorded rather than repaired, because each is a
  refactor of a mature module without a bug to justify it:
  - `define_body()` takes 14 parameters, five of them runtime `FuncId`s; a
    small `Runtime` struct would collapse the list.
  - `gen_binop()` is ~100 lines with `unreachable!()` arms that are only
    unreachable because of the earlier type checks.
  - `common.rs` in the rewrites has five depth-guarded recursive walkers that
    share the same guard; a generic walker would collapse them.
  - `int_hint()` returns `Some(Int)` or `None` and nothing else — it is
    `is_int()` wearing a disguise.
  - The tree is not rustfmt-clean, on purpose (see MEMORY.md).
- **scriptc's `--exceptions=rare`** — "unwinds through recursive functions
  instead of checking a status after each call". That per-call stack check is
  part of why scriptc's entries cost 2.4x ours, and it is optional there, so
  its own numbers depend on the flag. Worth measuring scriptc with the flag
  before quoting a per-entry cost against it.
- **The literature on recursion elimination** — Kapur, Kogge & Palem (1986)
  is the canonical reference for turning *linear* recursive functions into
  loops; the Cornell CS 6120 write-up is the practical version, with the
  four-step recipe and the "hopping backwards over the offsets" refinement.
  Stoller's *From recursion to iteration* paper is the general framework
  (incrementalisation). All three are named in the High-priority item above
  rather than in a list of ideas — the point of reading them is to scope
  *that* item, not to start a new one.
- **`tsrust` (ferrotype/tsr)** — TypeScript 7's Go compiler ported to Rust, with
  upstream parity suites as its acceptance criteria. It emits JavaScript, so it
  is not a native-binary competitor, but it is the reference for "what does a
  serious Rust TypeScript compiler look like" — the workspace layout (a crate
  per compiler phase: scanner, parser, binder, checker, printer, sourcemap,
  module resolution), the arena-based ownership model instead of a GC, and the
  parity-reporting discipline are all worth studying.
- **An `is_pic`-style shared-settings note** — tsr pins its toolchain one
  release behind stable and forbids `unsafe` in leaf crates with an explicit
  allow-list for the FFI crates. A similar "why is this lint allowed here"
  discipline would fit this project, which currently has no lint policy.

## Scorecard

Recorded per AGENTS.md Step 7. Total is /30.

| Date | Correctness | Codegen | Maintainability | Docs | Wiring | Footprint | Total |
|---|---|---|---|---|---|---|---|
| 2026-11-01 | 4 | 4 | 3 | 4 | 4 | 5 | 24 |
| 2026-11-01 (post-benchmark) | 5 | 4 | 4 | 5 | 4 | 5 | 27 |
| 2026-11-01 (post-int) | 5 | 4 | 4 | 5 | 5 | 5 | 28 |
| 2026-11-02 (post-stack-guard) | 5 | 4 | 4 | 5 | 5 | 5 | 28 |
| 2026-11-02 (post-profiling) | 5 | 4 | 4 | 5 | 5 | 5 | 28 |
| 2026-11-03 (post-tail-call) | 5 | 4 | 5 | 5 | 5 | 5 | 29 |
| 2026-11-03 (post-recurrence) | 5 | 4 | 5 | 5 | 5 | 5 | 29 |
| 2026-11-03 (post-audit) | 5 | 5 | 5 | 5 | 5 | 5 | 30 |
| 2026-11-05 (post-doc-audit) | 5 | 5 | 5 | 5 | 5 | 5 | 30 |
| 2026-11-07 (post-for-loop) | 5 | 5 | 5 | 5 | 5 | 5 | 30 |
| 2026-11-07 (post-performance) | 5 | 5 | 5 | 5 | 5 | 5 | 30 |

**Evidence (2026-11-07, post-for-loop)**: the C-style `for` loop landed as a
parser-level desugar — no AST variant, no codegen arm, no runtime change — so
generated code for every program that does not use `for` is byte-identical
(no benchmark workload does; the report stands). Coverage grew with the
feature: 4 parser unit tests, 4 e2e tests (including a differential one
against the hand-written `while`), the sanitizer gate, and the fuzzer's
vocabulary. The docs were audited against the code in the same pass: README,
SPEC (grammar included), ARCHITECTURE, TODO, AGENTS and MEMORY all describe
the desugar, and the two doc-sync count gates pass at 121 inline + 45
integration tests. One wrong arithmetic comment in the example itself
(`129` for a sum of `130`) was caught by the audit and fixed.
*Maintainability caveat, recorded the same day and settled the same day:
with the `as` casts and the `number`-recurrence work landed, two files
exceeded the ~450-line convention — `src/rewrite/recurrence/mod.rs` (629)
and `tests/rewrite.rs` (570). Both are now split (see Done), proven by
byte-identical object-file SHA-256s, and AGENTS.md Step 6a makes the check
standing rather than advisory. The same pass found two more over the line
(the parser's and the code generator's test files) and a 203-line `lex`
function; all are now split, so no file in the tree is over the
convention.*

**Evidence (2026-11-05, post-doc-audit)**: an audit of every markdown file
against the code. It found one real bug and a dozen drifts.

**Docs stays 5, but the previous 5 was not earned.** The 2026-11-03 scorecard
recorded Docs 5/5 on the strength of a doc-sync test that checks the docs
*exist* and that the counts match. It checked nothing about whether the
numbers in the prose were true, and the README was quoting a benchmark table
from a run that predated a commit which changed the generated code. The
audit found that, fixed it, and added three gates that now check the
figures against the generated report. The score is unchanged; what changed
is what the score means.

- **The bug**: `scripts/bench_int.py` compared a `number` fibonacci against an
  `int` one *in the shape the recurrence rewrite fires on*. The rewrite needs
  an `int` parameter, so the `int` variant had become a loop and the
  `number` one had not — the script was measuring "rewrite vs no rewrite"
  and reporting it as "int vs number". It read 2.90x. The script now uses
  the shape the rewrite cannot fire on, and reads 0.83x. The same bug was in
  `scripts/bench.py`'s headline `fib` workload, which is why the committed
  `bench/RESULTS.md` showed a 28 ms fibonacci that the current compiler
  takes 73 ms to run. Both scripts are fixed and the report regenerated.
- **The benchmark gained a workload**: `int-fib-rewritten` measures what the
  rewrite actually buys — 24 ms against 77 ms for the same program written
  so the rewrite cannot fire. The report has a section for it.
- **The drifts**: the README's benchmark table and prose quoted the old run;
  the README's language example claimed a `number` fibonacci was rewritten
  (it is not, since the audit commit); the README said "Four design points"
  over five bullets and omitted `%` from the operator list; SPEC.md's
  grammar had no `%` in it and described the type names as reserved words
  when `let number = 5;` compiles; ARCHITECTURE.md and TODO.md said the
  call tree has `2^depth` leaves when it has `F(depth)` — a formula
  MEMORY.md already had right and explicitly called out as a trap;
  MEMORY.md still quoted the pre-fix `%g` runtime, and three files put the
  fixed-notation range at `[-6, 21]` when it is `[-6, 20]`. All corrected.
- **The gates added**: the README's quoted benchmark factors, its table
  cells, and the load average and spawn floor it names are now all checked
  against `bench/RESULTS.md` by `tests/docs_sync.rs`. Each one was verified
  to fail on injected drift.
- **The sweep found a third instance of the same bug**: `bench_shape.py`'s
  `binary_int` shape used the shape the recurrence rewrite fires on, so it
  reported "the shape was optimised away" for what was supposed to be the
  per-entry cost of `int` recursion. It now uses the unrewritten shape and
  reads 1.46 ns/entry. AGENTS.md Step 3a now says to grep the benchmark
  scripts for the shape a rewrite changes.
- **The lesson**: the scorecard measures the *code*, not the *claims about
  the code*. A 30/30 score was recorded on 2026-11-03 while the README
  quoted numbers from a run that predated the audit commit. The doc-sync
  test checks that the docs *exist* and that the counts match; nothing
  checked that the numbers were true. AGENTS.md Step 3a already says to
  re-benchmark any change that affects generated code — the audit commit
  changed it and did not. The benchmark is now regenerated as part of this
  change, and the README quotes it rather than a remembered version.

**Evidence (2026-11-03, post-audit)**: an independent review of the whole
codebase along the AGENTS.md Step 6 checklist. It found five real bugs, all
fixed with tests (listed under Done); twenty documentation drifts, all
corrected; no dead code and no wiring gaps. The sanitizer, ABI and fuzz gates
now close the Codegen dimension's last item — the runtime is checked under
the address and undefined-behaviour sanitizers on every `cargo test`, the
generated code is called from C to pin the calling convention, and the fuzz
test asserts the compiler never panics on malformed input. The remaining
known limitation is recorded under Correctness: integer arithmetic wraps,
there is no cross-platform pass, and the recurrence rewrite only applies to
integer parameters.

**Evidence (2026-11-03, post-recurrence)**:
- *Correctness 5/5*: `cargo test` green — 121 inline unit tests + 45 integration
  tests. Every language feature has a runnable example, and every number the
  runtime prints is verified byte-identical to JavaScript's `Number::toString`.
  The benchmark found and fixed a real precision bug (`%g` truncating to six
  significant digits) and a real type bug (`return 0;` rejected in an `int`
  function). Both rewrites were **audited before they shipped**: the
  tail-call rewrite had two real miscompiles (an accumulator term with a
  call in it was evaluated in the opposite order; a local declared before
  the base case kept the outermost level's value), both fixed, and both
  locked in by tests. The end-to-end tests now compile every rewritten shape
  twice — with the rewrite and as the equivalent program written by hand —
  and assert the two agree. Coverage measurement is still the one gap.
- *Codegen 4/5*: binaries produce reference outputs; PIC verified on macOS
  arm64. Benchmarked against deno, bun, scriptc, tsrust, tsc and our own
  published build: **~2000x smaller artifacts than the engine-bundling
  toolchains, ~1.3x faster startup, fastest compile time**. Unbounded
  recursion is reported cleanly instead of crashing, a tail recursion costs
  no stack, and a linear recurrence is compiled to a loop. The one remaining
  reason this is not 5: there is still no cross-platform or UB/valgrind pass.
  *(This block predates the documentation audit; the benchmark numbers it
  quotes were measured before the recurrence rewrite stopped applying to
  `number` parameters, and the fibonacci claim no longer held. See the
  current evidence below.)*
- *Maintainability 5/5*: the three biggest modules are directories, and no file
  is over about 450 lines: `parser/` (the statement forms and the precedence
  levels), `rewrite/` (the two rewrites and the predicates they share), and
  `codegen/` (the type lattice, the C shim, and the two halves of the code
  generator). The split followed the seams the modules already had, so no
  abstraction was invented — and it was a *move*, so the unit tests and the
  end-to-end tests are what prove it changed nothing. The optimisations
  live in their own module rather than inside the code generator, which is
  what the Mature Module Policy asks for; `rewrite/` has 35 unit tests of its
  own, one per shape it accepts and one per shape it must refuse. Clippy
  clean. `scripts/bench_calls.py` was rewritten because it measured the wrong
  thing — it is now self-checking (`--self-test`).
- *Docs 5/5*: README, SPEC, ARCHITECTURE, TODO, MEMORY, AGENTS, and
  `bench/RESULTS.md` all present, mutually linked, and doc-sync enforced by
  `cargo test`. The benchmark report is generated, not hand-written, so it
  cannot drift from reality. Both rewrites are specified in SPEC.md and
  their rules are stated in the module, the spec, and the tests.
- *Wiring 5/5*: AST variants handled in both parser and codegen; a wiring
  guard drives one program per AST variant and asserts the compiler never
  panics; rejected programs always produce a diagnostic. The rewrites are
  wired between the parser and the code generator, and they must not hide a
  type error — a rule with its own test. CLI flags documented; the benchmark
  exercises the real CLI end to end.
- *Footprint 5/5*: five dependencies, all essential (Cranelift × 4 +
  target-lexicon); no speculative crates; the CLI avoids `clap` in favour of
  hand-rolled parsing; the shipped runtime is ~150 lines of C. The rewrites
  added no dependency and no runtime code.

**Next weakest area**: Codegen (5/5), Docs (5/5) — but the *documentation
audit* below found two things the scorecard had not: a benchmark script that
compared two different programs, and a set of docs that had drifted away from
the code after the audit commit. Both are fixed. The recurring lesson is that
the scorecard measures the *code*, not the *claims about the code*: a 30/30
score said nothing about whether the README's numbers were true.

**The competitive gap, updated (2026-11-07)**: the `number`-recurrence gap is
closed — the rewrite fires on `number` parameters by replaying the argument's
descent, and the `fib` workload flipped to a win (23 ms against scriptc's
50 ms). What remains is coverage of *shapes*: on the recursion written so no
rewrite can fire (`fib_recursive`), scriptc's more general optimiser still
wins (46 ms against our 67 ms). Widening the recognisable shapes — and the
"hop backwards over the offsets" refinement — are the High-priority items
above.
