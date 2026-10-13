"""Differential test: otio-algo flatten_stack vs reference Python OTIO.

Flattens each timeline's stack with both implementations and compares the
resulting track's child layout (schema + range per child).

Usage (from repo root):
    cargo build -p otio-algo --example flatten_dump
    python crates/otio-algo/tests/diff_flatten.py
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import opentimelineio as otio

REPO = Path(__file__).resolve().parents[3]
SAMPLES = REPO / "_ref" / "OpenTimelineIO" / "tests" / "sample_data"
DUMP = REPO / "target" / "debug" / "examples" / (
    "flatten_dump.exe" if sys.platform == "win32" else "flatten_dump"
)
TOL = 1e-6
SKIP = {"big_int.otio"}  # non-standard JSON, see otio-core corpus skip


def py_lines(path: Path) -> list[str]:
    try:
        tl = otio.adapters.read_from_file(str(path))
    except Exception:
        return []
    if not isinstance(tl, otio.schema.Timeline):
        return []
    try:
        flat = otio.algorithms.flatten_stack(tl.tracks)
    except Exception:
        return ["ERR"]
    out = []
    for ci, child in enumerate(flat):
        try:
            r = flat.range_of_child_at_index(ci)
            res = f"{r.start_time.value},{r.start_time.rate},{r.duration.value},{r.duration.rate}"
        except Exception:
            res = "ERR"
        out.append(f"{ci}\t{child.schema_name()}\t{res}")
    return out


def cmp_res(a: str, b: str) -> bool:
    if a == "ERR" or b == "ERR":
        return a == b
    av = [float(x) for x in a.split(",")]
    bv = [float(x) for x in b.split(",")]
    return len(av) == len(bv) and all(abs(x - y) <= TOL for x, y in zip(av, bv))


def main() -> int:
    if not DUMP.exists():
        print(f"flatten_dump not built: {DUMP}", file=sys.stderr)
        return 2

    files = 0
    children = 0
    mismatches = 0
    for path in sorted(SAMPLES.glob("*.otio")):
        if path.name in SKIP:
            continue
        py = py_lines(path)
        proc = subprocess.run([str(DUMP), str(path)], capture_output=True, text=True)
        rust = [l for l in proc.stdout.splitlines() if l.strip()]
        if not py and not rust:
            continue
        files += 1
        if len(py) != len(rust):
            mismatches += 1
            print(f"MISMATCH {path.name}: line count py={len(py)} rust={len(rust)}")
            continue
        for pl, rl in zip(py, rust):
            children += 1
            if pl != rl and not cmp_flexible(pl, rl):
                mismatches += 1
                if mismatches <= 30:
                    print(f"MISMATCH {path.name}: py={pl!r} rust={rl!r}")

    print(f"\n{files} timeline files, {children} flat children, {mismatches} mismatches")
    return 1 if mismatches else 0


def cmp_flexible(pl: str, rl: str) -> bool:
    """Compare two lines: exact for idx/schema, tolerant for the numeric range."""
    if pl == "ERR" or rl == "ERR":
        return pl == rl
    pp = pl.split("\t")
    rp = rl.split("\t")
    if len(pp) != 3 or len(rp) != 3:
        return False
    return pp[0] == rp[0] and pp[1] == rp[1] and cmp_res(pp[2], rp[2])


if __name__ == "__main__":
    raise SystemExit(main())
