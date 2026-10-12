import { expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { blend, CHECKS, derive, fmt1, fmt2, fmt3, INITIAL, moveBar, norm2, outward, sumOf, unit, upPlaneNormal, walk3 } from '../../src/viz/lessons/length.math';
import { rowsFor } from '../../src/viz/lessons/length.rows';
import { rowRanges } from '../../src/viz/scene/NPanel';

/** Lesson 1.3: values checked against hand computations from the prose, not against the module itself. */

test('length is Pythagoras over all components: the prose examples', () => {
	expect(sumOf([3, 4])).toMatchObject({ sq: [9, 16], run: [9, 25], s2: 25, len: 5 });
	expect(sumOf([1, 1, 1]).len).toBeCloseTo(1.7320508075688772, 15);
	expect(sumOf([2, 1, 2, 0, 4, 0, 1, 2])).toMatchObject({ s2: 30, run: [4, 5, 9, 9, 25, 25, 26, 30] });
	expect(sumOf(Array(8).fill(1)).len).toBeCloseTo(2.8284271247461903, 15);
	// Squares forget the sign; scaling by s scales the length by |s|.
	expect(sumOf([-3, -4]).len).toBe(5);
	expect(sumOf([-6, -8]).len).toBe(10);
	expect(sumOf([0, 0]).len).toBe(0);
	expect(() => sumOf([NaN, 1])).toThrow();
});

test('normalization keeps the direction, sets the length to 1, and refuses the zero vector', () => {
	expect(unit([0, 5])).toEqual([0, 1]);
	// 3 · (1/5) rounds to 0.6000000000000001: the reason the lesson compares with a tolerance.
	unit([3, 4])!.forEach((x, i) => expect(x).toBeCloseTo([0.6, 0.8][i], 15));
	const h = unit([1, 1])!;
	expect(h[0]).toBeCloseTo(Math.SQRT1_2, 15);
	expect(Math.abs(Math.hypot(...h) - 1)).toBeLessThan(1e-15);
	// 3D: (2, 2, 1) has length 3.
	expect(unit([2, 2, 1])!.map((x) => +x.toFixed(6))).toEqual([0.666667, 0.666667, 0.333333]);
	expect(unit([0, 0])).toBeNull();
	expect(unit([0, 0, 0])).toBeNull();
	const z = norm2([0, 0], 0.5);
	expect(z.zero).toBe(true);
	expect(z.hatC.every(Number.isNaN)).toBe(true);
	expect(z.mid).toEqual([0, 0]);
	// Many random directions: the normalized length is 1 within a tolerance (never compared with ===).
	for (let i = 0; i < 200; i++) {
		const v = [Math.sin(i * 1.7) * 4, Math.cos(i * 2.3) * 4, Math.sin(i) * 0.01];
		expect(Math.abs(Math.hypot(...unit(v)!) - 1)).toBeLessThan(1e-12);
	}
});

test('the blend keeps the direction and moves the length linearly from |v| to 1', () => {
	const v = [3, 4];
	expect(blend(v, 0)).toEqual([3, 4]);
	expect(blend(v, 1).map((x) => +x.toFixed(12))).toEqual([0.6, 0.8]);
	// k = 0.5: length (5 + 1) / 2 = 3, direction (0.6, 0.8).
	expect(blend(v, 0.5).map((x) => +x.toFixed(12))).toEqual([1.8, 2.4]);
	expect(norm2(v, 0.25).midLen).toBe(4);
	// A short vector grows to the unit circle; it never flips.
	expect(blend([0.2, 0], 1)).toEqual([1, 0]);
	expect(blend([0.2, 0], 0.5)[0]).toBeCloseTo(0.6, 15);
});

test('the 3D walk: x, then z on the floor, then up y; tiles face away from each right triangle', () => {
	const w = walk3([2, 1, 2]);
	expect(w).toMatchObject({ c1: [2, 0, 0], c2: [2, 0, 2], c3: [2, 1, 2] });
	expect(w.d1).toBeCloseTo(Math.sqrt(8), 15);
	// x leg on the floor (normal ±y): its tile must face away from c2 = (2, 0, 2), i.e. toward -z.
	const n = outward(w.c0, w.c1, w.c2, [0, 1, 0]);
	// AreaTile puts a 3D tile on the side (b - a) × n.
	const e = [2, 0, 0];
	const r = [e[1] * n[2] - e[2] * n[1], e[2] * n[0] - e[0] * n[2], e[0] * n[1] - e[1] * n[0]];
	expect(r[2]).toBeLessThan(0);
	// The up-plane holds the floor diagonal (2, 0, 2) and y: its normal is perpendicular to both.
	const u = upPlaneNormal([2, 1, 2]);
	expect(u[0] * 2 + u[2] * 2).toBeCloseTo(0, 15);
	expect(u[1]).toBe(0);
	expect(Math.hypot(...u)).toBeCloseTo(1, 15);
	expect(upPlaneNormal([0, 3, 0])).toEqual([0, 0, 1]);
});

test('LN-3 bars move one component on the grid inside the box', () => {
	const vals = { ...INITIAL };
	expect(moveBar(4)(vals, [5, 0.34]).set.u).toEqual([2, 1, 2, 0, 4, 0, 1, 2].map((x, i) => (i === 4 ? 4 : x)));
	expect(moveBar(3)(vals, [0, -1.26]).set.u).toEqual([2, 1, 2, -1.3, 4, 0, 1, 2]);
	expect((moveBar(0)(vals, [0, 9]).set.u as number[])[0]).toBe(4);
});

test('derive and the challenges read the store values', () => {
	const z = derive(INITIAL);
	expect(z.s.len).toBe(5);
	expect(z.s3.len).toBe(3);
	expect(z.su.s2).toBe(30);
	expect(z.n.hat![0]).toBeCloseTo(2 / Math.sqrt(5), 15);
	expect(CHECKS.five(INITIAL)).toBe(false);
	expect(CHECKS.five({ ...INITIAL, v: [0, -5] })).toBe(true);
	expect(CHECKS.five({ ...INITIAL, v: [4, 3] })).toBe(true);
	expect(CHECKS.sixEight(INITIAL)).toBe(false);
	expect(CHECKS.sixEight({ ...INITIAL, w: [1.5, 2] })).toBe(true);
	expect(CHECKS.zero({ ...INITIAL, w: [0, 0] })).toBe(true);
	expect(CHECKS.four({ ...INITIAL, u: [4, 0, 0, 0, 0, 0, 0, 0] })).toBe(true);
	expect(CHECKS.four({ ...INITIAL, u: [2, 2, 2, 2, 0, 0, 0, 0] })).toBe(true);
	expect(CHECKS.ones8({ ...INITIAL, u: Array(8).fill(1) })).toBe(true);
	expect(CHECKS.ones3({ ...INITIAL, v3: [1, 1, 1] })).toBe(true);
	expect(CHECKS.onSphere({ ...INITIAL, k: 1 })).toBe(true);
});

test('formats: fixed decimals, no negative zero, a dash for undefined', () => {
	expect(fmt1(-0.04)).toBe('0.0');
	expect(fmt2(Math.sqrt(30))).toBe('5.48');
	expect(fmt3(Math.SQRT1_2)).toBe('0.707');
	expect(fmt3(NaN)).toBe('–');
});

test('N-panel rows: same ids in both locales, ranges match the spec', () => {
	for (const scene of [1, 2, 3, 4, 5] as const) {
		const en = rowsFor(scene, 'en'), ru = rowsFor(scene, 'ru');
		expect(ru.map((r) => r.id)).toEqual(en.map((r) => r.id));
		expect(rowRanges(ru)).toEqual(rowRanges(en));
	}
	expect(rowRanges(rowsFor(1, 'en')).v).toEqual({ min: -4, max: 4 });
	expect(rowRanges(rowsFor(3, 'en')).k).toEqual({ min: 0, max: 1 });
});
