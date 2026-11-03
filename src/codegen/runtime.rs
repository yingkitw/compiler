//! The C runtime that the generated object is linked against.
//!
//! It is kept minimal so that the generated code only needs a few
//! well-defined (non-variadic) C ABI calls.

/// Tiny C runtime linked against the generated object. Kept minimal so the
/// generated code only needs a few well-defined (non-variadic) C ABI calls.
pub const RUNTIME_C: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <math.h>
#include <signal.h>
#include <unistd.h>

/* Print a double the way JavaScript's Number::toString does: the shortest
 * decimal string that round-trips back to the same f64, in fixed notation for
 * decimal exponents in [-6, 21] and exponential notation otherwise.
 * Printing "%g" instead would silently drop precision beyond six
 * significant digits. */
void ts_print_number(double x) {
    if (isnan(x)) { puts("NaN"); return; }
    if (isinf(x)) { puts(x < 0 ? "-Infinity" : "Infinity"); return; }
    if (x == 0.0) { puts("0"); return; }

    int neg = x < 0;
    double a = neg ? -x : x;

    /* Shortest round-tripping representation: try 1..17 significant digits. */
    char buf[64];
    int prec, exp10 = 0;
    for (prec = 1; prec <= 17; prec++) {
        snprintf(buf, sizeof buf, "%.*e", prec - 1, a);
        if (strtod(buf, NULL) == a) break;
    }

    /* Split into digits and exponent: value == 0.<digits> * 10^n */
    char digits[32];
    int k = 0, i = 0;
    while (buf[i] && buf[i] != 'e') {
        if (buf[i] != '.') digits[k++] = buf[i];
        i++;
    }
    if (buf[i] == 'e') exp10 = atoi(buf + i + 1);
    while (k > 1 && digits[k - 1] == '0') k--;
    digits[k] = '\0';
    int n = exp10 + 1;

    char out[80];
    char *p = out;
    if (neg) *p++ = '-';
    if (k <= n && n <= 21) {
        memcpy(p, digits, (size_t)k); p += k;
        for (int j = 0; j < n - k; j++) *p++ = '0';
    } else if (0 < n && n <= 21) {
        memcpy(p, digits, (size_t)n); p += n;
        *p++ = '.';
        memcpy(p, digits + n, (size_t)(k - n)); p += k - n;
    } else if (-6 < n && n <= 0) {
        *p++ = '0'; *p++ = '.';
        for (int j = 0; j < -n; j++) *p++ = '0';
        memcpy(p, digits, (size_t)k); p += k;
    } else {
        *p++ = digits[0];
        if (k > 1) {
            *p++ = '.';
            memcpy(p, digits + 1, (size_t)(k - 1)); p += k - 1;
        }
        *p++ = 'e';
        int e = n - 1;
        *p++ = e < 0 ? '-' : '+';
        if (e < 0) e = -e;
        /* snprintf, not sprintf: it cannot write past the buffer, and it
         * returns the length it would have written. */
        size_t left = sizeof(out) - (size_t)(p - out);
        int written = snprintf(p, left, "%d", e);
        if (written > 0) p += written;
    }
    *p = '\0';
    puts(out);
}

void ts_print_bool(signed char x) { puts(x ? "true" : "false"); }

void ts_print_int(long long x) { printf("%lld\n", x); }

/* Remainder for doubles. Cranelift has no float remainder instruction, so
 * this calls libm. C's fmod truncates toward zero and takes the sign of the
 * dividend, which is exactly what JavaScript's % does. */
double ts_fmod(double x, double y) { return fmod(x, y); }

/* --- Stack-overflow detection -------------------------------------------
 *
 * Recursion without a base case exhausts the thread stack, and the process
 * dies with SIGSEGV: the program is killed mid-way and whatever it had
 * printed is lost with it. The generated `main` calls ts_runtime_init()
 * before anything else; it installs a handler that turns the fault into a
 * clean diagnostic and a non-zero exit status.
 *
 * Two details make this work:
 *
 *   - The handler runs on a separate signal stack (sigaltstack), because
 *     the main stack is exhausted by definition when the fault arrives.
 *   - The handler flushes stdout before writing the message, so the
 *     program's own output survives the error.
 *
 * The language has no pointers, so the only way a compiled program can
 * fault is by running out of stack. Every fault is therefore reported as a
 * recursion overflow.
 */
#define TS_SIGSTACK_SIZE (256 * 1024)
static char ts_sigstack[TS_SIGSTACK_SIZE];

static void ts_on_sigsegv(int sig, siginfo_t *info, void *ctx) {
    (void)sig; (void)info; (void)ctx;
    /* stdout may still be holding buffered output. */
    fflush(stdout);
    static const char msg[] = "error: maximum recursion depth exceeded\n";
    ssize_t ignored = write(2, msg, sizeof msg - 1);
    (void)ignored;
    _exit(1);
}

/* An integer division by zero traps: the hardware raises SIGFPE on some
 * platforms, and Cranelift raises SIGILL with an explicit check on others.
 * Float division does not trap — it produces Infinity, like JavaScript's — so
 * this only ever fires for the `int` type. */
static void ts_on_sigfpe(int sig, siginfo_t *info, void *ctx) {
    (void)sig; (void)info; (void)ctx;
    fflush(stdout);
    static const char msg[] = "error: integer division by zero or overflow\n";
    ssize_t ignored = write(2, msg, sizeof msg - 1);
    (void)ignored;
    _exit(1);
}

void ts_runtime_init(void) {
    stack_t ss;
    ss.ss_sp = ts_sigstack;
    ss.ss_size = sizeof ts_sigstack;
    ss.ss_flags = 0;
    if (sigaltstack(&ss, NULL) != 0) return;

    struct sigaction sa;
    memset(&sa, 0, sizeof sa);
    sa.sa_flags = SA_SIGINFO | SA_ONSTACK | SA_RESETHAND;
    sa.sa_sigaction = ts_on_sigsegv;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGSEGV, &sa, NULL);

    memset(&sa, 0, sizeof sa);
    sa.sa_flags = SA_SIGINFO | SA_ONSTACK | SA_RESETHAND;
    sa.sa_sigaction = ts_on_sigfpe;
    sigemptyset(&sa.sa_mask);
    sigaction(SIGFPE, &sa, NULL);
    /* SIGILL is how Cranelift's own divide-by-zero check traps. A generated
     * program has no other illegal instruction, so the only trap it can take
     * is a division that cannot be performed. */
    sigaction(SIGILL, &sa, NULL);
}
"#;
