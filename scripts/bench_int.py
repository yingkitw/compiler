#!/usr/bin/env python3
"""Measure the `int` type against `number` on the same workload.

Isolated from the full benchmark so the measurement is not distorted by the
other toolchains' builds. Run it when the machine is otherwise quiet.
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

NUMBER_FIB = """function fib(n: number): number {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
print(fib(35));
"""

INT_FIB = """function fib(n: int): int {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
print(fib(35));
"""

LOOP_NUMBER = """let sum = 0;
let i = 0;
while (i < 100000000) {
    sum = sum + i * i;
    i = i + 1;
}
print(sum);
"""

LOOP_INT = """let sum: int = 0;
let i: int = 0;
while (i < 100000000) {
    sum = sum + i * i;
    i = i + 1;
}
print(sum);
"""


def build(src: str, work: Path, name: str) -> Path:
    path = work / f"{name}.ts"
    path.write_text(src)
    out = work / name
    subprocess.run([str(TSC), str(path), "-o", str(out)], check=True, capture_output=True)
    return out


def time_run(binary: Path, reps: int) -> tuple[float, float]:
    times = []
    for _ in range(reps):
        start = time.perf_counter()
        subprocess.run([str(binary)], check=True, capture_output=True)
        times.append(time.perf_counter() - start)
    return min(times), statistics.median(times)


def main() -> int:
    if not TSC.exists():
        print("run `cargo build --release` first")
        return 1
    reps = int(sys.argv[1]) if len(sys.argv) > 1 else 15
    with tempfile.TemporaryDirectory(prefix="tsc-int-") as tmp:
        work = Path(tmp)
        cases = [
            ("fib (number)", NUMBER_FIB, "fibnum"),
            ("fib (int)", INT_FIB, "fibint"),
            ("loop (number)", LOOP_NUMBER, "loopnum"),
            ("loop (int)", LOOP_INT, "loopint"),
        ]
        built = [(label, build(src, work, name)) for label, src, name in cases]
        # Warm up the page cache for every binary first.
        for _, binary in built:
            subprocess.run([str(binary)], check=True, capture_output=True)

        results = {}
        for label, binary in built:
            best, median = time_run(binary, reps)
            results[label] = best
            print(f"{label:<16} best={best * 1000:8.2f}ms  median={median * 1000:8.2f}ms")

        print()
        for a, b in [("fib (number)", "fib (int)"), ("loop (number)", "loop (int)")]:
            speedup = results[a] / results[b]
            print(f"{a} -> {b}: {speedup:.2f}x faster")
    return 0


if __name__ == "__main__":
    sys.exit(main())
