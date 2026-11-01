// Recursion without a base case exhausts the stack. The compiler's runtime
// turns the resulting fault into a clean error:
//
//     error: maximum recursion depth exceeded
//
// The program exits with status 1 instead of dying from SIGSEGV, and the
// output printed before the overflow is preserved.
function recurse(n: number): number {
    return recurse(n + 1);
}

print(42);
print(recurse(0));
