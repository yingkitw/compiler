// Compound assignment and the increment operators.
//
// They are sugar: `x += e` means `x = x + e`, and `x++` means `x = x + 1`.
// The compiler desugars them while parsing, so the rules that apply are the
// assignment's own — the target must be a variable, it must not be a
// `const`, and the arithmetic must type-check.
let x = 1;
x += 2;
print(x);            // 3
x -= 1;
print(x);            // 2
x *= 10;
print(x);            // 20
x /= 4;
print(x);            // 5
x %= 3;
print(x);            // 2

// The increment and decrement operators, in both their forms.
x++;
print(x);            // 3
++x;
print(x);            // 4
x--;
print(x);            // 3
--x;
print(x);            // 2

// They work on the `int` type as well, and keep it exact.
let i: int = 5;
i++;
print(i);            // 6
i += 10;
print(i);            // 16

// The right-hand side is an ordinary expression, evaluated once.
let n = 2;
n *= n + 1;
print(n);            // 6
