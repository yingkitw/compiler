// A C-style `for` loop. The compiler desugars it into the statements it
// already knows — the init runs once, then a `while` over the condition
// whose body is the loop body followed by the step — so the code generator
// never learns about `for` at all.
//
// The init lives in its own block, which is why two sequential loops may
// each declare `let i`.
let total = 0;
for (let i = 0; i < 5; i = i + 1) {
    total = total + i;
}
print(total);          // 0 + 1 + 2 + 3 + 4 = 10

// The init, condition and step are each optional. A missing condition is
// `true`, like `while (true)`.
let countdown = 3;
for (; countdown > 0; countdown = countdown - 1) {
    print(countdown);  // 3, 2, 1
}

// Each init is in its own scope, so two loops may declare the same name.
let factorial: int = 1;
for (let i: int = 1; i <= 5; i = i + 1) {
    factorial = factorial * i;
}
for (let i: int = 1; i <= 4; i = i + 1) {
    factorial = factorial + i;
}
print(factorial);      // 120 + 1 + 2 + 3 + 4 = 130

// The body may be a single statement.
let squares = 0;
for (let i = 0; i < 4; i = i + 1) squares = squares + i * i;
print(squares);        // 0 + 1 + 4 + 9 = 14

// `int` loops step by whole numbers, so a big count stays exact.
let powers: int = 1;
for (let i: int = 0; i < 10; i = i + 1) powers = powers * 2;
print(powers);         // 1024
