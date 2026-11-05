#!/usr/bin/env python3
"""Isolate where the `int` type helps and where it does not.

Four shapes, each timed in interleaved rounds:

  loop-add      - arithmetic only, no calls: the int type should win.
  call-add      - a call per iteration, no recursion: pure call overhead.
  fib           - recursion: calls *and* arithmetic, and a value that must
                  survive across a call (so it needs a callee-saved register
                  or a stack slot).

The point is to attribute the difference: the `int` type changes how a value
is represented, and a value that must survive a call has to live somewhere
callee-saved.

The fib case deliberately uses the shape the recurrence rewrite cannot fire
on:

    let a = fib(n - 1);
    let b = fib(n - 2);
    return a + b;

The shape `return fib(n - 1) + fib(n - 2);` *is* rewritten, but only when the
parameter is an `int` -- so comparing that shape across `number` and `int`
would measure the rewrite, not the type. The `number` variant would stay
exponential while the `int` one became a loop. This script measures the type,
so both variants have to stay exponential.
"""

from __future__ import annotations

import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSC = ROOT / "target" / "release" / "tsc"

CASES = [
    (
        "loop-add (number)",
        """let sum = 0;
let i = 0;
while (i < 100000000) { sum = sum + i; i = i + 1; }
print(sum);
""",
        "loopnum",
    ),
    (
        "loop-add (int)",
        """let sum: int = 0;
let i: int = 0;
while (i < 100000000) { sum = sum + i; i = i + 1; }
print(sum);
""",
        "loopint",
    ),
    (
        "call-add (number)",
        """function f(n: number): number { return n + 1; }
let sum = 0;
let i = 0;
while (i < 20000000) { sum = sum + f(i); i = i + 1; }
print(sum);
""",
        "callnum",
    ),
    (
        "call-add (int)",
        """function f(n: int): int { return n + 1; }
let sum: int = 0;
let i: int = 0;
while (i < 20000000) { sum = sum + f(i); i = i + 1; }
print(sum);
""",
        "callint",
    ),
    (
        "fib (number)",
        """function fib(n: number): number {
    if (n < 2) { return n; }
    let a = fib(n - 1);
    let b = fib(n - 2);
    return a + b;
}
print(fib(35));
""",
        "fibnum",
    ),
    (
        "fib (int)",
        """function fib(n: int): int {
    if (n < 2) { return n; }
    let a = fib(n - 1);
    let b = fib(n - 2);
    return a + b;
}
print(fib(35));
""",
        "fibint",
    ),
]


def main() -> int:
    if not TSC.exists():
        print("run `cargo build --release` first")
        return 1
    reps = int(sys.argv[1]) if len(sys.argv) > 1 else 11
    with tempfile.TemporaryDirectory(prefix="tsc-int-") as tmp:
        work = Path(tmp)
        built = []
        for label, src, name in CASES:
            path = work / f"{name}.ts"
            path.write_text(src)
            out = work / name
            subprocess.run([str(TSC), str(path), "-o", str(out)], check=True, capture_output=True)
            built.append((label, out))
            subprocess.run([str(out)], check=True, capture_output=True)  # warm

        times: dict[str, list[float]] = {label: [] for label, _ in built}
        for _ in range(reps):
            for label, binary in built:
                start = time.perf_counter()
                subprocess.run([str(binary)], check=True, capture_output=True)
                times[label].append(time.perf_counter() - start)

        best = {label: min(t) for label, t in times.items()}
        for label, _ in built:
            print(
                f"{label:<20} best={best[label] * 1000:8.2f}ms  "
                f"median={statistics.median(times[label]) * 1000:8.2f}ms"
            )

        print()
        for a, b in [
            ("loop-add (number)", "loop-add (int)"),
            ("call-add (number)", "call-add (int)"),
            ("fib (number)", "fib (int)"),
        ]:
            print(f"{a:<18} -> {b:<18} {best[a] / best[b]:.2f}x faster")
    return 0


if __name__ == "__main__":
    sys.exit(main())
