// Explicit conversions between the two numeric types with `as`.
//
// Unlike TypeScript's `as` — an assertion the compiler checks and erases —
// these are real conversions: `number` and `int` have different machine
// representations, so `x as int` emits a conversion instruction.
let x = 3.9;
print(x as int);           // 3 — truncates toward zero
print(-x as int);          // -3 — the cast wraps the whole left side

// Casts chain, each wrapping the one before it.
print(x as int as number); // 3

// `int` keeps values a double cannot; converting back is honest about it.
let big: int = 9007199254740993;   // 2^53 + 1, exact as an int
print(big);                // 9007199254740993
print(big as number);      // 9007199254740992 — a double cannot hold it

// Out-of-range and NaN are defined, not traps: NaN becomes 0 and the value
// saturates at the ends of the i64 range, the same as Rust's `as`.
let nan = 0.0 / 0.0;
print(nan as int);         // 0
let huge = 10000000000.0 * 10000000000.0;  // 1e20 — beyond the i64 range
print(huge as int);        // 9223372036854775807 — saturated
print(-huge as int);       // -9223372036854775808

// A cast to the value's own type is a no-op.
let same = 7;
print(same as number);     // 7

// `as` applies to the expression accumulated to its left — TypeScript's
// precedence — and the result may keep taking operators.
print(1 + 2.7 as int);     // trunc(3.7) = 3
let n = 5;
let m: int = 4;
print(n as int + m);       // 9
