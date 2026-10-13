"""Differential test: otio-time (Rust) vs reference opentimelineio.opentime.

Builds a battery of cases, computes expected results with the reference Python
library, runs the same inputs through the Rust `oracle` example, and compares.

Usage (from repo root):
    cargo build -p otio-time --example oracle
    python crates/otio-time/tests/diff_opentime.py

Exits non-zero on any mismatch.
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

from opentimelineio import opentime as ot

REPO = Path(__file__).resolve().parents[3]
ORACLE = REPO / "target" / "debug" / "examples" / ("oracle.exe" if sys.platform == "win32" else "oracle")

SMPTE = [24.0, 24000 / 1001, 25.0, 30.0, 30000 / 1001, 48.0, 48000 / 1001, 50.0, 60.0, 60000 / 1001]
BAD_RATES = [29.0, 12.0, 23.0]

TOL = 1e-6


def py_call(op: str, a: str, rate: float):
    """Return the reference result as a string, or 'ERR' if it raises."""
    try:
        if op == "to_timecode":
            return ot.RationalTime(float(a), rate).to_timecode(rate)
        if op == "from_timecode":
            return repr(ot.RationalTime.from_timecode(a, rate).value)
        if op == "to_time_string":
            return ot.RationalTime(float(a), rate).to_time_string()
        if op == "from_time_string":
            return repr(ot.RationalTime.from_time_string(a, rate).to_seconds())
        if op == "to_seconds":
            return repr(ot.RationalTime(float(a), rate).to_seconds())
    except Exception:
        return "ERR"
    raise ValueError(op)


def build_cases():
    """Yield (op, arg, rate) tuples covering the conversion surface."""
    values = [0, 1, 2, 15, 23, 24, 29, 30, 1000, 1799, 1800, 17981, 17982, 86399, 100000, -1, 1.5]
    for rate in SMPTE + BAD_RATES:
        for v in values:
            yield ("to_timecode", repr(float(v)), rate)
            yield ("to_seconds", repr(float(v)), rate)

    # from_timecode: valid ones generated from the reference + hand-made invalids.
    for rate in SMPTE:
        for v in [0, 1, 30, 1800, 17982, 100000]:
            try:
                tc = ot.RationalTime(float(v), rate).to_timecode(rate)
                yield ("from_timecode", tc, rate)
            except Exception:
                pass
    for tc, rate in [
        ("00:00:00:99", 30.0),
        ("ab:cd:ef:gh", 24.0),
        ("00:00:00;00", 30.0),
        ("1:2:3", 24.0),
        ("00:00:00;00", 30000 / 1001),
        ("00:00:00:00", 29.0),
    ]:
        yield ("from_timecode", tc, rate)

    # time strings.
    for s in ["00:00:01.0", "01:00:00.5", "90", "1:30", "-01:00:00.0",
              "00:00:00.123456", "1:90", "12:34:56.789"]:
        yield ("from_time_string", s, 24.0)
    for v in [0, 1, 12, 36000, 86399, -1, 1.5, 0.123456, 90061.5]:
        for rate in [24.0, 30000 / 1001, 25.0]:
            yield ("to_time_string", repr(float(v)), rate)


def main() -> int:
    if not ORACLE.exists():
        print(f"oracle not built: {ORACLE}\nrun: cargo build -p otio-time --example oracle", file=sys.stderr)
        return 2

    cases = list(build_cases())
    lines = [f"{op}\t{arg}\t{repr(rate)}" for (op, arg, rate) in cases]
    proc = subprocess.run([str(ORACLE)], input="\n".join(lines), capture_output=True, text=True)
    rust = proc.stdout.splitlines()
    if len(rust) != len(cases):
        print(f"oracle returned {len(rust)} lines, expected {len(cases)}", file=sys.stderr)
        print(proc.stderr, file=sys.stderr)
        return 2

    mismatches = 0
    for (op, arg, rate), got in zip(cases, rust):
        want = py_call(op, arg, rate)
        ok = False
        if want == "ERR" or got == "ERR":
            ok = want == got
        elif op in ("from_timecode", "from_time_string", "to_seconds"):
            try:
                ok = abs(float(want) - float(got)) <= TOL
            except ValueError:
                ok = False
        else:  # string ops
            ok = want == got
        if not ok:
            mismatches += 1
            if mismatches <= 30:
                print(f"MISMATCH {op}({arg!r}, rate={rate}): py={want!r} rust={got!r}")

    print(f"\n{len(cases)} cases, {mismatches} mismatches")
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())
