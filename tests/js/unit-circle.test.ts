import { describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import '../../src/i18n/ui';
import {
	angleOf, arc, CHECKS, cosSin, derive, fmt2, fmtDeg, graph, GRAPH_X0, INITIAL, MAGNET, moveAlong, moveOnCircle, quadrant, rad,
	snapAngle, unrollAt, unrolled,
} from '../../src/viz/lessons/unit-circle.math';
import { rowsFor } from '../../src/viz/lessons/unit-circle.rows';

const vals = (th: number, roll = 0) => ({ ...INITIAL, th, roll });
const close = (a: number, b: number, eps = 1e-12) => expect(Math.abs(a - b)).toBeLessThan(eps);
const len = (pts: readonly (readonly number[])[]) => pts.slice(1).reduce((s, p, i) => s + Math.hypot(p[0] - pts[i][0], p[1] - pts[i][1]), 0);

describe('unit circle numbers', () => {
	test('the worked example: 60 degrees is (0.5, 0.866) and 1.0472 radians', () => {
		const z = derive(vals(60));
		close(z.cos, 0.5, 1e-15);
		close(z.sin, Math.sqrt(3) / 2, 1e-15);
		close(z.rad, 1.0471975511965976);
		close(z.one, 1);
		close(z.turn, 1 / 6);
	});
	test('quadrant angles are exact zeros, not 6e-17', () => {
		expect(cosSin(0)).toEqual([1, 0]);
		expect(cosSin(90)).toEqual([0, 1]);
		expect(cosSin(180)).toEqual([-1, 0]);
		expect(cosSin(270)).toEqual([0, -1]);
		expect(cosSin(360)).toEqual([1, 0]);
	});
	test('cos^2 + sin^2 = 1 for every whole degree', () => {
		for (let d = 0; d <= 360; d++) close(derive(vals(d)).one, 1, 1e-15);
	});
	test('signs per quadrant: cos negative between 90 and 270, sin negative between 180 and 360', () => {
		expect(quadrant(45)).toBe(1);
		expect(quadrant(120)).toBe(2);
		expect(quadrant(225)).toBe(3);
		expect(quadrant(300)).toBe(4);
		for (const d of [0, 90, 180, 270, 360]) expect(quadrant(d)).toBe(0);
		expect(derive(vals(120)).cos).toBeLessThan(0);
		expect(derive(vals(120)).sin).toBeGreaterThan(0);
		expect(derive(vals(200)).sin).toBeLessThan(0);
	});
	test('mirror across y = x is 90 - theta, kept in 0..360, and swaps cos and sin', () => {
		expect(derive(vals(30)).mir).toBe(60);
		expect(derive(vals(120)).mir).toBe(330);
		expect(derive(vals(45)).mir).toBe(45);
		for (const d of [0, 30, 100, 225, 359]) {
			const z = derive(vals(d));
			const [mc, ms] = cosSin(z.mir);
			close(mc, z.sin, 1e-12);
			close(ms, z.cos, 1e-12);
		}
	});
	test('derive rejects non-numbers', () => {
		expect(() => derive({ th: [1, 2], roll: 0 })).toThrow();
		expect(() => derive({ th: Number.NaN, roll: 0 })).toThrow();
	});
});

describe('dragging by angle', () => {
	test('atan2 angle in whole degrees, 0..360', () => {
		expect(angleOf(1, 0, 10)).toBe(0);
		expect(angleOf(0, 1, 10)).toBe(90);
		expect(angleOf(-1, 1, 100)).toBe(135);
		expect(angleOf(0, -1, 200)).toBe(270);
	});
	test('crossing the positive x axis wraps instead of jumping a turn; 360 is reachable', () => {
		// Just below the axis seen from 355: 359.7 rounds to 360, not 0.
		expect(angleOf(Math.cos(rad(-0.3)), Math.sin(rad(-0.3)), 355)).toBe(360);
		// Past the axis counter-clockwise from 350: the angle wraps to the small value.
		expect(angleOf(Math.cos(rad(5)), Math.sin(rad(5)), 350)).toBe(5);
		// Clockwise past the axis from 2: wraps to 355.
		expect(angleOf(Math.cos(rad(-5)), Math.sin(rad(-5)), 2)).toBe(355);
	});
	test('a point at the origin keeps the previous angle', () => {
		expect(angleOf(0, 0, 77)).toBe(77);
	});
	test('magnet catches special angles in every quadrant, but a key step leaves one', () => {
		expect(MAGNET).toContain(135);
		expect(MAGNET).toContain(300);
		expect(snapAngle(58)).toBe(60);
		expect(snapAngle(56)).toBe(56);
		expect(snapAngle(358)).toBe(360);
		// A key step from 60 to 61 must not be pulled back onto 60.
		expect(snapAngle(61, 60)).toBe(61);
	});
	test('moveOnCircle: pointer travel from P gives the new angle', () => {
		// From 0 degrees (1, 0), moving the pointer to (0, 1) is 90 degrees.
		const m = moveOnCircle(vals(0), [-1, 1], 'drag');
		expect(m.set.th).toBe(90);
		// 52 degrees is not near a target: no magnet, no ease.
		const [c, s] = cosSin(52);
		const n = moveOnCircle(vals(0), [c - 1, s], 'drag');
		expect(n.set.th).toBe(52);
		expect(n.ease).toEqual([]);
		// 58 is caught at 60, which eases.
		const [c2, s2] = cosSin(58);
		const q = moveOnCircle(vals(0), [c2 - 1, s2], 'drag');
		expect(q.set.th).toBe(60);
		expect(q.ease).toEqual(['th']);
	});
	test('moveAlong: one world unit along the graph is one radian', () => {
		expect(moveAlong(vals(0), [Math.PI / 2, 0], 'drag').set.th).toBe(90);
		expect(moveAlong(vals(90), [-Math.PI / 2, 0], 'drag').set.th).toBe(0);
		expect(moveAlong(vals(10), [1, 0], 'drag').set.th).toBe(67);
	});
});

describe('arcs, unrolling and graphs', () => {
	test('an arc of any span up to a full turn, either direction, on the radius', () => {
		const full = arc([0, 0], 1, 0, 360)!;
		expect(full[0][0]).toBe(1);
		expect(full.at(-1)![0]).toBe(1);
		close(len(full), 2 * Math.PI, 1e-3);
		const cw = arc([0, 0], 0.5, 90, -30)!;
		close(cw[0][1], 0.5);
		close(cw.at(-1)![0], 0.5 * Math.cos(rad(-30)));
		for (const p of cw) close(Math.hypot(p[0], p[1]), 0.5, 1e-12);
		expect(arc([0, 0], 1, 30, 30)).toBeNull();
	});
	test('unrolling keeps the ends exact: arc on the circle, then flat on the ruler from (1, 0)', () => {
		const s = rad(60);
		const [x0, y0] = unrollAt(s, 0);
		close(x0, 0.5, 1e-15);
		close(y0, Math.sqrt(3) / 2, 1e-15);
		expect(unrollAt(s, 1)).toEqual([1 + s, 0, 0]);
		expect(unrollAt(0, 0.4)).toEqual([1, 0, 0]);
		// A full turn flat on the ruler ends at 1 + 2 pi and is 2 pi long.
		const flat = unrolled(360, 1)!;
		close(flat.at(-1)![0], 1 + 2 * Math.PI, 1e-12);
		close(len(flat), 2 * Math.PI, 1e-9);
		close(len(unrolled(360, 0)!), 2 * Math.PI, 1e-3);
		expect(unrolled(0, 1)).toBeNull();
	});
	test('graph: x = GRAPH_X0 + theta in radians, y = cos or sin; empty span draws nothing', () => {
		const c = graph('cos', 0, 360)!;
		expect(c[0]).toEqual([GRAPH_X0, 1, 0]);
		close(c.at(-1)![0], GRAPH_X0 + 2 * Math.PI);
		const s = graph('sin', 0, 90)!;
		expect(s.at(-1)).toEqual([GRAPH_X0 + Math.PI / 2, 1, 0]);
		expect(graph('cos', 90, 90)).toBeNull();
	});
});

describe('challenges and formatting', () => {
	test('each challenge is solved by its intended angle and not by the default', () => {
		const d = vals(60);
		for (const [name, check] of Object.entries(CHECKS)) expect([name, check(d)]).toEqual([name, false]);
		expect(CHECKS.cosHalfNeg(vals(120))).toBe(true);
		expect(CHECKS.cosHalfNeg(vals(240))).toBe(true);
		expect(CHECKS.down(vals(270))).toBe(true);
		expect(CHECKS.cosTwin(vals(300))).toBe(true);
		expect(CHECKS.fullTurnFlat(vals(360, 1))).toBe(true);
		expect(CHECKS.fullTurnFlat(vals(360, 0.5))).toBe(false);
		expect(CHECKS.oneRadian(vals(57))).toBe(true);
		expect(CHECKS.crossFirst(vals(45))).toBe(true);
		expect(CHECKS.crossSecond(vals(225))).toBe(true);
		expect(CHECKS.onMirror(vals(225))).toBe(true);
	});
	test('formatting: real minus, no negative zero, whole degrees', () => {
		expect(fmt2(-0.5)).toBe('−0.50');
		expect(fmt2(-1e-17)).toBe('0.00');
		expect(fmtDeg(59.6)).toBe('60°');
	});
	test('rows: theta first, same term ids in both languages', () => {
		for (const s of [1, 2, 3, 4] as const) {
			const en = rowsFor(s, 'en').map((r) => r.id);
			expect(en[0]).toBe('th');
			expect(rowsFor(s, 'ru').map((r) => r.id)).toEqual(en);
		}
	});
});

/** Page composition: every card is followed directly by its own controls, in order, in both languages. */
for (const page of ['src/content/docs/ground/unit-circle.mdx', 'src/content/docs/ru/ground/unit-circle.mdx']) {
	test('UC cards are followed by their own controls: ' + page, () => {
		const tags = [...readFileSync(page, 'utf8').matchAll(/<(Unit(?:Scene|Controls) scene=\{\d\})/g)].map((m) => m[1]);
		expect(tags).toEqual([1, 2, 3, 4].flatMap((n) => [`UnitScene scene={${n}}`, `UnitControls scene={${n}}`]));
	});
}
