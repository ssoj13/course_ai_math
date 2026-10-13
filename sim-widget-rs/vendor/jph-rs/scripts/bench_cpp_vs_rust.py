#!/usr/bin/env python3
"""Compare C++ ojph_compress vs Rust ojph_bench on the same 4K synthetic RGB corpus."""

from __future__ import annotations

import os
import statistics
import subprocess
import sys
import time
from pathlib import Path

WIDTH = 3840
HEIGHT = 2160
WARMUP = 2
ITERS = 5

ROOT = Path(__file__).resolve().parents[1]
TMP = ROOT / "target" / "bench_tmp"
CPP_DEFAULT = Path(
    r"C:\projects\projects.rust.cg.offload\vfx.ref\OpenJPH\build\src\apps\ojph_compress\Release\ojph_compress.exe"
)
RUST_BENCH = ROOT.parents[2] / "target" / "release" / "ojph_bench.exe"


def synthetic_rgb_u8(width: int, height: int) -> bytes:
    """Same pattern as ojph_bench.rs synthetic_rgb_u8."""
    out = bytearray(width * height * 3)
    i = 0
    for y in range(height):
        for x in range(width):
            t = ((x ^ y) * 17) & 0xFF
            out[i] = (t + (x & 0xFF)) & 0xFF
            out[i + 1] = (t + (y & 0xFF)) & 0xFF
            out[i + 2] = (t + 128) & 0xFF
            i += 3
    return bytes(out)


def write_ppm(path: Path, width: int, height: int, pixels: bytes) -> None:
    hdr = f"P6\n{width} {height}\n255\n".encode("ascii")
    path.write_bytes(hdr + pixels)


def median_ms(samples: list[float]) -> float:
    return statistics.median(samples)


def mpix_s(ms: float) -> float:
    return (WIDTH * HEIGHT) / (ms / 1000.0) / 1e6


def run_cpp(compress: Path, ppm: Path, j2c: Path) -> tuple[float, int]:
    cmd = [
        str(compress),
        "-i",
        str(ppm),
        "-o",
        str(j2c),
        "-reversible",
        "true",
        "-colour_trans",
        "true",
        "-num_decomps",
        "5",
        "-block_size",
        "{64,64}",
        "-prog_order",
        "RPCL",
    ]
    core_dll = compress.parent.parent.parent / "core" / "Release"
    env = os.environ.copy()
    env["PATH"] = str(core_dll) + os.pathsep + env.get("PATH", "")

    for _ in range(WARMUP):
        subprocess.run(cmd, check=True, capture_output=True, env=env)

    samples: list[float] = []
    for _ in range(ITERS):
        t0 = time.perf_counter()
        subprocess.run(cmd, check=True, capture_output=True, env=env)
        samples.append((time.perf_counter() - t0) * 1000.0)

    size = j2c.stat().st_size if j2c.exists() else 0
    return median_ms(samples), size


def run_rust(bench: Path, level: int | None) -> tuple[float, int]:
    cmd = [str(bench), "--worker"]
    env = os.environ.copy()
    env.pop("OJPH_CPU_MAX_LEVEL", None)
    env.pop("OJPH_CPU_FORCE_LEVEL", None)
    if level is not None:
        env["OJPH_CPU_MAX_LEVEL"] = str(level)

    out = subprocess.run(cmd, check=True, capture_output=True, text=True, env=env)
    line = next(l for l in out.stdout.splitlines() if l.startswith("BENCH_JSON "))
    parts = dict(p.split("=", 1) for p in line.split()[1:])
    return float(parts["ms"]), int(parts["bytes"])


def main() -> int:
    ref = os.environ.get("OPENJPH_REF_DIR", "").strip()
    compress = CPP_DEFAULT
    if ref:
        alt = Path(ref) / "build" / "src" / "apps" / "ojph_compress" / "Release" / "ojph_compress.exe"
        if alt.exists():
            compress = alt

    bench = RUST_BENCH
    if not bench.exists():
        print(f"Missing Rust bench: {bench}", file=sys.stderr)
        return 1
    if not compress.exists():
        print(f"Missing C++ compress: {compress}", file=sys.stderr)
        return 1

    TMP.mkdir(parents=True, exist_ok=True)
    ppm = TMP / "bench_3840x2160.ppm"
    j2c = TMP / "bench_3840x2160.j2c"
    pixels = synthetic_rgb_u8(WIDTH, HEIGHT)
    write_ppm(ppm, WIDTH, HEIGHT, pixels)

    print(f"CPU benchmark Ryzen — 3840x2160 RGB lossless (median of {ITERS}, warmup {WARMUP})")
    print(f"PPM: {ppm} ({ppm.stat().st_size // (1024*1024)} MiB)")
    print()

    cpp_ms, cpp_bytes = run_cpp(compress, ppm, j2c)
    print(
        f"C++ ojph_compress (native SIMD)  {cpp_ms:8.2f} ms  {mpix_s(cpp_ms):6.2f} Mpix/s  {cpp_bytes} bytes"
    )

    for label, level in [("generic", 0), ("avx", 8), ("avx2", 9), ("native cap=10", 10)]:
        ms, nbytes = run_rust(bench, level)
        ratio = cpp_ms / ms if ms > 0 else 0.0
        print(
            f"Rust ojph_bench level={level:2d} ({label:12s}) {ms:8.2f} ms  {mpix_s(ms):6.2f} Mpix/s  {nbytes} bytes  (cpp/rust {ratio:.2f}x)"
        )

    print()
    if cpp_bytes == int(run_rust(bench, 9)[1]):
        print("J2C size: C++ matches Rust avx2 tier")
    else:
        print(f"J2C size: C++ {cpp_bytes} vs Rust avx2 {run_rust(bench, 9)[1]} (input pattern may differ from parity corpus)")

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
