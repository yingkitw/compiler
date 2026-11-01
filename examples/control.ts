// Loops, conditions, booleans, short-circuit evaluation.
let i = 0;
let sum = 0;
while (i < 10) {
    sum = sum + i;
    i = i + 1;
}
print(sum); // 45

let flag: boolean = sum == 45 && i == 10;
print(flag); // true

if (flag) {
    print(1);
} else {
    print(0);
}

// Short-circuit must not evaluate the right side here.
let safe = false && (1 / 0 > 0);
print(safe); // false
print(true || false); // true
print(!false); // true
