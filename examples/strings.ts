// String literals, concatenation, and comparison.
let hello: string = "hello";
let world: string = "world";
print(hello);
print(hello + " " + world);
print(hello == "hello");
print(hello == world);
print(hello + world == "helloworld");

function greet(name: string): string {
    return "hi, " + name;
}
print(greet("tsc"));
