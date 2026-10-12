import { describe, expect, test } from 'bun:test';
import {
	CHECKS, derive2, derive3, deriveDis, deriveN, deriveR, distance, DIS_STEPS, INITIAL, leftovers, MAX_N, naiveNorm, polyArea, scaledNorm, triangle, triNormal,
} from '../../src/viz/lessons/pythagoras.math';
import type { V } from '../../src/viz/geom';

const vals = (patch: Record<string, unknown> = {}) => ({ ...INITIAL, ...patch }) as typeof INITIAL;
const close = (a: number, b: number) => expect(Math.abs(a - b)).toBeLessThan(1e-12);

describe('distance fixtures', () => {
	test('3-4-5 in 2D: legs, squares, distance, grid walk', () => {
		const z = derive2(vals());
		expect([z.dx, z.dy, z.dx2, z.dy2, z.d2, z.d, z.walk]).toEqual([3, 4, 9, 16, 25, 5, 7]);
		expect(z.R).toEqual([4, 2]);
		expect(z.ccw).toBe(true);
	});
	test('swapping the points flips the legs, not the distance', () => {
		const z = derive2(vals({ P: [4, 6], Q: [1, 2] }));
		expect([z.dx, z.dy, z.d]).toEqual([-3, -4, 5]);
		const mixed = derive2(vals({ Q: [-2, 6] }));
		expect([mixed.dx, mixed.d, mixed.ccw]).toEqual([-3, 5, false]);
	});
	test('a zero leg: the straight line is the grid walk; coincident points have distance 0', () => {
		const z = derive2(vals({ Q: [1, 6] }));
		expect([z.d, z.walk]).toEqual([4, 4]);
		expect(CHECKS.walkEqual(vals({ Q: [1, 6] }))).toBe(true);
		expect(CHECKS.walkEqual(vals({ Q: [1, 2] }))).toBe(false);
		expect(derive2(vals({ Q: [1, 2] })).d).toBe(0);
	});
	test('3D: floor diagonal, then up (prose example and the by-hand exercise)', () => {
		const z = derive3(vals());
		expect([z.dx, z.dy, z.dz, z.f2, z.d2, z.d]).toEqual([1, 2, 2, 5, 9, 3]);
		expect(z.F).toEqual([1, 0, 2]);
		expect(derive3(vals({ P3: [1, 1, 1], Q3: [2, 3, 3] })).d).toBe(3);
	});
	test('n-D: 4D example 3, one more dimension 5, n = 1 is the absolute difference', () => {
		expect(deriveN(vals()).d).toBe(3);
		expect(deriveN(vals({ n: 5 })).d).toBe(5);
		const one = deriveN(vals({ n: 1, p1: 2, q1: -1.5 }));
		expect([one.legs[0], one.d]).toEqual([-3.5, 3.5]);
		expect(() => deriveN(vals({ n: MAX_N + 1 }))).toThrow();
		expect(() => deriveN(vals({ n: 2.5 }))).toThrow();
	});
	test('distance() rejects points of different dimension and agrees with Math.hypot', () => {
		expect(() => distance([0, 0], [1, 2, 3])).toThrow();
		close(distance([1, -2, 3, 0.5], [4, 2, -1, 2]).d, Math.hypot(3, 4, -4, 1.5));
	});
});

describe('dissection conserves area', () => {
	const cases: [number, number][] = [[3, 4], [4, 3], [1.8, 2.4], [0.5, 4], [2, 2]];
	for (const [a, b] of cases) {
		test(`a = ${a}, b = ${b}: every step tiles the big square exactly`, () => {
			const s = a + b;
			const left = leftovers(a, b);
			close(polyArea(left.c2), a * a + b * b);
			close(polyArea(left.a2), a * a);
			close(polyArea(left.b2), b * b);
			for (let step = 0; step < DIS_STEPS; step++) {
				const tris = ([0, 1, 2, 3] as const).map((k) => triangle(a, b, k, step));
				// Each piece keeps its area (a slide is a translation) and stays inside the big square.
				for (const t of tris) {
					close(Math.abs(polyArea(t)), (a * b) / 2);
					for (const [x, y] of t) {
						expect(x).toBeGreaterThanOrEqual(-1e-12);
						expect(y).toBeGreaterThanOrEqual(-1e-12);
						expect(x).toBeLessThanOrEqual(s + 1e-12);
						expect(y).toBeLessThanOrEqual(s + 1e-12);
					}
				}
				close(4 * ((a * b) / 2) + polyArea(left.c2), s * s);
				close(4 * ((a * b) / 2) + polyArea(left.a2) + polyArea(left.b2), s * s);
			}
		});
	}
	test('moved pieces are pure translations of their first placement (same edge vectors)', () => {
		const edges = (t: V[]) => t.map((p, i) => [t[(i + 1) % 3][0] - p[0], t[(i + 1) % 3][1] - p[1]]);
		for (const k of [0, 1, 2, 3] as const) expect(edges(triangle(3, 4, k, 4))).toEqual(edges(triangle(3, 4, k, 0)));
		expect(triangle(3, 4, 0, 4)).toEqual(triangle(3, 4, 0, 0));
		expect(triangle(3, 4, 2, 2)).toEqual([[3, 4], [0, 4], [3, 0]]);
	});
	test('after the slide the pieces close two rectangles and leave the squares a² and b² uncovered', () => {
		const a = 3, b = 4;
		const all = ([0, 1, 2, 3] as const).flatMap((k) => triangle(a, b, k, 4));
		// No triangle vertex lies strictly inside either uncovered square.
		const inside = (p: V, [x0, y0, x1, y1]: number[]) => p[0] > x0 + 1e-9 && p[0] < x1 - 1e-9 && p[1] > y0 + 1e-9 && p[1] < y1 - 1e-9;
		for (const p of all) {
			expect(inside(p, [0, b, a, a + b])).toBe(false);
			expect(inside(p, [a, 0, a + b, b])).toBe(false);
		}
		expect(deriveDis(vals({ a, b })).tris).toBe(24);
	});
});

describe('3D squares and the radius test', () => {
	test('triNormal: counter-clockwise normal, null for a flat triangle', () => {
		expect(triNormal([0, 0, 0], [1, 0, 0], [1, 1, 0])).toEqual([0, 0, 1]);
		expect(triNormal([0, 0, 0], [1, 0, 0], [2, 0, 0])).toBeNull();
		const z = derive3(vals({ Q3: [0, 2, 0] }));
		expect(triNormal(z.P, z.X, z.F)).toBeNull();
		expect(CHECKS.above(vals({ Q3: [0, 2, 0] }))).toBe(true);
	});
	test('d² < r² gives the same verdict as d < r, including the boundary', () => {
		for (const [x, y, r] of [[2, 1.5, 3], [1.5, 2, 2.5], [3, 2, 2.5], [0, 0, 0.5], [-4, 0.5, 4]]) {
			const z = deriveR(vals({ X: [x, y], r }));
			const byRoot = Math.hypot(x, y) < r - 1e-9 ? 'in' : Math.abs(Math.hypot(x, y) - r) < 1e-9 ? 'on' : 'out';
			expect(z.side).toBe(byRoot);
		}
		expect(CHECKS.onCircle(vals({ X: [1.5, 2], r: 2.5 }))).toBe(true);
	});
});

describe('stable norm (machine layer)', () => {
	test('squares overflow at 1e200 and underflow at 1e-200; rescaling keeps the answer', () => {
		expect(naiveNorm([1e200, 1e200])).toBe(Infinity);
		expect(naiveNorm([1e-200, 1e-200])).toBe(0);
		close(scaledNorm([1e200, 1e200]) / 1e200, Math.SQRT2);
		close(scaledNorm([1e-200, 1e-200]) / 1e-200, Math.SQRT2);
		close(scaledNorm([3, 4]), 5);
		expect(scaledNorm([0, 0])).toBe(0);
	});
});

describe('challenges', () => {
	test('defaults solve nothing that asks for a change', () => {
		const v = vals();
		expect(CHECKS.fiveElsewhere(v)).toBe(false);
		expect(CHECKS.threeElsewhere(v)).toBe(false);
		expect(CHECKS.eightFive(v)).toBe(false);
		expect(CHECKS.onCircle(v)).toBe(false);
	});
	test('each has a reachable solution', () => {
		expect(CHECKS.fiveElsewhere(vals({ P: [0, 0], Q: [3, 4] }))).toBe(true);
		expect(CHECKS.threeElsewhere(vals({ Q3: [3, 0, 0] }))).toBe(true);
		expect(CHECKS.threeElsewhere(vals({ Q3: [2, 1, 2] }))).toBe(true);
		// 8D: the default q has 1+0+4+4+16+0+4+1 = 30; with q8 = 0 and q7 = 0 it is 25.
		expect(CHECKS.eightFive(vals({ n: 8, q7: 0, q8: 0 }))).toBe(true);
		expect(CHECKS.slid(vals(), { 'dis:max': DIS_STEPS - 1 })).toBe(true);
		expect(CHECKS.wholeC(vals({ a: 1.8, b: 2.4 }))).toBe(true);
		expect(CHECKS.wholeC(vals({ a: 4, b: 3 }))).toBe(false);
		expect(CHECKS.wholeC(vals({ a: 1.5, b: 2 }))).toBe(false);
	});
});
