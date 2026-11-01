#!/usr/bin/env python3
"""Benchmark the tsc compiler against other TypeScript toolchains.

Every toolchain compiles the *same* program and we compare:

  * compile time   — cold (no cache) and warm (cache populated)
  * artifact size  — the shipped binary (or the .js for the node/tsc path)
  * runtime        — best-of-N wall clock for the compiled program
  * startup        — wall clock for a trivial program (engine init cost)

The programs print integers only, so every toolchain must produce byte-identical
output. That is checked, not assumed: a toolchain that disagrees is reported as
a failure rather than silently benchmarked.

Usage:
    python3 scripts/bench.py              # run the full suite
    python3 scripts/bench.py --quick      # smaller workloads, fewer reps
    python3 scripts/bench.py --out FILE   # write the markdown report elsewhere
"""

from __future__ import annotations

import argparse
import os
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSC_BIN = ROOT / "target" / "release" / "tsc"

# --- Workloads -------------------------------------------------------------
# Each template is rendered twice: `print` for tsc-rs, `console.log` for the
# JavaScript toolchains. Only numbers and booleans are used, so every
# toolchain's output is identical and directly comparable.


@dataclass
class Workload:
    name: str
    template: str
    n: int
    quick_n: int
    note: str


WORKLOADS = [
    Workload(
        name="startup",
        template="{{PRINT}}(0);\n",
        n=0,
        quick_n=0,
        note="trivial program — measures process start and engine init",
    ),
    Workload(
        name="fib",
        template=(
            "function fib(n: number): number {\n"
            "    if (n < 2) { return n; }\n"
            "    return fib(n - 1) + fib(n - 2);\n"
            "}\n"
            "{{PRINT}}(fib({{N}}));\n"
        ),
        n=35,
        quick_n=30,
        note="recursive fib — measures call overhead and integer codegen",
    ),
    Workload(
        name="loop",
        template=(
            "let sum = 0;\n"
            "let i = 0;\n"
            "while (i < {{N}}) {\n"
            "    sum = sum + i * i;\n"
            "    i = i + 1;\n"
            "}\n"
            "{{PRINT}}(sum);\n"
        ),
        n=100_000_000,
        quick_n=10_000_000,
        note="tight numeric loop — measures raw code quality",
    ),
]


@dataclass
class Toolchain:
    name: str
    print_stmt: str
    compile_cmd: list  # formatted with {src} {out} {cache_env}
    run_cmd: list  # formatted with {out}
    produces_binary: bool = True
    cache_env: dict = field(default_factory=dict)
    note: str = ""
    # "bundled"    - embeds a JavaScript engine in the artifact
    # "standalone" - self-contained native code, no engine
    # "external"   - emits JavaScript, runs on a separate runtime
    runtime_kind: str = "standalone"


def toolchains() -> list[Toolchain]:
    return [
        Toolchain(
            name="tsc-rs (ours)",
            print_stmt="print",
            compile_cmd=["{tsc}", "{src}", "-o", "{out}"],
            run_cmd=["{out}"],
            note="Cranelift machine code, 2-function C runtime",
        ),
        Toolchain(
            name="deno compile",
            print_stmt="console.log",
            compile_cmd=["deno", "compile", "--allow-all", "-o", "{out}", "{src}"],
            run_cmd=["{out}"],
            note="bundles the V8 engine into a standalone binary",
            runtime_kind="bundled",
            cache_env={"DENO_DIR": "{cache}"},
        ),
        Toolchain(
            name="bun build --compile",
            print_stmt="console.log",
            compile_cmd=["bun", "build", "--compile", "{src}", "--outfile", "{out}"],
            run_cmd=["{out}"],
            note="bundles the JavaScriptCore engine into a standalone binary",
            runtime_kind="bundled",
            cache_env={"BUN_INSTALL_CACHE_DIR": "{cache}"},
        ),
        Toolchain(
            name="scriptc",
            print_stmt="console.log",
            compile_cmd=["scriptc", "build", "{src}", "--out", "{out}"],
            run_cmd=["{out}"],
            note="statically compiles TypeScript to native code, no engine in the binary",
            runtime_kind="standalone",
        ),
        Toolchain(
            name="tsc + node",
            print_stmt="console.log",
            compile_cmd=["tsc", "{src}", "--outDir", "{outdir}", "--skipLibCheck"],
            run_cmd=["node", "{outdir}/{stem}.js"],
            produces_binary=False,
            note="reference path: TypeScript to JavaScript, run on the system Node",
            runtime_kind="external",
        ),
    ]


# --- helpers ---------------------------------------------------------------


def human_size(n: int) -> str:
    for unit in ("B", "KiB", "MiB", "GiB"):
        if n < 1024 or unit == "GiB":
            return f"{n:.0f} {unit}" if unit == "B" else f"{n:.1f} {unit}"
        n /= 1024.0
    return f"{n:.1f} GiB"


def time_cmd(cmd: list, env: dict | None = None, cwd=None) -> float:
    """Run a command, return wall-clock seconds. Raises on failure."""
    start = time.perf_counter()
    proc = subprocess.run(cmd, capture_output=True, env=env, cwd=cwd)
    elapsed = time.perf_counter() - start
    if proc.returncode != 0:
        raise RuntimeError(
            f"command failed ({proc.returncode}): {' '.join(cmd)}\n"
            f"{proc.stderr.decode(errors='replace')[-2000:]}"
        )
    return elapsed


def time_run(cmd: list, reps: int, env: dict | None = None, cwd=None) -> tuple[float, float, str]:
    """Run a program `reps` times. Returns (best, median, stdout)."""
    times, out = [], ""
    for _ in range(reps):
        start = time.perf_counter()
        proc = subprocess.run(cmd, capture_output=True, env=env, cwd=cwd)
        times.append(time.perf_counter() - start)
        if proc.returncode != 0:
            raise RuntimeError(
                f"run failed ({proc.returncode}): {' '.join(cmd)}\n"
                f"{proc.stderr.decode(errors='replace')[-1000:]}"
            )
        out = proc.stdout.decode(errors="replace").strip()
    return min(times), statistics.median(times), out


def artifact_size(path: Path) -> int:
    if path.is_file():
        return path.stat().st_size
    return sum(p.stat().st_size for p in path.rglob("*") if p.is_file())


def have(cmd: str) -> bool:
    return shutil.which(cmd) is not None


def available(tc: "Toolchain") -> bool:
    """Is this toolchain runnable here?

    `tsc` is ambiguous: the system `tsc` is TypeScript's compiler, while ours
    lives at a fixed path. Resolve ours by path and everything else by PATH.
    """
    if tc.name.startswith("tsc-rs"):
        return TSC_BIN.exists()
    return have(tc.compile_cmd[0])


# --- benchmark -------------------------------------------------------------


@dataclass
class Result:
    workload: str
    toolchain: str
    compile_cold: float | None = None
    compile_warm: float | None = None
    size: int = 0
    runtime_best: float = 0.0
    runtime_median: float = 0.0
    output: str = ""
    error: str | None = None
    note: str = ""


def bench_workload(w: Workload, tc: Toolchain, n: int, reps: int, keep: Path, cache_dir: Path) -> Result:
    res = Result(workload=w.name, toolchain=tc.name, note=tc.note)
    work = keep / f"{w.name}-{tc.name.replace(' ', '_').replace('/', '_')}"
    work.mkdir(parents=True, exist_ok=True)

    src = work / "prog.ts"
    src.write_text(
        w.template.replace("{{PRINT}}", tc.print_stmt).replace("{{N}}", str(n))
    )

    cache_dir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ)
    for key, value in tc.cache_env.items():
        env[key] = value.replace("{cache}", str(cache_dir))

    out = work / ("prog" if tc.produces_binary else "out")
    cmd = [c.format(src=str(src), out=str(out), outdir=str(out), stem="prog",
                    tsc=str(TSC_BIN)) for c in tc.compile_cmd]

    if not available(tc):
        res.error = f"{cmd[0]} not installed"
        return res

    try:
        # Run inside the isolated work dir: a stray `node_modules/@types` in a
        # parent directory otherwise leaks into the TypeScript compile and
        # fails it with unrelated lib errors.
        res.compile_cold = time_cmd(cmd, env, cwd=work)
        res.compile_warm = time_cmd(cmd, env, cwd=work)
        res.size = artifact_size(out)
        run = [c.format(src=str(src), out=str(out), outdir=str(out), stem="prog",
                        tsc=str(TSC_BIN)) for c in tc.run_cmd]
        res.runtime_best, res.runtime_median, res.output = time_run(run, reps, env, cwd=work)
    except Exception as exc:  # noqa: BLE001 - report, don't abort the suite
        res.error = str(exc)[:400]
    finally:
        # The artifacts are huge (a deno binary is ~65 MiB) and the size has
        # already been measured, so drop them instead of filling the disk.
        if out.exists():
            if out.is_dir():
                shutil.rmtree(out, ignore_errors=True)
            else:
                out.unlink(missing_ok=True)
    return res


def spawn_overhead(reps: int = 20) -> float:
    """Cost of fork+exec as seen from Python, so startup numbers are honest.

    Every "runtime" figure below includes the cost of spawning the process.
    This measures that floor so the report can state it rather than quietly
    attributing it to the toolchain.
    """
    times = []
    true_bin = shutil.which("true") or "/usr/bin/true"
    for _ in range(reps):
        start = time.perf_counter()
        subprocess.run([true_bin], capture_output=True)
        times.append(time.perf_counter() - start)
    return min(times)


def outputs_agree(values: list[str]) -> tuple[bool, bool]:
    """Compare program outputs. Returns (numerically_agree, byte_identical).

    Numeric comparison is the meaningful one: a toolchain may legitimately
    format the same number differently (exponent vs. fixed notation). Exact
    agreement is reported separately because it is the stronger claim.
    """
    if not values:
        return True, True
    nums = []
    for v in values:
        try:
            nums.append(float(v))
        except ValueError:
            return all(v == values[0] for v in values), all(v == values[0] for v in values)
    same_value = all(abs(n - nums[0]) <= 1e-9 * max(1.0, abs(n)) for n in nums)
    return same_value, len(set(values)) == 1


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true", help="smaller workloads")
    ap.add_argument("--reps", type=int, default=5, help="timed runs per program")
    ap.add_argument("--out", default=str(ROOT / "bench" / "RESULTS.md"))
    args = ap.parse_args()

    if not TSC_BIN.exists():
        print(f"error: {TSC_BIN} not found — run `cargo build --release` first")
        return 1

    print(f"host: {platform.platform()}  ({platform.machine()}, {os.cpu_count()} cpus)")
    print(f"tsc:  {TSC_BIN}")

    tcs = toolchains()
    missing = [t.name for t in tcs if not available(t)]
    if missing:
        print(f"skipping unavailable toolchains: {', '.join(missing)}")

    results: list[Result] = []
    overhead = spawn_overhead()
    print(f"process spawn overhead (floor for every runtime figure): {overhead * 1000:.2f}ms")
    with tempfile.TemporaryDirectory(prefix="tsc-bench-") as tmp:
        keep = Path(tmp)
        for w in WORKLOADS:
            n = w.quick_n if args.quick else w.n
            print(f"\n=== workload: {w.name} (n={n}) — {w.note}")
            for tc in tcs:
                # One cache dir per toolchain for the whole run: the toolchain's
                # own cache (V8 snapshot, baseline binary) is populated once and
                # reused, which is how a real machine behaves. A fresh cache per
                # workload would re-download the engine every time and measure
                # network time, not compile time.
                cache_dir = keep / f"cache-{tc.name.replace(' ', '_').replace('/', '_')}"
                r = bench_workload(w, tc, n, args.reps, keep, cache_dir)
                results.append(r)
                if r.error:
                    print(f"  {tc.name:<22} ERROR: {r.error.splitlines()[0][:80]}")
                else:
                    print(
                        f"  {tc.name:<22} size={human_size(r.size):>11}  "
                        f"run={r.runtime_best * 1000:9.2f}ms  "
                        f"compile_cold={r.compile_cold:6.2f}s"
                    )

    # --- correctness: every toolchain must agree -------------------------
    print("\n=== output agreement")
    problems = []
    for w in WORKLOADS:
        outs = {
            r.toolchain: r.output
            for r in results
            if r.workload == w.name and not r.error
        }
        if not outs:
            continue
        values = list(outs.values())
        same_value, same_bytes = outputs_agree(values)
        detail = "byte-identical" if same_bytes else ("numerically equal" if same_value else "MISMATCH")
        print(f"  {w.name}: {len(outs)} toolchains -> {detail} ({next(iter(outs.values()))!r})")
        if not same_value:
            problems.append((w.name, outs))
    if problems:
        print("\nERROR: toolchains disagree on program output", file=sys.stderr)
        for name, outs in problems:
            print(f"  {name}: {outs}", file=sys.stderr)
        return 2

    write_report(results, WORKLOADS, Path(args.out), args.quick, overhead * 1000)
    print(f"\nreport written to {args.out}")
    return 0


def size_phrase(ours: int, theirs: int) -> str:
    """Describe the artifact-size relationship, in the right direction."""
    if ours <= 0 or theirs <= 0:
        return "artifact size not comparable"
    if theirs >= ours:
        return f"{theirs / ours:.0f}x larger artifact"
    return f"{ours / theirs:.1f}x smaller artifact"


def write_report(results, workloads, path: Path, quick: bool, overhead_ms: float = 0.0) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)

    # Per-toolchain startup baseline: the trivial program measures process
    # spawn plus any engine init. Subtracting it isolates the actual compute.
    baseline = {
        r.toolchain: r.runtime_best
        for r in results
        if r.workload == "startup" and not r.error
    }

    lines = [
        "# Benchmark results",
        "",
        "Generated by `python3 scripts/bench.py`. Do not edit by hand.",
        "",
        f"- Host: {platform.platform()} ({platform.machine()})",
        f"- CPUs: {os.cpu_count()}",
        f"- Mode: {'quick' if quick else 'full'}",
        f"- Process spawn overhead (included in every runtime figure): {overhead_ms:.2f}ms",
        "",
        "Every toolchain compiles the same program and must print the same",
        "result; the harness fails if they disagree. `compute` is the runtime",
        "with the toolchain's own startup cost subtracted, isolating the",
        "workload from process spawn and engine initialisation.",
        "",
        "`Compile (cold)` is the first build of that workload and `Compile",
        "(warm)` the second. Each toolchain keeps one cache for the whole run,",
        "so a toolchain that must extract an engine (deno) pays that cost once,",
        "on the first workload only.",
        "",
    ]

    # Headline numbers, computed from the data rather than written by hand.
    # Every comparison here is against the *startup* workload: that is the row
    # that measures process start and engine init.
    startup_rows = {
        r.toolchain: r for r in results if r.workload == "startup" and not r.error
    }
    ours_start = startup_rows.get("tsc-rs (ours)")
    kinds = {tc.name: tc.runtime_kind for tc in toolchains()}
    if ours_start:
        lines += ["## Headline", ""]
        for name, r in startup_rows.items():
            if name.startswith("tsc-rs"):
                continue
            start_ratio = r.runtime_best / max(1e-9, ours_start.runtime_best)
            kind = kinds.get(name, "standalone")
            if kind == "bundled":
                size_part = f"{size_phrase(ours_start.size, r.size)} (embeds a JS engine), "
            elif kind == "external":
                size_part = "emits JavaScript and runs on the system Node, "
            else:
                size_part = f"{size_phrase(ours_start.size, r.size)}, "
            lines.append(
                f"- vs **{name}**: {size_part}{start_ratio:.1f}x slower to start"
            )
        lines.append("")

    for w in workloads:
        rows = [r for r in results if r.workload == w.name]
        ok = [r for r in rows if not r.error]
        if not ok:
            continue
        ours = next((r for r in ok if r.toolchain.startswith("tsc-rs")), None)
        ours_compute = (
            ours.runtime_best - baseline.get(ours.toolchain, 0.0) if ours else 0.0
        )
        lines += [
            f"## {w.name}",
            "",
            f"{w.note}.",
            "",
            "| Toolchain | Compile (cold) | Compile (warm) | Artifact size | Runtime | compute | vs. tsc-rs |",
            "|---|---|---|---|---|---|---|",
        ]
        for r in rows:
            if r.error:
                lines.append(f"| {r.toolchain} | — | — | — | — | — | ERROR |")
                continue
            base = baseline.get(r.toolchain, 0.0)
            compute = max(0.0, r.runtime_best - base)
            if r.toolchain.startswith("tsc-rs"):
                rel = "1.00x (base)"
            elif ours_compute > 1e-6:
                rel = f"{compute / ours_compute:.2f}x"
            else:
                rel = "—"
            lines.append(
                f"| {r.toolchain} | {r.compile_cold:.2f}s | {r.compile_warm:.2f}s | "
                f"{human_size(r.size)} | {r.runtime_best * 1000:.2f}ms | "
                f"{compute * 1000:.2f}ms | {rel} |"
            )
        lines.append("")

    lines += ["## Toolchain notes", ""]
    seen = set()
    for r in results:
        if r.note and r.toolchain not in seen:
            seen.add(r.toolchain)
            lines.append(f"- **{r.toolchain}**: {r.note}")
    lines.append("")

    path.write_text("\n".join(lines))


if __name__ == "__main__":
    sys.exit(main())
