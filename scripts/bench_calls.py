#!/usr/bin/env python3
"""Attribute a recursive workload's cost to *entries* or to *per-entry cost*.

Two numbers decide who wins a recursive benchmark:

  * **entries**   - how many times the recursive function is entered
  * **ns/entry**  - what one entry costs

A compiler can be ahead on one and behind on the other, and the two cancel
out.  Guessing from disassembly gets this wrong, so this script measures
both.

Entries are counted exactly: the program is compiled with a `print` at the
*top* of the recursive function, so every entry prints one line and the
number of output lines is the number of entries.  `--self-test` asserts
that the count matches the analytic count for this compiler, which is what
makes the counting method trustworthy; the analytic count is then what the
timing is divided by.

Timing uses *differencing*: the cost per entry is taken from the change in
wall clock between two depths, divided by the change in entries.  That
cancels the process-start cost exactly, which matters because at these
depths the process start is a large part of the measurement.

Limitation, stated plainly: instrumentation can change what a compiler
generates.  A compiler that rewrites recursion may compile the *counting*
build differently from the clean one, in which case the counted entries do
not describe the clean build - and the implied ns/entry comes out too
small, which is the signal that a rewrite happened.

Usage:
    python3 scripts/bench_calls.py              # default shapes and depths
    python3 scripts/bench_calls.py --reps 7
    python3 scripts/bench_calls.py --self-test  # check the counting method
"""

from __future__ import annotations

import argparse
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSC = ROOT / "target" / "release" / "tsc"

COMPILERS = ("ours", "scriptc")


def fib(n: int) -> int:
    """F(1) = F(2) = 1."""
    a, b = 1, 1
    for _ in range(n - 1):
        a, b = b, a + b
    return a


# --- the shapes -------------------------------------------------------------
# `body` is the part of the function after the base case; `base` is the
# condition under which the function stops recursing.

SHAPES = {
    "binary": {
        "base": "if (n < 2) { return 1; }",
        "body": "return f(n - 1) + f(n - 2);",
        # A full binary tree of depth d: leaves = F(d+1), internal nodes =
        # leaves-1, so entries = 2*F(d+1)-1.  (F is the Fibonacci sequence.)
        "analytic": lambda d: 2 * fib(d + 1) - 1,
        # The recursion is exponential, so the depths stay small - but they
        # have to be big enough that the work dominates the process start,
        # which is ~22ms on this host.  At depth 34 the work is ~30ms and at
        # depth 38 it is ~200ms, so the noise stops dominating.
        "depths": [34, 36, 38],
        # The counting build prints one line per entry, so it runs at a
        # shallower depth than the timing builds.
        "count_depth": 24,
    },
}


# --- building ---------------------------------------------------------------


def source(shape: str, depth: int, counting: bool, js: bool) -> str:
    spec = SHAPES[shape]
    print_stmt = "console.log" if js else "print"
    # The counter goes *above* the base case: every entry must print, not
    # just the ones that recurse.
    counter = f"    {print_stmt}(0);\n" if counting else ""
    return (
        "function f(n: number): number {\n"
        f"{counter}"
        f"    {spec['base']}\n"
        f"    {spec['body']}\n"
        "}\n"
        f"{print_stmt}(f({depth}));\n"
    )


def build(compiler: str, shape: str, depth: int, counting: bool, work: Path) -> Path:
    """Compile one program; return the path of the binary."""
    js = compiler != "ours"
    tag = f"{compiler}-{shape}-{depth}" + ("-count" if counting else "")
    src = work / f"{tag}.ts"
    out = work / tag
    src.write_text(source(shape, depth, counting, js))
    if compiler == "ours":
        subprocess.run(
            [str(TSC), str(src), "-o", str(out)], check=True, capture_output=True
        )
    else:
        subprocess.run(
            ["scriptc", "build", str(src), "--out", str(out)],
            check=True,
            capture_output=True,
        )
    return out


def count_entries(compiler: str, shape: str, depth: int, work: Path) -> int:
    """Count function entries exactly, by counting the printed lines."""
    exe = build(compiler, shape, depth, counting=True, work=work)
    proc = subprocess.run([str(exe)], capture_output=True)
    if proc.returncode != 0:
        raise RuntimeError(
            f"{compiler} counting build failed ({proc.returncode}): "
            f"{proc.stderr.decode(errors='replace')[:400]}"
        )
    # The last line is the final print of the result, not an entry.
    return max(0, len(proc.stdout.decode(errors="replace").splitlines()) - 1)


def measure_all(exes: list[Path], reps: int) -> list[float]:
    """Time every program in interleaved rounds; return the best time each.

    Interleaving matters: measuring all repetitions of one program and then
    moving to the next lets a load spike on this shared machine skew exactly
    one of them.
    """
    times: list[list[float]] = [[] for _ in exes]
    for exe in exes:  # warm the page cache
        subprocess.run([str(exe)], capture_output=True)
    for _ in range(reps):
        for i, exe in enumerate(exes):
            start = time.perf_counter()
            subprocess.run([str(exe)], capture_output=True)
            times[i].append(time.perf_counter() - start)
    return [min(t) for t in times]


# --- reporting --------------------------------------------------------------


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--reps", type=int, default=5, help="timed runs per program")
    ap.add_argument(
        "--self-test",
        action="store_true",
        help="only check that the counted entries match the analytic count",
    )
    args = ap.parse_args()

    if not TSC.exists():
        print(f"error: {TSC} not found - run `cargo build --release` first")
        return 1

    failures = 0
    with tempfile.TemporaryDirectory(prefix="tsc-calls-") as tmp:
        work = Path(tmp)
        for shape, spec in SHAPES.items():
            depths = spec["depths"]
            count_depth = spec["count_depth"]
            print(f"\n=== shape: {shape}   {spec['body']}")

            # --- entries, counted exactly -----------------------------------
            analytic = spec["analytic"](count_depth)
            print(f"\n  entries at depth {count_depth} (counting build)")
            print(f"  {'compiler':>9}  {'entries':>12}  {'analytic':>12}  {'ent/anal':>9}")
            for compiler in COMPILERS:
                counted = count_entries(compiler, shape, count_depth, work)
                if compiler == "ours" and counted != analytic:
                    failures += 1
                    print(
                        f"  SELF-TEST FAILURE: counted {counted} entries, "
                        f"analytic count is {analytic}"
                    )
                print(
                    f"  {compiler:>9}  {counted:>12}  {analytic:>12}"
                    f"  {counted / analytic:>8.3f}"
                )

            # --- timing ------------------------------------------------------
            exes: list[Path] = []
            for compiler in COMPILERS:
                for depth in depths:
                    exes.append(build(compiler, shape, depth, False, work))
            times = measure_all(exes, args.reps)

            print(f"\n  clean build, best of {args.reps}")
            print(f"  {'compiler':>9}  {'depth':>8}  {'entries':>12}  {'time':>11}")
            for i, compiler in enumerate(COMPILERS):
                for j, depth in enumerate(depths):
                    print(
                        f"  {compiler:>9}  {depth:>8}"
                        f"  {spec['analytic'](depth):>12}"
                        f"  {times[i * len(depths) + j] * 1000:>10.2f}ms"
                    )

            # Per-entry cost by differencing two depths: the process start
            # cancels exactly, so no absolute baseline is needed.
            #
            # The divisor is the *naive* entry count.  For a compiler that
            # compiles the recursion as written, that is the true number of
            # entries.  For one that rewrote the recursion it is the amount
            # of logical work the program represents, which is what makes
            # the number comparable between compilers.
            print(f"\n  {'compiler':>9}  {'from':>8}  {'to':>8}  {'entries delta':>14}"
                  f"  {'time delta':>11}  {'ns/entry':>9}")
            for i, compiler in enumerate(COMPILERS):
                lo, hi = depths[0], depths[-1]
                d_entries = spec["analytic"](hi) - spec["analytic"](lo)
                dt = max(
                    0.0,
                    times[i * len(depths) + len(depths) - 1]
                    - times[i * len(depths)],
                )
                ns = dt / d_entries * 1e9 if d_entries else float("nan")
                print(
                    f"  {compiler:>9}  {lo:>8}  {hi:>8}  {d_entries:>14}"
                    f"  {dt * 1000:>10.2f}ms  {ns:>8.2f}"
                )

            # The competitive number: the clean build's wall clock, same
            # workload, side by side.
            print(f"\n  {'depth':>8}  {'ours':>12}  {'scriptc':>12}  {'ours/scriptc':>13}")
            for j, depth in enumerate(depths):
                ours = times[j]
                theirs = times[len(depths) + j]
                print(
                    f"  {depth:>8}  {ours * 1000:>10.2f}ms  {theirs * 1000:>10.2f}ms"
                    f"  {ours / theirs:>12.2f}x"
                )

    if args.self_test:
        if failures:
            print(f"\nself-test FAILED ({failures} mismatches)")
            return 1
        print("\nself-test passed: counted entries match the analytic count")
        return 0

    print(
        "\n  entries   - how many times the function is entered\n"
        "  ns/entry  - what one entry costs, from differencing two depths\n"
        "  A compiler can win on one and lose on the other; both are\n"
        "  needed to explain a gap.  ent/anal below 1.0 means the counting\n"
        "  build itself was rewritten, so the counted entries do not\n"
        "  describe the clean build and the implied ns/entry is too small."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
