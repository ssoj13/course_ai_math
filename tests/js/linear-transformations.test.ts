import { describe, expect, test } from 'bun:test';
import '../../src/i18n/ui';
import {
	BEND, CHECKS, EXT, FIG, INITIAL, SAMPLES, SHIFT, applyCols, derive1, derive2, handleCol, mapPt, mappedGrid, morphCols, moveCol,
} from '../../src/viz/lessons/linear-transformations.math';
import { rowsFor } from '../../src/viz/lessons/linear-transformations.rows';

/** Independent pure checks of lesson 2.1's model: every number of the prose, the morph, the drag mapping, edge cases. */

const vals = (patch: Record<string, number | number[]> = {}) => ({ ...INITIAL, ...patch });
const close = (a: readonly number[], b: readonly number[], eps = 1e-12) => {
	expect(a.length).toBe(b.length);
	a.forEach((x, i) => expect(Math.abs(x - b[i])).toBeLessThan(eps));
};
/** Plain row-by-row matrix product, written out independently of the model. */
const rowsOf = (m: number[][], p: number[]) => m.map((r) => r[0] * p[0] + r[1] * p[1]);
const A = [[2, 1], [1, 2]];

describe('2.1 prose numbers', () => {
	test('T(v) = 2·(2, 1) − 1·(1, 2) = (3, 0); A(1, 3) = (5, 7); A(v + w) = (8, 7)', () => {
		const z = derive1(vals());
		close(z.av, [3, 0]);
		close(z.avT, [3, 0]);
		close(applyCols([2, 1], [1, 2], [1, 3]), [5, 7]);
		close(applyCols([2, 1], [1, 2], [3, 2]), [8, 7]);
		close(z.av, rowsOf(A, [2, -1]));
	});
	test('scaling: A(−2v) = (−6, 0), A(0v) = 0, A(0.5v) = (1.5, 0)', () => {
		close(applyCols([2, 1], [1, 2], [-4, 2]), [-6, 0]);
		close(applyCols([2, 1], [1, 2], [0, 0]), [0, 0]);
		close(applyCols([2, 1], [1, 2], [1, -0.5]), [1.5, 0]);
	});
	test('exercise 3: the quarter turn sends (3, 1) to (−1, 3)', () => {
		close(applyCols([0, 1], [-1, 0], [3, 1]), [-1, 3]);
	});
});

describe('LT-1 morph and drag', () => {
	test('t = 0 is the identity, t = 1 the map, and every t is linear (columns are a lerp)', () => {
		close(morphCols([2, 1], [1, 2], 0).flat(), [1, 0, 0, 1]);
		close(morphCols([2, 1], [1, 2], 1).flat(), [2, 1, 1, 2]);
		close(morphCols([2, 1], [1, 2], 0.5).flat(), [1.5, 0.5, 0.5, 1.5]);
		const z = derive1(vals({ t: 0.5 }));
		close(z.avT, [1.5 * 2 - 0.5, 0.5 * 2 - 1.5]);
	});
	test('signed area: mirror < 0, squash = 0, start = 3', () => {
		expect(derive1(vals()).det).toBe(3);
		expect(derive1(vals({ a1: [1, 2], a2: [2, 1] })).det).toBe(-3);
		const flat = derive1(vals({ a1: [1, 1], a2: [2, 2] }));
		expect(flat.flat).toBe(true);
		expect(CHECKS.squash(vals({ a1: [1, 1], a2: [2, 2] }))).toBe(true);
		expect(CHECKS.mirror(vals({ a1: [1, 2], a2: [2, 1] }))).toBe(true);
		expect(CHECKS.mirror(vals({ a1: [1, 2], a2: [2, 1], t: 0.5 }))).toBe(false);
		expect(CHECKS.squash(vals({ a1: [0, 0], a2: [2, 2] }))).toBe(false);
	});
	test('the F maps vertex by vertex', () => {
		const z = derive1(vals());
		expect(z.fig.length).toBe(FIG.length);
		z.fig.forEach((p, i) => close(p, rowsOf(A, [...FIG[i]])));
	});
	test('the handle stays under the pointer: a travel d moves the column by d / t (snapped to 0.1)', () => {
		const v = vals({ t: 0.5 });
		close(handleCol(1)(v), [1.5, 0.5]);
		const m = moveCol(1)(v, [0.5, 0]);
		close(m.set.a1 as number[], [3, 1]);
		close(handleCol(1)(vals({ t: 0.5, a1: [3, 1] })), [2, 0.5]);
		const k = moveCol(2)(vals(), [0.1, 0]);
		close(k.set.a2 as number[], [1.1, 2]);
	});
});

describe('LT-2 linear or not', () => {
	test('linear: both routes meet and the origin stays', () => {
		const z = derive2(vals());
		expect(z.addOk && z.scaleOk && z.originOk).toBe(true);
		close(z.tvw, [8, 7]);
		close(z.tsv, [-6, 0]);
	});
	test('shift: F(0) = (1, −2), F(v + w) = (9, 5) vs (10, 3), gap = |b|, scaling gap = |1 − s|·|b|', () => {
		const z = derive2(vals({ mode: 1 }));
		close(z.origin, SHIFT);
		close(z.tvw, [9, 5]);
		close(z.sum, [10, 3]);
		expect(z.gapAdd).toBeCloseTo(Math.hypot(1, 2), 12);
		expect(z.gapScale).toBeCloseTo(3 * Math.hypot(1, 2), 12);
		expect(CHECKS.shiftScale(vals({ mode: 1, s: 1 }))).toBe(true);
		expect(CHECKS.shiftScale(vals({ mode: 1, s: 0.9 }))).toBe(false);
	});
	test('bend: origin fixed, S(v + w) = (3, 4.25) vs (3, 3.25), S(2e₁) = (2, 1) vs 2S(e₁) = (2, 0.5)', () => {
		const z = derive2(vals({ mode: 2 }));
		expect(z.originOk).toBe(true);
		close(z.tvw, [3, 4.25]);
		close(z.sum, [3, 3.25]);
		expect(z.gapAdd).toBeCloseTo(1, 12);
		expect(z.gapScale).toBeCloseTo(6, 12);
		close(mapPt(2, [2, 1], [1, 2], [2, 0]), [2, 1]);
		close(mapPt(2, [2, 1], [1, 2], [1, 0]), [1, BEND]);
	});
	test('bend: a pair with v₁·w₁ = 0 passes the sum test (the lucky challenge), others do not', () => {
		expect(CHECKS.lucky(vals({ mode: 2, w: [0, 3] }))).toBe(true);
		expect(CHECKS.lucky(vals({ mode: 2 }))).toBe(false);
		expect(CHECKS.lucky(vals({ mode: 0, w: [0, 3] }))).toBe(false);
	});
	test('a bad mode throws instead of drawing something', () => {
		expect(() => derive2(vals({ mode: 3 }))).toThrow();
		expect(() => derive2(vals({ mode: 0.5 }))).toThrow();
	});
	test('mapped grid: 2·(2·EXT + 1) lines of SAMPLES segments; bend keeps verticals straight, curves horizontals', () => {
		const g = mappedGrid(2, [2, 1], [1, 2]);
		expect(g.vertical.length).toBe((2 * EXT + 1) * SAMPLES);
		expect(g.horizontal.length).toBe((2 * EXT + 1) * SAMPLES);
		// A vertical image keeps one x; a horizontal image changes slope from segment to segment.
		const vline = g.vertical.slice(0, SAMPLES);
		expect(vline.every(([a, b]) => a[0] === -EXT && b[0] === -EXT)).toBe(true);
		const h = g.horizontal.slice(0, SAMPLES).map(([a, b]) => (b[1] - a[1]) / (b[0] - a[0]));
		expect(h[0]).not.toBeCloseTo(h[SAMPLES - 1], 3);
		// Linear and shifted images are straight: equal slopes along one line.
		const s = mappedGrid(1, [2, 1], [1, 2]).horizontal.slice(0, SAMPLES).map(([a, b]) => (b[1] - a[1]) / (b[0] - a[0]));
		s.forEach((x) => expect(x).toBeCloseTo(s[0], 12));
	});
});

describe('rows', () => {
	test('row ids are locked (term ids shared with formulas and hover links)', () => {
		expect(rowsFor(1, 'en').map((r) => r.id)).toEqual(['a1', 'a2', 'v', 't', 'av', 'area']);
		expect(rowsFor(2, 'en').map((r) => r.id)).toEqual(['mode', 'v', 'w', 's', 'gapAdd', 'gapScale', 'origin']);
		expect(rowsFor(2, 'ru').map((r) => r.id)).toEqual(rowsFor(2, 'en').map((r) => r.id));
	});
	test('the mode row prints names in both languages', () => {
		const en = rowsFor(2, 'en')[0].fmt!;
		const ru = rowsFor(2, 'ru')[0].fmt!;
		expect([0, 1, 2].map(en)).toEqual(['linear', 'shifted', 'bent']);
		expect([0, 1, 2].map(ru)).toEqual(['линейное', 'сдвиг', 'изгиб']);
	});
});
