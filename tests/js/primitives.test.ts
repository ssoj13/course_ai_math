import '../../src/i18n/ui';
// Pure parts of the Task 6e lesson primitives: magnet, circle/tick/half-plane geometry, the chip model, the ledger
// layout, the screen-space wedge and the demo's custom drag mappings. No DOM, no three.
import { expect, test } from 'bun:test';
import { arcTicks, circlePoints, cross, dot, halfPlanePoly, norm, planeBasis, signedArea2 } from '../../src/viz/geom';
import { magnet } from '../../src/viz/scene/drag';
import { CHIP, chipAvail, chipBox, chipHeight, chipLines, frameMismatch, wrapLines } from '../../src/viz/scene/chips';
import { clampMove, screenWedge } from '../../src/viz/scene/handles';
import { rowRanges } from '../../src/viz/scene/NPanel';
import { fmt2, ledgerLayout, spreadLabels } from '../../src/viz/Ledger';
import { lamp, MAGNET, moveA, moveB, moveLight, moveP, moveSb, ROWS2, ROWS3, ROWSLAMP, ROWSLC, ROWSPOLAR, ROWSSIGN, SPHERE_R, tri } from '../../src/viz/demo/engine.rows';

const close = (a: readonly number[], b: readonly number[], eps = 1e-9) => a.every((x, i) => Math.abs(x - b[i]) < eps) && a.length === b.length;

test('magnet: nearest target within the zone (inclusive), else unchanged', () => {
	expect(magnet(58.4, MAGNET, 2.5)).toBe(60);
	expect(magnet(57.5, MAGNET, 2.5)).toBe(60);
	expect(magnet(57.4, MAGNET, 2.5)).toBe(57.4);
	expect(magnet(46, [45, 47], 2.5)).toBe(45); // ties of distance keep the first, a closer one wins
	expect(magnet(46.5, [45, 47], 2.5)).toBe(47);
	expect(magnet(-1, MAGNET, 2.5)).toBe(0);
});

test('magnet with `from` is one-way: a step off a target never snaps back, a step toward one catches it', () => {
	// The keyboard trap this prevents: 0.1 world units at radius 4 is ~1.4 degrees, less than the 2.5 degree zone.
	expect(magnet(58.6, MAGNET, 2.5, 60)).toBe(58.6);
	expect(magnet(57.2, MAGNET, 2.5, 58.6)).toBe(57.2);
	expect(magnet(58.6, MAGNET, 2.5, 57.2)).toBe(60);
	expect(magnet(61.2, MAGNET, 2.5, 59)).toBe(60); // crossing the target catches it
});

test('circlePoints: closed loop on the radius, 2D in z = 0, 3D perpendicular to the normal', () => {
	const c2 = circlePoints([1, 2], 3, 64)!;
	expect(c2.length).toBe(65);
	expect(c2[64]).toEqual(c2[0]);
	for (const p of c2) expect(Math.hypot(p[0] - 1, p[1] - 2)).toBeCloseTo(3, 12);
	const n = [1, 2, 2];
	const c3 = circlePoints([0, 1, 0], 2, 32, n)!;
	for (const p of c3) {
		const d = [p[0], p[1] - 1, p[2]];
		expect(norm(d)).toBeCloseTo(2, 12);
		expect(dot(d, n)).toBeCloseTo(0, 12);
	}
	expect(circlePoints([0, 0, 0], 1, 16, [0, 0, 0])).toBeNull();
	expect(() => circlePoints([0, 0, 0], 1)).toThrow();
	expect(() => circlePoints([0, 0], 1, 16, [0, 0, 1])).toThrow();
	expect(() => circlePoints([0, 0], -1)).toThrow();
});

test('planeBasis: orthonormal and right-handed for any normal, also along an axis', () => {
	for (const n of [[0, 1, 0], [1, 0, 0], [0.3, -2, 5], [0, 0, -4]]) {
		const [u, v] = planeBasis(n)!;
		expect(norm(u)).toBeCloseTo(1, 12);
		expect(norm(v)).toBeCloseTo(1, 12);
		expect(dot(u, v)).toBeCloseTo(0, 12);
		expect(dot(u, n)).toBeCloseTo(0, 12);
		expect(dot(cross(u, v), n)).toBeGreaterThan(0);
	}
	expect(planeBasis([0, 0, 0])).toBeNull();
});

test('arcTicks: radial marks at the target angles, measured from `from` toward the arc side', () => {
	const t = arcTicks([0, 0], [1, 0], [0, 2], 1, [0, 90, 200], 0.1);
	expect(t.length).toBe(4); // 200 is outside [0, 180]
	expect(close(t[0], [1, 0])).toBe(true);
	expect(close(t[1], [1.1, 0])).toBe(true);
	expect(close(t[2], [0, 1])).toBe(true);
	// `to` clockwise of `from`: the ticks follow the arc to that side.
	const cw = arcTicks([0, 0], [1, 0], [0, -1], 1, [90], 0.1);
	expect(close(cw[0], [0, -1])).toBe(true);
	// Parallel rays: 2D counter-clockwise, so 90 is +y.
	expect(close(arcTicks([0, 0], [2, 0], [1, 0], 1, [90], 0.1)[0], [0, 1])).toBe(true);
	expect(arcTicks([0, 0], [0, 0], [1, 0], 1, [90], 0.1)).toEqual([]);
});

test('halfPlanePoly: the clipped square on the right side, CCW, empty for a zero normal', () => {
	const pos = halfPlanePoly([1, 0], 1, 10);
	expect(pos.every((p) => p[0] >= 0)).toBe(true);
	const area = (ps: number[][]) => ps.reduce((s, p, i) => s + signedArea2(p, ps[(i + 1) % ps.length]), 0) / 2;
	expect(area(pos)).toBeCloseTo(200, 9);
	const neg = halfPlanePoly([1, 1], -1, 10);
	expect(neg.every((p) => p[0] + p[1] <= 1e-9)).toBe(true);
	expect(area(neg)).toBeCloseTo(200, 9);
	expect(halfPlanePoly([0, 0], 1, 10)).toEqual([]);
	expect(halfPlanePoly([3, 1], 1, 10).length).toBeLessThanOrEqual(5);
});

test('chip model: widths from the row declaration only, greedy wrapping, height with the row gap', () => {
	expect(chipBox({ id: 'r', label: 'arc r', kind: 'num', spec: { min: 0.3, max: 2, step: 0.1 } })).toEqual({ label: 35, field: 28 + 4 * 7, w: 35 + 5 + 56 });
	const v = chipBox({ id: 'a', label: 'a', kind: 'vec', dims: 3, spec: { min: -3, max: 3, step: 0.1 } });
	expect(v.field).toBe(32 + 5 * 7); // "−3.00"
	expect(v.w).toBe(7 + 5 + 3 * v.field + 2);
	expect(chipBox({ id: 'f', label: 'flip', kind: 'bool' }).w).toBe(4 * 7 + 5 + CHIP.box);
	expect(chipBox({ id: 'c', label: 'c', kind: 'ro' }).field).toBe(8 + CHIP.roChars * 7);
	expect(() => chipBox({ id: 'v', label: 'v', kind: 'vec', spec: { min: 0, max: 1, step: 0.1 } })).toThrow();
	expect(wrapLines([100, 100, 100], 212)).toBe(2); // 100 + 12 + 100 = 212 fits, a third does not
	expect(wrapLines([100, 100, 100], 211)).toBe(3);
	expect(wrapLines([500], 100)).toBe(1); // an over-wide chip sits alone on its line
	expect(chipAvail(320)).toBe(272);
	expect(chipHeight(1)).toBe(1 + 10 + 20);
	expect(chipHeight(4)).toBe(1 + 10 + 80 + 12);
	expect(chipHeight(0)).toBe(0);
});

test('chip model: wider phones never need more lines than the 320 px basis (the reservation is an upper bound)', () => {
	for (const rows of [ROWS2('en'), ROWS3('en'), ROWSLC('en'), ROWSPOLAR('en'), ROWSSIGN('en'), ROWSLAMP('en')]) {
		const at320 = chipLines(rows, 320);
		for (let w = 320; w < 720; w += 7) expect(chipLines(rows, w)).toBeLessThanOrEqual(at320);
	}
});

test('screenWedge: bisector on screen at the furthest projected radius; straight angles use the arc middle', () => {
	const w = screenWedge([0, 0], [10, -10], [20, 0], [0, -20])!;
	expect(w.angle).toBeCloseTo(Math.PI / 2, 12);
	expect(close(w.at, [20 / Math.SQRT2, -20 / Math.SQRT2])).toBe(true);
	// Foreshortened (3D perspective): the projected sides are not symmetric about the projected arc middle.
	const f = screenWedge([0, 0], [5, -2], [30, 0], [0, -10])!;
	expect(Math.atan2(-f.at[1], f.at[0])).toBeCloseTo(Math.PI / 4, 12);
	expect(Math.hypot(...f.at)).toBeCloseTo(30, 12);
	const s = screenWedge([0, 0], [0, -10], [10, 0], [-10, 0])!;
	expect(s.angle).toBeCloseTo(Math.PI, 6);
	expect(close(s.at, [0, -10])).toBe(true);
	expect(screenWedge([0, 0], [1, 1], null, [1, 0])).toBeNull();
	expect(screenWedge([0, 0], [1, 1], [0, 0], [1, 0])).toBeNull();
});

test('ledger layout: stacked areas, a signed last band hanging down or added on top, a zero line', () => {
	const neg = ledgerLayout(['a2', 'b2', 'corr'], [9, 16, -12]);
	expect(neg.bars).toEqual([
		{ id: 'a2', lo: 0, hi: 9, neg: false },
		{ id: 'b2', lo: 9, hi: 25, neg: false },
		{ id: 'corr', lo: 13, hi: 25, neg: true },
	]);
	const pos = ledgerLayout(['a2', 'b2', 'corr'], [9, 16, 12]);
	expect(pos.bars[2]).toEqual({ id: 'corr', lo: 25, hi: 37, neg: false });
	const zero = ledgerLayout(['a2', 'b2', 'corr'], [9, 16, 1e-13]);
	expect(zero.zero).toBe(true);
	expect(zero.bars.length).toBe(2);
	expect(zero.top).toBe(25);
	expect(() => ledgerLayout(['a2', 'b2', 'corr'], [-1, 16, 0])).toThrow();
	expect(() => ledgerLayout(['a2'], [1])).toThrow();
});

test('spreadLabels keeps order, a minimum gap and the bounds', () => {
	expect(spreadLabels([50, 52, 100], 0, 200, 15)).toEqual([50, 65, 100]);
	expect(spreadLabels([195, 198, 199], 0, 200, 15)).toEqual([170, 185, 200]);
	expect(spreadLabels([52, 50], 0, 200, 15)).toEqual([65, 50]);
	expect(fmt2(-0.001)).toBe('0.00');
	expect(fmt2(-2.5)).toBe('−2.50');
	expect(fmt2(Number.NaN)).toBe('–');
});

const base = { la: 3, lb: 4, lth: 60, sa: [2, 1], sb: [1, 2], nl_light: [2, 2.5, 1.5], nl_pt: [0, 0, SPHERE_R], nl_raw: 0 };

test('moveB slides B along x and writes the clamped side length', () => {
	expect(moveB(base, [0.47, 3])).toEqual({ set: { la: 3.5 } });
	expect(moveB(base, [-9, 0]).set).toEqual({ la: 0.5 });
	expect(moveB(base, [9, 0]).set).toEqual({ la: 4 });
});

test('moveA: polar length and angle, the magnet on drags (eased catch), one-way on key steps, folding below the axis', () => {
	const A = tri(base).A;
	const to = (deg: number, r: number) => [r * Math.cos((deg * Math.PI) / 180) - A[0], r * Math.sin((deg * Math.PI) / 180) - A[1]];
	// Drag A to 58 degrees at radius 4: the angle sticks to 60 and only the angle is marked to ease; b stays 1:1.
	expect(moveA(base, to(58, 4), 'drag')).toEqual({ set: { lb: 4, lth: 60 }, ease: ['lth'] });
	// Outside every zone: nothing eases.
	expect(moveA(base, to(70, 3.04), 'drag')).toEqual({ set: { lb: 3, lth: 70 }, ease: [] });
	// A key step of 0.1 to the right from 60 degrees: about 1.2 degrees, off the target, and it stays off.
	expect(moveA(base, [0.1, 0], 'key').set.lth).toBe(59);
	expect(moveA({ ...base, lth: 59 }, [0.1, 0], 'key').set.lth).toBe(58);
	// Below the x axis: folds to the nearer end of [0, 180].
	expect(moveA(base, to(-20, 4), 'drag').set.lth).toBe(0);
	expect(moveA(base, to(-160, 4), 'drag').set.lth).toBe(180);
});

test('moveSb: snaps exactly perpendicular within 3 degrees, on b own side, eased; a key step can leave', () => {
	const a = base.sa;
	// b = (-1, 2.1): 2.6 degrees off the perpendicular, counter-clockwise of a.
	const m = moveSb(base, [-2, 0.1], 'drag');
	expect(m.ease).toEqual(['sb']);
	const caught = m.set.sb as number[];
	expect(Math.abs(dot(a, caught))).toBeLessThan(1e-12);
	expect(signedArea2(a, caught)).toBeGreaterThan(0);
	expect(norm(caught)).toBeCloseTo(Math.hypot(-1, 2.1), 12);
	// Clockwise side.
	const cw = moveSb(base, [0.1, -4], 'drag').set.sb as number[]; // b = (1.1, -2)
	expect(Math.abs(dot(a, cw))).toBeLessThan(1e-12);
	expect(signedArea2(a, cw)).toBeLessThan(0);
	// Far from the perpendicular: plain grid move, no ease.
	expect(moveSb(base, [0.5, 0], 'drag')).toEqual({ set: { sb: [1.5, 2] } });
	// A key step away from the perpendicular is not pulled back.
	expect(moveSb({ ...base, sb: [-1, 2] }, [0.1, 0], 'key')).toEqual({ set: { sb: [-0.9, 2] } });
});

test('moveLight keeps the light outside the sphere; moveP keeps P on it', () => {
	expect(moveLight(base, [0.1, 0, 0])).toEqual({ set: { nl_light: [2.1, 2.5, 1.5] } });
	const inside = moveLight({ ...base, nl_light: [2, 0, 0] }, [-1, 0, 0]).set.nl_light as number[];
	expect(norm(inside)).toBeCloseTo(SPHERE_R + 0.3, 12);
	expect(inside[0]).toBeGreaterThan(0);
	const p = moveP(base, [1, 0, 0]).set.nl_pt as number[];
	expect(norm(p)).toBeCloseTo(SPHERE_R, 12);
	expect(moveP({ ...base, nl_pt: [1, 0, 0] }, [-1, 0, 0])).toEqual({ set: { nl_pt: [1, 0, 0] } });
});

test('clampMove clamps keys with a range (vectors per component), rowRanges reads the editable rows', () => {
	const r = rowRanges([
		{ id: 'a', label: 'a', kind: 'vec', key: 'a', dims: 2, spec: { min: -4, max: 4, step: 0.1 } },
		{ id: 't', label: 't', kind: 'num', key: 't', spec: { min: 0, max: 180, step: 1 } },
		{ id: 'c', label: 'c', kind: 'ro', key: 'c' },
	]);
	expect(r).toEqual({ a: { min: -4, max: 4 }, t: { min: 0, max: 180 } });
	expect(clampMove({ a: [-10, 3], t: 200, c: 99 }, r)).toEqual({ a: [-4, 3], t: 180, c: 99 });
	expect(clampMove({ a: [1, 2] }, undefined)).toEqual({ a: [1, 2] });
	expect(() =>
		rowRanges([
			{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: { min: 0, max: 1, step: 0.1 } },
			{ id: 'b', label: 'b', kind: 'num', key: 'a', spec: { min: 0, max: 2, step: 0.1 } },
		]),
	).toThrow(/two ranges/);
});

test('a caught vector is clamped by uniform scaling, so the caught direction survives the range edge', () => {
	// The reviewer's case: a = (2, 1), b dragged to about (-2.2, 4) is caught exactly perpendicular, |b| > 4 in y.
	const v = { ...base, sa: [2, 1], sb: [-2, 4] };
	const m = moveSb(v, [-0.2, 0.05], 'drag');
	expect(m.ease).toEqual(['sb']);
	const ranges = { sb: { min: -4, max: 4 } };
	const sb = clampMove(m.set, ranges, m.ease).sb as number[];
	expect(Math.abs(dot([2, 1], sb))).toBeLessThan(1e-9);
	expect(Math.max(...sb.map(Math.abs))).toBeLessThanOrEqual(4 + 1e-12);
	expect(Math.max(...sb.map(Math.abs))).toBeCloseTo(4, 12);
	// The same vector not caught: component-wise clip (the ordinary drag rule), which bends the direction.
	const plain = clampMove(m.set, ranges).sb as number[];
	expect(Math.abs(dot([2, 1], plain))).toBeGreaterThan(1e-3);
	// A range without 0 cannot be reached by scaling: falls back to the component clamp.
	expect(clampMove({ k: [5, 0.5] }, { k: { min: 1, max: 4 } }, ['k'])).toEqual({ k: [4, 1] });
});

test('frameMismatch: panel presence and line count must match the server frame unless overridden', () => {
	const rows = ROWS2('en');
	const lines = chipLines(rows);
	expect(frameMismatch({ panel: true, lines, override: false }, rows)).toBeNull();
	expect(frameMismatch({ panel: true, lines: lines - 1, override: false }, rows)).toMatch(/reserved/);
	expect(frameMismatch({ panel: true, lines: 1, override: true }, rows)).toBeNull();
	expect(frameMismatch({ panel: false, lines: null, override: false }, rows)).toMatch(/no rows/);
	expect(frameMismatch({ panel: false, lines: null, override: false }, [])).toBeNull();
	expect(frameMismatch({ panel: true, lines: 1, override: false }, [])).toMatch(/has rows/);
});

test('Circle-style guards: a non-finite normal throws instead of spreading NaN', () => {
	expect(() => planeBasis([Number.NaN, 0, 1])).toThrow(/not finite/);
	expect(() => circlePoints([0, 0, 0], 1, 8, [0, Number.POSITIVE_INFINITY, 0])).toThrow();
});

test('lamp: N·L, brightness and the terminator circle (every point on it has N·L = 0 up to the 0.01 offset)', () => {
	const l = lamp(base);
	expect(norm(l.N)).toBeCloseTo(1, 12);
	expect(l.bright).toBe(Math.max(0, l.ndotl));
	const t = l.term!;
	const ring = circlePoints(t.c, t.r, 16, t.n)!;
	for (const q of ring) {
		const onSphere = q.map((x) => (x * SPHERE_R) / norm(q));
		const L = [2 - onSphere[0], 2.5 - onSphere[1], 1.5 - onSphere[2]];
		expect(Math.abs(dot(onSphere, L)) / (SPHERE_R * norm(L))).toBeLessThan(0.02);
	}
	expect(lamp({ ...base, nl_light: [1, 0, 0] }).term).toBeNull(); // a typed light inside the sphere has no horizon
});
