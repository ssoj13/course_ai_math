import { expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { chipLines } from '../../src/viz/scene/chips';
import { readout } from '../../src/viz/lessons/vector-arithmetic';
import { rowsFor } from '../../src/viz/lessons/vector-arithmetic.rows';
import {
	blend, CHECKS, combo, combo3, CORNER, INITIAL, JAW, lattice, MOUTH, scaled, SMILE, sum,
} from '../../src/viz/lessons/vector-arithmetic.math';

/**
 * Lesson 1.2 model against the prose's worked numbers and the edge cases: zero and negative coefficients, aligned and
 * opposite sums, parallel arrows (no plane), u = 0 (the plane), negative blend weights.
 */
type Vals = Parameters<typeof sum>[0];
const base = { ...INITIAL } as Vals;
const w = (o: Record<string, unknown>) => ({ ...base, ...o }) as Vals;
const close = (a: readonly number[], b: readonly number[]) => a.forEach((x, i) => expect(x).toBeCloseTo(b[i], 12));

test('VA-1: (1, 2) + (3, −1) = (4, 1); both walks end at the sum; lengths add only for aligned arrows', () => {
	const z = sum(base);
	expect(z.ab).toEqual([4, 1]);
	expect(z.nab).toBeLessThan(z.na + z.nb);
	expect(z.aligned).toBe(false);
	// (3, 0) + (0, 4) = (3, 4): length 5, not 7 (the misconception of the prose).
	const right = sum(w({ a: [3, 0], b: [0, 4] }));
	expect([right.na, right.nb, right.nab]).toEqual([3, 4, 5]);
	expect(sum(w({ a: [1, 2], b: [2, 4] })).aligned).toBe(true);
	expect(sum(w({ a: [1, 2], b: [-2, -4] })).aligned).toBe(false);
	expect(sum(w({ a: [1, 2], b: [0, 0] })).aligned).toBe(false);
});

test('VA-2: 3·(2, −1) = (6, −3); s = −1 flips, s = 0 is the zero vector, the length factor is |s|', () => {
	expect(scaled(base).sc).toEqual([6, -3]);
	expect(scaled(w({ s: -1 }))).toMatchObject({ sc: [-2, 1], sense: -1, factor: 1 });
	const zero = scaled(w({ s: 0 }));
	expect(zero.sense).toBe(0);
	expect(zero.sc.every((x) => x === 0)).toBe(true);
	expect(scaled(w({ s: 0.5 })).sc).toEqual([1, -0.5]);
});

test('VA-3: 2a − b = (−1, 5); (5, 3) = 2a + b; parallel arrows are detected, zero arrows are not parallel', () => {
	expect(combo(w({ ls: 2, lt: -1 })).r).toEqual([-1, 5]);
	expect(combo(w({ ls: 2, lt: 1 })).r).toEqual([5, 3]);
	expect(combo(w({ ls: 0, lt: 0 })).r).toEqual([0, 0]);
	// The axis arrows: s·(1, 0) + t·(0, 1) = (s, t).
	expect(combo(w({ la: [1, 0], lb: [0, 1], ls: 2, lt: -1.5 })).r).toEqual([2, -1.5]);
	expect(combo(w({ lb: [2, 4] })).parallel).toBe(true);
	expect(combo(w({ lb: [-0.3, -0.6] })).parallel).toBe(true);
	expect(combo(w({ lb: [0, 0] }))).toMatchObject({ parallel: false, zero: true });
	// Lattice: one line per whole s and per whole t over [−2, 2]; each s-line runs along b.
	const lines = lattice([1, 2], [3, -1]);
	expect(lines.length).toBe(10);
	expect(lines[0]).toEqual([[-2 - 6, -4 + 2], [-2 + 6, -4 - 2]]);
});

test('VA-4: r = a + b + c with the defaults; u = 0 stays on the plane of a and b (its normal is perpendicular to r)', () => {
	expect(combo3(base).r).toEqual([1.5, 1.5, 2.5]);
	const flat = combo3(w({ u3: 0 }));
	expect(flat.onPlane).toBe(true);
	expect(flat.r).toEqual([1.5, 0, 2.5]);
	// Normal of the plane of a = (2, 0, .5) and b = (−.5, 0, 2) is ±y; r has no y component.
	expect(flat.r[1]).toBe(0);
	expect(combo3(w({ s3: 0, t3: 0, u3: 2 })).r).toEqual([0, 3, 0]);
});

test('VA-5: the prose vertex (1.0, 0.5) + 0.5·(0.2, 0.3) + 0.25·(0.0, −0.4) = (1.1, 0.55); every vertex blends on its own', () => {
	expect(MOUTH[CORNER]).toEqual([1, 0.5]);
	expect(SMILE[CORNER]).toEqual([0.2, 0.3]);
	expect(JAW[CORNER]).toEqual([0, -0.4]);
	close(blend(base).corner, [1.1, 0.55]);
	const z = blend(w({ w1: 1, w2: 0.75 }));
	close(z.corner, [1.2, 0.5]);
	z.out.forEach((p, i) => close(p, [MOUTH[i][0] + SMILE[i][0] + 0.75 * JAW[i][0], MOUTH[i][1] + SMILE[i][1] + 0.75 * JAW[i][1]]));
	// Zero weights: the base shape exactly.
	expect(blend(w({ w1: 0, w2: 0 })).out).toEqual(MOUTH.map((p) => [...p]));
	// A negative smile weight pulls the corners down: a frown.
	expect(blend(w({ w1: -0.5, w2: 0 })).corner[1]).toBeLessThan(MOUTH[CORNER][1]);
});

test('challenges: every default is unsolved; each has a solution reachable by typing', () => {
	for (const [name, check] of Object.entries(CHECKS)) expect([name, check(base)]).toEqual([name, false]);
	expect(CHECKS.cancel(w({ b: [-1, -2] }))).toBe(true);
	expect(CHECKS.cancel(w({ a: [0, 0], b: [0, 0] }))).toBe(false);
	expect(CHECKS.aligned(w({ b: [2, 4] }))).toBe(true);
	expect(CHECKS.flip(w({ s: -1 }))).toBe(true);
	expect(CHECKS.zero(w({ s: 0 }))).toBe(true);
	expect(CHECKS.reach(w({ ls: 2, lt: 1 }))).toBe(true);
	expect(CHECKS.line(w({ paint: 1, lb: [2, 4] }))).toBe(true);
	expect(CHECKS.line(w({ paint: 0, lb: [2, 4] }))).toBe(false);
	expect(CHECKS.up(w({ s3: 0, t3: 0, u3: 2 }))).toBe(true);
	expect(CHECKS.plane(w({ u3: 0 }))).toBe(true);
	expect(CHECKS.plane(w({ s3: 0, t3: 0, u3: 0 }))).toBe(false);
	expect(CHECKS.level(w({ w1: 1, w2: 0.75 }))).toBe(true);
	expect(CHECKS.frown(w({ w1: -0.05 }))).toBe(true);
});

test('readouts: the zero sum, flipped and zero scaling, parallel arrows, the plane', () => {
	expect(readout(1, 'en', w({ b: [-1, -2] }))).toContain('undoes');
	expect(readout(1, 'en', base)).toContain('a + b = (4.0, 1.0)');
	expect(readout(2, 'en', w({ s: -1.5 }))).toContain('turned around');
	expect(readout(2, 'ru', w({ s: 0 }))).toContain('нулевой вектор');
	expect(readout(3, 'en', w({ lb: [2, 4] }))).toContain('one line');
	expect(readout(4, 'en', w({ u3: 0 }))).toContain('plane of a and b');
	expect(readout(5, 'en', base)).toContain("P' = (1.10, 0.55)");
});

test('rows: semantic ids identical in EN and RU; chips reserve the same lines', () => {
	const ids = [['a', 'b', 'sum', 'na', 'nb', 'nsum'], ['c', 's', 'sc'], ['la', 'lb', 'ls', 'lt', 'r', 'paint'], ['a3', 'b3', 'c3', 's3', 't3', 'u3', 'r3'], ['w1', 'w2', 'corner']];
	([1, 2, 3, 4, 5] as const).forEach((scene, i) => {
		const en = rowsFor(scene, 'en'), ru = rowsFor(scene, 'ru');
		expect(en.map((r) => r.id)).toEqual(ids[i]);
		expect(ru.map((r) => r.id)).toEqual(ids[i]);
		const contract = (rows: typeof en) => rows.map(({ label, tip, group, value, fmt, visible, ...row }) => ({ ...row, value: value?.toString(), fmt: fmt?.toString(), visible: visible?.toString() }));
		expect(contract(ru)).toEqual(contract(en));
		expect(chipLines(en)).toBeGreaterThan(0);
	});
});
