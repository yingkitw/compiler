// Conditional expression `?:` — only the taken branch is evaluated.
print(true ? 1 : 2);
print(false ? 1 : 2);

let n: number = 10;
print(n > 0 ? n : 0);

print(false ? 1 / 0 : 42);
print(true ? 7 : 1 / 0);

// Nested ternaries associate to the right.
print(true ? false ? 1 : 2 : 3);
