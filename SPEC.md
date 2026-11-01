# Specification

This document defines the contract of the **tsc** compiler: its CLI, the language
subset it accepts, the type rules it enforces, and the runtime ABI it produces.

## CLI

```
tsc <input.ts> [-o <output>] [-c]
```

| Flag | Meaning |
|---|---|
| `<input.ts>` | Source file to compile (required, exactly one) |
| `-o`, `--output <path>` | Output path. Default: input path with its extension replaced |
| `-c`, `--compile-only` | Emit an object file only, do not link. Default output: `<stem>.o` |
| `-h`, `--help` | Print usage and exit 0 |

**Behaviour**
- Success: prints the output path to stdout, exits `0`.
- Compile error: prints `error: <message>` to stderr, exits `1`.
- Linker failure: prints `error: linker failed`, exits `1`.
- Usage error (unknown flag, missing or repeated input): prints usage to stderr,
  exits `2`.

**Requirements**
- A C compiler must be available on `PATH` as `cc` (linking only; `-c` does not
  need one).
- The compiler targets the host platform (`Triple::host()`).

## Language

### Types

| Type | Representation | Notes |
|---|---|---|
| `number` | IEEE-754 double (`f64`) | All numeric literals are `number`; there is no integer type |
| `boolean` | 8-bit integer (`i8`), 0 = false, 1 = true | |
| `void` | no value | Return type of a function with no value |

A type annotation is optional where the type can be inferred from the
initializer. Annotations are mandatory for parameters.

### Lexical structure

- **Identifiers**: `[A-Za-z_$][A-Za-z0-9_$]*`
- **Numbers**: digit sequences with an optional fractional part — `1`, `2.5`, `0.`
  No exponent notation; no numeric separators.
- **Comments**: `// line comment` and `/* block comment */` (block comments do not
  nest).
- **Keywords**: `let`, `const`, `function`, `if`, `else`, `while`, `return`,
  `true`, `false`.
- **Reserved for future use**: `number`, `boolean`, `void` are recognised only in
  type-annotation position.

### Grammar

```ebnf
program      ::= statement* ;

statement    ::= letDecl
               | funcDecl
               | ifStmt
               | whileStmt
               | returnStmt
               | block
               | exprStmt ;

letDecl      ::= ("let" | "const") IDENT (":" type)? ("=" expr)? ";" ;
               (* at least one of the type annotation and the initializer is required *)

funcDecl     ::= "function" IDENT "(" params? ")" (":" type)? block ;
params       ::= param ("," param)* ;
param        ::= IDENT ":" type ;

ifStmt       ::= "if" "(" expr ")" stmtOrBlock ("else" stmtOrBlock)? ;
whileStmt    ::= "while" "(" expr ")" stmtOrBlock ;
returnStmt   ::= "return" expr? ";" ;
block        ::= "{" statement* "}" ;
stmtOrBlock  ::= block | statement ;
exprStmt     ::= expr ";" ;

expr         ::= assignment ;
assignment   ::= orExpr ("=" assignment)? ;   (* LHS must be a variable *)
orExpr       ::= andExpr ("||" andExpr)* ;
andExpr      ::= equality ("&&" equality)* ;
equality     ::= comparison (("==" | "!=") comparison)* ;
comparison   ::= term (("<" | "<=" | ">" | ">=") term)* ;
term         ::= factor (("+" | "-") factor)* ;
factor       ::= unary (("*" | "/") unary)* ;
unary        ::= ("!" | "-") unary | call ;
call         ::= primary ("(" args? ")")* ;
primary      ::= NUMBER | "true" | "false" | IDENT | "(" expr ")" ;
args         ::= expr ("," expr)* ;
type         ::= "number" | "boolean" | "void" ;
```

**Operator precedence**, loosest to tightest: assignment → `||` → `&&` →
equality → relational → additive → multiplicative → unary → call → primary.
Assignment is right-associative; all binary operators are left-associative.

**Not supported**: `%`, `++`, `--`, compound assignment, `? :`, `for`, `do`,
`switch`, `break`, `continue`, `throw`, `try`, object literals, array literals,
string literals, `new`, `typeof`, `instanceof`, `in`, `of`, `delete`, `void`
operator, `as`, generics, classes, modules, arrow functions.

### Statements

- **`let` / `const`** — declares a variable in the current block scope. `const`
  bindings cannot be reassigned. Redeclaring a name in the same scope is an
  error; shadowing in a nested scope is allowed.
- **`function`** — declares a function in the enclosing scope. Functions may be
  called before their declaration appears in the file, and may recurse. Function
  declarations are not allowed nested inside another function body.
- **`if` / `else`** — the else branch is optional. The condition must be a
  `boolean` or a `number`.
- **`while`** — the condition must be a `boolean` or a `number`.
- **`return`** — returns from the enclosing function. A `void` function may
  `return;`; a value-returning function must `return <value>;` on every path.
- **Block** — introduces a new scope.
- **Expression statement** — evaluates an expression and discards the result.

### Entry points

- **Top-level statements** are compiled into a synthetic `void` function and run
  before anything else.
- A user-declared `function main(): void` — no parameters, `void` return — is
  called after the top-level statements.
- The generated binary's C `main` returns `0` on normal completion.
- A program with neither top-level statements nor `main` is valid and does
  nothing.

### Built-ins

| Function | Signature | Behaviour |
|---|---|---|
| `print` | `print(number)` / `print(boolean)` | Prints the value followed by a newline |

- `print` is reserved: it cannot be redefined.
- Numbers print with `%g` formatting (`42`, `12.5664`).
- Booleans print as `true` / `false`.
- There is no `print` overload for `void`; printing a void expression is an error.

### Type rules

- Arithmetic (`+`, `-`, `*`, `/`) requires two `number` operands and yields
  `number`.
- Relational operators (`<`, `<=`, `>`, `>=`) require two `number` operands and
  yield `boolean`.
- Equality (`==`, `!=`) requires two operands of the *same* type (`number` with
  `number`, `boolean` with `boolean`) and yields `boolean`.
- `&&`, `||` accept a `boolean` or `number` operand, yield `boolean`, and
  **short-circuit** — the right operand is not evaluated when the result is
  already determined.
- `!` accepts a `boolean` or `number`, yields `boolean`.
- Unary `-` requires a `number`, yields `number`.
- A `number` is false iff it equals `0`.
- Assignment requires the assigned expression's type to equal the variable's
  declared type exactly — there are no implicit conversions.
- A function call's argument types must match the parameter types exactly.
- A function whose declared return type is not `void` must return a value on
  every path.
- A `void`-returning function may not return a value.

### Diagnostics

All diagnostics are single-line messages prefixed with the source line:

```
line 3: expected ';', found Ident("x")
```

Errors are reported for: unknown types, type mismatches on initialization and
assignment, arithmetic or relational type errors, comparing mismatched types,
using a void value where a value is required, assigning to a `const`, undefined
variables, undefined functions, wrong argument counts, wrong argument types,
functions that don't return on all paths, nested function declarations,
`print` of a void value, and redefining `print` or `main` incorrectly.

## Runtime ABI

The compiler emits a relocatable object file and links it with a small C
translation unit containing exactly two functions:

```c
void ts_print_number(double x);
void ts_print_bool(signed char x);
```

These are imported by the generated code under `Linkage::Import`. `double` maps
to Cranelift's `F64` and `signed char` to `I8`, so the ABI matches exactly on
every supported platform.

User-declared functions are exported as `tsfn_<name>`; the synthetic top-level
function is `__ts_start`; the C entry point is `main`. All calls use the
platform's default calling convention (`CallConv::triple_default`).

Position-independent code is enabled (`is_pic`), so calls to the imported
runtime functions use PC-relative references and link cleanly on macOS arm64.
