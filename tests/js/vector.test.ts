import { expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { lessonStore } from '../../src/viz/store';
import { clampMove, writeMove } from '../../src/viz/scene/handles';
import { rowRanges } from '../../src/viz/scene/NPanel';
import { chipLines } from '../../src/viz/scene/chips';
import { readout } from '../../src/viz/lessons/vector';
import { rowsFor } from '../../src/viz/lessons/vector.rows';
import {
	arrow2, BOARD2, BOARD5, CHECKS, clampShift, fmtV, freeLink, INITIAL, moveBoth, moveT, moveTip2, moveWhole2, NORMAL, STRIP, translate, trip,
} from '../../src/viz/lessons/vector.math';

/**
 * Lesson 1.1 model: the free-arrow link of VE-2, the translation drags that must never change components, the w rule
 * of VE-3, B − A and its reversal, the challenges and the rows. Values are checked against hand-computed numbers.
 */
let n = 0;
const fresh = () => {
	const id = 've-test-' + n++;
	return { id, store: lessonStore(id, { ...INITIAL }, { groups: [freeLink()] }) };
};
type Vals = Parameters<typeof arrow2>[0];
const base = { ...INITIAL } as Vals;
const w = (o: Record<string, unknown>) => ({ ...base, ...o }) as Vals;
const close = (a: readonly number[], b: readonly number[]) => a.forEach((x, i) => expect(x).toBeCloseTo(b[i], 12));

test('VE-2: sliding the start keeps the components; the slide stops where the tip would leave the board', () => {
	const { store } = fresh();
	store.getState().set('tail', [5, 5]);
	expect(store.getState().vals).toMatchObject({ tail: [5, 5], w: [3, 2] });
	expect(arrow2(store.getState().vals).to).toEqual([8, 7]);
	// The tip may not pass x = 8: the start stops at 8 − 3 = 5, the components stay (3, 2).
	store.getState().set('tail', [7, -9]);
	expect(store.getState().vals).toMatchObject({ tail: [5, BOARD2.min], w: [3, 2] });
});

test('VE-2: new components keep the start and stop the tip at the board edge; anchoring keeps a free arrow\'s v', () => {
	const { store } = fresh();
	store.getState().set('tail', [5, 5]);
	store.getState().set('w', [10, -1]);
	expect(store.getState().vals).toMatchObject({ tail: [5, 5], w: [BOARD2.max - 5, -1] });
	store.getState().set('anchor', 1);
	const a = arrow2(store.getState().vals);
	expect(a.from).toEqual([0, 0]);
	expect(a.w).toEqual([3, -1]);
	// Anchored, the components are the point: typed values are clamped to the board, the stored start is kept.
	store.getState().set('w', [9, -5]);
	expect(store.getState().vals).toMatchObject({ w: [8, BOARD2.min], tail: [5, 5], anchor: 1 });
	expect(() => store.getState().set('anchor', 2)).toThrow(/0 or 1/);
});

test('VE-2 drags: whole moves the start only (components exact), tip moves only the components; anchored whole moves the point', () => {
	const { id, store } = fresh();
	const ranges = rowRanges(rowsFor(2, 'en'));
	const drag = (m: { set: Record<string, unknown> }) => writeMove(id, clampMove(m.set as never, ranges));
	drag(moveWhole2(store.getState().vals, [5.04, 4.96]));
	expect(store.getState().vals).toMatchObject({ tail: [5, 5], w: [3, 2] });
	// Past the edge the slide stops where the tip meets it (8, 8); the components never change.
	drag(moveWhole2(store.getState().vals, [20, 20]));
	expect(store.getState().vals).toMatchObject({ tail: [5, 6], w: [3, 2] });
	drag(moveTip2(store.getState().vals, [-1, -0.5]));
	expect(store.getState().vals).toMatchObject({ tail: [5, 6], w: [2, 1.5] });
	store.getState().set('anchor', 1);
	drag(moveWhole2(store.getState().vals, [1, 1]));
	expect(store.getState().vals).toMatchObject({ tail: [5, 6], w: [3, 2.5] });
});

test('clampShift and moveBoth: both ends move by the same shift, which stops at the board edge', () => {
	expect(clampShift([1, 2], [4, 6], [10, -10], BOARD5)).toEqual([BOARD5.max - 4, BOARD5.min - 2]);
	expect(clampShift([0, 0], [0, 0], [0.5, -0.5], { min: -1, max: 1 })).toEqual([0.5, -0.5]);
	const m = moveBoth('A', 'B', BOARD5)(base, [-1.04, -2.02]);
	expect(m.set).toEqual({ A: [0, 0], B: [3, 4] });
	const edge = moveBoth('A', 'B', BOARD5)(base, [9, 9]).set as Record<string, number[]>;
	expect(edge.B).toEqual([7, 7]);
	expect(trip(w(edge)).d).toEqual([3, 4]);
	const three = moveBoth('A3', 'B3', { min: -3, max: 3 })(base, [0, 0, 5]).set as Record<string, number[]>;
	close(trip(w(three), true).d, trip(base, true).d);
});

test('VE-3: points get the translation (w = 1), the normal does not (w = 0); the bug adds T to the normal', () => {
	expect(Math.hypot(...NORMAL)).toBeCloseTo(1, 12);
	// Perpendicular to the strip direction (1.5, −1).
	expect(NORMAL[0] * 1.5 - NORMAL[1] * 1).toBeCloseTo(0, 12);
	const z = translate(w({ T: [3, 3] }));
	expect(z.after.map((p, i) => [p[0] - STRIP[i][0], p[1] - STRIP[i][1]])).toEqual([[3, 3], [3, 3], [3, 3]]);
	expect(z.N2).toEqual([...NORMAL]);
	const bug = translate(w({ T: [3, 3], asPoint: 1 }));
	close(bug.N2, [NORMAL[0] + 3, NORMAL[1] + 3]);
	expect(bug.lenN2).toBeGreaterThan(1);
	expect(moveT(w({ T: [1, 1] }), [0.26, -0.04]).set).toEqual({ T: [1.3, 1] });
});

test('B − A: the example (3, 4) of length 5; the trip back is its negative; A = B is the zero vector', () => {
	const z = trip(base);
	expect(z.d).toEqual([3, 4]);
	expect(z.len).toBe(5);
	const back = trip(w({ rev: 1 }));
	expect(back.d).toEqual([-3, -4]);
	expect(back.len).toBe(5);
	expect(trip(w({ B: [1, 2] })).len).toBe(0);
	// Exercise 2: (2, 0, 1) -> (5, 4, 1) is (3, 4, 0).
	expect(trip(w({ A3: [2, 0, 1], B3: [5, 4, 1] }), true).d).toEqual([3, 4, 0]);
});

test('challenges: every default is unsolved; each has a solution that typing alone reaches', () => {
	for (const [name, check] of Object.entries(CHECKS)) expect([name, check(base)]).toEqual([name, false]);
	expect(CHECKS.v1(w({ v: [-2, 3] }))).toBe(true);
	expect(CHECKS.down(w({ v: [0, -1.5] }))).toBe(true);
	expect(CHECKS.down(w({ v: [0, 0] }))).toBe(false);
	expect(CHECKS.slide(w({ tail: [5, 5] }))).toBe(true);
	expect(CHECKS.slide(w({ tail: [5, 5], anchor: 1 }))).toBe(false);
	expect(CHECKS.position(w({ anchor: 1, w: [5, 5] }))).toBe(true);
	expect(CHECKS.move33(w({ T: [3, 3] }))).toBe(true);
	expect(CHECKS.bug(w({ asPoint: 1 }))).toBe(true);
	expect(CHECKS.bug(w({ asPoint: 1, T: [0, 0] }))).toBe(false);
	expect(CHECKS.up(w({ v3: [0, 2, 0] }))).toBe(true);
	expect(CHECKS.flat(w({ v3: [1, 0, -2] }))).toBe(true);
	expect(CHECKS.flat(w({ v3: [0, 0, 0] }))).toBe(false);
	expect(CHECKS.back(w({ A: [4, 6], B: [1, 2] }))).toBe(true);
	expect(CHECKS.back(w({ rev: 1 }))).toBe(false);
	expect(CHECKS.origin(w({ A: [0, 0], B: [3, 4] }))).toBe(true);
	expect(CHECKS.trip3(w({ A3: [-1, 0, 1], B3: [2, 4, 1] }))).toBe(true);
});

test('readouts and labels: typographic minus, one decimal, the zero and reversed cases', () => {
	expect(fmtV([-3, 0.04])).toBe('(−3.0, 0.0)');
	expect(readout(1, 'en', base)).toContain('3.0 along x and 2.0 along y');
	expect(readout(1, 'ru', w({ v: [0, 0] }))).toContain('Нулевой вектор');
	expect(readout(5, 'en', w({ rev: 1 }))).toContain('A − B = (−3.0, −4.0)');
	expect(readout(5, 'en', w({ B: [1, 2] }))).toContain('zero vector');
	expect(readout(3, 'en', w({ asPoint: 1 }))).toContain('length');
	expect(readout(2, 'en', w({ anchor: 1 }))).toContain('P = (3.0, 2.0)');
});

test('rows: semantic ids, ranges and math labels identical in EN and RU; chips reserve lines', () => {
	const ids = [['v'], ['w', 'tail', 'head', 'anchor'], ['T', 'N2', 'asPoint'], ['v3'], ['A', 'B', 'd', 'len', 'rev'], ['A3', 'B3', 'd3', 'len3']];
	([1, 2, 3, 4, 5, 6] as const).forEach((scene, i) => {
		const en = rowsFor(scene, 'en'), ru = rowsFor(scene, 'ru');
		expect(en.map((r) => r.id)).toEqual(ids[i]);
		expect(ru.map((r) => r.id)).toEqual(ids[i]);
		const contract = (rows: typeof en) => rows.map(({ label, tip, group, value, fmt, visible, ...row }) => ({ ...row, value: value?.toString(), fmt: fmt?.toString(), visible: visible?.toString() }));
		expect(contract(ru)).toEqual(contract(en));
		expect(chipLines(en)).toBeGreaterThan(0);
		expect(chipLines(en)).toBe(chipLines(ru));
	});
	// The "trip back" label has the width of the forward one, so toggling it never moves the chips.
	expect(chipLines(rowsFor(5, 'en', 1))).toBe(chipLines(rowsFor(5, 'en', 0)));
	expect(rowsFor(5, 'en', 1)[2].label).toBe('A − B');
});
