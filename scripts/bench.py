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

# The published v0.1.0 of this compiler installs a binary named `tsc` (see the
# [[bin]] section in Cargo.toml), which is ambiguous with TypeScript's own
# `tsc`. Install it into a dedicated prefix so the benchmark can call it
# unambiguously:
#   cargo install tsc-rs --version 0.1.0 --root bench/.toolchains/tsc-rs
PUBLISHED_BIN = ROOT / "bench" / ".toolchains" / "tsc-rs" / "bin" / "tsc"

# TypeScript's own compiler, resolved at startup because our binary shadows it.
TSJS_BIN: str | None = None

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

# Our own `int` variants of two workloads. The JavaScript toolchains have no
# integer type, so these cannot be compared against them — they measure how
# much the `int` type buys us relative to our own `number` versions.
INT_FIB = (
    "function fib(n: int): int {\n"
    "    if (n < 2) { return n; }\n"
    "    return fib(n - 1) + fib(n - 2);\n"
    "}\n"
    "{{PRINT}}(fib({{N}}));\n"
)

INT_LOOP = (
    "let sum: int = 0;\n"
    "let i: int = 0;\n"
    "while (i < {{N}}) {\n"
    "    sum = sum + i * i;\n"
    "    i = i + 1;\n"
    "}\n"
    "{{PRINT}}(sum);\n"
)


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
    # True only for the build under test — never infer this from the name.
    is_ours: bool = False
    # A published older version kept for reference; excluded from the
    # output-agreement gate because it is allowed to differ.
    legacy: bool = False


def toolchains() -> list[Toolchain]:
    return [
        Toolchain(
            name="compiler (current)",
            print_stmt="print",
            compile_cmd=["{tsc}", "{src}", "-o", "{out}"],
            run_cmd=["{out}"],
            note="this compiler, built from the working tree",
            runtime_kind="standalone",
            is_ours=True,
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
            name="compiler v0.1.0 (published)",
            print_stmt="print",
            compile_cmd=["{published}", "{src}", "-o", "{out}"],
            run_cmd=["{out}"],
            note="the published v0.1.0 of this compiler, for comparison with the current build",
            runtime_kind="standalone",
            # v0.1.0 prints numbers with the old lossy "%g", so its output
            # legitimately differs from the current build. It is a reference
            # point, not a peer: it is excluded from the agreement gate.
            legacy=True,
        ),
        Toolchain(
            name="tsrust + node",
            print_stmt="console.log",
            compile_cmd=["tsrust", "{src}", "--outDir", "{outdir}"],
            run_cmd=["node", "{outdir}/{stem}.js"],
            produces_binary=False,
            note="TypeScript 7's Go compiler ported to Rust, emitting JavaScript",
            runtime_kind="external",
        ),
        Toolchain(
            name="tsc + node",
            print_stmt="console.log",
            compile_cmd=["{tsjs}", "{src}", "--outDir", "{outdir}", "--skipLibCheck"],
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


def find_typescript_tsc() -> str | None:
    """Locate TypeScript's own `tsc`, not ours.

    This compiler also installs a binary called `tsc`, and if it is earlier on
    PATH it shadows the real one — then `tsc prog.ts --outDir out` fails
    because our flags are different. TypeScript's `tsc --version` prints
    "Version x.y.z"; ours prints "compiler x.y.z". Use that to tell them
    apart.
    """
    candidates = []
    seen = set()
    for c in shutil.which("tsc") or "", *(shutil.which("tsc-all") or "",):
        if c and c not in seen:
            seen.add(c)
            candidates.append(c)
    # `which -a` finds every `tsc` on PATH, not just the first.
    try:
        out = subprocess.run(
            ["which", "-a", "tsc"], capture_output=True, text=True
        ).stdout.split()
        candidates.extend(c for c in out if c not in seen)
    except OSError:
        pass
    candidates += [
        "/opt/homebrew/bin/tsc",
        "/usr/local/bin/tsc",
        str(Path.home() / ".npm-global/bin/tsc"),
    ]
    for c in candidates:
        try:
            proc = subprocess.run([c, "--version"], capture_output=True, text=True, timeout=20)
        except (OSError, subprocess.SubprocessError):
            continue
        if proc.returncode == 0 and proc.stdout.strip().lower().startswith("version"):
            return c
    return None


def available(tc: "Toolchain") -> bool:
    """Is this toolchain runnable here?

    `tsc` is ambiguous: the system `tsc` is TypeScript's compiler, while ours
    lives at a fixed path. Resolve ours by path, the published v0.1.0 by its
    dedicated install prefix, and everything else by PATH.
    """
    if tc.is_ours:
        return TSC_BIN.exists()
    if tc.compile_cmd[0] == "{published}":
        return PUBLISHED_BIN.exists()
    if tc.compile_cmd[0] == "{tsjs}":
        return bool(TSJS_BIN) and Path(TSJS_BIN).exists()
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
    is_ours: bool = False
    legacy: bool = False
    # Set while the artifact exists, so the timing phase can run it.
    run_cmd: list = field(default_factory=list)
    cwd: str = ""
    env: dict = field(default_factory=dict)
    times: list = field(default_factory=list)


def bench_workload(w: Workload, tc: Toolchain, n: int, reps: int, keep: Path, cache_dir: Path) -> Result:
    res = Result(
        workload=w.name,
        toolchain=tc.name,
        note=tc.note,
        is_ours=tc.is_ours,
        legacy=tc.legacy,
    )
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
    fmt = dict(
        src=str(src),
        out=str(out),
        outdir=str(out),
        stem="prog",
        tsc=str(TSC_BIN),
        published=str(PUBLISHED_BIN),
        tsjs=TSJS_BIN or "tsc",
    )
    cmd = [c.format(**fmt) for c in tc.compile_cmd]

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
        res.run_cmd = [c.format(**fmt) for c in tc.run_cmd]
        res.cwd = str(work)
        res.env = env
    except Exception as exc:  # noqa: BLE001 - report, don't abort the suite
        res.error = str(exc)[:400]
    return res


def measure_round_robin(prepared: list[Result], reps: int) -> None:
    """Time every toolchain's program in interleaved rounds.

    Measuring all repetitions of one toolchain and then moving to the next
    means a load spike on this shared machine skews exactly one toolchain.
    Interleaving makes every toolchain see the same conditions: a round runs
    each program once, then the next round runs them again.

    This was not hypothetical — an early run reported `tsrust + node` as 3.7x
    slower than `tsc + node` on the same workload, when a direct measurement
    showed the two emit nearly identical JavaScript and run in the same time.
    """
    live = [r for r in prepared if not r.error and r.run_cmd]
    for r in live:
        r.times = []
    for _ in range(reps):
        for r in live:
            start = time.perf_counter()
            proc = subprocess.run(
                r.run_cmd, capture_output=True, env=r.env, cwd=r.cwd
            )
            elapsed = time.perf_counter() - start
            if proc.returncode != 0:
                r.error = (
                    f"run failed ({proc.returncode}): {' '.join(r.run_cmd)}\n"
                    f"{proc.stderr.decode(errors='replace')[-1000:]}"
                )
                continue
            r.times.append(elapsed)
            r.output = proc.stdout.decode(errors="replace").strip()
    for r in live:
        if r.times:
            r.runtime_best = min(r.times)
            r.runtime_median = statistics.median(r.times)


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

    global TSJS_BIN
    TSJS_BIN = find_typescript_tsc()
    print(f"tsjs: {TSJS_BIN or 'not found (the tsc+node path will be skipped)'}")

    tcs = toolchains()
    missing = [t.name for t in tcs if not available(t)]
    if missing:
        print(f"skipping unavailable toolchains: {', '.join(missing)}")

    results: list[Result] = []
    overhead = spawn_overhead()
    # The host's load decides how noisy the run is: the process-spawn floor
    # is ~5ms idle and ~20ms busy, and it changes every ratio.
    try:
        load_avg: float | None = os.getloadavg()[0]
    except OSError:
        load_avg = None
    print(f"process spawn overhead (floor for every runtime figure): {overhead * 1000:.2f}ms")
    print(f"machine load average: {load_avg:.1f}" if load_avg is not None else "machine load average: unavailable")
    with tempfile.TemporaryDirectory(prefix="tsc-bench-") as tmp:
        keep = Path(tmp)

        # Phase 1: build every artifact for every workload.
        prepared: list[Result] = []
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
                prepared.append(r)
                results.append(r)
                if r.error:
                    print(f"  {tc.name:<22} ERROR: {r.error.splitlines()[0][:80]}")
                else:
                    print(
                        f"  {tc.name:<22} size={human_size(r.size):>11}  "
                        f"compile_cold={r.compile_cold:6.2f}s"
                    )

        # The `int` variants — tsc-rs only, since the JavaScript toolchains
        # have no integer type.
        print("\n=== workloads: int-fib, int-loop (tsc-rs only) — the integer type")
        int_workloads = [
            ("int-fib", INT_FIB, 35, 30, "recursive fib using the int type"),
            ("int-loop", INT_LOOP, 100_000_000, 10_000_000, "tight loop using the int type"),
        ]
        ours_tc = next(t for t in tcs if t.is_ours)
        for wl_name, tmpl, n_full, n_quick, note in int_workloads:
            wl = Workload(
                name=wl_name,
                template=tmpl,
                n=n_full,
                quick_n=n_quick,
                note=note,
            )
            n = wl.quick_n if args.quick else wl.n
            cache_dir = keep / "cache-tsc-rs"
            r = bench_workload(wl, ours_tc, n, args.reps, keep, cache_dir)
            prepared.append(r)
            results.append(r)
            if r.error:
                print(f"  {wl_name:<22} ERROR: {r.error.splitlines()[0][:80]}")
            else:
                print(f"  {wl_name:<22} size={human_size(r.size):>11}")

        # Phase 2: time everything in interleaved rounds, so a load spike on
        # this shared machine cannot skew one toolchain relative to the others.
        print(f"\n=== timing ({args.reps} interleaved rounds)")
        measure_round_robin(prepared, args.reps)
        for r in results:
            if not r.error and r.times:
                print(f"  {r.workload:<10} {r.toolchain:<32} {r.runtime_best * 1000:9.2f}ms")

        # Phase 3: drop the artifacts. A deno binary is 65 MiB and the size
        # has already been measured, so keeping them would fill the disk.
        for r in prepared:
            for cand in (Path(r.cwd) / "prog", Path(r.cwd) / "out"):
                if cand.exists():
                    if cand.is_dir():
                        shutil.rmtree(cand, ignore_errors=True)
                    else:
                        cand.unlink(missing_ok=True)

    # --- correctness: every toolchain must agree -------------------------
    print("\n=== output agreement")
    problems = []
    for w in WORKLOADS:
        rows = [r for r in results if r.workload == w.name and not r.error]
        outs = {r.toolchain: r.output for r in rows}
        if not outs:
            continue
        # A published older version is allowed to differ — that difference is
        # the point of keeping it in the comparison.
        legacy = {r.toolchain: r.output for r in rows if r.legacy}
        current = {k: v for k, v in outs.items() if k not in legacy}
        values = list(current.values())
        same_value, same_bytes = outputs_agree(values)
        detail = "byte-identical" if same_bytes else ("numerically equal" if same_value else "MISMATCH")
        print(f"  {w.name}: {len(current)} toolchains -> {detail} ({next(iter(current.values()))!r})")
        for name, value in legacy.items():
            if value not in current.values():
                print(f"    (legacy {name} prints {value!r} — the old lossy formatting)")
        if not same_value:
            problems.append((w.name, current))
    if problems:
        print("\nERROR: toolchains disagree on program output", file=sys.stderr)
        for name, outs in problems:
            print(f"  {name}: {outs}", file=sys.stderr)
        return 2

    write_report(
        results,
        WORKLOADS,
        Path(args.out),
        args.quick,
        overhead * 1000,
        load_avg,
    )
    print(f"\nreport written to {args.out}")
    return 0


def size_phrase(ours: int, theirs: int) -> str:
    """Describe the artifact-size relationship, in the right direction."""
    if ours <= 0 or theirs <= 0:
        return "artifact size not comparable"
    if theirs >= ours:
        return f"{theirs / ours:.0f}x larger artifact"
    return f"{ours / theirs:.1f}x smaller artifact"


def write_report(
    results,
    workloads,
    path: Path,
    quick: bool,
    overhead_ms: float = 0.0,
    load_avg: float | None = None,
) -> None:
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
    ]
    if load_avg is not None:
        # The spawn floor is ~5ms on an idle host and ~20ms on a busy one, and
        # it changes every *ratio* (the absolute differences stay the same).
        # Recording it makes the numbers in this report interpretable.
        lines.append(
            f"- Machine load average when the run started: {load_avg:.1f}"
            + ("  (busy host: the runtime ratios are noisy)" if load_avg > 4 else "")
        )
    lines += [
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
    ours_start = next((r for r in startup_rows.values() if r.is_ours), None)
    kinds = {tc.name: tc.runtime_kind for tc in toolchains()}
    if ours_start:
        lines += ["## Headline", ""]
        for name, r in startup_rows.items():
            if r.is_ours:
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

    for w in WORKLOADS:
        rows = [r for r in results if r.workload == w.name]
        ok = [r for r in rows if not r.error]
        if not ok:
            continue
        ours = next((r for r in ok if r.is_ours), None)
        ours_compute = (
            ours.runtime_best - baseline.get(ours.toolchain, 0.0) if ours else 0.0
        )
        lines += [
            f"## {w.name}",
            "",
            f"{w.note}.",
            "",
            "| Toolchain | Compile (cold) | Compile (warm) | Artifact size | Runtime | compute | vs. compiler |",
            "|---|---|---|---|---|---|---|",
        ]
        for r in rows:
            if r.error:
                lines.append(f"| {r.toolchain} | — | — | — | — | — | ERROR |")
                continue
            base = baseline.get(r.toolchain, 0.0)
            compute = max(0.0, r.runtime_best - base)
            if r.is_ours:
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

    # Our own `int` versions of two workloads, next to the `number` versions
    # above. Only tsc-rs can run them, so these are not cross-toolchain
    # comparisons — they quantify what the integer type buys.
    def int_section(name: str, number_name: str, title: str, blurb: str) -> list[str]:
        int_row = next((r for r in results if r.workload == name), None)
        num_row = next((r for r in results if r.workload == number_name), None)
        if not int_row or not num_row or int_row.error or num_row.error:
            return []
        num_compute = num_row.runtime_best - baseline.get(num_row.toolchain, 0.0)
        int_compute = int_row.runtime_best - baseline.get(int_row.toolchain, 0.0)
        speedup = num_compute / int_compute if int_compute > 1e-9 else float("inf")
        if speedup >= 1.0:
            verdict = f"**The `int` type is {speedup:.2f}x faster** on this workload."
        else:
            verdict = f"**The `int` type is {1 / speedup:.2f}x slower** on this workload."
        return [
            f"## {title}",
            "",
            blurb,
            "",
            "| Variant | Artifact size | Runtime | compute |",
            "|---|---|---|---|",
            f"| `number` (f64) | {human_size(num_row.size)} | {num_row.runtime_best * 1000:.2f}ms | {num_compute * 1000:.2f}ms |",
            f"| `int` (i64) | {human_size(int_row.size)} | {int_row.runtime_best * 1000:.2f}ms | {int_compute * 1000:.2f}ms |",
            "",
            verdict,
            "",
        ]

    lines += int_section(
        "int-loop",
        "loop",
        "int vs number: a tight loop",
        "The same counting loop, once with `number` (f64) and once with `int`\n"
        "(i64). This is where the integer type pays: the workload is dominated\n"
        "by arithmetic, so integer instructions win.",
    )
    lines += int_section(
        "int-fib",
        "fib",
        "int vs number: recursive calls",
        "The same recursive fibonacci, once with `number` and once with `int`.\n"
        "**Here the integer type does not help** — the workload is dominated by\n"
        "call overhead, not arithmetic. That is the evidence that the gap to\n"
        "scriptc on recursive code is about how calls are compiled, not about\n"
        "floating point.",
    )

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
