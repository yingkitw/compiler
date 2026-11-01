#!/usr/bin/env python3
"""Profile our fib against scriptc's fib with macOS `sample`.

The recursion gap to scriptc is the top open question in TODO.md. Three
hypotheses have already been ruled out (instruction count, float-vs-int
arithmetic, stack spills of the argument). MEMORY.md says the next step is
to *profile* rather than reason from the disassembly.

This prints, for each binary, the hot program counters — the
`Total number in stack` section of the `sample` report — together with the
instruction at each hot offset, so the two can be compared directly.

    python3 scripts/profile_calls.py [n] [seconds]
"""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TSC = ROOT / "target" / "release" / "tsc"

FIB = """function fib(n: number): number {
    if (n < 2) { return n; }
    return fib(n - 1) + fib(n - 2);
}
print(fib({N}));
"""

HOT = re.compile(r"^\s*(\d+)\s+(\S+)\s+\(in ([^)]+)\)\s+\+\s*(\d+)")


def build(work: Path, n: int) -> tuple[Path, Path]:
    ours_src = work / "ours.ts"
    ours_src.write_text(FIB.replace("{N}", str(n)))
    ours = work / "ours"
    subprocess.run([str(TSC), str(ours_src), "-o", str(ours)], check=True)

    theirs_src = work / "theirs.ts"
    theirs_src.write_text(FIB.replace("{N}", str(n)).replace("print(", "console.log("))
    theirs = work / "theirs"
    subprocess.run(
        ["scriptc", "build", str(theirs_src), "--out", str(theirs)],
        check=True,
        capture_output=True,
    )
    return ours, theirs


def sample(binary: Path, seconds: int) -> str:
    report = Path(tempfile.mkdtemp(prefix="sample-")) / "sample.txt"
    proc = subprocess.Popen(
        [str(binary)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL
    )
    try:
        subprocess.run(
            ["sample", str(proc.pid), str(seconds), "-file", str(report), "-mayDie"],
            capture_output=True,
            check=False,
        )
    finally:
        proc.wait()
    return report.read_text() if report.exists() else ""


def hot_pcs(report: str) -> list[tuple[str, int, int]]:
    """(symbol, offset, samples) from the "Total number in stack" section."""
    lines = report.splitlines()
    try:
        start = next(i for i, l in enumerate(lines) if l.startswith("Total number in stack"))
    except StopIteration:
        return []
    out: list[tuple[str, int, int]] = []
    for line in lines[start + 1 :]:
        if line.strip() and not line.startswith(" "):
            break
        m = HOT.match(line)
        if m:
            out.append((m.group(2), int(m.group(4)), int(m.group(1))))
    return out


def disasm(binary: Path) -> dict[str, str]:
    """{symbol: {offset: instruction}} for the binary's own text."""
    text = subprocess.run(
        ["otool", "-tvV", str(binary)], capture_output=True, text=True
    ).stdout
    out: dict[str, dict[int, str]] = {}
    symbol = ""
    base = 0
    for line in text.splitlines():
        if line.endswith(":") and not line.startswith(" "):
            symbol = line[:-1]
            base = 0
            out.setdefault(symbol, {})
            continue
        m = re.match(r"^([0-9a-f]{16})\s+(.*)$", line)
        if m and symbol:
            addr = int(m.group(1), 16)
            if base == 0:
                base = addr
            out[symbol][addr - base] = m.group(2).strip()
    return out


def main() -> int:
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 42
    seconds = int(sys.argv[2]) if len(sys.argv) > 2 else 3
    with tempfile.TemporaryDirectory(prefix="tsc-profile-") as tmp:
        work = Path(tmp)
        ours, theirs = build(work, n)

        for label, binary in (("ours", ours), ("scriptc", theirs)):
            report = sample(binary, seconds)
            hot = hot_pcs(report)
            total = sum(c for _, _, c in hot)
            code = disasm(binary)
            print(f"\n=== {label} ===  ({total} samples in program code)")
            for symbol, offset, count in hot:
                share = 100.0 * count / total if total else 0
                instr = code.get(symbol, code.get("_" + symbol, {})).get(offset, "?")
                print(f"  {count:7d}  {share:5.1f}%  {symbol}+{offset:<4} {instr}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
