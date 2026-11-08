// Block expressions: `{ stmt*; expr }` introduces a scope; the value is the
// final expression statement.
const x = {
  let y = 1;
  let z = 2;
  y + z;
};
print(x);

let outer = 10;
const twice = {
  let n = outer;
  n + n;
};
print(twice);

print({
  let a: int = 3;
  a * a;
});
