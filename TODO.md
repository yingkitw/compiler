# TODO

## Done

### Core
- [x] **Lexer** — tokens, line tracking, `//` and `/* */` comments, numeric literals
- [x] **Parser** — recursive descent, full operator precedence, `let`/`const`,
      functions, `if`/`else`, `while`, `return`, blocks, type annotations
- [x] **Type system** — `number` / `int` / `boolean` / `void`, fused type checking
      in codegen, diagnostics for mismatches, const reassignment, undefined names,
      missing returns
- [x] **`int` type (i64)** — exact integer arithmetic, integer division that
      truncates toward zero, `%` for integers, integer comparisons. A literal
      adopts the type of its context, and the lexer keeps a literal's *exact*
      integer value so `9007199254740993` (2^53 + 1) survives — a double would
      round it. No implicit conversion between `int` and `number`.
- [x] **Measured what the `int` type is worth** (`scripts/bench_int.py`,
      interleaved rounds, min-of-N): **1.43x** on arithmetic-only code, **1.94x**
      on a function call per iteration, and **0.98x** — a wash — on recursion.
      The recursion result is the interesting one: it says the scriptc gap is
      not about arithmetic
- [x] **`return` adapts a literal to the function's return type** — `return 0;`
      in an `int` function used to be rejected, because the return statement did
      not pass the expected type to the expression. Found while writing a
      benchmark program, fixed, and covered by a regression test
- [x] **Tail-call elimination** — a self-recursive call in tail position
      becomes a loop, so the recursion costs one frame instead of one per
      level. A recursion of ten million levels now runs in constant stack
      space; before, it died with "maximum recursion depth exceeded". The
      rewrite runs between parsing and code generation (`src/rewrite.rs`),
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
      `integers.ts`, `stack_overflow.ts`
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
- [x] **104 inline unit tests + 26 integration tests** — all passing; `cargo clippy
      --all-targets` exits 0; doc-sync enforced by `tests/docs_sync.rs`
- [x] **Inline unit tests for the lexer, parser, and type rules** — 12 lexer tests
      (token kinds, comments, line tracking, exact integer literals, lex errors),
      23 parser tests (precedence, associativity, statement forms, parse errors),
      46 codegen tests (every type rule in `FnGen`, the `int` type, the runtime
      entry points, plus acceptance cases for scoping, shadowing, recursion, and
      forward references), and 23 rewrite tests (every shape the tail-call
      rewrite accepts, and every shape it must refuse)
- [x] **Wiring guard** — a test that drives one program per AST variant and
      asserts the compiler never panics, plus a test that rejected programs
      always produce a diagnostic and never a successful compile
- [x] **Competitive benchmark** — `scripts/bench.py` compares the same programs
      compiled by tsc-rs, `deno compile`, `bun build --compile`, and `tsc + node`;
      output agreement is a gate, and `bench/RESULTS.md` is committed with each
      change. Result: ~2000x smaller artifacts, 2-3x faster startup, fastest
      compile time, ~2.2x faster compute than deno on call-heavy code, and
      roughly tied with a JIT on tight numeric loops (reported honestly)
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

## Next

### High priority
- [ ] **The linear-recurrence rewrite — the rest of the reason we lose to
      scriptc on recursive code.** The tail-call rewrite (now done) closes the
      gap on the *chain* shape: `f(n) = f(n-1)` is a loop for us too, which
      is what scriptc compiles it to. The gap that remains is the *binary*
      shape. Profiling (2026-11-02: `sample` on both binaries plus an exact
      entry count) showed the gap is *algorithmic*, not a codegen problem: on
      `fib(44)` we make **2,269,806,339** function entries at **1.47 ns**
      each, scriptc makes **433,494,437** entries at **3.8 ns** each.
      scriptc rewrites the recursion into a double loop around one recursive
      call, which cuts the entry count by a constant **5.24x**. Its own
      entries are ~2.4x more expensive than ours (it saves 12 registers and
      checks the stack limit per call), so the net is ~2x.
      The transform to implement is the classical one: a *linear recurrence*
      `f(n) = f(n-1) + f(n-2)` is a loop with two accumulators, so fib
      becomes O(n) with no recursion at all — which would beat scriptc's
      partial rewrite rather than match it. The work is in recognising the
      shape and computing the base values: the accumulator's type, the base
      case's contribution, and the coefficients all have to line up. Scope it
      before starting: a first version could target only `f(n) = f(n-1) +
      f(n-2)` with a literal base case.
      Evidence and method: MEMORY.md → *Profiling the Recursion Gap* and
      *Counting Function Entries Exactly*; `scripts/bench_calls.py`,
      `scripts/bench_shape.py`.
- [ ] **Strings** — add a `string` type (UTF-8 bytes + length, not a Rust
      `String`), string literals in the lexer, `+` concatenation, `print`
      support, comparison operators. Needs a GC or arena decision first — see
      ARCHITECTURE.md
- [ ] **Arrays** — `number[]` and `boolean[]` with bounds-checked indexing, `push`,
      `length`. Same allocation strategy decision as strings
- [ ] **`for` loops** — `for (let i = 0; i < n; i = i + 1)` and `for (const x of arr)`
- [ ] **Explicit `int`/`number` conversion** — `x as int` would let a program
      narrow deliberately instead of relying on literal adaptation

### Medium priority
- [ ] **Decide the binary name** — `cargo install compiler` currently installs a
      command called `tsc`, the same name as TypeScript's official compiler, and
      ours shadows it when earlier on `PATH`. Options: keep `tsc` as the primary
      name and also install a second, unambiguous name (`tscc`); or rename
      outright. `--version` now answers sanely, which removes the worst of the
      clash, but the shadowing itself remains
- [ ] **`++` / `--` operators** — prefix and postfix forms
- [ ] **Compound assignment** — `+=`, `-=`, `*=`, `/=`
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
     optimiser, not a fib special case. **We now do the tail-call half of
     it** (`src/rewrite.rs`): the chain shape is a loop on our side too, and
     `scripts/bench_shape.py` reports no per-entry cost for either compiler.
     The half that is left is the *linear recurrence* — scoped as a
     High-priority item above.
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

**Evidence (2026-11-03, post-tail-call)**:
- *Correctness 5/5*: `cargo test` green — 104 inline unit tests + 26 integration
  tests. Every language feature has a runnable example, and every number the
  runtime prints is verified byte-identical to JavaScript's `Number::toString`.
  The benchmark found and fixed a real precision bug (`%g` truncating to six
  significant digits) and a real type bug (`return 0;` rejected in an `int`
  function). The tail-call rewrite was **audited before it shipped**: two real
  miscompiles were found (an accumulator term with a call in it was evaluated
  in the opposite order; a local declared before the base case kept the
  outermost level's value), both fixed, and both locked in by tests. The
  end-to-end test now compiles every rewritten shape twice — with the tail
  call and as the equivalent loop — and asserts the two agree. Coverage
  measurement is still the one gap.
- *Codegen 4/5*: binaries produce reference outputs; PIC verified on macOS
  arm64. Benchmarked against deno, bun, scriptc, tsrust, tsc and our own
  published build: **~2000x smaller artifacts than the engine-bundling
  toolchains, ~1.4x faster startup, fastest compile time**, and faster than
  scriptc on tight loops. Unbounded recursion is reported cleanly instead of
  crashing, and a tail recursion now costs no stack at all. Not 5 — the
  recursion gap to scriptc is still open on the *binary* shape, but
  **profiling has shown it is not a codegen defect**: our calls are the
  cheaper of the two (1.48 ns per entry) and the gap is that scriptc makes
  5.24x fewer entries. The chain shape is closed: both compilers now compile
  `f(n) = f(n-1)` to a loop. There is also still no cross-platform or
  UB/valgrind pass.
- *Maintainability 5/5*: the code generator is still one large `FnGen` impl,
  but it is covered by 46 unit tests, and the new optimisation lives in its
  own module (`src/rewrite.rs`) instead of inside it — the rewrite pass was
  added without touching the mature module, which is what the Mature Module
  Policy asks for. `rewrite.rs` has 23 unit tests of its own, one per shape
  it accepts and one per shape it must refuse. Clippy clean.
  `scripts/bench_calls.py` was rewritten because it measured the wrong thing —
  it is now self-checking (`--self-test`).
- *Docs 5/5*: README, SPEC, ARCHITECTURE, TODO, MEMORY, AGENTS, and
  `bench/RESULTS.md` all present, mutually linked, and doc-sync enforced by
  `cargo test`. The benchmark report is generated, not hand-written, so it
  cannot drift from reality. The tail-call rewrite is specified in SPEC.md
  and its rules are stated in the module, the spec, and the tests.
- *Wiring 5/5*: AST variants handled in both parser and codegen; a wiring
  guard drives one program per AST variant and asserts the compiler never
  panics; rejected programs always produce a diagnostic. The rewrite is wired
  between the parser and the code generator, and it must not hide a type
  error — a rule with its own test. CLI flags documented; the benchmark
  exercises the real CLI end to end.
- *Footprint 5/5*: five dependencies, all essential (Cranelift × 4 +
  target-lexicon); no speculative crates; the CLI avoids `clap` in favour of
  hand-rolled parsing; the shipped runtime is ~120 lines of C. The rewrite
  added no dependency and no runtime code.

**Next weakest area**: Codegen (4/5) — two concrete items. **The recursion gap
to scriptc on the binary shape** is now scoped: it needs a linear-recurrence
rewrite (`f(n) = f(n-1) + f(n-2)` is a loop with two accumulators), and it is
a missing feature rather than a defect (our per-entry cost is already the
better of the two). **A UB/valgrind pass** is the other, and is the only
remaining gap under *Correctness*.
