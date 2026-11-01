#!/usr/bin/env python3
"""Measure the per-call cost of recursion, separately for each compiler.

A single timing of a recursive program is meaningless: process startup and the
print dominate the recursion itself. (100,000 calls of an empty chain are
~0.2 ms of actual work inside a ~37 ms process.)

So measure each program at two depths and take the **slope**: the extra time
for the extra calls. Startup cancels exactly.

The shapes:

  chain     - `f(n) = f(n-1)`: one call per level.
  binary    - `f(n) = f(n-1) + f(n-2)`: two calls per level.

Comparing the two shapes shows whether a difference is about the call
sequence itself or about carrying values across calls.
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

# Depths chosen to stay inside the 8 MiB main-thread stack: roughly 48 bytes
# per frame, so 80,000 levels is about 3.8 MiB.
DEPTHS = [10_000, 20_000, 40_000, 80_000]

SHAPES = {
    "chain": """function f(n: number): number {
    if (n < 1) { return 0; }
    return f(n - 1);
}
print(f({DEPTH}));
""",
    "binary": """function f(n: number): number {
    if (n < 2) { return 1; }
    return f(n - 1) + f(n - 2);
}
print(f({DEPTH}));
""",
}

# The binary shape's depth is capped by its own call count (2^depth), so it
# uses a smaller depth ladder.
BINARY_DEPTHS = [10, 15, 20, 25]


def ours_program(shape: str, depth: int) -> str:
    return SHAPES[shape].replace("{DEPTH}", str(depth))


def scriptc_program(shape: str, depth: int) -> str:
    return SHAPES[shape].replace("{DEPTH}", str(depth)).replace(
        "print(", "console.log("
    )


def build_and_time(compiler: str, shape: str, depth: int, work: Path, reps: int) -> float:
    src = work / f"{compiler}-{shape}-{depth}.ts"
    out = work / f"{compiler}-{shape}-{depth}"
    if compiler == "ours":
        src.write_text(ours_program(shape, depth))
        subprocess.run([str(TSC), str(src), "-o", str(out)], check=True, capture_output=True)
    else:
        src.write_text(scriptc_program(shape, depth))
        subprocess.run(
            ["scriptc", "build", str(src), "--out", str(out)], check=True, capture_output=True
        )
    subprocess.run([str(out)], check=True, capture_output=True)  # warm
    times = []
    for _ in range(reps):
        start = time.perf_counter()
        subprocess.run([str(out)], check=True, capture_output=True)
        times.append(time.perf_counter() - start)
    return min(times)


def slope(compiler: str, shape: str, depths: list[int], work: Path, reps: int) -> float:
    """Nanoseconds per call, from the difference between two depths."""
    points = []
    for d in depths:
        t = build_and_time(compiler, shape, d, work, reps)
        calls = d if shape == "chain" else 2**d
        points.append((calls, t))
        print(f"    depth {d:<7} calls {calls:<12} {t * 1000:8.2f}ms")
    (c0, t0), (c1, t1) = points[0], points[-1]
    return (t1 - t0) / (c1 - c0) * 1e9  # ns per call


def main() -> int:
    reps = int(sys.argv[1]) if len(sys.argv) > 1 else 7
    with tempfile.TemporaryDirectory(prefix="tsc-percall-") as tmp:
        work = Path(tmp)
        print("per-call cost of recursion (ns), startup cancelled by differencing")
        results = {}
        for shape, depths in [("chain", DEPTHS), ("binary", BINARY_DEPTHS)]:
            print(f"\n  {shape}:")
            for compiler in ("ours", "scriptc"):
                ns = slope(compiler, shape, depths, work, reps)
                results[(compiler, shape)] = ns
                print(f"    -> {compiler:<8} {ns:7.2f} ns/call")
        print()
        for shape in ("chain", "binary"):
            ours = results[("ours", shape)]
            theirs = results[("scriptc", shape)]
            print(f"{shape:<8} ours {ours:7.2f} ns/call   scriptc {theirs:7.2f} ns/call"
                  f"   -> ours is {theirs / ours:.2f}x")
    return 0


if __name__ == "__main__":
    sys.exit(main())
