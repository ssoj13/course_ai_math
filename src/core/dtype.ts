/**
 * Storage dtypes and accumulation semantics of the tensor core.
 *
 * Two separate questions, kept apart on purpose:
 * - **Storage** (`Store`): the typed array that holds the numbers. `f32` storage rounds on every write (the typed
 *   array does it), exactly like a NumPy float32 array.
 * - **Arithmetic** (`Arith`) + **order** (`Order`): how a contraction (sum, dot, matmul, einsum) multiplies and adds.
 *   The default is fp64 arithmetic, sequential order, whatever the storage: the lessons want the exact value first.
 *   `fp32` imitates a float32 machine: `Math.fround` on every operand, every product and every partial sum.
 *   Lower precisions (fp16, bf16, int8; Branch Q) plug in as an `Arith` object whose `round` maps a double to the
 *   nearest representable value; the ids are reserved and throw until Branch Q supplies them.
 *
 * Why the order matters: floating-point addition is not associative, so `sequential` (((p0+p1)+p2)+p3) and
 * `pairwise` ((p0+p1)+(p2+p3)), the tree BLAS/GPU reductions use, can differ in the last digits under fp32.
 */
import { DtypeError } from './errors';

export type Store = 'f64' | 'f32';
export type Data = Float64Array | Float32Array;

export function alloc(st: Store, n: number): Data {
	return st === 'f32' ? new Float32Array(n) : new Float64Array(n);
}

export const storeOf = (d: Data): Store => (d instanceof Float32Array ? 'f32' : 'f64');

/** NumPy promotion for float storages: any f64 operand makes the result f64. */
export const promote = (...s: Store[]): Store => (s.includes('f64') || s.length === 0 ? 'f64' : 'f32');

/** Arithmetic of a contraction: `round` is applied to each operand, each product and each partial sum. */
export interface Arith {
	readonly id: string;
	readonly round: (x: number) => number;
}

export const FP64: Arith = { id: 'fp64', round: (x) => x };
export const FP32: Arith = { id: 'fp32', round: Math.fround };

export type ArithId = 'fp64' | 'fp32' | 'fp16' | 'bf16' | 'int8';

export type Order = 'sequential' | 'pairwise';

/** Options shared by every accumulating operation. */
export interface AccOpts {
	arith?: ArithId | Arith;
	order?: Order;
}

/** Resolves an id or passes a custom `Arith` through. Reserved Branch Q ids throw instead of falling back to fp64. */
export function arith(a: ArithId | Arith = 'fp64'): Arith {
	if (typeof a === 'object') return a;
	if (a === 'fp64') return FP64;
	if (a === 'fp32') return FP32;
	if (a === 'fp16' || a === 'bf16' || a === 'int8')
		throw new DtypeError(`arith "${a}" is reserved for Branch Q: pass an Arith object with its round()`);
	throw new DtypeError(`unknown arith "${String(a)}"`);
}

export function order(o: Order = 'sequential'): Order {
	if (o !== 'sequential' && o !== 'pairwise') throw new DtypeError(`unknown summation order "${String(o)}"`);
	return o;
}

/**
 * Sums `p[0..n)` (already rounded products) in the given order. `scratch` (length >= n) is overwritten by the
 * pairwise tree so a caller looping over many cells allocates nothing; omit it for one-off sums.
 * Empty sum = 0, like NumPy.
 */
export function accumulate(p: ArrayLike<number>, n: number, ar: Arith, ord: Order, scratch?: Float64Array): number {
	if (n === 0) return 0;
	if (ord === 'sequential') {
		let acc = p[0];
		for (let i = 1; i < n; i++) acc = ar.round(acc + p[i]);
		return acc;
	}
	const buf = scratch ?? new Float64Array(n);
	for (let i = 0; i < n; i++) buf[i] = p[i];
	// Bottom-up tree of adjacent pairs; an odd last node is carried to the next level unchanged (see `sumSteps`).
	for (let len = n; len > 1; len = (len + 1) >> 1) {
		const half = len >> 1;
		for (let j = 0; j < half; j++) buf[j] = ar.round(buf[2 * j] + buf[2 * j + 1]);
		if (len & 1) buf[half] = buf[len - 1];
	}
	return buf[0];
}

/** One addition of a summation: node `out` = node `a` + node `b`. Nodes 0..n-1 are the products themselves. */
export interface Merge {
	a: number;
	b: number;
	out: number;
	/** Tree level (0 = first additions). Sequential sums have one merge per level. */
	level: number;
}

/**
 * The exact additions `accumulate` performs, as a node list for the tile animation: drop order, which stacks
 * merge, and the root node holding the result (`root`; with n = 1 it is the only product, n = 0 has no root).
 */
export function sumSteps(n: number, ord: Order): { merges: Merge[]; root: number } {
	const merges: Merge[] = [];
	if (n === 0) return { merges, root: -1 };
	let next = n;
	if (ord === 'sequential') {
		let acc = 0;
		for (let i = 1; i < n; i++) {
			merges.push({ a: acc, b: i, out: next, level: i - 1 });
			acc = next++;
		}
		return { merges, root: acc };
	}
	let nodes = Array.from({ length: n }, (_, i) => i);
	for (let level = 0; nodes.length > 1; level++) {
		const up: number[] = [];
		for (let j = 0; j + 1 < nodes.length; j += 2) {
			merges.push({ a: nodes[j], b: nodes[j + 1], out: next, level });
			up.push(next++);
		}
		if (nodes.length & 1) up.push(nodes[nodes.length - 1]);
		nodes = up;
	}
	return { merges, root: nodes[0] };
}
