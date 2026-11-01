#!/usr/bin/env python3
"""Per-call cost of recursion, for each compiler and each code shape.

The recursion gap to scriptc is the top open question in TODO.md, and the
question a recursive benchmark is decided by is two questions: *how many
times is the function entered* and *what does one entry cost*. This
script answers the second one, for several shapes of the same program:

  chain       f(n) = f(n-1)                 one call per level, no arithmetic
  binary      f(n) = f(n-1) + f(n-2)        the fib shape
  binary_int  the same with `int`           no float arithmetic, no spills
  hoisted     the arguments computed first   scriptc's shape, by hand

Three disciplines, all learned the hard way (see MEMORY.md):

  - **Interleaved rounds.** Every variant runs once per round, so a load
    spike during one variant's window cannot be blamed on that variant.
    Measuring one variant all the way through, then the next, produced a
    phantom 9% "gain" here that vanished under interleaving.
  - **CPU time, not wall clock.** `RUSAGE_CHILDREN` reports what the CPU
    actually did. On a loaded machine the wall clock measures the
    scheduler, and `time` reported 0.01s of user CPU for a run that took
    0.87s.
  - **The startup cost is cancelled by differencing.** Two depths are
    measured and the slope between them gives the per-call cost, so the
    fixed process startup drops out exactly.

    python3 scripts/bench_shape.py [rounds]
"""

from __future__ import annotations

import resource
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSC = ROOT / "target" / "release" / "tsc"

SHAPES = {
    "chain": """function f(n: number): number {
    if (n < 1) { return 0; }
    return f(n - 1);
}
print(f({N}));
""",
    "binary": """function f(n: number): number {
    if (n < 2) { return n; }
    return f(n - 1) + f(n - 2);
}
print(f({N}));
""",
    "binary_int": """function f(n: int): int {
    if (n < 2) { return n; }
    return f(n - 1) + f(n - 2);
}
print(f({N}));
""",
    # The same recursion with both arguments computed before either call.
    "hoisted": """function f(n: number): number {
    if (n < 2) { return n; }
    let a = n - 1;
    let b = n - 2;
    let x = f(a);
    let y = f(b);
    return x + y;
}
print(f({N}));
""",
}

# Depths chosen to stay inside the 8 MiB main-thread stack and to make the
# startup cost small next to the work.
DEPTHS = {
    "chain": [50_000, 100_000, 150_000],
    "binary": [32, 34, 35],
    "binary_int": [32, 34, 35],
    "hoisted": [32, 34, 35],
}

# scriptc is a TypeScript compiler: it has no `int` type, so that shape has
# no scriptc side.
NO_SCRIPTC = {"binary_int"}


def calls_of(shape: str, n: int) -> int:
    """How many times the function is entered for this shape and depth."""
    if shape == "chain":
        return n
    a, b = 0, 1
    for _ in range(n + 1):
        a, b = b, a + b
    return 2 * a - 1


def build(work: Path, shape: str, n: int, compiler: str) -> Path:
    src = work / f"{compiler}_{shape}_{n}.ts"
    text = SHAPES[shape].replace("{N}", str(n))
    if compiler == "scriptc":
        text = text.replace("print(", "console.log(")
    src.write_text(text)
    binary = work / f"{compiler}_{shape}_{n}"
    if compiler == "scriptc":
        subprocess.run(
            ["scriptc", "build", str(src), "--out", str(binary)],
            check=True,
            capture_output=True,
        )
    else:
        subprocess.run([str(TSC), str(src), "-o", str(binary)], check=True)
    return binary


def cpu_ms(binary: Path) -> float:
    """The child's user+system CPU time for one run, in milliseconds."""
    before = resource.getrusage(resource.RUSAGE_CHILDREN)
    subprocess.run([str(binary)], capture_output=True, check=True)
    after = resource.getrusage(resource.RUSAGE_CHILDREN)
    return (
        after.ru_utime - before.ru_utime + after.ru_stime - before.ru_stime
    ) * 1000


def main() -> int:
    rounds = int(sys.argv[1]) if len(sys.argv) > 1 else 9
    with tempfile.TemporaryDirectory(prefix="tsc-shape-") as tmp:
        work = Path(tmp)
        for shape, depths in DEPTHS.items():
            # Build every artifact first: the timing must not include a
            # compile, and every variant must see the same conditions.
            binaries: dict[tuple[str, int], Path] = {}
            for compiler in ("ours", "scriptc"):
                if compiler == "scriptc" and shape in NO_SCRIPTC:
                    continue
                for n in depths:
                    binaries[(compiler, n)] = build(work, shape, n, compiler)

            best = {k: float("inf") for k in binaries}
            for _ in range(rounds):
                for key, binary in binaries.items():
                    best[key] = min(best[key], cpu_ms(binary))

            print(f"\n{shape}:")
            print(
                f"  {'n':>9} {'entries':>13} | {'ours ms':>9} | {'scriptc ms':>11}"
            )
            for n in depths:
                scriptc_ms = (
                    f"{best[('scriptc', n)]:>11.1f}"
                    if ("scriptc", n) in best
                    else f"{'-':>11}"
                )
                print(
                    f"  {n:>9} {calls_of(shape, n):>13} | "
                    f"{best[('ours', n)]:>9.1f} | {scriptc_ms}"
                )

            # The slope between the two deepest levels cancels the fixed
            # startup cost exactly.
            n0, n1 = depths[-2], depths[-1]
            dcalls = calls_of(shape, n1) - calls_of(shape, n0)
            slopes = {}
            for compiler in ("ours", "scriptc"):
                if (compiler, n1) not in best:
                    continue
                slopes[compiler] = (
                    best[(compiler, n1)] - best[(compiler, n0)]
                ) * 1e6 / dcalls
            print("  -- per entry, startup cancelled by differencing --")
            for compiler, slope in slopes.items():
                if slope <= 0:
                    # The compiler turned the recursion into a loop (or
                    # otherwise made it free), so there is no per-entry cost
                    # left to measure.
                    print(
                        f"  {compiler:>8}: no per-entry cost — the shape was "
                        "optimised away"
                    )
                    continue
                print(f"  {compiler:>8}: {slope:6.2f} ns/entry")
            if len(slopes) == 2 and all(s > 0 for s in slopes.values()):
                print(
                    f"  {'ratio':>8}: ours is "
                    f"{slopes['ours'] / slopes['scriptc']:.2f}x scriptc"
                )
    return 0


if __name__ == "__main__":
    sys.exit(main())
