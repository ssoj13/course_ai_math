"""Differential test: otio-algo transformed_time vs reference Python OTIO.

For each clip (depth-first), maps its local start time into the top stack's
coordinate space with both implementations and compares.

Usage (from repo root):
    cargo build -p otio-algo --example transform_dump
    python crates/otio-algo/tests/diff_transform.py
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

import opentimelineio as otio

REPO = Path(__file__).resolve().parents[3]
SAMPLES = REPO / "_ref" / "OpenTimelineIO" / "tests" / "sample_data"
DUMP = REPO / "target" / "debug" / "examples" / (
    "transform_dump.exe" if sys.platform == "win32" else "transform_dump"
)
TOL = 1e-6
SKIP = {"big_int.otio"}


def py_lines(path: Path) -> list[str]:
    try:
        tl = otio.adapters.read_from_file(str(path))
    except Exception:
        return []
    if not isinstance(tl, otio.schema.Timeline):
        return []
    out = []
    for i, clip in enumerate(tl.find_clips()):
        try:
            start = clip.trimmed_range().start_time
            tt = clip.transformed_time(start, tl.tracks)
            res = f"{tt.value},{tt.rate}"
        except Exception:
            res = "ERR"
        out.append(f"{i}\t{res}")
    return out


def cmp_line(pl: str, rl: str) -> bool:
    pp, rp = pl.split("\t"), rl.split("\t")
    if pp[0] != rp[0]:
        return False
    if pp[1] == "ERR" or rp[1] == "ERR":
        return pp[1] == rp[1]
    pv = [float(x) for x in pp[1].split(",")]
    rv = [float(x) for x in rp[1].split(",")]
    return all(abs(a - b) <= TOL for a, b in zip(pv, rv))


def main() -> int:
    if not DUMP.exists():
        print(f"transform_dump not built: {DUMP}", file=sys.stderr)
        return 2

    files = 0
    clips = 0
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
            clips += 1
            if not cmp_line(pl, rl):
                mismatches += 1
                if mismatches <= 30:
                    print(f"MISMATCH {path.name}: py={pl!r} rust={rl!r}")

    print(f"\n{files} timeline files, {clips} clips, {mismatches} mismatches")
    return 1 if mismatches else 0


if __name__ == "__main__":
    raise SystemExit(main())
