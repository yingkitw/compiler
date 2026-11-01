// A main() entry point plus a helper function.
function double(x: number): number {
    return x * 2;
}

function main(): void {
    print(double(21));
    print(!(1 > 2));

    const pi: number = 3.14159;
    let r = 2;
    print(pi * r * r);
}
