"""Generate NumPy reference fixtures for the TS tensor core (src/core).

Run from the repo root:  python -I scripts/tensor-fixtures.py
Writes tests/fixtures/tensor/core.json (committed; tests/js/tensor-core.test.ts reads it).

Every expected value comes from NumPy itself (einsum, matmul, broadcasting, reductions, strides/offsets of
views), except two hand-ported references that NumPy has no API for:
- mulberry32: the PRNG the course uses for seeded demo values (bit-exact port of the TS function);
- fp32 sequential / pairwise sums: the exact addition orders of src/core/dtype.ts, executed in np.float32
  arithmetic, so the TS Math.fround emulation is checked against real float32 rounding.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import numpy as np

OUT = Path(__file__).resolve().parent.parent / "tests" / "fixtures" / "tensor" / "core.json"
RNG = np.random.default_rng(20261011)


def arr(a: np.ndarray) -> dict[str, Any]:
    """Shape + row-major values of an array (a 0-d array gives shape [])."""
    a = np.asarray(a, dtype=np.float64)
    return {"shape": list(a.shape), "data": [float(x) for x in a.reshape(-1)]}


def ints(shape: tuple[int, ...]) -> np.ndarray:
    """Small integers: products and sums are exact, so any summation order must match NumPy bit for bit."""
    return RNG.integers(-3, 4, size=shape).astype(np.float64)


def floats(shape: tuple[int, ...]) -> np.ndarray:
    return RNG.uniform(-2.0, 2.0, size=shape)


# ---- mulberry32 ----

def mulberry32(seed: int):
    s = seed & 0xFFFFFFFF

    def imul(a: int, b: int) -> int:
        return (a * b) & 0xFFFFFFFF

    def nxt() -> float:
        nonlocal s
        s = (s + 0x6D2B79F5) & 0xFFFFFFFF
        t = s
        t = imul(t ^ (t >> 15), t | 1)
        t ^= (t + imul(t ^ (t >> 7), t | 61)) & 0xFFFFFFFF
        return ((t ^ (t >> 14)) & 0xFFFFFFFF) / 4294967296

    return nxt


def prng_cases() -> list[dict[str, Any]]:
    out = []
    for seed in (0, 1, 42, 123456789):
        r = mulberry32(seed)
        out.append({"seed": seed, "values": [r() for _ in range(6)]})
    return out


def rand_case() -> dict[str, Any]:
    # rand([2, 3], seed 7, range [-3, 3], step 1): lo + step * floor(u * m), m = 7 grid values.
    r = mulberry32(7)
    vals = [-3 + 1 * int(np.floor(r() * 7)) for _ in range(6)]
    return {"shape": [2, 3], "seed": 7, "range": [-3, 3], "step": 1, "data": [float(v) for v in vals]}


# ---- views: shape / strides (elements) / offset / values ----

def to_slice(e: Any) -> Any:
    if e is None:
        return slice(None)
    if isinstance(e, int):
        return e
    return slice(*e)


def apply(a: np.ndarray, op: list[Any]) -> np.ndarray:
    kind, arg = op
    if kind == "transpose":
        return a.transpose(arg) if arg is not None else a.T
    if kind == "slice":
        return a[tuple(to_slice(e) for e in arg)]
    if kind == "reshape":
        return a.reshape(arg)
    if kind == "expand":
        return np.expand_dims(a, arg)
    if kind == "squeeze":
        return np.squeeze(a, arg) if arg is not None else np.squeeze(a)
    if kind == "broadcast":
        return np.broadcast_to(a, arg)
    raise ValueError(kind)


VIEWS: list[tuple[list[int], list[list[Any]]]] = [
    ([3, 4, 5], [["transpose", None]]),
    ([3, 4, 5], [["transpose", [1, 0, 2]]]),
    ([3, 4, 5], [["slice", [1]]]),
    ([3, 4, 5], [["slice", [None, [None, None, 2], [4, 0, -2]]]]),
    ([3, 4, 5], [["slice", [[-1, None, -1]]]]),
    ([3, 4, 5], [["transpose", None], ["slice", [[1, 4], 2]]]),
    ([3, 4, 5], [["slice", [[1, 3], None, [1, 4]]], ["transpose", [2, 0, 1]]]),
    ([3, 4, 5], [["reshape", [-1, 6]]]),
    ([3, 4, 5], [["slice", [2]], ["reshape", [20]]]),
    ([3, 4, 5], [["expand", 1], ["slice", [None, 0, [None, None, -1]]]]),
    ([3, 4, 5], [["slice", [[0, 1]]], ["squeeze", 0]]),
    ([3, 1], [["broadcast", [2, 3, 4]]]),
    ([4], [["broadcast", [3, 4]], ["transpose", None]]),
    ([6], [["slice", [[-2, -6, -2]]]]),
]


def view_cases() -> list[dict[str, Any]]:
    out = []
    for base_shape, ops in VIEWS:
        base = np.arange(int(np.prod(base_shape)), dtype=np.float64).reshape(base_shape)
        a = base
        for op in ops:
            a = apply(a, op)
        ptr = a.__array_interface__["data"][0] - base.__array_interface__["data"][0]
        out.append({
            "base": base_shape,
            "ops": ops,
            "shape": list(a.shape),
            "strides": [s // a.itemsize for s in a.strides],
            "offset": ptr // a.itemsize,
            "data": arr(a)["data"],
        })
    return out


INVALID_VIEWS: list[tuple[list[int], list[list[Any]]]] = [
    ([3, 4], [["slice", [3]]]),               # index out of bounds
    ([3, 4], [["slice", [0, 0, 0]]]),         # too many indices
    ([3, 4], [["reshape", [5, 3]]]),          # size mismatch
    ([3, 4], [["transpose", [0, 0]]]),        # not a permutation
    ([3, 4], [["squeeze", 0]]),               # extent 3 cannot be squeezed
    ([3, 4], [["broadcast", [3, 5]]]),        # 4 does not stretch to 5
]


def invalid_view_cases() -> list[dict[str, Any]]:
    out = []
    for base_shape, ops in INVALID_VIEWS:
        a = np.zeros(base_shape)
        try:
            for op in ops:
                a = apply(a, op)
            numpy = "accepts"
        except (ValueError, IndexError, TypeError):
            numpy = "rejects"
        out.append({"base": base_shape, "ops": ops, "numpy": numpy})
    return out


# ---- einsum ----

PRESETS = [
    ("i,i->", {"i": 3}),
    ("ij,j->i", {"i": 3, "j": 4}),
    ("ij,jk->ik", {"i": 2, "j": 3, "k": 4}),
    ("bij,bjk->bik", {"b": 2, "i": 3, "j": 4, "k": 2}),
    ("bhqd,bhkd->bhqk", {"b": 2, "h": 2, "q": 3, "k": 4, "d": 5}),
]

EXTRA = [
    ("i,j->ij", {"i": 3, "j": 2}),          # outer product: shaft of length 1
    ("ijk,jk->i", {"i": 2, "j": 3, "k": 4}),  # two summed letters flatten into one shaft
    ("ij,jk,kl->il", {"i": 2, "j": 3, "k": 2, "l": 3}),
    ("ijk->ki", {"i": 2, "j": 3, "k": 4}),
    ("ij,ij->", {"i": 3, "j": 3}),
]


def random_specs(n: int) -> list[tuple[str, dict[str, int]]]:
    """Two-operand specs over letters a..f, at least one shared letter, distinct letters per operand."""
    letters = list("abcdef")
    out = []
    while len(out) < n:
        ra, rb = (int(x) for x in RNG.integers(1, 4, size=2))
        a = "".join(RNG.choice(letters, size=ra, replace=False))
        b = "".join(RNG.choice(letters, size=rb, replace=False))
        if not set(a) & set(b):
            continue
        union = sorted(set(a) | set(b))
        keep = [c for c in union if RNG.random() < 0.5]
        RNG.shuffle(keep)
        op = f"{a},{b}->{''.join(keep)}"
        if any(o == op for o, _ in out):
            continue
        out.append((op, {c: int(RNG.integers(1, 5)) for c in union}))
    return out


def einsum_case(op: str, dims: dict[str, int], make, transpose: bool = False) -> dict[str, Any]:
    terms = op.split("->")[0].split(",")
    ops = [make(tuple(dims[c] for c in t)) for t in terms]
    case: dict[str, Any] = {"op": op, "dims": dims}
    if transpose:
        # Operands arrive as transposed (non-contiguous) views of stored arrays; TS applies the same transposes.
        stored = [np.ascontiguousarray(o.T) for o in ops]
        case["inputs"] = [arr(s) for s in stored]
        case["transpose"] = True
        ops = [s.T for s in stored]
    else:
        case["inputs"] = [arr(o) for o in ops]
    case["out"] = arr(np.einsum(op, *ops))
    return case


def einsum_cases() -> list[dict[str, Any]]:
    out = []
    for op, dims in PRESETS:
        out.append(einsum_case(op, dims, ints) | {"kind": "preset-int"})
        out.append(einsum_case(op, dims, floats) | {"kind": "preset-float"})
    for op, dims in EXTRA:
        out.append(einsum_case(op, dims, floats) | {"kind": "extra"})
    for op, dims in random_specs(10):
        out.append(einsum_case(op, dims, floats) | {"kind": "random"})
    out.append(einsum_case("ij,jk->ik", {"i": 3, "j": 4, "k": 2}, floats, transpose=True) | {"kind": "view"})
    out.append(einsum_case("bij,bjk->bik", {"b": 2, "i": 2, "j": 3, "k": 2}, floats, transpose=True) | {"kind": "view"})
    return out


INVALID_EINSUM: list[tuple[str, list[list[int]], str]] = [
    ("ij,jk", [[2, 3], [3, 4]], "spec"),                 # implicit output
    ("ii->i", [[3, 3]], "spec"),                         # trace
    ("...ij,jk->...ik", [[2, 2, 3], [3, 4]], "spec"),    # ellipsis
    ("iJ,Jk->ik", [[2, 3], [3, 4]], "spec"),             # uppercase
    ("ij, jk->ik", [[2, 3], [3, 4]], "spec"),            # whitespace
    ("ij,jk->iz", [[2, 3], [3, 4]], "spec"),             # output letter not in an operand
    ("ij,jk->ii", [[2, 3], [3, 4]], "spec"),             # repeated output letter
    ("ij,jk->i,k", [[2, 3], [3, 4]], "spec"),            # comma in output
    ("ij->j->j", [[2, 3]], "spec"),                      # two arrows
    ("ij,jk->ik", [[2, 3], [4, 5]], "shape"),            # contraction extents differ
    ("ij,j->i", [[2, 3, 1], [3]], "shape"),              # rank mismatch
    ("ij,jk->ik", [[2, 3]], "shape"),                    # operand count
]


def invalid_einsum_cases() -> list[dict[str, Any]]:
    out = []
    for op, shapes, err in INVALID_EINSUM:
        try:
            np.einsum(op, *[np.zeros(s) for s in shapes])
            numpy = "accepts"
        except ValueError:
            numpy = "rejects"
        out.append({"op": op, "shapes": shapes, "error": err, "numpy": numpy})
    return out


# ---- matmul / dot ----

MATMUL = [
    ([3], [3]),
    ([2, 3], [3]),
    ([3], [3, 4]),
    ([2, 3], [3, 4]),
    ([2, 2, 3], [2, 3, 4]),
    ([2, 1, 2, 3], [5, 3, 2]),
    ([4, 2, 3], [3, 1]),
    ([3], [2, 3, 4]),
]
MATMUL_BAD = [([2, 3], [4, 5]), ([2, 3, 4], [3, 4, 5]), ([], [3])]


def matmul_cases() -> dict[str, Any]:
    ok = []
    for sa, sb in MATMUL:
        a, b = floats(tuple(sa)), floats(tuple(sb))
        ok.append({"a": arr(a), "b": arr(b), "out": arr(np.matmul(a, b))})
    bad = []
    for sa, sb in MATMUL_BAD:
        try:
            np.matmul(np.zeros(sa), np.zeros(sb))
            numpy = "accepts"
        except ValueError:
            numpy = "rejects"
        bad.append({"a": sa, "b": sb, "numpy": numpy})
    return {"ok": ok, "bad": bad}


# ---- broadcasting elementwise ----

BROADCAST = [
    ([3, 1], [1, 4], "add"),
    ([2, 3, 4], [4], "mul"),
    ([5, 1, 3], [2, 1], "sub"),
    ([3], [], "div"),
    ([1], [2, 3], "add"),
    ([3], [4], "add"),
    ([2, 3], [3, 2], "mul"),
]
UFUNC = {"add": np.add, "sub": np.subtract, "mul": np.multiply, "div": np.divide}


def broadcast_cases() -> list[dict[str, Any]]:
    out = []
    for sa, sb, op in BROADCAST:
        a, b = floats(tuple(sa)), floats(tuple(sb)) + (3.0 if op == "div" else 0.0)
        case: dict[str, Any] = {"op": op, "a": arr(a), "b": arr(b)}
        try:
            case["out"] = arr(UFUNC[op](a, b))
        except ValueError:
            case["error"] = "shape"
        out.append(case)
    return out


# ---- reductions ----

def reduce_cases() -> dict[str, Any]:
    x = floats((2, 3, 4))
    out = []
    for fn, axis, keep, transpose in [
        ("sum", None, False, False),
        ("sum", 0, False, False),
        ("sum", -1, True, False),
        ("sum", [0, 2], False, False),
        ("mean", 1, False, False),
        ("mean", None, True, False),
        ("max", [1, 2], False, False),
        ("max", None, False, False),
        ("sum", 1, False, True),
        ("max", 0, True, True),
    ]:
        src = x.transpose() if transpose else x
        ax = tuple(axis) if isinstance(axis, list) else axis
        res = getattr(np, fn)(src, axis=ax, keepdims=keep)
        out.append({"fn": fn, "axis": axis, "keepdims": keep, "transpose": transpose, "out": arr(res)})
    return {"x": arr(x), "cases": out}


# ---- fp32 vs fp64 summation order ----

def seq(vals: list[Any], zero: Any) -> Any:
    if not vals:
        return zero
    acc = vals[0]
    for v in vals[1:]:
        acc = acc + v
    return acc


def pairwise(vals: list[Any]) -> Any:
    """Bottom-up adjacent pairs, odd last node carried: the tree of dtype.ts `accumulate` / `sumSteps`."""
    nodes = list(vals)
    while len(nodes) > 1:
        up = [nodes[j] + nodes[j + 1] for j in range(0, len(nodes) - 1, 2)]
        if len(nodes) % 2:
            up.append(nodes[-1])
        nodes = up
    return nodes[0]


def order_cases() -> list[dict[str, Any]]:
    out = []
    examples = {
        "ten tenths": [0.1] * 10,
        "big plus smalls": [1.0] + [1e-8] * 7,
        "random 13": [float(v) for v in RNG.uniform(-1.0, 1.0, size=13)],
    }
    for name, vals in examples.items():
        f32 = [np.float32(v) for v in vals]
        out.append({
            "name": name,
            "values": vals,
            "fp64": {"sequential": seq(vals, 0.0), "pairwise": pairwise(vals)},
            "fp32": {"sequential": float(seq(f32, np.float32(0))), "pairwise": float(pairwise(f32))},
            "repr": {
                "fp64 sequential": repr(seq(vals, 0.0)),
                "fp32 sequential": str(seq(f32, np.float32(0))),
                "fp32 pairwise": str(pairwise(f32)),
            },
        })
    return out


def dot32_case() -> dict[str, Any]:
    """i,i-> under fp32: operands, each product and each partial sum rounded to float32."""
    a = [float(v) for v in RNG.uniform(-3.0, 3.0, size=9)]
    b = [float(v) for v in RNG.uniform(-3.0, 3.0, size=9)]
    prods = [np.float32(x) * np.float32(y) for x, y in zip(a, b)]
    return {
        "a": a,
        "b": b,
        "fp64": float(np.dot(np.array(a), np.array(b))),
        "fp32": {"sequential": float(seq(prods, np.float32(0))), "pairwise": float(pairwise(prods))},
    }


def main() -> None:
    data = {
        "generator": "scripts/tensor-fixtures.py",
        "numpy": np.__version__,
        "mulberry32": prng_cases(),
        "rand": rand_case(),
        "views": view_cases(),
        "invalidViews": invalid_view_cases(),
        "einsum": einsum_cases(),
        "invalidEinsum": invalid_einsum_cases(),
        "matmul": matmul_cases(),
        "broadcast": broadcast_cases(),
        "reduce": reduce_cases(),
        "order": order_cases(),
        "dot32": dot32_case(),
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(data, indent=1) + "\n", encoding="utf-8")
    print(f"wrote {OUT} ({OUT.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
