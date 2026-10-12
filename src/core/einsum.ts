/**
 * Einsum over strides, with a typed plan that scenes reuse for layout.
 *
 * Grammar (course subset of NumPy, enforced with `SpecError` carrying the bad token and its position):
 *   op      = operand ("," operand)* "->" output
 *   operand = letter*        output = letter*        letter = a..z
 * Explicit output only (no implicit mode), no ellipsis, no spaces, no letter twice in one operand (no traces /
 * diagonals) or in the output, every output letter must occur in an operand. An operand may be empty (a scalar).
 * Kept letters = the output letters; summed letters = all other operand letters, in order of first appearance
 * (they flatten into ONE row-major shaft, last letter innermost: the DotUnit's dot-product axis).
 *
 * One plan drives everything: `einsum` (values), `products` (the tiles of one cell) and `explain` (the pairs and the
 * order of additions). They share `cellProducts` and `accumulate`, so the scene, the tables and the result are
 * the same floats by construction.
 */
import { accumulate, arith, order, promote, sumSteps, type AccOpts, type Merge } from './dtype';
import { ExtentError, IndexError, ShapeError, SpecError, type SpecReason } from './errors';
import { broadcastShapes, broadcastTo, expandDims, offsets, sizeOf, squeeze, zeros, type Shape, type Tensor } from './tensor';

export interface Parsed {
	readonly op: string;
	/** Letters of each operand, in its axis order. */
	readonly inputs: readonly string[];
	/** Output letters = kept indices, in output axis order. */
	readonly output: string;
	readonly kept: readonly string[];
	/** Contracted letters in order of first appearance; flattened row-major into one shaft. */
	readonly summed: readonly string[];
}

export interface Plan extends Parsed {
	/** Extent of every letter. */
	readonly dims: Readonly<Record<string, number>>;
	readonly shapes: readonly Shape[];
	readonly outShape: Shape;
	/** Extents of the summed letters; `sumLen` = their product = shaft length (1 for an outer product). */
	readonly sumShape: Shape;
	readonly sumLen: number;
	/** Number of output cells. */
	readonly cells: number;
}

/** Parses an op string. `arity` (e.g. 2 for the DotUnit scenes) additionally fixes the operand count. */
export function parseEinsum(op: string, arity?: number): Parsed {
	const bad = (reason: SpecReason, msg: string, pos: number, token = op[pos] ?? ''): never => {
		throw new SpecError(msg, op, token, pos, reason);
	};
	const arrow = op.indexOf('->');
	if (arrow < 0) bad('noArrow', 'missing "->" (implicit output is not supported)', op.length, '');
	if (op.indexOf('->', arrow + 1) >= 0) bad('arrows', 'more than one "->"', op.indexOf('->', arrow + 1), '->');
	for (let i = 0; i < op.length; i++) {
		if (i === arrow || i === arrow + 1) continue;
		const c = op[i];
		if (c >= 'a' && c <= 'z') continue;
		if (c === ',' && i < arrow) continue;
		if (c === '.') bad('ellipsis', 'ellipsis is not supported', i);
		if (c === ',') bad('outComma', '"," in the output', i);
		if (c.trim() === '') bad('space', 'whitespace is not allowed', i);
		bad('char', 'invalid character (letters a-z only)', i);
	}
	const lhs = op.slice(0, arrow);
	const output = op.slice(arrow + 2);
	const inputs = lhs.split(',');
	if (arity !== undefined && inputs.length !== arity) bad('arity', `expected ${arity} operands, got ${inputs.length}`, 0, lhs);
	let at = 0;
	for (const term of inputs) {
		for (let i = 0; i < term.length; i++)
			if (term.indexOf(term[i]) !== i) bad('repeatIn', 'index repeated inside one operand (traces are not supported)', at + i);
		at += term.length + 1;
	}
	for (let i = 0; i < output.length; i++) {
		if (output.indexOf(output[i]) !== i) bad('repeatOut', 'index repeated in the output', arrow + 2 + i);
		if (!lhs.includes(output[i])) bad('unknownOut', 'output index does not occur in any operand', arrow + 2 + i);
	}
	const summed: string[] = [];
	for (const c of lhs) if (c !== ',' && !output.includes(c) && !summed.includes(c)) summed.push(c);
	return { op, inputs, output, kept: [...output], summed };
}

/**
 * Binds extents to a parsed op. `shapes` is either the operand shapes (each letter must have one extent everywhere:
 * that equality IS the contraction) or a letter -> extent record (exactly the op's letters).
 */
export function planEinsum(spec: string | Parsed, shapes: readonly Shape[] | Readonly<Record<string, number>>): Plan {
	const p = typeof spec === 'string' ? parseEinsum(spec) : spec;
	const dims: Record<string, number> = {};
	const letters = new Set(p.inputs.join(''));
	const ext = (l: string, n: number, where: string): void => {
		if (!Number.isInteger(n) || n < 0) throw new ShapeError(`bad extent ${n} for index "${l}" ${where}`);
		if (l in dims && dims[l] !== n)
			throw new ExtentError(`index "${l}" has extent ${dims[l]} and ${n} ${where} in "${p.op}"`, l, dims[l], n);
		dims[l] = n;
	};
	if (Array.isArray(shapes)) {
		const ss = shapes as readonly Shape[];
		if (ss.length !== p.inputs.length)
			throw new ShapeError(`"${p.op}" takes ${p.inputs.length} operands, got ${ss.length}`);
		p.inputs.forEach((term, o) => {
			if (ss[o].length !== term.length)
				throw new ShapeError(`operand ${o} "${term}" needs rank ${term.length}, got shape (${ss[o].join(', ')})`);
			[...term].forEach((l, d) => ext(l, ss[o][d], `(operand ${o})`));
		});
	} else {
		const rec = shapes as Readonly<Record<string, number>>;
		for (const l of Object.keys(rec)) if (!letters.has(l)) throw new ShapeError(`extent given for unknown index "${l}"`);
		for (const l of letters) {
			if (!(l in rec)) throw new ShapeError(`missing extent for index "${l}"`);
			ext(l, rec[l], '');
		}
	}
	const outShape = p.kept.map((l) => dims[l]);
	const sumShape = p.summed.map((l) => dims[l]);
	return {
		...p,
		dims,
		shapes: p.inputs.map((t) => [...t].map((l) => dims[l])),
		outShape,
		sumShape,
		sumLen: sizeOf(sumShape),
		cells: sizeOf(outShape),
	};
}

/** Per operand: base offsets over the output cells and relative offsets along the shaft. */
interface Kernel {
	base: Int32Array[];
	rel: Int32Array[];
}

function kernel(p: Plan, ts: readonly Tensor[]): Kernel {
	if (ts.length !== p.inputs.length) throw new ShapeError(`"${p.op}" takes ${p.inputs.length} operands, got ${ts.length}`);
	ts.forEach((t, o) => {
		if (t.ndim !== p.shapes[o].length || t.shape.some((n, d) => n !== p.shapes[o][d]))
			throw new ShapeError(`operand ${o} has shape (${t.shape.join(', ')}), plan expects (${p.shapes[o].join(', ')})`);
	});
	// A letter missing from an operand has stride 0 there: that operand is reused across that index.
	const strideOf = (o: number, l: string): number => {
		const d = p.inputs[o].indexOf(l);
		return d < 0 ? 0 : ts[o].strides[d];
	};
	return {
		base: ts.map((t, o) => offsets(p.outShape, p.kept.map((l) => strideOf(o, l))).map((r) => r + t.offset)),
		rel: ts.map((_, o) => offsets(p.sumShape, p.summed.map((l) => strideOf(o, l)))),
	};
}

/** Fills `out[0..sumLen)` with the products of output cell `c` in shaft order; operands left to right, each rounded. */
function cellProducts(p: Plan, ts: readonly Tensor[], k: Kernel, c: number, round: (x: number) => number, out: Float64Array): void {
	for (let j = 0; j < p.sumLen; j++) {
		let v = round(ts[0].data[k.base[0][c] + k.rel[0][j]]);
		for (let o = 1; o < ts.length; o++) v = round(v * round(ts[o].data[k.base[o][c] + k.rel[o][j]]));
		out[j] = v;
	}
}

function cellIndex(p: Plan, cell: readonly number[]): number {
	if (cell.length !== p.outShape.length) throw new IndexError(`cell of rank ${cell.length} for output (${p.outShape.join(', ')})`);
	let c = 0;
	cell.forEach((i, d) => {
		const n = p.outShape[d];
		if (!Number.isInteger(i) || i < 0 || i >= n) throw new IndexError(`cell index ${i} out of range for "${p.kept[d]}" (${n})`);
		c = c * n + i;
	});
	return c;
}

const asPlan = (spec: string | Plan, ts: readonly Tensor[]): Plan =>
	typeof spec === 'string' ? planEinsum(spec, ts.map((t) => t.shape)) : spec;

/**
 * `einsum(op | plan, ...tensors[, opts])`. Reads operands through their strides (transposed or broadcast views
 * are never materialized). Result storage follows NumPy promotion (f32 only if every operand is f32); the
 * contraction runs in `opts.arith` (default fp64) and `opts.order` (default sequential).
 */
export function einsum(spec: string | Plan, ...args: [...Tensor[]] | [...Tensor[], AccOpts]): Tensor {
	const last = args[args.length - 1];
	const hasOpts = last !== undefined && !('data' in last);
	const ts = (hasOpts ? args.slice(0, -1) : args) as Tensor[];
	const o = (hasOpts ? last : {}) as AccOpts;
	const p = asPlan(spec, ts);
	const k = kernel(p, ts);
	const ar = arith(o.arith);
	const ord = order(o.order);
	const out = zeros(p.outShape, promote(...ts.map((t) => t.store)));
	const prods = new Float64Array(p.sumLen);
	const scratch = new Float64Array(p.sumLen);
	for (let c = 0; c < p.cells; c++) {
		cellProducts(p, ts, k, c, ar.round, prods);
		out.data[c] = accumulate(prods, p.sumLen, ar, ord, scratch);
	}
	return out;
}

/** The products of one output cell in shaft order (the DotUnit tiles). Their `accumulate` is the einsum value. */
export function products(plan: Plan, ts: readonly Tensor[], cell: readonly number[], o: AccOpts = {}): Float64Array {
	const c = cellIndex(plan, cell);
	const out = new Float64Array(plan.sumLen);
	cellProducts(plan, ts, kernel(plan, ts), c, arith(o.arith).round, out);
	return out;
}

/** One multiplied tuple of an output cell. */
export interface Term {
	/** Position along the shaft (row-major over `plan.summed`). */
	k: number;
	/** Values of the summed letters at this position, in `plan.summed` order. */
	sum: number[];
	/** Full index into each operand (its own letter order). */
	at: number[][];
	/** Operand values and their product (present when tensors were given). */
	vals?: number[];
	prod?: number;
}

export interface Explain {
	cell: number[];
	terms: Term[];
	/** Additions in execution order; node ids < terms.length are products (see dtype.sumSteps). */
	merges: (Merge & { value?: number })[];
	/** Node holding the result (-1 for an empty shaft, whose value is 0). */
	root: number;
	value?: number;
}

/**
 * For one output cell: exactly which elements are multiplied, in shaft order, and the order in which the products
 * are added. With tensors it also carries every product and partial sum, so the tile animation can show the running
 * value; `value` equals the corresponding `einsum` element bit for bit (same kernel, same accumulation).
 */
export function explain(plan: Plan, cell: readonly number[], ts?: readonly Tensor[], o: AccOpts = {}): Explain {
	cellIndex(plan, cell); // validates the cell before any work
	const ar = arith(o.arith);
	const prods = ts ? products(plan, ts, cell, o) : undefined;
	const terms: Term[] = [];
	const sidx = new Array<number>(plan.summed.length).fill(0);
	for (let j = 0; j < plan.sumLen; j++) {
		const val = (l: string): number => {
			const s = plan.summed.indexOf(l);
			return s < 0 ? cell[plan.kept.indexOf(l)] : sidx[s];
		};
		const at = plan.inputs.map((term) => [...term].map(val));
		const t: Term = { k: j, sum: [...sidx], at };
		if (ts && prods) {
			t.vals = ts.map((x, i) => x.get(at[i]));
			t.prod = prods[j];
		}
		terms.push(t);
		for (let d = sidx.length - 1; d >= 0 && ++sidx[d] === plan.sumShape[d]; d--) sidx[d] = 0;
	}
	const { merges, root } = sumSteps(plan.sumLen, order(o.order));
	const ex: Explain = { cell: [...cell], terms, merges, root };
	if (prods) {
		const node = [...prods];
		for (const m of ex.merges) node[m.out] = m.value = ar.round(node[m.a] + node[m.b]);
		ex.value = root < 0 ? 0 : node[root];
	}
	return ex;
}

// ---- thin wrappers ----

/** Batch letters for matmul: anything but the i, j, k used for the matrix axes. */
const BATCH = 'abcdefghlmnopqrstuvwxyz';

/**
 * NumPy `matmul`: 1-D operands are promoted (a -> row, b -> column) and the promoted axis removed again; leading
 * batch axes broadcast. Implemented as einsum "…ij,…jk->…ik" over broadcast views, so batched matmul and the
 * DotUnit share one kernel and one accumulation semantics.
 */
export function matmul(a: Tensor, b: Tensor, o: AccOpts = {}): Tensor {
	if (a.ndim === 0 || b.ndim === 0) throw new ShapeError('matmul of a 0-d tensor: use mul for scalars');
	const A = a.ndim === 1 ? expandDims(a, 0) : a;
	const B = b.ndim === 1 ? expandDims(b, -1) : b;
	const [m, ka] = A.shape.slice(-2);
	const [kb, n] = B.shape.slice(-2);
	if (ka !== kb)
		throw new ShapeError(`matmul: inner extents differ, (${a.shape.join(', ')}) @ (${b.shape.join(', ')})`);
	const batch = broadcastShapes(A.shape.slice(0, -2), B.shape.slice(0, -2));
	if (batch.length > BATCH.length) throw new ShapeError(`matmul: ${batch.length} batch axes is more than ${BATCH.length}`);
	const bt = BATCH.slice(0, batch.length);
	let r = einsum(
		`${bt}ij,${bt}jk->${bt}ik`,
		broadcastTo(A, [...batch, m, ka]),
		broadcastTo(B, [...batch, kb, n]),
		o,
	);
	if (b.ndim === 1) r = squeeze(r, -1);
	if (a.ndim === 1) r = squeeze(r, b.ndim === 1 ? -1 : -2);
	return r;
}

/** Dot product of two 1-D tensors as a number (for N-D use `matmul` or `einsum`; NumPy's N-D `dot` is not matmul). */
export function dot(a: Tensor, b: Tensor, o: AccOpts = {}): number {
	if (a.ndim !== 1 || b.ndim !== 1)
		throw new ShapeError(`dot needs two 1-D tensors, got (${a.shape.join(', ')}) and (${b.shape.join(', ')})`);
	return einsum('i,i->', a, b, o).item();
}
