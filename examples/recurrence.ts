// A linear recurrence — a function defined in terms of its own previous
// values — is exponential in the source program: the tree of calls has F(n)
// leaves. The compiler rewrites it into a loop, so the same program computes
// the same answer in linear time, in constant stack space.

// The classic: fibonacci. As written, fib(90) would make an astronomical
// number of calls; the rewrite makes ninety of them.
//
// The rewrite needs the parameter to be an `int`: the loop counts the values
// it computes with its own index, and a fractional argument would make the
// loop's count and the recursion's depth disagree. A `number` fibonacci is
// left exactly as it was written.
function fib(n: int): int {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
print(fib(90));   // 2880067194370816120

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

// The rewrite is not limited to two terms or to neighbouring ones: this
// recurrence calls itself one and three steps back, and adds a term that
// reads the parameter.
function sparse(n: int): int {
    if (n < 4) { return n; }
    return sparse(n - 1) + sparse(n - 3) + n;
}
print(sparse(12));   // 324
