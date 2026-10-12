// Lesson 3.6 (src/viz/lessons/einsum.*) against independent references: hand-written loops, NumPy's printed results
// for the page's examples, the core's typed errors, and the socket diagram edits.
import { describe, expect, test } from 'bun:test';
import { ExtentError, flat, parseEinsum, planEinsum, SpecError } from '../../src/core';
import type { LessonState } from '../../src/viz/store';
import {
	axisTags, canon, CHECKS, errInfo, extKey, flops, freshLetter, INITIAL, LAB_PRESETS, lab, loops, opOf, opVal, plug,
	presetPatch, roleOf, s4Layout, sumText, toggleKeep,
} from '../../src/viz/lessons/einsum.math';

type Vals = LessonState['vals'];
const vals = (patch: Record<string, number | number[]> = {}): Vals => ({ ...INITIAL, ...patch });
const withOp = (op: string, ext: Record<string, number> = {}) =>
	vals({ op: opVal(op), ...Object.fromEntries(Object.entries(ext).map(([l, n]) => [extKey(l), n])) });
const ok = (v: Vals) => {
	const b = lab(v);
	if (!b.ok) throw new Error('expected a valid lab, got ' + b.error.message);
	return b;
};

/** Reference einsum by brute force over every letter assignment (independent of the core's stride kernel). */
function brute(op: string, dims: Record<string, number>, A: (i: number[]) => number, B: (i: number[]) => number): number[] {
	const p = parseEinsum(op);
	const all = [...new Set(p.inputs.join(''))];
	const out = new Map<string, number>();
	const keys: string[] = [];
	const rec = (k: number, at: Record<string, number>) => {
		if (k === all.length) {
			const key = [...p.output].map((l) => at[l]).join(',');
			if (!out.has(key)) { out.set(key, 0); keys.push(key); }
			out.set(key, out.get(key)! + A([...p.inputs[0]].map((l) => at[l])) * B([...p.inputs[1]].map((l) => at[l])));
			return;
		}
		for (let i = 0; i < dims[all[k]]; i++) rec(k + 1, { ...at, [all[k]]: i });
	};
	rec(0, {});
	// Row-major order of the output cells.
	const shape = [...p.output].map((l) => dims[l]);
	const res: number[] = [];
	const walk = (d: number, idx: number[]) => {
		if (d === shape.length) return void res.push(out.get(idx.join(','))!);
		for (let i = 0; i < shape[d]; i++) walk(d + 1, [...idx, i]);
	};
	walk(0, []);
	return res;
}

describe('the lab builds from the store', () => {
	test('default = the course matmul: C = [[3, 5], [-1, 4]] (NumPy A @ B)', () => {
		const b = ok(vals());
		expect(b.op).toBe('ij,jk->ik');
		expect(Array.from(b.d.out)).toEqual([3, 5, -1, 4]);
		expect(b.plan.kept).toEqual(['i', 'k']);
		expect(b.plan.summed).toEqual(['j']);
	});
	test('every preset agrees with a brute-force sum over all letters', () => {
		for (const p of LAB_PRESETS) {
			const b = ok(vals(presetPatch(p) as Record<string, number | number[]>));
			const [ta, tb] = b.d.ts;
			const ref = brute(p.op, b.plan.dims as Record<string, number>, (i) => ta.get(i), (i) => tb.get(i));
			// sceneData's `out` lists drawn cells; the full result comes from the same plan.
			expect(b.d.L.cells.n).toBe(b.plan.cells);
			const full = new Array(b.plan.cells);
			for (let c = 0; c < b.d.L.cells.n; c++) full[b.d.L.cells.flat[c]] = b.d.out[c];
			expect(full).toEqual(ref);
		}
	});
	test('batched: A[1] = −A[0], B shared, so C[1] = −C[0]; attention head 0 = [[5, 1, 6], [1, 0, 2]]', () => {
		const bt = ok(vals(presetPatch(LAB_PRESETS.find((p) => p.id === 'batched')!) as Record<string, number[]>));
		expect(Array.from(bt.d.out)).toEqual([3, 5, -1, 4, -3, -5, 1, -4]);
		const at = ok(vals(presetPatch(LAB_PRESETS.find((p) => p.id === 'attention')!) as Record<string, number[]>));
		expect(Array.from(at.d.out).slice(0, 6)).toEqual([5, 1, 6, 1, 0, 2]);
	});
	test('seeded ops use integers in [−3, 3] and change with the seed', () => {
		const a = ok(withOp('ij,ij->', { i: 2, j: 3 }));
		const b = ok({ ...withOp('ij,ij->', { i: 2, j: 3 }), seed: 4 });
		for (const x of [...a.d.ts[0].data, ...b.d.ts[1].data]) expect(Number.isInteger(x) && Math.abs(x) <= 3).toBe(true);
		expect(flat(a.d.ts[0])).not.toEqual(flat(b.d.ts[0]));
	});
	test('a bad spec gives the typed core error, not a picture', () => {
		const b = lab(withOp('ij,jk->iz'));
		expect(b.ok).toBe(false);
		if (b.ok) return;
		expect(b.error).toBeInstanceOf(SpecError);
		expect(errInfo(b.error)).toEqual({ kind: 'spec', reason: 'unknownOut', token: 'z', pos: 8 });
		const three = lab(withOp('i,i,i->'));
		expect(!three.ok && errInfo(three.error)).toMatchObject({ kind: 'spec', reason: 'arity' });
		expect(!lab(withOp('ij,jk')).ok).toBe(true);
		const spaced = lab(withOp('ij, jk->ik'));
		if (spaced.ok) throw new Error('a spec with a space must not parse');
		expect(errInfo(spaced.error as SpecError)).toMatchObject({ reason: 'space', pos: 3 });
	});
});

describe('reading an op', () => {
	test('roles: batch, free, contracted, reduced', () => {
		const p = parseEinsum('bij,bjk->bik');
		expect(['b', 'i', 'j', 'k'].map((l) => roleOf(p, l))).toEqual(['batch', 'free', 'contracted', 'free']);
		expect(roleOf(parseEinsum('ij,k->i'), 'j')).toBe('reduced');
	});
	test('canonical form ignores letter names', () => {
		expect(canon('bij,bjk->bik')).toBe('abc,acd->abd');
		expect(canon('xyz,xzw->xyw')).toBe('abc,acd->abd');
		expect(canon('ij,kj->ik')).toBe('ab,cb->ac');
		expect(canon('ij,jk->')).toBe('ab,bc->');
		expect(canon('ij,jk->iz')).toBeNull();
	});
	test('the loops of matmul: kept letters outside, the summed letter inside', () => {
		expect(loops(planEinsum('ij,jk->ik', { i: 2, j: 3, k: 2 }))).toBe(
			'for i in range(2):\n    for k in range(2):\n        s = 0\n        for j in range(3):\n            s += A[i, j] * B[j, k]\n        C[i, k] = s',
		);
		expect(loops(planEinsum('i,i->', { i: 3 }))).toBe('s = 0\nfor i in range(3):\n    s += A[i] * B[i]\nC = s');
	});
	test('sum text and FLOPs = 2 · cells · shaft', () => {
		expect(sumText(parseEinsum('ij,jk->ik'))).toBe('C[i, k] = Σ_j A[i, j] · B[j, k]');
		expect(sumText(parseEinsum('i,j->ij'))).toBe('C[i, j] = A[i] · B[j]');
		expect(flops(planEinsum('bij,bjk->bik', { b: 2, i: 2, j: 3, k: 2 }))).toBe(2 * 8 * 3);
	});
	test('challenge checks', () => {
		expect(CHECKS.frob(withOp('xy,xy->'))).toBe(true);
		expect(CHECKS.frob(withOp('ij,jk->ik'))).toBe(false);
		expect(CHECKS.abt(withOp('ij,kj->ik'))).toBe(true);
		expect(CHECKS.batched(withOp('nij,njk->nik'))).toBe(true);
		expect(CHECKS.batched(withOp('ij,jk->ik'))).toBe(false);
		expect(CHECKS.mismatch(vals({ bad: 1 }))).toBe(true);
	});
	test('op string round trip through the store', () => {
		expect(opOf(vals({ op: opVal('bhqd,bhkd->bhqk') }))).toBe('bhqd,bhkd->bhqk');
	});
});

describe('S4: plugs and cables', () => {
	const ext = (m: Record<string, number>) => (l: string) => m[l] ?? 2;
	test('plugging equal lengths contracts the letter', () => {
		// i,j->ij (outer) with i = 3, j = 3: plug A's i into B's j -> i,i-> (dot product).
		const e = plug('i,j->ij', 0, 0, ext({ i: 3, j: 3 }));
		expect(e).toEqual({ ok: true, op: 'i,i->' });
	});
	test('plugging different lengths is refused by the core with an ExtentError', () => {
		const e = plug('ij,kl->ijkl', 1, 0, ext({ i: 2, j: 3, k: 4, l: 2 }));
		expect(e.ok).toBe(false);
		if (e.ok) return;
		expect(e.error).toBeInstanceOf(ExtentError);
		expect(errInfo(e.error)).toEqual({ kind: 'extent', letter: 'j', a: 3, b: 4 });
	});
	test('a letter B already has elsewhere is refused as a repeated index', () => {
		const e = plug('ij,jk->ik', 1, 1, ext({}));
		expect(!e.ok && errInfo(e.error)).toMatchObject({ kind: 'spec', reason: 'repeatIn' });
	});
	test('pulling a cable out gives B a fresh letter of the same length, both ends kept', () => {
		const e = plug('ij,jk->ik', 1, 0, ext({ j: 3 }));
		expect(e).toEqual({ ok: true, op: 'ij,ak->ikja', ext: { a: 3 } });
		expect(freshLetter('abc,cd->')).toBe('e');
	});
	test('a cable made into a batch axis and back', () => {
		expect(toggleKeep('ij,jk->ik', 'j')).toEqual({ ok: true, op: 'ij,jk->ikj' });
		expect(toggleKeep('ij,jk->ikj', 'j')).toEqual({ ok: true, op: 'ij,jk->ik' });
	});
	test('diagram: sockets per axis with sizes, one cable per letter, a Σ mark on contracted cables', () => {
		const p = parseEinsum('bij,bjk->bik');
		const L = s4Layout(p, ext({ b: 2, i: 2, j: 3, k: 2 }));
		expect(L.sockets.A.map((s) => s.letter + s.size)).toEqual(['b2', 'i2', 'j3']);
		expect(L.sockets.C.map((s) => s.letter)).toEqual(['b', 'i', 'k']);
		expect(L.blocks[2].shape).toEqual([2, 2, 2]);
		const j = L.cables.find((c) => c.letter === 'j')!;
		expect(j.role).toBe('contracted');
		expect(j.mark).not.toBeNull();
		expect(L.cables.find((c) => c.letter === 'b')!.d).toContain(' L');
		for (const s of [...L.sockets.A, ...L.sockets.B, ...L.sockets.C]) {
			expect(s.x).toBeGreaterThan(0);
			expect(s.x).toBeLessThan(360);
		}
	});
});

describe('axis letters in the light box', () => {
	test('matmul: k under the front face, i right of it, j along the shaft', () => {
		const b = ok(vals());
		const tags = axisTags(b.d);
		expect(tags.map((t) => t.text)).toEqual(['i = 2', 'k = 2', 'j = 3 (Σ)']);
		const [i, k] = tags;
		expect(k.at[1]).toBeLessThan(i.at[1]);
		expect(i.at[0]).toBeGreaterThan(k.at[0]);
	});
	test('batched: one "b = n" tag per block', () => {
		const b = ok(vals(presetPatch(LAB_PRESETS.find((p) => p.id === 'batched')!) as Record<string, number[]>));
		expect(axisTags(b.d).filter((t) => t.role === 'batch').map((t) => t.text)).toEqual(['b = 0', 'b = 1']);
	});
});
