// A self-recursive call in tail position does not need a stack frame: the
// compiler turns it into a loop, so the recursion can be as deep as the
// numbers allow rather than as deep as the stack does.

// A tail call — the result of the call is returned directly. The recursion
// runs a hundred thousand times and still leaves one frame behind.
function lastDigit(n: number): number {
    if (n < 10) { return n; }
    return lastDigit(n - 10);
}
print(lastDigit(1000003));   // 3

// The same shape with an accumulator: the call's result is combined with
// something, and the combination is carried along the loop instead of being
// left on the stack.
function sumTo(n: int): int {
    if (n < 1) { return 0; }
    return sumTo(n - 1) + n;
}
print(sumTo(1000000));   // 500000500000

// Subtraction and multiplication accumulate too.
function minusTo(n: int): int {
    if (n < 1) { return 100; }
    return minusTo(n - 1) - n;
}
print(minusTo(5));   // 85

function timesTo(n: int): int {
    if (n < 1) { return 1; }
    return timesTo(n - 1) * 3;
}
print(timesTo(4));   // 81

// The accumulator can be a parameter instead: it is stepped along with the
// recursion, one level at a time.
//
// The accumulator forms above are written with `int`, and that is not an
// accident: the rewrite folds the terms in a different order than the
// source program does, which is exact for integers and not for floating
// point. A `number` program is left exactly as it was written.
function countBy(n: int, step: int): int {
    if (n < 1) { return step; }
    return countBy(n - 1, step + 2);
}
print(countBy(4, 1));   // 9

// The `int` type takes the same route, and stays exact.
function sumToInt(n: int): int {
    if (n < 1) { return 0; }
    return sumToInt(n - 1) + n;
}
print(sumToInt(1000000));   // 500000500000

// A tail call in a void function works the same way: the call is a
// statement, and the statements that run on every level move into the loop.
function countdown(n: number): void {
    if (n < 1) { return; }
    print(n);
    countdown(n - 1);
}
countdown(3);   // 3, 2, 1
