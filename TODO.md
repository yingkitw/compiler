# TODO

## Done

### Core
- [x] **Lexer** — tokens, line tracking, `//` and `/* */` comments, numeric literals
- [x] **Parser** — recursive descent, full operator precedence, `let`/`const`,
      functions, `if`/`else`, `while`, `return`, blocks, type annotations
- [x] **Type system** — `number` / `boolean` / `void`, fused type checking in codegen,
      diagnostics for mismatches, const reassignment, undefined names, missing returns
- [x] **Cranelift backend** — SSA variables, `if`/`else`, `while`, short-circuit
      `&&` / `||`, recursion, forward references, two-pass function collection
- [x] **Native binary output** — Mach-O/ELF object via `cranelift-object`, linked
      with `cc` against an embedded C runtime shim (`ts_print_number` / `ts_print_bool`)
- [x] **Entry points** — top-level statements (`__ts_start`) run before a user
      `function main(): void`; a C-ABI `main` wrapper drives both
- [x] **CLI** — `tsc <input.ts> [-o <output>] [-c]`, `--help`, exit codes 0/1/2
- [x] **PIC** — `is_pic` enabled so external calls link on macOS arm64
- [x] **Examples** — `hello.ts`, `fib.ts`, `control.ts`, `main.ts`
- [x] **Doc-sync enforcement** — `tests/docs_sync.rs` checks the AGENTS.md module
      tree, ARCHITECTURE.md mentions, TODO.md test counts, and README doc links

### Quality
- [x] **0 inline unit tests + 15 integration tests** — all passing; `cargo clippy
      --all-targets` exits 0; doc-sync enforced by `tests/docs_sync.rs`
- [x] **End-to-end test harness** — `tests/e2e.rs` compiles each example, runs the
      binary, asserts exact stdout
- [x] **Negative tests** — type errors, const reassignment, undefined variables,
      missing returns, bad arithmetic, wrong arity
- [x] **Publishable package metadata** — description, Apache-2.0 `LICENSE`,
      repository/homepage, keywords, categories, `rust-version = "1.96"` (the
      dependency MSRV), `[[bin]] name = "tsc"` so the installed binary keeps its
      name despite the `tsc-rs` package name
- [x] **Published to crates.io as `tsc-rs` v0.1.0** — `cargo publish --dry-run`
      green; `cargo package --list` reviewed (no local-only files shipped)

## Next

### High priority
- [ ] **Strings** — add a `string` type (UTF-8 bytes + length, not a Rust `String`),
      string literals in the lexer, `+` concatenation, `print` support, comparison
      operators. Needs a GC or arena decision first — see ARCHITECTURE.md
- [ ] **Arrays** — `number[]` and `boolean[]` with bounds-checked indexing, `push`,
      `length`. Same allocation strategy decision as strings
- [ ] **`for` loops** — `for (let i = 0; i < n; i = i + 1)` and `for (const x of arr)`
- [ ] **`%` (modulo)** — float remainder; Cranelift has no `frem` instruction, so
      this needs a runtime helper (`fmod`) rather than a single instruction
- [ ] **Integer type** — `number` is f64 only. Add `int` (i64) as a distinct type so
      bitwise ops, `%`, and integer division are exact

### Medium priority
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

- **AssemblyScript-style model** — AssemblyScript compiles a TS-like language to
  wasm/native with an explicit memory model and no GC by default. Worth studying
  their managed-object layout before designing tsc's runtime.
- **swc / oxc** — Rust-based TS toolchains with a much larger language surface.
  Their AST and swc's `common` crate show how to structure a parser that stays
  fast and how to keep a type-checking layer separate from codegen — a
  candidate refactor for tsc's fused checker if error ordering starts to matter.
- **`deno compile`** — bundles a TS program into a single self-contained binary.
  Relevant to a future "compile a whole program" mode with multiple files.
- **Cranelift's `cranelift-wasm`** — not applicable now, but shows how the same
  frontend can target multiple backends. Relevant if tsc ever needs a wasm target.
- **Selective SSA vs. stack slots** — Cranelift's SSA variables handle shadowing
  for free, but a future optimizer might prefer explicit stack slots for
  address-taken locals (once arrays/structs arrive and locals become
  addressable).

## Scorecard

Recorded per AGENTS.md Step 7. Total is /30.

| Date | Correctness | Codegen | Maintainability | Docs | Wiring | Footprint | Total |
|---|---|---|---|---|---|---|---|
| 2026-11-01 | 4 | 4 | 3 | 4 | 4 | 5 | 24 |

**Evidence (2026-11-01)**:
- *Correctness 4/5*: `cargo test` green (15 integration tests); every language
  feature has a runnable example. Not 5 — no inline unit tests for lexer/parser
  layers, and no coverage measurement yet.
- *Codegen 4/5*: binaries produce reference outputs; PIC fixed and verified on
  macOS arm64; unreachable-block handling verified. Not 5 — no cross-platform
  testing, no UB/valgrind pass.
- *Maintainability 3/5*: `codegen.rs` is 682 lines with a large `FnGen` impl;
  `gen_expr`/`gen_stmt` carry several responsibilities each. Clippy clean.
- *Docs 4/5*: README, SPEC, ARCHITECTURE, TODO, MEMORY, AGENTS all present and
  doc-sync enforced. Not 5 — the docs describe a tiny language, so the surface to
  document is still small.
- *Wiring 4/5*: AST variants handled in both parser and codegen; CLI flags
  documented. Not 5 — no wiring test that would catch a new AST variant missing
  a codegen arm.
- *Footprint 5/5*: five dependencies, all essential (Cranelift × 4 + target-lexicon);
  no speculative crates; the CLI avoids `clap` in favour of hand-rolled parsing.

**Next weakest area**: Maintainability (3/5) — but per the Mature Module Policy,
`codegen.rs` is fixed by **adding tests first** (inline unit tests for the
lexer/parser and for the type rules), not by restructuring the module. The
highest-value single item is therefore inline unit tests, folded into the
"Next" section as the first concrete task.
