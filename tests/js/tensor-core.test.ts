import { describe, expect, test } from 'bun:test';
import fx from '../fixtures/tensor/core.json';
import {
	add, arange, broadcastShapes, broadcastTo, contiguous, CoreError, div, dot, DtypeError, einsum, expandDims, explain,
	flat, FP32, IndexError, isContiguous, matmul, max, mean, mul, mulberry32, parseEinsum, planEinsum, products, rand,
	reshape, ShapeError, slice, SpecError, squeeze, sub, sum, sumSteps, Tensor, tensor, toNested, transpose, zeros,
	type Order, type SliceSpec,
} from '../../src/core';

type Arr = { shape: number[]; data: number[] };
const T = (a: Arr): Tensor => new Tensor(Float64Array.from(a.data), a.shape);

/** NumPy's einsum/matmul may add in another order (SIMD), so floats agree to rounding, integers exactly. */
function close(got: Tensor, want: Arr): void {
	expect([...got.shape]).toEqual(want.shape);
	const g = flat(got);
	want.data.forEach((w, i) => {
		if (Math.abs(g[i] - w) > 1e-12 * Math.max(1, Math.abs(w))) throw new Error(`element ${i}: ${g[i]} vs ${w}`);
	});
}

type Op = [string, unknown];
function apply(t: Tensor, [kind, arg]: Op): Tensor {
	switch (kind) {
		case 'transpose': return transpose(t, (arg as number[] | null) ?? undefined);
		case 'slice': return slice(t, arg as SliceSpec[]);
		case 'reshape': return reshape(t, arg as number[]);
		case 'expand': return expandDims(t, arg as number);
		case 'squeeze': return squeeze(t, (arg as number | null) ?? undefined);
		case 'broadcast': return broadcastTo(t, arg as number[]);
	}
	throw new Error(kind);
}
/** JSON-typed dims (a union with optional letters) as the plain record the core takes. */
const dimsOf = (d: object): Record<string, number> =>
	Object.fromEntries(Object.entries(d).filter((e): e is [string, number] => typeof e[1] === 'number'));
const base = (shape: number[]): Tensor => arange(shape.reduce((p, n) => p * n, 1), shape);

describe('construction and indexing', () => {
	test('nested arrays, get/set, negative indices, 0-d', () => {
		const t = tensor([[1, 2, 3], [4, 5, 6]]);
		expect([...t.shape]).toEqual([2, 3]);
		expect([...t.strides]).toEqual([3, 1]);
		expect(t.get([1, -1])).toBe(6);
		t.set([0, 1], 9);
		expect(toNested(t)).toEqual([[1, 9, 3], [4, 5, 6]]);
		expect(tensor(7).item()).toBe(7);
		expect(tensor([[1, 2]], 'f32').store).toBe('f32');
	});
	test('bounds and ragged input are typed errors', () => {
		const t = zeros([2, 3]);
		expect(() => t.get([2, 0])).toThrow(IndexError);
		expect(() => t.get([0])).toThrow(IndexError);
		expect(() => t.get([0, 1.5])).toThrow(IndexError);
		expect(() => tensor([[1, 2], [3]])).toThrow(ShapeError);
		expect(() => zeros([2, -1])).toThrow(ShapeError);
		// A view that would read past the buffer is refused at construction.
		expect(() => new Tensor(new Float64Array(5), [2, 3])).toThrow(ShapeError);
		expect(() => new Tensor(new Float64Array(6), [2, 3], [3, 1], 1)).toThrow(ShapeError);
		expect(new IndexError('x')).toBeInstanceOf(CoreError);
	});
	test('f32 storage rounds on write', () => {
		const t = zeros([1], 'f32');
		t.set([0], 0.1);
		expect(t.get([0])).toBe(Math.fround(0.1));
	});
});

describe('mulberry32 / rand (Python port)', () => {
	test('bit-exact stream', () => {
		for (const c of fx.mulberry32) {
			const r = mulberry32(c.seed);
			expect(c.values.map(() => r())).toEqual(c.values);
		}
	});
	test('quantized seeded tensor', () => {
		const c = fx.rand;
		const t = rand(c.shape, { seed: c.seed, range: c.range as [number, number], step: c.step });
		expect(flat(t)).toEqual(c.data);
		expect(flat(rand([2, 3], { seed: 7 }))).toEqual(flat(rand([2, 3], { seed: 7 })));
	});
});

describe('views (NumPy shape / strides / offset / values)', () => {
	test.each(fx.views.map((v) => [JSON.stringify(v.ops), v] as const))('%s', (_, v) => {
		const t = (v.ops as Op[]).reduce(apply, base(v.base));
		expect([...t.shape]).toEqual(v.shape);
		// A size-1 axis is never stepped, so NumPy and we may store any stride there.
		v.shape.forEach((n, d) => n !== 1 && expect(t.strides[d]).toBe(v.strides[d]));
		if (t.size > 0) expect(t.offset).toBe(v.offset);
		expect(flat(t)).toEqual(v.data);
	});
	test.each(fx.invalidViews.map((v) => [JSON.stringify(v.ops), v] as const))('rejects %s', (_, v) => {
		expect(v.numpy).toBe('rejects');
		expect(() => (v.ops as Op[]).reduce(apply, base(v.base))).toThrow(CoreError);
	});
	test('aliasing: views share the buffer, contiguous() copies only when needed', () => {
		const a = arange(6, [2, 3]);
		const at = transpose(a);
		const row = slice(a, [1]);
		at.set([2, 1], -1); // = a[1, 2]
		expect(a.get([1, 2])).toBe(-1);
		expect(row.get([2])).toBe(-1);
		expect(at.data).toBe(a.data);
		expect(isContiguous(at)).toBe(false);
		const c = contiguous(at);
		expect(c.data).not.toBe(a.data);
		expect(toNested(c)).toEqual(toNested(at));
		expect(contiguous(a)).toBe(a);
		expect(reshape(a, [3, 2]).data).toBe(a.data);
	});
	test('reshape of a non-contiguous view must go through contiguous()', () => {
		const at = transpose(arange(6, [2, 3]));
		expect(() => reshape(at, [6])).toThrow(ShapeError);
		expect(flat(reshape(contiguous(at), [6]))).toEqual([0, 3, 1, 4, 2, 5]);
	});
	test('broadcast views are read-only and stride 0', () => {
		const b = broadcastTo(tensor([1, 2]), [3, 2]);
		expect([...b.strides]).toEqual([0, 1]);
		expect(() => b.set([0, 0], 5)).toThrow(IndexError);
		expect(contiguous(b).writable).toBe(true);
	});
});

describe('elementwise broadcasting', () => {
	const f = { add, sub, mul, div } as const;
	test.each(fx.broadcast.map((c) => [`${c.op} ${JSON.stringify(c.a.shape)} ${JSON.stringify(c.b.shape)}`, c] as const))('%s', (_, c) => {
		const run = () => f[c.op as keyof typeof f](T(c.a), T(c.b));
		if ('error' in c) expect(run).toThrow(ShapeError);
		else close(run(), c.out as Arr);
	});
	test('weak scalars and f32 promotion', () => {
		const x = tensor([1, 2], 'f32');
		expect(add(x, 0.1).store).toBe('f32');
		expect(add(x, tensor([0.1])).store).toBe('f64');
		expect(add(x, 0.1).get([0])).toBe(Math.fround(1.1));
		expect(broadcastShapes([2, 1, 3], [4, 1])).toEqual([2, 4, 3]);
	});
});

describe('reductions', () => {
	const x = T(fx.reduce.x);
	const fns = { sum, mean, max } as const;
	test.each(fx.reduce.cases.map((c) => [`${c.fn} axis=${JSON.stringify(c.axis)} keep=${c.keepdims} T=${c.transpose}`, c] as const))('%s', (_, c) => {
		const src = c.transpose ? transpose(x) : x;
		close(fns[c.fn as keyof typeof fns](src, { axis: c.axis ?? undefined, keepdims: c.keepdims }), c.out);
	});
	test('empty and bad axes', () => {
		expect(sum(zeros([0])).item()).toBe(0);
		expect(() => max(zeros([0]))).toThrow(ShapeError);
		expect(() => mean(zeros([2, 0]), { axis: 1 })).toThrow(ShapeError);
		expect(() => sum(x, { axis: 3 })).toThrow(ShapeError);
		expect(() => sum(x, { axis: [0, -3] })).toThrow(ShapeError);
		expect(max(tensor([1, NaN, 3])).item()).toBeNaN();
	});
});

describe('einsum', () => {
	test.each(fx.einsum.map((c) => [`${c.kind} ${c.op}`, c] as const))('%s', (_, c) => {
		let ts = c.inputs.map(T);
		if ('transpose' in c) ts = ts.map((t) => transpose(t));
		const got = einsum(c.op, ...ts);
		close(got, c.out);
		if (c.kind === 'preset-int') expect(flat(got)).toEqual(c.out.data);
		// The plan from a letter -> extent record equals the one inferred from the operands.
		const p = planEinsum(c.op, dimsOf(c.dims));
		expect(p.outShape).toEqual(c.out.shape);
		// products of every cell add up to the einsum value, and explain() replays the same additions.
		const out = flat(got);
		for (let k = 0; k < p.cells; k++) {
			const cell: number[] = [];
			for (let d = p.outShape.length - 1, r = k; d >= 0; d--) {
				cell.unshift(r % p.outShape[d]);
				r = Math.floor(r / p.outShape[d]);
			}
			const ex = explain(p, cell, ts);
			expect(ex.value).toBe(out[k]);
			expect(ex.terms.map((t) => t.prod)).toEqual([...products(p, ts, cell)]);
		}
	});
	test.each(fx.invalidEinsum.map((c) => [c.op, c] as const))('rejects %s', (_, c) => {
		const ts = c.shapes.map((s) => zeros(s));
		expect(() => einsum(c.op, ...ts)).toThrow(c.error === 'spec' ? SpecError : ShapeError);
	});
	test('spec errors carry the bad token and position', () => {
		const e = (op: string): SpecError => {
			try {
				parseEinsum(op);
			} catch (x) {
				return x as SpecError;
			}
			throw new Error('no error');
		};
		expect(e('ii->i')).toMatchObject({ token: 'i', pos: 1, code: 'spec' });
		expect(e('ij,jk->iz')).toMatchObject({ token: 'z', pos: 8 });
		expect(e('...i->i')).toMatchObject({ token: '.', pos: 0 });
		expect(e('ij,jk')).toMatchObject({ token: '', pos: 5 });
		expect(() => parseEinsum('i,j,k->ijk', 2)).toThrow(SpecError);
		expect(() => planEinsum('ij,jk->ik', { i: 2, j: 3 })).toThrow(ShapeError);
		expect(() => planEinsum('ij,jk->ik', { i: 2, j: 3, k: 1, z: 4 })).toThrow(ShapeError);
	});
	test('the plan: kept and summed letters for the five DotUnit presets', () => {
		const p = planEinsum('bhqd,bhkd->bhqk', { b: 2, h: 3, q: 4, k: 5, d: 6 });
		expect(p.kept).toEqual(['b', 'h', 'q', 'k']);
		expect(p.summed).toEqual(['d']);
		expect(p.outShape).toEqual([2, 3, 4, 5]);
		expect(p.sumLen).toBe(6);
		expect(planEinsum('i,i->', { i: 3 })).toMatchObject({ kept: [], summed: ['i'], outShape: [], cells: 1 });
		expect(planEinsum('ij,j->i', { i: 3, j: 4 })).toMatchObject({ kept: ['i'], summed: ['j'] });
		// Two summed letters: one shaft, row-major, last letter innermost.
		const two = planEinsum('ijk,jk->i', { i: 2, j: 3, k: 4 });
		expect(two).toMatchObject({ summed: ['j', 'k'], sumShape: [3, 4], sumLen: 12 });
		// Outer product: no summed letter, a shaft of one tile.
		expect(planEinsum('i,j->ij', { i: 2, j: 3 })).toMatchObject({ summed: [], sumLen: 1 });
	});
	test('explain lists the exact pairs and the order of additions', () => {
		const a = tensor([[1, 2, 3], [4, 5, 6]]);
		const b = tensor([[7, 8], [9, 10], [11, 12]]);
		const p = planEinsum('ij,jk->ik', [a.shape, b.shape]);
		const ex = explain(p, [1, 0], [a, b]);
		expect(ex.terms.map((t) => t.at)).toEqual([[[1, 0], [0, 0]], [[1, 1], [1, 0]], [[1, 2], [2, 0]]]);
		expect(ex.terms.map((t) => t.vals)).toEqual([[4, 7], [5, 9], [6, 11]]);
		expect(ex.terms.map((t) => t.sum)).toEqual([[0], [1], [2]]);
		expect(ex.merges).toEqual([
			{ a: 0, b: 1, out: 3, level: 0, value: 73 },
			{ a: 3, b: 2, out: 4, level: 1, value: 139 },
		]);
		expect(ex.root).toBe(4);
		expect(ex.value).toBe(matmul(a, b).get([1, 0]));
		// Structure without values (layout only) and the pairwise tree.
		const tree = explain(p, [0, 0], undefined, { order: 'pairwise' });
		expect(tree.value).toBeUndefined();
		expect(tree.merges.map(({ a, b, out }) => [a, b, out])).toEqual([[0, 1, 3], [3, 2, 4]]);
		expect(() => explain(p, [2, 0])).toThrow(IndexError);
	});
	test('pairwise tree shape', () => {
		expect(sumSteps(5, 'pairwise').merges.map(({ a, b, out, level }) => [a, b, out, level])).toEqual([
			[0, 1, 5, 0], [2, 3, 6, 0], [5, 6, 7, 1], [7, 4, 8, 2],
		]);
		expect(sumSteps(1, 'sequential')).toEqual({ merges: [], root: 0 });
		expect(sumSteps(0, 'pairwise').root).toBe(-1);
	});
	test('a plan rejects tensors of other shapes', () => {
		const p = planEinsum('i,i->', { i: 3 });
		expect(() => einsum(p, zeros([4]), zeros([4]))).toThrow(ShapeError);
		expect(einsum(p, tensor([1, 2, 3]), tensor([4, 5, 6])).item()).toBe(32);
	});
});

describe('matmul / dot', () => {
	test.each(fx.matmul.ok.map((c) => [`${JSON.stringify(c.a.shape)} @ ${JSON.stringify(c.b.shape)}`, c] as const))('%s', (_, c) => {
		close(matmul(T(c.a), T(c.b)), c.out);
	});
	test.each(fx.matmul.bad.map((c) => [`${JSON.stringify(c.a)} @ ${JSON.stringify(c.b)}`, c] as const))('rejects %s', (_, c) => {
		expect(c.numpy).toBe('rejects');
		expect(() => matmul(zeros(c.a), zeros(c.b))).toThrow(ShapeError);
	});
	test('dot is 1-D only and returns a number', () => {
		expect(dot(tensor([1, 2, 3]), tensor([4, 5, 6]))).toBe(32);
		expect(() => dot(zeros([2, 2]), zeros([2, 2]))).toThrow(ShapeError);
	});
});

describe('dtype and summation order', () => {
	test.each(fx.order.map((c) => [c.name, c] as const))('%s: fp64 / fp32 x sequential / pairwise', (_, c) => {
		const t = tensor(c.values);
		for (const order of ['sequential', 'pairwise'] as Order[]) {
			expect(sum(t, { order }).item()).toBe(c.fp64[order]);
			expect(sum(t, { arith: 'fp32', order }).item()).toBe(c.fp32[order]);
		}
	});
	test('ten tenths: the last digit differs between fp64, fp32 sequential and fp32 pairwise', () => {
		const t = tensor(fx.order[0].values);
		expect(String(sum(t).item())).toBe('0.9999999999999999');
		expect(sum(t, { arith: 'fp32' }).item().toPrecision(8)).toBe('1.0000001');
		expect(sum(t, { arith: 'fp32', order: 'pairwise' }).item()).toBe(1);
	});
	test('fp32 dot: operands, products and partial sums rounded (NumPy float32 reference)', () => {
		const a = tensor(fx.dot32.a);
		const b = tensor(fx.dot32.b);
		expect(Math.abs(dot(a, b) - fx.dot32.fp64)).toBeLessThan(1e-12);
		expect(dot(a, b, { arith: 'fp32' })).toBe(fx.dot32.fp32.sequential);
		expect(dot(a, b, { arith: FP32, order: 'pairwise' })).toBe(fx.dot32.fp32.pairwise);
	});
	test('Branch Q ids are reserved; a custom Arith plugs in', () => {
		expect(() => dot(tensor([1]), tensor([1]), { arith: 'fp16' })).toThrow(DtypeError);
		expect(() => sum(tensor([1]), { arith: 'bf16' })).toThrow(DtypeError);
		const int = { id: 'round', round: Math.round };
		expect(dot(tensor([1.4, 2.6]), tensor([1, 1]), { arith: int })).toBe(4);
		expect(() => sum(tensor([1]), { order: 'tree' as Order })).toThrow(DtypeError);
	});
	test('fp64 accumulation over f32 storage, f32 result storage', () => {
		const a = tensor(fx.order[0].values, 'f32');
		const s = sum(a);
		expect(s.store).toBe('f32');
		expect(s.item()).toBe(Math.fround(10 * Math.fround(0.1)));
		expect(einsum('i,i->', a, a).store).toBe('f32');
		expect(einsum('i,i->', a, tensor(fx.order[0].values)).store).toBe('f64');
	});
});
