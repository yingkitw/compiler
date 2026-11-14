//! The C runtime that the generated program is linked against.
//!
//! It is kept minimal so that the generated code only needs a few
//! well-defined (non-variadic) C ABI calls. The compiler writes it next to
//! the assembly and the system C compiler assembles and links both.

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
 * decimal exponents in [-6, 20] and exponential notation otherwise.
 * Printing "%g" instead would silently drop precision beyond six
 * significant digits. */
void ts_print_number(double x) {
    if (isnan(x)) { puts("NaN"); fflush(stdout); return; }
    if (isinf(x)) { puts(x < 0 ? "-Infinity" : "Infinity"); fflush(stdout); return; }
    if (x == 0.0) { puts("0"); fflush(stdout); return; }

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
    fflush(stdout);
}

void ts_print_bool(signed char x) {
    puts(x ? "true" : "false");
    fflush(stdout);
}

void ts_print_int(long long x) {
    char buf[32];
    char *end = buf + sizeof buf;
    char *p = end;
    *--p = '\n';
    int neg = x < 0;
    unsigned long long u;
    if (neg) {
        u = (unsigned long long)(-(x + 1)) + 1u;
    } else {
        u = (unsigned long long)x;
    }
    do {
        *--p = (char)('0' + (u % 10u));
        u /= 10u;
    } while (u != 0);
    if (neg) {
        *--p = '-';
    }
    ssize_t ignored = write(1, p, (size_t)(end - p));
    (void)ignored;
    fflush(stdout);
}

/* Explicit integer division by zero check for targets whose `sdiv` does not
 * trap. Matches the SIGFPE/SIGILL handler message. */
void ts_int_div_zero(void) {
    fflush(stdout);
    static const char msg[] = "error: integer division by zero or overflow\n";
    ssize_t ignored = write(2, msg, sizeof msg - 1);
    (void)ignored;
    _exit(1);
}

/* Remainder for doubles. The AArch64 ISA has no float remainder
 * instruction, so this calls libm. C's fmod truncates toward zero and takes
 * the sign of the dividend, which is exactly what JavaScript's % does. */
double ts_fmod(double x, double y) { return fmod(x, y); }

/* Convert a double to an int: truncate toward zero, with NaN becoming zero
 * and an out-of-range value saturating at the ends of the range -- the
 * semantics of Rust's `as`. The hardware's conversion traps instead, so the
 * conversion is done here. */
long long ts_double_to_int(double x) {
    if (isnan(x)) return 0;
    if (x >= 9223372036854775808.0) return 9223372036854775807LL;
    if (x <= -9223372036854775808.0) return (-9223372036854775807LL - 1);
    return (long long)x;
}

/* --- Strings (bump arena; literals live in .const) -------------------- */

typedef struct { char *data; unsigned long len; } ts_str;

#define TS_ARENA_BYTES (256 * 1024)
static char ts_arena[TS_ARENA_BYTES];
static size_t ts_arena_used;

static char *ts_bump(size_t n) {
    n = (n + 7) & ~((size_t)7);
    if (ts_arena_used + n > TS_ARENA_BYTES) {
        fflush(stdout);
        static const char msg[] = "error: string allocation limit exceeded\n";
        ssize_t ignored = write(2, msg, sizeof msg - 1);
        (void)ignored;
        _exit(1);
    }
    char *p = ts_arena + ts_arena_used;
    ts_arena_used += n;
    return p;
}

void ts_print_string(const char *data, unsigned long len) {
    ssize_t ignored = write(1, data, len);
    (void)ignored;
    ignored = write(1, "\n", 1);
    (void)ignored;
    fflush(stdout);
}

unsigned char ts_str_equal(const char *a, unsigned long alen,
                           const char *b, unsigned long blen) {
    if (alen != blen) return 0;
    return memcmp(a, b, alen) == 0 ? 1 : 0;
}

ts_str ts_str_concat(const char *a, unsigned long alen,
                     const char *b, unsigned long blen) {
    size_t n = (size_t)alen + (size_t)blen;
    char *p = ts_bump(n);
    memcpy(p, a, (size_t)alen);
    memcpy(p + (size_t)alen, b, (size_t)blen);
    return (ts_str){p, n};
}

/* --- Arrays (same bump arena) ----------------------------------------- */

typedef struct {
  double *data;
  unsigned long len;
  unsigned long cap;
} ts_number_array;

typedef struct {
  signed char *data;
  unsigned long len;
  unsigned long cap;
} ts_bool_array;

static void ts_array_bounds(void) {
    fflush(stdout);
    static const char msg[] = "error: array index out of bounds\n";
    ssize_t ignored = write(2, msg, sizeof msg - 1);
    (void)ignored;
    _exit(1);
}

ts_number_array *ts_number_array_new(unsigned long cap) {
    ts_number_array *a = (ts_number_array *)ts_bump(sizeof(ts_number_array));
    a->len = 0;
    a->cap = cap;
    a->data = cap ? (double *)ts_bump(cap * sizeof(double)) : NULL;
    return a;
}

static void ts_number_array_grow(ts_number_array *a) {
    unsigned long ncap = a->cap ? a->cap * 2 : 4;
    double *p = (double *)ts_bump(ncap * sizeof(double));
    if (a->len) memcpy(p, a->data, a->len * sizeof(double));
    a->data = p;
    a->cap = ncap;
}

void ts_number_array_push(ts_number_array *a, double v) {
    if (a->len >= a->cap) ts_number_array_grow(a);
    a->data[a->len++] = v;
}

double ts_number_array_get(ts_number_array *a, long long i) {
    if (i < 0 || (unsigned long)i >= a->len) ts_array_bounds();
    return a->data[(unsigned long)i];
}

unsigned long ts_array_len(ts_number_array *a) { return a->len; }

ts_bool_array *ts_bool_array_new(unsigned long cap) {
    ts_bool_array *a = (ts_bool_array *)ts_bump(sizeof(ts_bool_array));
    a->len = 0;
    a->cap = cap;
    a->data = cap ? (signed char *)ts_bump(cap) : NULL;
    return a;
}

static void ts_bool_array_grow(ts_bool_array *a) {
    unsigned long ncap = a->cap ? a->cap * 2 : 4;
    signed char *p = (signed char *)ts_bump(ncap);
    if (a->len) memcpy(p, a->data, a->len);
    a->data = p;
    a->cap = ncap;
}

void ts_bool_array_push(ts_bool_array *a, signed char v) {
    if (a->len >= a->cap) ts_bool_array_grow(a);
    a->data[a->len++] = v;
}

signed char ts_bool_array_get(ts_bool_array *a, long long i) {
    if (i < 0 || (unsigned long)i >= a->len) ts_array_bounds();
    return a->data[(unsigned long)i];
}

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

/* An integer division by zero: AArch64 `sdiv` does NOT trap (it returns
 * zero), so the generated code checks the divisor itself and calls
 * ts_int_div_zero. This SIGFPE handler covers platforms whose hardware does
 * trap, and the SIGILL registration below is vestigial — it dates from the
 * Cranelift backend, whose divide-by-zero check trapped with an illegal
 * instruction; the hand-written assembly emits none. Float division does
 * not trap anywhere — it produces Infinity, like JavaScript's — so this
 * only ever fires for the `int` type. */
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
    /* SIGILL is how the Cranelift backend's divide-by-zero check trapped.
     * The hand-written assembly emits no illegal instructions, so this
     * registration never fires today; it is kept because removing it buys
     * nothing and the handler is the same one. */
    sigaction(SIGILL, &sa, NULL);
}
"#;
