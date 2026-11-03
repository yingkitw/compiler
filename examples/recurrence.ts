// A linear recurrence — a function defined in terms of its own previous
// values — is exponential in the source program: the tree of calls has 2^n
// leaves. The compiler rewrites it into a loop, so the same program computes
// the same answer in linear time, in constant stack space.

// The classic: fibonacci. As written, fib(90) would make 2^90 calls; the
// rewrite makes ninety of them.
function fib(n: int): int {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
print(fib(90));   // 2880067194370816120

// The same shape with `number`, where the answer is a double.
function fibNumber(n: number): number {
    if (n < 2) { return n; }
    return fibNumber(n - 1) + fibNumber(n - 2);
}
print(fibNumber(70));   // 190392490709135

// Subtraction and multiplication are rewritten the same way.
function difference(n: int): int {
    if (n < 2) { return n; }
    return difference(n - 1) - difference(n - 2);
}
print(difference(10));   // -1

function product(n: int): int {
    if (n < 2) { return 1; }
    return product(n - 1) * product(n - 2);
}
print(product(10));   // 1
