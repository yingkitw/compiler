// `for (const x of arr)` — desugared to an indexed `while` in the parser.
let nums: number[] = [10.0, 20.0, 30.0];
let sum = 0.0;
for (const n of nums) {
    sum = sum + n;
}
print(sum); // 60

let flags: boolean[] = [true, false, true];
for (const f of flags) {
    print(f);
}
