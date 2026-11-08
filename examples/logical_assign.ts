// Logical compound assignment: &&= and ||= are parser sugar for x = x && e
// and x = x || e. The right-hand side is skipped when the left-hand side
// already decides the result (short-circuit), like && and ||.
let a: boolean = false;
a &&= 1 / 0 > 0;
print(a); // false — RHS not evaluated

let b: boolean = true;
b &&= false;
print(b); // false

let c: boolean = true;
c ||= 1 / 0 > 0;
print(c); // true — RHS not evaluated

let d: boolean = false;
d ||= true;
print(d); // true

let e: boolean = false;
e ||= 1 / 0 > 0;
print(e); // true — RHS evaluated
