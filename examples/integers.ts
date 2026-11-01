// The `int` type: 64-bit integers with exact arithmetic.
//
// Literals are `number` by default, but in an `int` context they become
// integers — so `let n: int = 42` is exact, and integer arithmetic compiles
// to integer instructions rather than floating point.
let a: int = 42;
let b: int = 10;

print(a + b);   // 52
print(a - b);   // 32
print(a * b);   // 420
print(a / b);   // 4  (integer division truncates toward zero)
print(a % b);   // 2
print(a < b);   // false
print(a == 42); // true

// A literal adopts the type of the other operand.
let c = a + 1;
print(c);       // 43

// Integer arithmetic stays exact well past 2^53.
let big: int = 9007199254740993;
print(big);     // 9007199254740993

// Recursive integer fibonacci — the workload that motivates the int type.
function fib(n: int): int {
    if (n < 2) {
        return n;
    }
    return fib(n - 1) + fib(n - 2);
}
print(fib(35));  // 9227465

// A literal adapts to the function's return type.
function zero(): int {
    return 0;
}
print(zero());   // 0
