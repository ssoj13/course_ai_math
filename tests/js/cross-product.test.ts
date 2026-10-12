import { describe, expect, test } from 'bun:test';
import { CHECKS, INITIAL, LIGHT, angleDeg, arcNormal, derive, derive2, fmt, fmtVec, terms, tri } from '../../src/viz/lessons/cross-product.math';
import { rowsFor } from '../../src/viz/lessons/cross-product.rows';

/**
 * Lesson 1.8 model. Expected numbers are worked by hand from the definition (the prose examples), not by calling the
 * engine's cross(), so a wrong formula in either place fails here.
 */
type Vals = Parameters<typeof derive>[0];
const vals = (patch: Partial<Record<keyof typeof INITIAL, number | number[]>> = {}): Vals => ({ ...INITIAL, ...patch }) as Vals;
const close = (a: readonly number[], b: readonly number[], eps = 1e-12) => a.forEach((x, i) => expect(Math.abs(x - b[i])).toBeLessThan(eps));

describe('CP-1: the 3D cross product', () => {
	test('prose examples: x × y = +z, y × x = −z, (1,2,3) × (4,5,6) = (−3,6,−3)', () => {
		close(derive(vals({ a: [1, 0, 0], b: [0, 1, 0] })).c, [0, 0, 1]);
		close(derive(vals({ a: [1, 0, 0], b: [0, 1, 0], swap: 1 })).c, [0, 0, -1]);
		const z = derive(vals({ a: [1, 2, 3], b: [4, 5, 6] }));
		close(z.c, [-3, 6, -3]);
		// The criss-cross terms: x = 2·6 − 3·5, y = 3·4 − 1·6, z = 1·5 − 2·4.
		close(z.plus, [12, 12, 5]);
		close(z.minus, [15, 6, 8]);
		expect(z.len).toBeCloseTo(Math.sqrt(54), 12);
		expect(z.area).toBeCloseTo(7.3485, 4);
	});

	test('perpendicular to both inputs; length equals |a||b| sin θ for the initial pair', () => {
		const z = derive(vals());
		// a = (0, −1, 2), b = (2, 0.5, 0): (−1·0 − 2·0.5, 2·2 − 0·0, 0·0.5 − (−1)·2).
		close(z.c, [-1, 4, 2]);
		expect(Math.abs(z.ca)).toBeLessThan(1e-12);
		expect(Math.abs(z.cb)).toBeLessThan(1e-12);
		expect(Math.abs(z.len - z.area)).toBeLessThan(1e-12);
		expect(z.len).toBeCloseTo(Math.sqrt(21), 12);
	});

	test('order: b × a = −(a × b), same length', () => {
		const ab = derive(vals({ a: [1.3, -0.7, 2.1], b: [-0.4, 1.9, 0.6] }));
		const ba = derive(vals({ a: [1.3, -0.7, 2.1], b: [-0.4, 1.9, 0.6], swap: 1 }));
		close(ba.c, ab.c.map((x) => -x));
		expect(ba.len).toBe(ab.len);
	});

	test('parallel and zero inputs give the zero vector, not an error', () => {
		const par = derive(vals({ a: [1, 2, 0], b: [2, 4, 0] }));
		close(par.c, [0, 0, 0]);
		expect(par.parallel).toBe(true);
		expect(par.zero).toBe(false);
		expect(par.area).toBe(0);
		const anti = derive(vals({ a: [0.3, 0.1, 0.7], b: [-0.6, -0.2, -1.4] }));
		expect(anti.parallel).toBe(true);
		const zero = derive(vals({ a: [0, 0, 0] }));
		close(zero.c, [0, 0, 0]);
		expect(zero.zero).toBe(true);
		expect(Number.isNaN(zero.th)).toBe(true);
		expect(zero.area).toBe(0);
	});

	test('perpendicular inputs: |a × b| = |a||b|; otherwise strictly smaller', () => {
		expect(derive(vals({ a: [2, 0, 0], b: [0, 3, 0] })).len).toBe(6);
		const z = derive(vals({ a: [2, 0, 0], b: [1, 3, 0] }));
		expect(z.len).toBeLessThan(z.na * z.nb);
	});

	test('terms() and the arc reference', () => {
		expect(terms([1, 2, 3], [4, 5, 6])).toEqual({ plus: [12, 12, 5], minus: [15, 6, 8] });
		expect(arcNormal([1, 0, 0], [0, 0, 2])).toEqual([0, 0, 2]);
		// Zero result: the coordinate axis least parallel to a.
		expect(arcNormal([1, 2, 0.5], [0, 0, 0])).toEqual([0, 0, 1]);
		expect(angleDeg([1, 0, 0], [0, 1, 0])).toBe(90);
		expect(Number.isNaN(angleDeg([0, 0, 0], [1, 0, 0]))).toBe(true);
	});
});

describe('CP-2: signed area', () => {
	test('(2, 0), (1, 3) gives +6; the other order −6; collinear 0', () => {
		const z = derive2(vals({ a2: [2, 0], b2: [1, 3] }));
		expect(z.sa).toBe(6);
		expect(z.orient).toBe('ccw');
		const w = derive2(vals({ a2: [1, 3], b2: [2, 0] }));
		expect(w.sa).toBe(-6);
		expect(w.orient).toBe('cw');
		const l = derive2(vals({ a2: [1, 2], b2: [-2, -4] }));
		expect(l.sa).toBe(0);
		expect(l.orient).toBe('line');
		expect(derive2(vals({ a2: [0, 0] })).orient).toBe('line');
	});

	test('the 2D number is the z component of the 3D formula with z = 0', () => {
		const a = [1.7, -0.9], b = [0.4, 2.2];
		expect(derive2(vals({ a2: a, b2: b })).sa).toBeCloseTo(derive(vals({ a: [...a, 0], b: [...b, 0] })).c[2], 12);
	});
});

describe('CP-3: triangle normal and winding', () => {
	test('prose example: (0,0,0), (1,0,0), (0,1,0) faces +z, area 0.5; reversed faces −z', () => {
		const t = tri(vals({ tA: [0, 0, 0], tB: [1, 0, 0], tC: [0, 1, 0] }));
		close(t.N!, [0, 0, 1]);
		expect(t.area).toBe(0.5);
		const r = tri(vals({ tA: [0, 0, 0], tB: [1, 0, 0], tC: [0, 1, 0], rev: 1 }));
		close(r.N!, [0, 0, -1]);
		expect(r.P).toEqual([[0, 0, 0], [0, 1, 0], [1, 0, 0]]);
		expect(r.area).toBe(0.5);
	});

	test('lighting: max(0, N·L) with the fixed light; reversed goes black', () => {
		expect(Math.hypot(...LIGHT)).toBeCloseTo(1, 12);
		const t = tri(vals());
		// A = (0, 0, 1), B = (2, 0, 0), C = (0, 2, 0): e1 = (2, 0, −1), e2 = (0, 2, −1), e1 × e2 = (2, 2, 4).
		close(t.n, [2, 2, 4]);
		expect(t.area).toBeCloseTo(Math.sqrt(24) / 2, 12);
		expect(t.ndotl).toBeCloseTo((2 * -1 + 2 * 2 + 4 * 1.5) / (Math.sqrt(24) * Math.hypot(-1, 2, 1.5)), 12);
		expect(t.bright).toBe(t.ndotl);
		const r = tri(vals({ rev: 1 }));
		expect(r.ndotl).toBeCloseTo(-t.ndotl, 12);
		expect(r.bright).toBe(0);
	});

	test('collinear vertices: no normal, nothing to light', () => {
		const t = tri(vals({ tA: [0, 0, 0], tB: [1, 1, 1], tC: [2, 2, 2] }));
		expect(t.N).toBeNull();
		expect(Number.isNaN(t.bright)).toBe(true);
		expect(t.area).toBe(0);
	});
});

describe('challenges and formatting', () => {
	test('each predicate: false at the initial values, true at a solution', () => {
		for (const check of Object.values(CHECKS)) expect(check(vals())).toBe(false);
		expect(CHECKS.plusZ(vals({ a: [1, 0, 0], b: [0, 1, 0] }))).toBe(true);
		expect(CHECKS.plusZ(vals({ a: [0, 1, 0], b: [1, 0, 0], swap: 1 }))).toBe(false);
		expect(CHECKS.parallel(vals({ b: [0, -2, 4] }))).toBe(true);
		expect(CHECKS.parallel(vals({ b: [0, 0, 0] }))).toBe(false);
		expect(CHECKS.areaSix(vals({ a: [2, 0, 0], b: [0, 3, 0] }))).toBe(true);
		expect(CHECKS.swapped(vals({ swap: 1 }))).toBe(true);
		expect(CHECKS.negative(vals({ b2: [1, -1] }))).toBe(true);
		expect(CHECKS.six(vals({ a2: [2, 0], b2: [-1.5, 3] }))).toBe(true);
		expect(CHECKS.line(vals({ b2: [-3, -1] }))).toBe(true);
		expect(CHECKS.black(vals({ rev: 1, cull: 0 }))).toBe(true);
		expect(CHECKS.black(vals({ rev: 1 }))).toBe(false);
		expect(CHECKS.turnAway(vals({ tC: [0, -2, 1] }))).toBe(true);
		expect(CHECKS.turnAway(vals({ tC: [0, 2, -3] }))).toBe(false);
	});

	test('formatting never prints −0', () => {
		expect(fmt(-0.001)).toBe('0.00');
		expect(fmtVec([-1.5, 0.5, -0.0001])).toBe('(-1.50, 0.50, 0.00)');
		expect(fmt(NaN)).toBe('–');
	});

	test('rows: ids are term ids, the same in both locales; ranges only on inputs', () => {
		const ids = (s: 1 | 2 | 3) => rowsFor(s, 'en').map((r) => r.id);
		expect(ids(1)).toEqual(['a', 'b', 'swap', 'c1', 'c2', 'c3', 'len', 'area', 'th']);
		expect(ids(2)).toEqual(['a2', 'b2', 'sa', 'th2']);
		expect(ids(3)).toEqual(['tA', 'tB', 'tC', 'rev', 'cull', 'N', 'triArea', 'NdotL', 'bright']);
		for (const s of [1, 2, 3] as const) expect(rowsFor(s, 'ru').map((r) => r.id)).toEqual(ids(s));
	});
});
