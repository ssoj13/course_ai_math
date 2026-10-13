"""Differential test: otio-algo range_of_child_at_index vs reference Python OTIO.

For every timeline sample, computes each top-level track child's range with the
reference library and with the Rust `algo_dump` example, then compares.

Usage (from repo root):
    cargo build -p otio-algo --example algo_dump
    python crates/otio-algo/tests/diff_algo.py
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import opentimelineio as otio

REPO = Path(__file__).resolve().parents[3]
SAMPLES = REPO / "_ref" / "OpenTimelineIO" / "tests" / "sample_data"
DUMP = REPO / "target" / "debug" / "examples" / (
    "algo_dump.exe" if sys.platform == "win32" else "algo_dump"
)
TOL = 1e-6
# big_int.otio uses non-standard JSON (Inf/NaN/bigint) that serde_json can't
# parse; tracked separately (see otio-core corpus skip). Python parses it, so it
# would always "mismatch" here — exclude it from this gate.
SKIP = {"big_int.otio"}


def py_lines(path: Path) -> list[str]:
    """Reference range_of_child_at_index for each top-level track child."""
    try:
        tl = otio.adapters.read_from_file(str(path))
    except Exception:
        return []
    if not isinstance(tl, otio.schema.Timeline):
        return []
    out = []
    for ti, track in enumerate(tl.tracks):
        if not isinstance(track, otio.schema.Track):
            continue
        for ci in range(len(track)):
            try:
                r = track.range_of_child_at_index(ci)
                res = f"{r.start_time.value},{r.start_time.rate},{r.duration.value},{r.duration.rate}"
            except Exception:
                res = "ERR"
            out.append(f"{ti}\t{ci}\t{res}")
    return out


def parse(line: str):
    ti, ci, res = line.split("\t")
    return (ti, ci, res)


def cmp_res(a: str, b: str) -> bool:
    if a == "ERR" or b == "ERR":
        return a == b
    av = [float(x) for x in a.split(",")]
    bv = [float(x) for x in b.split(",")]
    return len(av) == len(bv) and all(abs(x - y) <= TOL for x, y in zip(av, bv))


def main() -> int:
    if not DUMP.exists():
        print(f"algo_dump not built: {DUMP}", file=sys.stderr)
        return 2

    total = 0
    mismatches = 0
    files = 0
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
            pti, pci, pres = parse(pl)
            rti, rci, rres = parse(rl)
            total += 1
            if (pti, pci) != (rti, rci) or not cmp_res(pres, rres):
                mismatches += 1
                if mismatches <= 30:
                    print(f"MISMATCH {path.name} t{pti}c{pci}: py={pres!r} rust={rres!r}")

    print(f"\n{files} timeline files, {total} child ranges, {mismatches} mismatches")
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())
