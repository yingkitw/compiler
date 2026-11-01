// The remainder operator works on `number` as well as `int`. For doubles it
// truncates toward zero and takes the sign of the dividend — exactly like
// JavaScript's `%`.
print(5.5 % 2.0);
print(-5.5 % 2.0);
print(5.5 % -2.0);
print(7.0 % 2.5);
print(5.0 % 2.0);

// Integer remainders truncate toward zero too.
let a: int = 7;
let b: int = 3;
print(a % b);
print(-7 % 3);
