// Lesson 3.5 math and choreography (src/viz/lessons/reductions.*) against independent references: NumPy's printed
// results for the page's examples, hand-written loops, and the frame the renderer draws.
import { describe, expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { lessonStore, type LessonState } from '../../src/viz/store';
import { Frame } from '../../src/viz/dotunit/choreo';
import { flat } from '../../src/core';
import {
	CHECKS, code, compute, firstTie, INITIAL, LINKS, model, orderRun, SHEET, source, SUM_NS,
} from '../../src/viz/lessons/reductions.math';
import { CAP, geo, instances, linked, writeFrame, writeIds, RESULT } from '../../src/viz/lessons/reductions.scene';
import { reduceRows } from '../../src/viz/lessons/reductions.rows';

type Vals = LessonState['vals'];
const vals = (patch: Record<string, number> = {}): Vals => ({ ...INITIAL, ...patch });
const res = (v: Vals) => flat(model(v).r);

describe('results as NumPy prints them', () => {
	test('the sheet: sums, means, maxima along each axis and over all', () => {
		expect(res(vals())).toEqual([8, 8, 2]);
		expect(res(vals({ ax: 0 }))).toEqual([3, 7, 4, 4]);
		expect(res(vals({ all: 1 }))).toEqual([18]);
		expect(res(vals({ op: 1 }))).toEqual([2, 2, 0.5]);
		expect(res(vals({ op: 1, all: 1 }))).toEqual([1.5]);
		expect(res(vals({ op: 2 }))).toEqual([4, 5, 3]);
	});

	test('shapes: the named axis disappears; keepdims keeps it with size 1; negative axes count from the end', () => {
		expect(model(vals()).out).toEqual([3]);
		expect(model(vals({ kd: 1 })).out).toEqual([3, 1]);
		expect(model(vals({ ax: -1 })).out).toEqual([3]);
		expect(res(vals({ ax: -1 }))).toEqual(res(vals({ ax: 1 })));
		expect(res(vals({ ax: -2 }))).toEqual(res(vals({ ax: 0 })));
		expect(model(vals({ all: 1 })).out).toEqual([]);
		expect(model(vals({ all: 1, kd: 1 })).out).toEqual([1, 1]);
		expect(model(vals({ src: 1, ax: 1 })).out).toEqual([2, 4]);
	});

	test('the grayscale icon: brightest pixel 231 (six wall pixels tie), mean 139.6875', () => {
		const m = model(vals({ src: 2, op: 2, all: 1 }));
		expect(flat(m.r)).toEqual([231]);
		const tie = firstTie(m)!;
		expect(tie.cells.length).toBe(6);
		// NumPy: np.unravel_index(np.argmax(g), g.shape) == (5, 1), flat index 41.
		expect(tie.cells[0]).toBe(41);
		expect(res(vals({ src: 2, op: 1, all: 1 }))).toEqual([139.6875]);
		expect(res(vals({ src: 2, op: 0, ax: 1 }))).toEqual([1294, 1294, 1129, 1051, 973, 1298, 1245, 656]);
	});

	test('max ties of the sheet: row 1 holds 5 twice', () => {
		const m = model(vals({ op: 2 }));
		expect(firstTie(m)).toEqual({ cell: 1, cells: [5, 7] });
		expect(firstTie(model(vals()))).toBeNull();
	});
});

describe('which cell collapses into which', () => {
	test('group, landing cell and count against a hand-written loop', () => {
		for (const src of [0, 1, 2])
			for (const ax of [0, 1, 2, -1]) {
				const x = source((['sheet', 'block', 'gray'] as const)[src]);
				if (ax >= x.ndim) continue;
				const m = compute(x, 0, ax, false);
				const d = ax < 0 ? ax + x.ndim : ax;
				const sums = new Array<number>(m.r.size).fill(0);
				for (let k = 0; k < x.size; k++) sums[m.group[k]] += x.data[k];
				expect(sums).toEqual(flat(m.r));
				for (let c = 0; c < m.r.size; c++) {
					expect(m.count[c]).toBe(x.shape[d]);
					expect(m.group[m.first[c]]).toBe(c);
				}
			}
	});

	test('the landing cell has index 0 along the reduced axis', () => {
		const m = compute(source('sheet'), 0, 0, false);
		expect(Array.from(m.first)).toEqual([0, 1, 2, 3]);
		const r = compute(source('sheet'), 0, 1, false);
		expect(Array.from(r.first)).toEqual([0, 4, 8]);
	});
});

describe('link group', () => {
	test('the axis is clamped to the rank of the data in the same write', () => {
		const s = lessonStore('reductions-test-clamp', INITIAL, LINKS);
		s.getState().set('src', 1);
		s.getState().set('ax', 2);
		expect(s.getState().vals.ax).toBe(2);
		s.getState().set('src', 0);
		expect(s.getState().vals.ax).toBe(1);
		s.getState().set('ax', -3);
		expect(s.getState().vals.ax).toBe(-2);
		s.getState().set('kd', 0.7);
		expect(s.getState().vals.kd).toBe(1);
	});
});

describe('machine layer: summation order in float32', () => {
	test('a million 0.1s: the running total drifts to 100958.34375, the tree and the wide accumulator do not', () => {
		const r = orderRun(1_000_000);
		// NumPy: np.cumsum(np.full(10**6, 0.1, np.float32))[-1] == 100958.34
		expect(r.seq.sum).toBe(100958.34375);
		expect(r.tree.sum).toBe(100000);
		expect(r.wide.sum).toBe(100000);
		expect(r.exact).toBeCloseTo(100000.00149011612, 6);
		expect(r.seq.mean).toBe(Math.fround(100958.34375 / 1e6));
	});

	test('ten numbers are already off in the last digit when added in order', () => {
		const r = orderRun(SUM_NS[0]);
		expect(r.seq.sum).toBe(Math.fround(1.0000001192092896));
		expect(r.tree.sum).toBe(1);
	});
});

describe('choreography', () => {
	test('every source fits the frame capacity', () => {
		for (const src of [0, 1, 2]) for (const all of [0, 1]) expect(instances(model(vals({ src, all, ax: 0 })))).toBeLessThanOrEqual(CAP);
	});

	test('input, collapse onto the landing cell, then the result below in its own shape', () => {
		const m = model(vals());
		const g = geo(m);
		const f = new Frame(CAP);
		writeIds(m, f);
		const n = m.x.size, r0 = 2 * n;
		writeFrame(m, g, 0, 1, new Set(), f);
		expect(f.alpha[0]).toBe(1);
		expect(f.alpha[n + 3]).toBe(0);
		expect(f.alpha[r0]).toBe(0);
		writeFrame(m, g, 1, 1, new Set(), f);
		// Result 1 (row 1) sits on its landing cell x[1, 0] = input cell 4.
		expect(f.c[3 * (r0 + 1)]).toBeCloseTo(g.input.pos[3 * 4], 6);
		expect(f.c[3 * (r0 + 1) + 1]).toBeCloseTo(g.input.pos[3 * 4 + 1], 6);
		expect(f.alpha[r0 + 1]).toBe(1);
		expect(f.s[3 * r0]).toBeCloseTo(RESULT, 6);
		writeFrame(m, g, 2, 1, new Set(), f);
		expect(f.c[3 * (r0 + 2)]).toBeCloseTo(g.output.pos[3 * 2], 6);
		expect(f.c[3 * (r0 + 2) + 1]).toBeCloseTo(g.output.pos[3 * 2 + 1], 6);
		// Without keepdims the three row sums lie in a row (shape (3,)); with keepdims in a column (3, 1).
		expect(g.output.w).toBe(3);
		expect(geo(model(vals({ kd: 1 }))).output.h).toBe(3);
		expect(f.id[r0 + 2]).toBe('r2');
	});

	test('max: every tying cell flashes once the collapse starts; a result lights its whole group', () => {
		const m = model(vals({ op: 2 }));
		const f = new Frame(CAP);
		writeIds(m, f);
		writeFrame(m, geo(m), 1, 0.5, new Set(), f);
		expect(f.flash[5]).toBe(1);
		expect(f.flash[7]).toBe(1);
		expect(f.flash[4]).toBe(0);
		expect([...linked(m, new Set(['r0']))].sort()).toEqual(['r0', 'x0', 'x1', 'x2', 'x3']);
		expect([...linked(m, new Set(['x6']))].sort()).toEqual(['r1', 'x4', 'x5', 'x6', 'x7']);
	});
});

describe('challenges, code and rows', () => {
	test('no challenge holds on the initial values; each holds on its target', () => {
		for (const check of Object.values(CHECKS)) expect(check(vals())).toBe(false);
		expect(CHECKS.colSums(vals({ ax: 0 }))).toBe(true);
		expect(CHECKS.colSums(vals({ ax: -2 }))).toBe(true);
		expect(CHECKS.keep(vals({ kd: 1 }))).toBe(true);
		expect(CHECKS.brightest(vals({ src: 2, op: 2, all: 1 }))).toBe(true);
		expect(CHECKS.negative(vals({ src: 1, ax: -1 }))).toBe(true);
		expect(CHECKS.negative(vals({ src: 1, ax: 2 }))).toBe(false);
	});

	test('the NumPy call of the shown reduction', () => {
		expect(code(model(vals()))).toBe('x.sum(axis=1)');
		expect(code(model(vals({ op: 2, all: 1, kd: 1 })))).toBe('x.max(axis=None, keepdims=True)');
		expect(code(model(vals({ op: 1, ax: -1 })))).toBe('x.mean(axis=-1)');
	});

	test('every editable row writes a key of the store; the sheet is the page\'s matrix', () => {
		for (const r of reduceRows('en')) if (r.key) expect(Object.hasOwn(INITIAL, r.key)).toBe(true);
		expect(SHEET.flat()).toEqual([2, -1, 4, 3, 0, 5, -2, 5, 1, 3, 2, -4]);
	});
});
