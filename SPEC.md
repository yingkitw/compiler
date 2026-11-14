# Specification

This document defines the contract of the **tsc** compiler: its CLI, the language
subset it accepts, the type rules it enforces, and the runtime ABI it produces.

## CLI

The installed commands are **`tsc`** (primary) and **`tscc`** (same program,
unambiguous on `PATH` when Microsoft's TypeScript `tsc` is present).

```
tsc <input.ts> [-o <output>] [-c]
```

| Flag | Meaning |
|---|---|
| `<input.ts>` | Source file to compile (required, exactly one) |
| `-o`, `--output <path>` | Output path. Default: input path with its extension replaced |
| `-c`, `--compile-only` | Emit an object file only, do not link. Default output: `<stem>.o` |
| `-V`, `--version` | Print the compiler's name and version and exit 0 |
| `-h`, `--help` | Print usage and exit 0 |

**Environment**

- `CC` chooses the C compiler used at link time (default: `cc`).
- `CFLAGS` supplies extra flags for that compiler, split on whitespace.
- `TSC_DUMP_ASM=<path>` writes the generated assembly to `<path>` before
  linking — the inspection aid the assembly backend makes nearly free.

All three are conventional, and `CC`/`CFLAGS` are what makes the sanitizers
reachable: `CC=clang CFLAGS="-fsanitize=address,undefined"` links the
runtime with the sanitizers.

**Behaviour**
- Success: prints the output path to stdout, exits `0`.
- Compile error: prints `error: <message>` to stderr, exits `1`.
- Assembler/linker failure: prints `error: the assembler failed` or
  `error: linker failed`, exits `1`.
- Usage error (unknown flag, missing or repeated input): prints usage to stderr,
  exits `2`.

**Requirements**
- A C compiler must be available on `PATH` — `cc` by default, or whatever
  `CC` names. It assembles the emitted `.s` for `-c` and links the binary
  otherwise, so both modes need one (before the assembly backend, `-c` did
  not).
- The compiler targets the host platform; the assembly it emits is AArch64
  (Apple silicon) today, assembled and linked by the system `cc`.

**Debug line information**

- Every `Stmt` carries the 1-based source line where it was parsed.
- Emitted assembly begins with `.file 1 "<input.ts>"` and each translated
  statement is preceded by `.loc 1 <line> 0`, so `cc -c` produces an object
  with a DWARF line table mapping machine code to TypeScript source lines.
- Type-check and codegen errors still have no source locations (parse/lex
  diagnostics use `line N, column M:`).

## Language

### Types

| Type | Representation | Notes |
|---|---|---|
| `number` | IEEE-754 double (`f64`) | The default type of a numeric literal |
| `int` | 64-bit signed integer (`i64`) | Integer division truncates toward zero; arithmetic wraps, including `i64::MIN / -1` (there is no overflow check). Division by zero reports `error: integer division by zero or overflow` and exits 1; `%` by zero currently yields the dividend (the AArch64 `srem` result) rather than an error — a known divergence, see TODO |
| `boolean` | 8-bit integer (`i8`), 0 = false, 1 = true | |
| `string` | pointer in `x0` + byte length in `x1` | Literals live in read-only data; `+` concatenates into a bump arena |
| `number[]` / `boolean[]` | pointer to `{ data, len, cap }` in x0 | Literals and `push` allocate from the bump arena; out-of-range indices abort with a runtime diagnostic |
| `void` | no value | The return type of a function with no value; not allowed as a variable or parameter type |

A type annotation is optional where the type can be inferred from the
initializer. Annotations are mandatory for parameters.

### Lexical structure

- **Identifiers**: `[A-Za-z_$][A-Za-z0-9_$]*`
- **Numbers**: digit sequences with an optional fractional part — `1`, `2.5`, `0.`
  No exponent notation; no numeric separators.
- **Comments**: `// line comment` and `/* block comment */` (block comments do not
  nest).
- **Keywords**: `let`, `const`, `function`, `if`, `else`, `while`, `for`,
  `as`, `of`, `return`, `true`, `false`.
- **Operators**: `+=`, `-=`, `*=`, `/=`, `%=`, `&&=`, `||=`, `++` and `--`
  are tokens of their own; the compound assignments are sugar for the
  assignments they stand for (see *Statements*).
- **Type names**: `number`, `int`, `boolean`, `string`, `void`, plus the
  array types `number[]` and `boolean[]`. These are ordinary identifiers —
  `let number = 5;` is legal — and they name a type only when they appear in
  a type-annotation position.

### Grammar

```ebnf
program      ::= statement* ;

statement    ::= letDecl
               | funcDecl
               | ifStmt
               | whileStmt
               | forStmt
               | forOfStmt
               | returnStmt
               | block
               | stepStmt
               | exprStmt ;

letDecl      ::= ("let" | "const") IDENT (":" valueType)? ("=" expr)? ";" ;
               (* at least one of the type annotation and the initializer is required *)

funcDecl     ::= "function" IDENT "(" params? ")" (":" type)? block ;
params       ::= param ("," param)* ;
param        ::= IDENT ":" valueType ;

ifStmt       ::= "if" "(" expr ")" stmtOrBlock ("else" stmtOrBlock)? ;
whileStmt    ::= "while" "(" expr ")" stmtOrBlock ;
forStmt      ::= "for" "(" forInit expr? ";" expr? ")" stmtOrBlock ;
forInit      ::= letDecl | expr ";" | ";" ;
               (* the init is optional: a `let`/`const` declaration consumes
                  its own ";", an expression is followed by one, and a lone
                  ";" means there is no init. An absent condition is `true`;
                  an absent step (nothing before the ")") is no step *)
forOfStmt    ::= "for" "(" ("let" | "const") IDENT "of" expr ")" stmtOrBlock ;
               (* the iterable must be number[] or boolean[]; it desugars to
                  a hidden binding, .length, an index, and a while loop *)
returnStmt   ::= "return" expr? ";" ;
block        ::= "{" statement* "}" ;
stmtOrBlock  ::= block | statement ;
exprStmt     ::= expr ";" ;
stepStmt     ::= ("++" | "--") IDENT ";" ;   (* the prefix form *)
               (* the postfix form is `IDENT ("++" | "--") ";"` *)

expr         ::= assignment ;
assignment   ::= conditional (assignOp assignment)? ;   (* LHS must be a variable *)
assignOp     ::= "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&&=" | "||=" ;
               (* every compound form is sugar: `x += e` is `x = x + e`,
                  `x &&= e` is `x = x && e` *)
conditional  ::= orExpr ("?" assignment ":" conditional)? ;   (* right-assoc *)
orExpr       ::= andExpr ("||" andExpr)* ;
andExpr      ::= equality ("&&" equality)* ;
equality     ::= comparison (("==" | "!=") comparison)* ;
comparison   ::= term (("<" | "<=" | ">" | ">=") term | "as" type castTail)* ;
castTail     ::= (("*" | "/" | "%") unary)* (("+" | "-") factor)* ;
               (* `as` has the precedence of the relational operators and
                  wraps the whole expression to its left — `1 + 2 as int`
                  is `(1 + 2) as int` — but a tighter operator may still
                  follow the cast, so `x as int + 1` is `(x as int) + 1`;
                  both are TypeScript's behaviour *)
term         ::= factor (("+" | "-") factor)* ;
factor       ::= unary (("*" | "/" | "%") unary)* ;
unary        ::= ("!" | "-") unary | call ;
call         ::= primary ( "(" args? ")" | "[" expr "]" | "." IDENT )* ;
               (* `(...)` is a call, `[e]` indexes an array, `.ident` is a
                  member: `.length` and the `push` method *)
primary      ::= NUMBER | STRING | "true" | "false" | IDENT
               | "(" expr ")" | "[" args? "]" | blockExpr ;
               (* "[" args? "]" is an array literal; the element types must
                  agree; "[]" needs a type annotation on its binding *)
blockExpr    ::= "{" statement+ "}" ;
               (* in expression position: the last statement must be an
                  expression statement, and the block cannot be empty — the
                  final expression is the block's value *)
args         ::= expr ("," expr)* ;
valueType    ::= "number" | "int" | "boolean" | "string"
               | ("number" | "boolean") "[]" ;
               (* everything a variable can hold; `void` is a return type
                  only, and arrays are number[] and boolean[] only *)
type         ::= valueType | "void" ;
               (* the return-type position may name void; the target of `as`
                  must be numeric — boolean and void casts are errors *)
```

**Operator precedence**, loosest to tightest: assignment → `?:` → `||` → `&&` →
equality → relational (`<` … `>=` and `as`) → additive → multiplicative
(`*`, `/`, `%`) → unary → call / index / member → primary.
Assignment is right-associative; all binary operators are left-associative.

**Not supported**: `do`, `switch`, `break`, `continue`, `throw`, `try`, `new`,
`typeof`, `instanceof`, `in`, `delete`, the `void` operator, generics, classes,
modules, and arrow functions.

The `%` (remainder) operator is supported for `int` and `number` operands. For
`number` it truncates toward zero and takes the sign of the dividend, like
JavaScript's `%`.

### Statements

- **`let` / `const`** — declares a variable in the current block scope. `const`
  bindings cannot be reassigned. Redeclaring a name in the same scope is an
  error; shadowing in a nested scope is allowed but produces a compiler
  warning (`'x' shadows an outer declaration`).
- **`function`** — declares a function in the enclosing scope. Functions may be
  called before their declaration appears in the file, and may recurse. Function
  declarations are not allowed nested inside another function body.
- **`if` / `else`** — the else branch is optional. The condition must be a
  `boolean`, `number`, or `int`.
- **`while`** — the condition must be a `boolean`, `number`, or `int`.
- **`for`** — C-style: `for (init; condition; step) body`. The init may be a
  `let` or `const` declaration, an assignment, or absent; the condition may be
  absent (`true`); the step may be absent. Each of the three parts is
  terminated by a `;`.
  The loop is the statements it desugars to: the init runs once, then a
  `while` over the condition whose body is the loop body followed by the
  step. The init lives in its own block, so two sequential loops may each
  declare the same name, and the init's variable is scoped to the loop.
  `break` and `continue` do not exist, so nothing in a loop body can target
  the loop rather than the `while` it becomes.
  The condition must be a `boolean`, `number`, or `int`.
- **`for-of`** — `for (let x of iterable)` or `for (const x of iterable)`. The
  iterable must be a `number[]` or `boolean[]`. Each iteration binds `x` to the
  next element. It desugars to a hidden array binding, a `length` counter, an
  index, and a `while (index < length)` whose body declares `x` from
  `array[index]` then runs the user body and increments the index. `break` and
  `continue` are not supported.
- **`return`** — returns from the enclosing function. A `void` function may
  `return;`; a value-returning function must `return <value>;` on every path.
- **Block** — introduces a new scope.
- **Block expression** — `{ stmt*; expr; }` in expression position. The
  statements run in a nested scope; the block's value is the final expression
  statement (which must be present; an empty block is an error). The value
  cannot be `void`.
- **Expression statement** — evaluates an expression and discards the result.
- **Compound assignment** — `x += e` means `x = x + e`, and likewise `-=`,
  `*=`, `/=` and `%=`. Logical `x &&= e` and `x ||= e` mean `x = x && e` and
  `x = x || e` with the same short-circuit evaluation as `&&` and `||`. The
  target must be a variable, and the result must have the variable's type — the
  rules are those of the assignment it stands for (so `&&=` / `||=` require a
  `boolean` variable, because `&&` and `||` yield `boolean`).
- **Increment and decrement** — `x++`, `++x`, `x--` and `--x` mean
  `x = x + 1` and `x = x - 1`. They are *statements*, not expressions:
  TypeScript's postfix `x++` yields the old value of `x`, which this subset
  does not model, so `print(x++)` is a parse error rather than a program
  that quietly means something else.

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
| `print` | `print(number)` / `print(int)` / `print(boolean)` / `print(string)` | Prints the value followed by a newline |

- `print` is reserved: it cannot be redefined.
- Numbers print exactly like JavaScript's `Number.prototype.toString`: the
  shortest decimal string that round-trips back to the same `f64`, in fixed
  notation for decimal exponents in [-6, 20] and exponential notation
  otherwise (`42`, `12.56636`, `0.3333333333333333`, `1e+21`, `1e-7`).
  `NaN` prints as `NaN`, division by zero as `Infinity`, and `-0` as `0`.
- Booleans print as `true` / `false`.
- Strings print as their raw UTF-8 bytes followed by a newline.
- There is no `print` overload for `void` or for arrays; printing either is
  an error.

### Tail-call elimination

A self-recursive call in tail position is turned into a loop before code
generation. The rewrite fires only on a shape it can recognise exactly:

```ts
function f(n: number): number {
    if (n < 1) { return 0; }   // the base case, and the first statement
    return f(n - 1);            // a tail call: the last statement
}
```

becomes

```ts
function f(n: number): number {
    while (n >= 1) { n = n - 1; }
    return 0;
}
```

The observable effect is that a tail recursion no longer uses the stack: a
recursion of ten million levels runs in constant stack space, where it used
to fail with `maximum recursion depth exceeded`. A tail recursion whose base
case is never reached now loops forever instead of overflowing the stack —
the same as any other infinite loop in the language.

The rewrite never changes what a program computes, and it is deliberately
narrow. It fires only when

- the base case is the *first* statement of the function body — a statement
  before it would run once per recursion level in the source program and once
  in the rewritten one — and is an `if` with no `else` whose only action is to
  return;
- its condition compares the first parameter against a literal;
- the recursive call is the last statement of the body, and every argument is
  a parameter plus or minus an integer constant, with the first parameter's
  step downwards;
- an accumulator term (`return f(n - 1) + x;`) has no side effects and the
  function returns an `int` — see below;
- the statements that run on every level of a void tail call (`f(n - 1);` in a
  `void` function) neither call the function again nor assign.

An accumulator changes the *order* the terms are combined in: the source
program folds them from the innermost level outwards, and the loop folds them
the other way. That is exact for `int`, whose wrapping arithmetic is
associative, but not for `number`, whose floating-point addition is not — so
a function returning `number` is left exactly as it was written, and only a
pure tail call (`return f(n - 1);`, with no accumulator) is rewritten for it.

Anything else is compiled exactly as written.

### Linear recurrences

A *linear recurrence* — a function defined in terms of its own previous
values — is exponential in the source program: the tree of calls has
`F(depth)` leaves and `2·F(depth) − 1` calls in total. The values satisfy a
recurrence that can be iterated, so the compiler rewrites the function into a
loop:

```ts
function fib(n: int): int {
    if (n < 2) { return n; }        // the base case
    return fib(n - 1) + fib(n - 2);
}
```

becomes, in effect,

```ts
function fib(n: int): int {
    if (n < 2) { return n; }        // the base case, unchanged
    let a = fib(0);                  // the two starting values
    let b = fib(1);
    while (n > 1) {
        let c = b + a;
        a = b;
        b = c;
        n = n - 1;
    }
    return b;
}
```

— linear time, constant stack, and the same value, bit for bit: the loop
performs the same additions, in the same order, as the recursion does.

The rewrite fires only when

- the function returns a `number` or an `int`, and its first parameter is an
  `int` — or a `number` whose expression does not read the parameter outside
  the self-calls (see below),
- the base case is the first statement of the body, with the same rules as
  the tail-call rewrite,
- the base expression has no side effects. The source program evaluates it
  once per leaf of the call tree; the rewrite evaluates it once per starting
  value,
- the rest of the body is a single `return` whose expression calls the
  function at least once, always at a constant offset — `n - 1`, `n - 3`, … —
  and whose every other part is free of side effects, and
- every other argument of those calls is a parameter passed on unchanged.

For an `int` parameter the loop counts a fixed lattice with its own index:
a fractional argument cannot occur, so the loop's count and the recursion's
depth cannot disagree. For a `number` parameter there is no fixed lattice —
the recursion's arguments are `n`, `n − 1`, `n − 2`, … at whatever fraction
`n` carries — so the loop first walks the argument down to the base case
with the same subtractions the recursion performs, finding the largest
argument that reaches it, and then counts *down* from the argument, replaying
that descent exactly. The values the loop computes sit on the recursion's
own lattice, so the answer is the same, bit for bit; and because the loop
counts down, an expression term that reads the parameter has no level's value
it could see — such a term refuses the rewrite on a `number` parameter (an
`int` loop, which counts up from the base, can give it one). One observable
difference remains: when the descent cannot make progress — an argument near
`i64::MAX`, at or above 2^53, or an infinity — the rewritten program loops
where the source program would have exhausted the stack and been reported.

The loop keeps as many values as the widest offset needs, so a recurrence
with a gap (`f(n - 1) + f(n - 3)`) is rewritten too, as is one with a third
term (`f(n - 1) + f(n - 2) + 1`) or a coefficient (`2 * f(n - 1)`). The
expression's shape is preserved exactly, so the answer is the same, bit for
bit.

When every offset the expression uses is a multiple of the same number — the
*stride* — the arguments the recursion reaches lie on one lattice (the ones
congruent to the initial argument), and the loop steps by the stride and
keeps one value per lattice position instead of one per argument:
`f(n - 2) + f(n - 4)` steps by 2 and keeps two values, not four. The first
value on the lattice is the first argument above the base point that is
congruent to the argument itself, which is only known at run time. The
values computed do not change.

### Type rules

- Arithmetic (`+`, `-`, `*`, `/`) requires both operands to have the *same*
  type, either two `number`s or two `int`s, and yields that type.
  - `int` division truncates toward zero: `7 / 2` is `4` when both operands
    are `int`.
  - There is **no implicit conversion** between `number` and `int`. Mixing them
    is an error.
- `%` (remainder) requires two operands of the same numeric type and yields
  that type. Integer `%` truncates toward zero; for `number` it truncates
  toward zero and takes the sign of the dividend, like JavaScript's `%`.
- Relational operators (`<`, `<=`, `>`, `>=`) require two `number`s or two
  `int`s, and yield `boolean`.
- `string` `+` requires two `string` operands and yields `string`; the result
  is allocated from a program-lifetime bump arena.
- Equality (`==`, `!=`) requires both operands to have the *same* type
  (`number`, `int`, `boolean`, or `string`) and yields `boolean`. For
  `string`, equality is bytewise on the UTF-8 payload (length must match).
- Arrays: a literal `[e, …]` may hold only `number` or `boolean` literals,
  all of the same kind; `[]` is allowed only where the binding's annotation
  supplies the element type. Indexing `a[i]` requires `i` to be an `int`
  and yields the element type; `a.length` yields an `int`; `a.push(x)`
  requires `x` to have the element type and yields `void`.
- The conditional `cond ? then : else` evaluates `cond` (with the same
  truthiness rules as `if`), then **only one** of `then` and `else`. Both
  branches must have the same type; mixing `number` and `int` is an error.
  Nested ternaries associate to the right (`a ? b : c ? d : e` is
  `a ? b : (c ? d : e)`).
- `&&`, `||` accept a `boolean`, `number` or `int` operand, yield `boolean`,
  and **short-circuit** — the right operand is not evaluated when the result is
  already determined.
- `!` accepts a `boolean`, `number` or `int`, yields `boolean`.
- Unary `-` requires a `number` or `int`, and yields that type.
- A `number` is false iff it equals `0`; an `int` is false iff it equals `0`.
- Assignment requires the assigned expression's type to equal the variable's
  declared type exactly.
- A function call's argument types must match the parameter types exactly.
- A function whose declared return type is not `void` must return a value on
  every path.
- A `void`-returning function may not return a value.

### Casts

`x as int` and `x as number` convert between the two numeric types. Unlike
TypeScript's `as` — an assertion the checker verifies and then erases —
these are real conversions, because the two types have different machine
representations:

- **`number` → `int` truncates toward zero**: `3.9 as int` is `3` and
  `-3.9 as int` is `-3`. `NaN` becomes `0`, and a value outside the `i64`
  range saturates at the range's ends — the same semantics as Rust's `as`.
- **`int` → `number` is exact up to 2^53** and rounds beyond it: a double
  cannot hold more.
- A cast to the value's own type is a no-op.
- `boolean` and `void` are not numeric: casting either from a numeric type,
  casting either to a numeric type, and casting to `void` are all errors
  (`cannot cast boolean to int`, …). The parser rejects `as void` with a
  line number.

A cast operates on *values*, unlike a literal's adaptation to its context:
`4.5 as int` is `4`, while `let x: int = 4.5;` is an error.

### Integer literals

A numeric literal has no fixed type of its own. It is a `number` by default,
and becomes an `int` when its context requires one:

```ts
let a: int = 42;      // 42 is an int here
let b = 42;           // 42 is a number here
let c = a + 1;        // 1 is an int here, because `a` is an int
let d = 1 + a;        // likewise, and the literal is evaluated after `a`
```

A literal used as an `int` must be written without a fraction point, be
integral, and fit in `i64`: `let x: int = 4.5` and `let x: int = 1.0` are
errors — the lexer treats `1.0` as a `number` literal even when its value is
integral. Integer syntax without a fitting `i64` (for example
`-9223372036854775808`) may still be used in an `int` context when the value
is exact. A negative literal adopts the context the same way a positive one
does: `let x: int = -5` is an `int`. Mixing `int` and `number` in arithmetic or
comparison (`a + 1.0` when `a` is `int`, or `a == 1.0`) is a compile error, not
an implicit conversion.

### Diagnostics

Lexer and parser diagnostics are single-line messages prefixed with the
source line and column (1-based):

```
line 3, column 9: expected ';', found Ident("x")
```

Diagnostics produced after parsing — the type errors and the code
generator's depth limit — are a single line without a prefix:

```
cannot assign boolean to 'x: number'
```

Errors are reported for: unknown types, type mismatches on initialization and
assignment, arithmetic or relational type errors, comparing mismatched types,
casting a non-numeric type, using a void value where a value is required,
assigning to a `const`, undefined variables, undefined functions, wrong
argument counts, wrong argument types, functions that don't return on all
paths, nested function declarations, duplicate function declarations, `print`
of a void value or an array, `void` used as a variable or parameter type,
array literals that mix element kinds or hold non-literals, a non-`int` array
index, an unknown field or method, expressions nested deeper than the
compiler's limit, and redefining `print` or `main` incorrectly.

## Runtime ABI

The compiler emits assembly text; the system C compiler assembles it and
links it with a small C translation unit. The functions the generated code
imports, by group:

```c
/* printing (JavaScript's Number::toString for doubles) */
void ts_print_number(double x);
void ts_print_bool(signed char x);
void ts_print_int(long long x);
void ts_print_string(const char *data, unsigned long len);

/* arithmetic the hardware does not offer */
double ts_fmod(double x, double y);       /* C fmod == JavaScript's % */
long long ts_double_to_int(double x);     /* truncate; NaN→0; saturate */
void ts_int_div_zero(void);               /* the int `/` divisor-zero path */

/* strings and arrays, allocated from one bump arena */
unsigned char ts_str_equal(const char *a, unsigned long alen,
                           const char *b, unsigned long blen);
struct { char *data; unsigned long len; }
ts_str_concat(const char *a, unsigned long alen,
              const char *b, unsigned long blen);
struct number_array *ts_number_array_new(unsigned long cap);
void ts_number_array_push(struct number_array *a, double v);
double ts_number_array_get(struct number_array *a, long long i);
struct bool_array *ts_bool_array_new(unsigned long cap);
void ts_bool_array_push(struct bool_array *a, signed char v);
signed char ts_bool_array_get(struct bool_array *a, long long i);
unsigned long ts_array_len(struct number_array *a);   /* bool arrays too */

void ts_runtime_init(void);
```

`ts_runtime_init` is called first by the generated C `main`. It installs
handlers on an alternate signal stack: a `SIGSEGV` handler, so that
unbounded recursion exhausting the stack is reported as

```
error: maximum recursion depth exceeded
```

and a `SIGFPE`/`SIGILL` handler, so that a trapping integer division is
reported as

```
error: integer division by zero or overflow
```

both on stderr with exit status 1, instead of killing the process. Output
printed before either is preserved. The language has no pointers, so the
only fault a compiled program can take besides those is a hardware one.

Two integer-division details are defined rather than left to the hardware:

- **AArch64 `sdiv` does not trap** — it returns 0 — so the generated code
  checks the divisor itself and calls `ts_int_div_zero`, which prints the
  message above and exits 1.
- **`i64::MIN / -1` wraps** to `i64::MIN`, like every other integer
  overflow: there is no overflow check anywhere in `int` arithmetic. The
  hardware performs it without trapping. Integer `%` by zero likewise takes
  the hardware's answer — the dividend — because `srem` neither traps nor
  is guarded today; that divergence from the `/` rule is a recorded TODO.

These are imported by the generated code as ordinary external symbols. At
the C ABI (AAPCS64 on arm64), `double` is passed and returned in `v0`/`d0`,
integers and pointers in `x0`/`x1`/…, and a `string` occupies two registers
(`x0` data, `x1` byte length). `ts_fmod` exists because the AArch64 ISA has
no float remainder instruction in the path we emit; it is C's `fmod`, which
truncates toward zero and takes the sign of the dividend — JavaScript's `%`.

The generated code keeps its locals in the callee-saved registers
(`d8`–`d13`, `x19`–`x24`) where it can, and saves and restores the ones it
uses around the function body — so it honours the platform's callee-saved
discipline, and an object produced with `-c` links against other C code
without surprises.

Strings and arrays allocate from a single program-lifetime **bump arena**
(256 KiB) in the runtime. Nothing is ever reclaimed; exceeding the arena
reports `error: string allocation limit exceeded` and exits 1. String
literals and array element data live elsewhere: literals in the object's
read-only data, growth by doubling. An out-of-range array index reports
`error: array index out of bounds` and exits 1.

User-declared functions are exported as `tsfn_<name>`; the synthetic
top-level function is `__ts_start`; the C entry point is `main`. All calls
follow the platform's default C calling convention.

Runtime calls use `bl` to linker-resolved symbols with the platform's usual
relocation rules, which link cleanly on macOS arm64 and Linux ELF when built
with `cc`.
