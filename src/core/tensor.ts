/**
 * A small strided tensor: one flat typed buffer plus `shape`, `strides` and `offset`, the NumPy memory model the
 * course teaches in 3.3 ("a tensor is strides over flat memory"). Views (slice, transpose, reshape, broadcast) share
 * the buffer and only rewrite the three numbers; `contiguous` is the only copy. Three-free and DOM-free, so scenes,
 * workers and tests use the same arithmetic.
 *
 * Conventions: strides and offset are counted in ELEMENTS (NumPy's are bytes: `np_strides / itemsize`); index
 * order is row-major (last axis fastest); negative indices and axes count from the end as in NumPy.
 */
import { IndexError, ShapeError } from './errors';
import { accumulate, alloc, arith, order, promote, storeOf, type AccOpts, type Data, type Store } from './dtype';

export type Shape = readonly number[];
export type Nested = number | readonly Nested[];

export const sizeOf = (shape: Shape): number => shape.reduce((p, n) => p * n, 1);

/** Row-major (C) strides in elements. */
export function cStrides(shape: Shape): number[] {
	const s = new Array<number>(shape.length);
	let acc = 1;
	for (let d = shape.length - 1; d >= 0; d--) {
		s[d] = acc;
		acc *= shape[d];
	}
	return s;
}

const fmtShape = (s: Shape): string => `(${s.join(', ')})`;

function checkShape(shape: Shape): void {
	for (const n of shape)
		if (!Number.isInteger(n) || n < 0) throw new ShapeError(`bad extent ${n} in shape ${fmtShape(shape)}`);
}

export class Tensor {
	readonly shape: Shape;
	readonly strides: Shape;

	/**
	 * Wraps a buffer without copying. Throws if any reachable element lies outside `data`, so every later `get`
	 * only needs per-axis bounds checks. `writable = false` marks broadcast views, where one buffer element appears
	 * at many indices and a write would silently change all of them (NumPy makes them read-only for the same reason).
	 */
	constructor(
		readonly data: Data,
		shape: Shape,
		strides: Shape = cStrides(shape),
		readonly offset = 0,
		readonly writable = true,
	) {
		checkShape(shape);
		if (strides.length !== shape.length || !strides.every(Number.isInteger))
			throw new ShapeError(`strides ${fmtShape(strides)} do not fit shape ${fmtShape(shape)}`);
		if (!Number.isInteger(offset) || offset < 0) throw new ShapeError(`bad offset ${offset}`);
		let lo = offset;
		let hi = offset;
		for (let d = 0; d < shape.length; d++) {
			const span = strides[d] * (shape[d] - 1);
			if (span < 0) lo += span;
			else hi += span;
		}
		const empty = sizeOf(shape) === 0;
		if (empty ? offset > data.length : lo < 0 || hi >= data.length)
			throw new ShapeError(`view ${fmtShape(shape)} strides ${fmtShape(strides)} offset ${offset} leaves a buffer of ${data.length}`);
		this.shape = Object.freeze([...shape]);
		this.strides = Object.freeze([...strides]);
	}

	get ndim(): number {
		return this.shape.length;
	}
	get size(): number {
		return sizeOf(this.shape);
	}
	get store(): Store {
		return storeOf(this.data);
	}

	/** Buffer position of a full multi-index. */
	pos(idx: readonly number[]): number {
		if (idx.length !== this.ndim) throw new IndexError(`index of rank ${idx.length} for a tensor of rank ${this.ndim}`);
		let p = this.offset;
		for (let d = 0; d < idx.length; d++) p += this.strides[d] * wrap(idx[d], this.shape[d], d);
		return p;
	}
	get(idx: readonly number[]): number {
		return this.data[this.pos(idx)];
	}
	set(idx: readonly number[], v: number): void {
		if (!this.writable) throw new IndexError('write into a read-only (broadcast) view');
		this.data[this.pos(idx)] = v;
	}
	/** The value of a one-element tensor (a 0-d result of a full reduction or `i,i->`). */
	item(): number {
		if (this.size !== 1) throw new ShapeError(`item() of a tensor of shape ${fmtShape(this.shape)}`);
		return this.data[this.offset];
	}
}

function wrap(i: number, n: number, axis: number): number {
	if (!Number.isInteger(i)) throw new IndexError(`index ${i} on axis ${axis} is not an integer`);
	const j = i < 0 ? i + n : i;
	if (j < 0 || j >= n) throw new IndexError(`index ${i} is out of bounds for axis ${axis} with size ${n}`);
	return j;
}

function axisOf(a: number, ndim: number): number {
	const d = a < 0 ? a + ndim : a;
	if (!Number.isInteger(a) || d < 0 || d >= ndim) throw new ShapeError(`axis ${a} is out of range for rank ${ndim}`);
	return d;
}

/**
 * Relative buffer offsets of every index of `shape`, in row-major order, for the given strides. The workhorse of
 * all loops: kernels precompute these once and then only add a base offset, instead of re-deriving offsets per element.
 */
export function offsets(shape: Shape, strides: Shape): Int32Array {
	const out = new Int32Array(sizeOf(shape));
	if (out.length === 0) return out;
	const nd = shape.length;
	const idx = new Array<number>(nd).fill(0);
	let p = 0;
	for (let k = 0; k < out.length; k++) {
		out[k] = p;
		// Odometer: bump the last axis, carry into earlier ones.
		for (let d = nd - 1; d >= 0; d--) {
			if (++idx[d] < shape[d]) {
				p += strides[d];
				break;
			}
			idx[d] = 0;
			p -= strides[d] * (shape[d] - 1);
		}
	}
	return out;
}

// ---- constructors ----

/** Tensor from nested arrays (a number gives a 0-d tensor). Ragged input is a ShapeError, not padded. */
export function tensor(v: Nested, st: Store = 'f64'): Tensor {
	const shape: number[] = [];
	for (let x: Nested = v; Array.isArray(x); x = (x as readonly Nested[])[0]) shape.push((x as readonly Nested[]).length);
	const data = alloc(st, sizeOf(shape));
	let k = 0;
	const fill = (x: Nested, d: number): void => {
		if (d === shape.length) {
			if (typeof x !== 'number') throw new ShapeError(`ragged nested array: expected a number at depth ${d}`);
			data[k++] = x;
			return;
		}
		if (!Array.isArray(x) || x.length !== shape[d])
			throw new ShapeError(`ragged nested array at depth ${d}: expected length ${shape[d]}`);
		for (const y of x as readonly Nested[]) fill(y, d + 1);
	};
	fill(v, 0);
	return new Tensor(data, shape);
}

export function zeros(shape: Shape, st: Store = 'f64'): Tensor {
	checkShape(shape);
	return new Tensor(alloc(st, sizeOf(shape)), shape);
}

export function full(shape: Shape, v: number, st: Store = 'f64'): Tensor {
	const t = zeros(shape, st);
	t.data.fill(v);
	return t;
}

/** 0, 1, ..., n-1 reshaped to `shape` (default 1-D): handy readable test data. */
export function arange(n: number, shape: Shape = [n], st: Store = 'f64'): Tensor {
	if (sizeOf(shape) !== n) throw new ShapeError(`arange(${n}) cannot fill shape ${fmtShape(shape)}`);
	const t = zeros(shape, st);
	for (let i = 0; i < n; i++) t.data[i] = i;
	return t;
}

/**
 * mulberry32: a 32-bit seeded PRNG (Tommy Ettinger's public-domain generator), uniform in [0, 1).
 * Chosen because it is tiny, fast, has one 32-bit state word (so a lesson seed fully reproduces the numbers) and
 * is trivial to port bit-exactly (the fixture script carries a Python port that the tests compare against).
 */
export function mulberry32(seed: number): () => number {
	let s = seed >>> 0;
	return () => {
		s = (s + 0x6d2b79f5) >>> 0;
		let t = s;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

export interface RandOpts {
	seed: number;
	/** Inclusive value range; default [-3, 3] (short demo numbers, intro spec 1.1). */
	range?: readonly [number, number];
	/** Quantization step: values are lo + step * m, uniform over the grid inside the range. Omit for continuous. */
	step?: number;
	store?: Store;
}

/** Seeded random tensor, filled in row-major order, one PRNG draw per element. */
export function rand(shape: Shape, o: RandOpts): Tensor {
	const [lo, hi] = o.range ?? [-3, 3];
	if (!(hi >= lo)) throw new ShapeError(`bad range [${lo}, ${hi}]`);
	if (o.step !== undefined && !(o.step > 0)) throw new ShapeError(`bad step ${o.step}`);
	const t = zeros(shape, o.store);
	const r = mulberry32(o.seed);
	// The epsilon absorbs (hi - lo) / step landing a hair under an integer (e.g. 0.6 / 0.1 = 5.999...).
	const m = o.step === undefined ? 0 : Math.floor((hi - lo) / o.step + 1e-9) + 1;
	for (let i = 0; i < t.data.length; i++) {
		const u = r();
		t.data[i] = o.step === undefined ? lo + u * (hi - lo) : lo + o.step * Math.floor(u * m);
	}
	return t;
}

/** Values in logical row-major order (whatever the strides). */
export function flat(t: Tensor): number[] {
	return Array.from(offsets(t.shape, t.strides), (r) => t.data[t.offset + r]);
}

export function toNested(t: Tensor): Nested {
	const v = flat(t);
	let k = 0;
	const build = (d: number): Nested =>
		d === t.ndim ? v[k++] : Array.from({ length: t.shape[d] }, () => build(d + 1));
	return build(0);
}

// ---- views (share `data`) ----

/** Per axis: an integer index (drops the axis), `[start, stop, step]` (Python slice; nulls = defaults), or null = all. */
export type SliceSpec = number | null | readonly [(number | null)?, (number | null)?, (number | null)?];

/** NumPy basic slicing. Missing trailing entries mean "all". Negative steps walk backwards with a negative stride. */
export function slice(t: Tensor, spec: readonly SliceSpec[]): Tensor {
	if (spec.length > t.ndim) throw new IndexError(`${spec.length} slice entries for a tensor of rank ${t.ndim}`);
	const shape: number[] = [];
	const strides: number[] = [];
	let off = t.offset;
	for (let d = 0; d < t.ndim; d++) {
		const s = spec[d] ?? null;
		const n = t.shape[d];
		if (typeof s === 'number') {
			off += wrap(s, n, d) * t.strides[d];
			continue;
		}
		const [a, b, c] = s ?? [];
		const step = c ?? 1;
		if (!Number.isInteger(step) || step === 0) throw new IndexError(`slice step ${step} on axis ${d}`);
		// Python's slice.indices(n): clamp start/stop into the walkable range for the step's direction.
		const norm = (x: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, x < 0 ? x + n : x));
		const start = a == null ? (step > 0 ? 0 : n - 1) : step > 0 ? norm(a, 0, n) : norm(a, -1, n - 1);
		const stop = b == null ? (step > 0 ? n : -1) : step > 0 ? norm(b, 0, n) : norm(b, -1, n - 1);
		const len = Math.max(0, Math.ceil((stop - start) / step));
		// An empty slice keeps the offset: its start may point one past the end, which is no element at all.
		if (len > 0) off += start * t.strides[d];
		shape.push(len);
		strides.push(t.strides[d] * step);
	}
	return new Tensor(t.data, shape, strides, off, t.writable);
}

/** Permutes axes (NumPy `transpose`; default reverses them, i.e. `.T`). No data moves, only strides. */
export function transpose(t: Tensor, axes?: readonly number[]): Tensor {
	const p = axes ? axes.map((a) => axisOf(a, t.ndim)) : t.shape.map((_, d) => t.ndim - 1 - d);
	if (p.length !== t.ndim || new Set(p).size !== p.length)
		throw new ShapeError(`axes [${axes?.join(', ')}] are not a permutation of rank ${t.ndim}`);
	return new Tensor(t.data, p.map((d) => t.shape[d]), p.map((d) => t.strides[d]), t.offset, t.writable);
}

/** True when the elements fill data[offset .. offset + size) in row-major order (size-1 axes may have any stride). */
export function isContiguous(t: Tensor): boolean {
	let want = 1;
	for (let d = t.ndim - 1; d >= 0; d--) {
		if (t.shape[d] !== 1 && t.strides[d] !== want) return false;
		want *= t.shape[d];
	}
	return true;
}

/**
 * New shape over the same buffer (one extent may be -1 = inferred). Only for contiguous tensors: NumPy silently
 * copies a non-contiguous one, which would break the "views share data" picture without saying so; here the caller
 * writes `reshape(contiguous(t), ...)` and the copy is visible in the code.
 */
export function reshape(t: Tensor, shape: Shape): Tensor {
	const free = shape.filter((n) => n === -1).length;
	if (free > 1) throw new ShapeError(`reshape ${fmtShape(shape)}: only one extent can be -1`);
	const known = shape.reduce((p, n) => (n === -1 ? p : p * n), 1);
	const s = shape.map((n) => (n === -1 ? (known === 0 ? -1 : t.size / known) : n));
	checkShape(s);
	if (sizeOf(s) !== t.size) throw new ShapeError(`cannot reshape ${fmtShape(t.shape)} (size ${t.size}) into ${fmtShape(shape)}`);
	if (!isContiguous(t)) throw new ShapeError('reshape needs a contiguous tensor: call contiguous() first');
	return new Tensor(t.data, s, cStrides(s), t.offset, t.writable);
}

/** Inserts an axis of extent 1 at `axis` (range -(ndim+1)..ndim, as in NumPy). */
export function expandDims(t: Tensor, axis: number): Tensor {
	const d = axisOf(axis, t.ndim + 1);
	const shape = [...t.shape];
	const strides = [...t.strides];
	// A size-1 axis is never stepped, so its stride is arbitrary; the C-order value keeps contiguous tensors contiguous.
	shape.splice(d, 0, 1);
	strides.splice(d, 0, d < t.ndim ? t.strides[d] * t.shape[d] : 1);
	return new Tensor(t.data, shape, strides, t.offset, t.writable);
}

/** Removes extent-1 axes: all of them, or exactly the given ones (a given axis of another extent is a ShapeError). */
export function squeeze(t: Tensor, axis?: number | readonly number[]): Tensor {
	const drop = new Set(
		axis === undefined ? t.shape.flatMap((n, d) => (n === 1 ? [d] : [])) : [axis].flat().map((a) => axisOf(a, t.ndim)),
	);
	for (const d of drop) if (t.shape[d] !== 1) throw new ShapeError(`cannot squeeze axis ${d} of extent ${t.shape[d]}`);
	const keep = t.shape.map((_, d) => d).filter((d) => !drop.has(d));
	return new Tensor(t.data, keep.map((d) => t.shape[d]), keep.map((d) => t.strides[d]), t.offset, t.writable);
}

/** NumPy broadcasting rule: align from the right; each pair of extents must be equal or one of them 1. */
export function broadcastShapes(...shapes: Shape[]): number[] {
	const nd = Math.max(0, ...shapes.map((s) => s.length));
	const out = new Array<number>(nd).fill(1);
	for (const s of shapes)
		for (let i = 0; i < s.length; i++) {
			const d = nd - s.length + i;
			if (s[i] === out[d] || s[i] === 1) continue;
			if (out[d] !== 1) throw new ShapeError(`shapes ${shapes.map(fmtShape).join(', ')} do not broadcast`);
			out[d] = s[i];
		}
	return out;
}

/** Read-only view of `t` with `shape`: stretched axes get stride 0 (one element seen many times, no copy). */
export function broadcastTo(t: Tensor, shape: Shape): Tensor {
	checkShape(shape);
	const lead = shape.length - t.ndim;
	if (lead < 0) throw new ShapeError(`cannot broadcast ${fmtShape(t.shape)} to lower rank ${fmtShape(shape)}`);
	const strides = shape.map((n, d) => {
		if (d < lead) return 0;
		const m = t.shape[d - lead];
		if (m === n) return t.strides[d - lead];
		if (m === 1) return 0;
		throw new ShapeError(`cannot broadcast ${fmtShape(t.shape)} to ${fmtShape(shape)}`);
	});
	return new Tensor(t.data, shape, strides, t.offset, false);
}

/**
 * Row-major copy, or `t` itself when it already is contiguous (NumPy `ascontiguousarray`): callers that need a
 * private buffer must not rely on this to copy. The copy is writable even if `t` was a broadcast view.
 */
export function contiguous(t: Tensor): Tensor {
	if (isContiguous(t) && t.writable) return t;
	const out = zeros(t.shape, t.store);
	const r = offsets(t.shape, t.strides);
	for (let k = 0; k < r.length; k++) out.data[k] = t.data[t.offset + r[k]];
	return out;
}

// ---- elementwise (broadcasting) ----

export function map(t: Tensor, f: (x: number) => number): Tensor {
	const out = zeros(t.shape, t.store);
	const r = offsets(t.shape, t.strides);
	for (let k = 0; k < r.length; k++) out.data[k] = f(t.data[t.offset + r[k]]);
	return out;
}

/**
 * Elementwise binary op with NumPy broadcasting. A plain number is a weak scalar (it does not promote f32 to f64,
 * as in NumPy 2). f32 results are exact: `f` runs in double and the f32 store rounds once, which for + - * / equals
 * a correctly rounded float32 operation (double carries more than 2 * 24 + 2 mantissa bits).
 */
export function zip(a: Tensor | number, b: Tensor | number, f: (x: number, y: number) => number): Tensor {
	const A = typeof a === 'number' ? full([], a) : a;
	const B = typeof b === 'number' ? full([], b) : b;
	const stores = [a, b].flatMap((x) => (typeof x === 'number' ? [] : [x.store]));
	const shape = broadcastShapes(A.shape, B.shape);
	const out = zeros(shape, promote(...stores));
	const ra = offsets(shape, broadcastTo(A, shape).strides);
	const rb = offsets(shape, broadcastTo(B, shape).strides);
	for (let k = 0; k < out.data.length; k++) out.data[k] = f(A.data[A.offset + ra[k]], B.data[B.offset + rb[k]]);
	return out;
}

export const add = (a: Tensor | number, b: Tensor | number): Tensor => zip(a, b, (x, y) => x + y);
export const sub = (a: Tensor | number, b: Tensor | number): Tensor => zip(a, b, (x, y) => x - y);
export const mul = (a: Tensor | number, b: Tensor | number): Tensor => zip(a, b, (x, y) => x * y);
export const div = (a: Tensor | number, b: Tensor | number): Tensor => zip(a, b, (x, y) => x / y);

// ---- reductions ----

export interface ReduceOpts extends AccOpts {
	/** Axis or axes to reduce (negative allowed); omitted = all. */
	axis?: number | readonly number[];
	keepdims?: boolean;
}

/**
 * Shared reduction loop: for every kept cell, `f` gets the reduced elements in row-major order of the reduced axes
 * (the order a sequential sum adds them). Output keeps the input storage dtype, like NumPy.
 */
function reduce(t: Tensor, o: ReduceOpts, f: (buf: Float64Array, n: number) => number): Tensor {
	const axes = o.axis === undefined ? t.shape.map((_, d) => d) : [o.axis].flat().map((a) => axisOf(a, t.ndim));
	if (new Set(axes).size !== axes.length) throw new ShapeError(`duplicate axis in [${axes.join(', ')}]`);
	const kept = t.shape.map((_, d) => d).filter((d) => !axes.includes(d));
	const red = t.shape.map((_, d) => d).filter((d) => axes.includes(d));
	const outer = offsets(kept.map((d) => t.shape[d]), kept.map((d) => t.strides[d]));
	const inner = offsets(red.map((d) => t.shape[d]), red.map((d) => t.strides[d]));
	const shape = o.keepdims ? t.shape.map((n, d) => (axes.includes(d) ? 1 : n)) : kept.map((d) => t.shape[d]);
	const out = zeros(shape, t.store);
	const buf = new Float64Array(inner.length);
	for (let c = 0; c < outer.length; c++) {
		const base = t.offset + outer[c];
		for (let j = 0; j < inner.length; j++) buf[j] = t.data[base + inner[j]];
		out.data[c] = f(buf, inner.length);
	}
	return out;
}

/** Sums `buf` in place-rounded form: operands rounded by `ar`, then accumulated in order `ord` (see dtype.ts). */
function summer(o: AccOpts, n: number): (buf: Float64Array, len: number) => number {
	const ar = arith(o.arith);
	const ord = order(o.order);
	const scratch = new Float64Array(n);
	return (buf, len) => {
		for (let j = 0; j < len; j++) buf[j] = ar.round(buf[j]);
		return accumulate(buf, len, ar, ord, scratch);
	};
}

/** Sum with the given arithmetic and summation order (default fp64, sequential; see dtype.ts). Empty sum = 0. */
export function sum(t: Tensor, o: ReduceOpts = {}): Tensor {
	return reduce(t, o, summer(o, t.size));
}

/** Sum / count, the division rounded by the same arithmetic. An empty mean is a ShapeError (NumPy returns NaN + warning). */
export function mean(t: Tensor, o: ReduceOpts = {}): Tensor {
	const ar = arith(o.arith);
	const s = summer(o, t.size);
	return reduce(t, o, (buf, n) => {
		if (n === 0) throw new ShapeError(`mean over an empty axis of ${fmtShape(t.shape)}`);
		return ar.round(s(buf, n) / n);
	});
}

/** Maximum; NaN propagates (as in NumPy). An empty reduction is a ShapeError: max of nothing has no value. */
export function max(t: Tensor, o: ReduceOpts = {}): Tensor {
	return reduce(t, o, (buf, n) => {
		if (n === 0) throw new ShapeError(`max over an empty axis of ${fmtShape(t.shape)}`);
		let m = -Infinity;
		for (let j = 0; j < n; j++) {
			if (Number.isNaN(buf[j])) return NaN;
			if (buf[j] > m) m = buf[j];
		}
		return m;
	});
}
