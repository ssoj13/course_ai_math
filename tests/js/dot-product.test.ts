// Lesson 1.4 model checks against independent arithmetic (the DP preflight fixtures and the controller rulings of
// 2026-10-11): defaults, zero axes, parallel/antiparallel of unequal length, sign boundary, orthogonality, signed
// rectangle area (sign from the shadow, never from the vertex winding), the 3D display plane, the lamp per surface
// point with its horizon circle, the radial input policy and the step-6 cancellation schedule.
import { describe, expect, test } from 'bun:test';
import '../../src/i18n/ui';
import {
	INITIAL, LAMP_MIN, SPHERE_R, derive, derive3, lamp, ndotlAt, moveB, moveLight, moveP, lightLink, probeLink, pair,
	strikeTarget, newestColumn, fmt, fmtAngle, fmtCos, tex, PERP_ZONE, RECT_NL,
} from '../../src/viz/lessons/dot-product.math';
import { chipLines } from '../../src/viz/scene/chips';
import { rowsFor, facesFor } from '../../src/viz/lessons/dot-product.rows';

const close = (a: number, b: number, eps = 1e-11) => expect(Math.abs(a - b)).toBeLessThanOrEqual(eps * Math.max(1, Math.abs(a), Math.abs(b)));
const sub = (p: readonly number[], q: readonly number[]) => p.map((x, i) => x - q[i]);
const dotI = (p: readonly number[], q: readonly number[]) => p.reduce((s, x, i) => s + x * q[i], 0);
const len = (p: readonly number[]) => Math.sqrt(dotI(p, p));
/** Independent signed shoelace area of a 2D polygon (winding sign). */
const shoelace = (p: readonly (readonly number[])[]) => p.reduce((s, a, i) => { const b = p[(i + 1) % p.length]; return s + a[0] * b[1] - a[1] * b[0]; }, 0) / 2;
const v2 = (a: number[], b: number[]) => ({ ...INITIAL, a, b });

describe('2D defaults (spec §2.1, brief evidence list)', () => {
	const z = derive(INITIAL);
	test('a = (3, 1), b = (1, 2): dot 5, |a|² 10, |b|² 5, |a − b|² 5, θ 45°', () => {
		expect(z.dot).toBe(5); expect(z.na2).toBe(10); expect(z.nb2).toBe(5); expect(z.namb2).toBe(5);
		close(z.th, 45, 1e-12); expect(fmtAngle(z.th)).toBe('45');
		close(z.rhs, 5); close(z.rect, 5); close(z.shadow, 5 / Math.sqrt(10));
		// Polarization identity, independently: |a − b|² = |a|² + |b|² − 2 a·b.
		expect(z.namb2).toBe(z.na2 + z.nb2 - 2 * z.dot);
		expect(z.perp).toBe(false);
	});
	test('component table cells', () => {
		expect([z.d1, z.d2, z.d1sq, z.d2sq, z.a1sq, z.a2sq, z.b1sq, z.b2sq, z.p1, z.p2]).toEqual([2, -1, 4, 1, 9, 1, 1, 4, 3, 2]);
	});
	test('rectangle: base along a of length shadow, height |a| away from b, area a·b, inside the square on a', () => {
		const r = z.rectPts!;
		close(len(r[1]), 5 / Math.sqrt(10)); close(len(r[3]), Math.sqrt(10));
		close(dotI(sub(r[1], r[0]), sub(r[3], r[0])), 0);
		close(Math.abs(shoelace(r)), 5);
		// Away from b: the height direction and b lie on opposite sides of a's line.
		const cr = (u: readonly number[], w: readonly number[]) => u[0] * w[1] - u[1] * w[0];
		expect(Math.sign(cr(z.a, r[3]))).toBe(-Math.sign(cr(z.a, z.b)));
		// Squares of the triangle are outward: each away from the opposite vertex.
		expect(Math.sign(cr(z.a, z.sq_a![3]))).toBe(-1);
		expect(Math.sign(cr(z.b, z.sq_b![3]))).toBe(1);
		close(Math.abs(shoelace(z.sq_amb!)), 5);
	});
});

describe('signed rectangle: sign from the shadow, not from winding (ruling 1)', () => {
	test('unequal parallel and antiparallel: |a| tall, |b| wide, area ±|a||b|; a square only when lengths match', () => {
		const par = derive(v2([2, 0], [3, 0])), anti = derive(v2([2, 0], [-3, 0]));
		expect(par.dot).toBe(6); expect(par.shadow).toBe(3); close(Math.abs(shoelace(par.rectPts!)), 6);
		expect(anti.dot).toBe(-6); expect(anti.shadow).toBe(-3); close(Math.abs(shoelace(anti.rectPts!)), 6);
		expect(len(par.rectPts![1])).toBe(3); expect(len(par.rectPts![3])).toBe(2);
		close(par.th, 0, 1e-12); close(anti.th, 180, 1e-12);
		// Hatch follows the shadow: reversing the vertex order flips the winding, never the dot product's sign.
		expect(shoelace([...par.rectPts!].reverse())).toBe(-shoelace(par.rectPts!));
		expect(Math.sign(par.shadow)).toBe(1); expect(Math.sign(anti.shadow)).toBe(-1);
		const same = derive(v2([3, 1], [3, 1]));
		expect(same.dot).toBe(10); close(same.shadow, Math.sqrt(10)); expect(same.namb2).toBe(0); expect(same.sq_amb).toBeNull();
		expect(fmtAngle(same.th)).toBe('0.0');
	});
	test('orthogonal nonzero: dot 0, perpendicular, zero-area rectangle with label anchor at O', () => {
		const z = derive(v2([2, 1], [-1, 2]));
		expect(z.dot).toBe(0); expect(z.perp).toBe(true); close(z.th, 90, 1e-12); expect(z.shadow).toBe(0);
		expect(Math.abs(shoelace(z.rectPts!))).toBe(0); expect(z.bright).toBe(0);
	});
	test('sign boundary: tiny positive and negative dots keep their sign; perpendicular needs |dot| < 1e-9', () => {
		const pos = derive(v2([1, 0], [0.1, 1])), neg = derive(v2([1, 0], [-0.1, 1]));
		expect(pos.dot).toBeGreaterThan(0); expect(neg.dot).toBeLessThan(0); expect(neg.shadow).toBeLessThan(0);
		expect(pos.perp || neg.perp).toBe(false);
	});
});

describe('zero arrows (rulings 2 and 3)', () => {
	test('a ≠ 0, b = 0: shadow 0, projection 0, area 0; angle, cosine route and brightness undefined', () => {
		const z = derive(v2([3, 1], [0, 0]));
		expect(z.dot).toBe(0); expect(z.shadow).toBe(0); expect(z.rect).toBe(0); expect(z.proj).toEqual([0, 0]);
		expect(Number.isNaN(z.th)).toBe(true); expect(Number.isNaN(z.rhs)).toBe(true); expect(Number.isNaN(z.bright)).toBe(true);
		expect(z.perp).toBe(false); expect(z.rectPts).not.toBeNull();
	});
	test('a = 0: no axis, so shadow and rectangle are undefined; the component dot is still 0', () => {
		for (const b of [[1, 2], [0, 0]]) {
			const z = derive(v2([0, 0], b));
			expect(z.dot).toBe(0); expect(Number.isNaN(z.shadow)).toBe(true); expect(Number.isNaN(z.rect)).toBe(true);
			expect(z.rectPts).toBeNull(); expect(z.perp).toBe(false); expect(Number.isNaN(z.th)).toBe(true);
		}
	});
	test('readouts never print NaN: DOM dash, TeX \\text{--}', () => {
		expect(fmt(NaN)).toBe('–'); expect(fmtCos(Infinity)).toBe('–'); expect(fmtAngle(NaN)).toBe('–');
		expect(tex.num(NaN)).toBe('\\text{--}'); expect(tex.deg(NaN)).toBe('\\text{--}');
		expect(fmt(-0.001)).toBe('0.00'); expect(fmtCos(-0.0001)).toBe('0.000'); expect(tex.deg(45.00000000000001)).toBe('45^\\circ');
	});
});

describe('3D (DP-3) and the display plane (ruling 4)', () => {
	test('defaults a = (2, 1, 1), b = (1, 2, −1): dot 3, |a|² = |b|² = 6, θ 60°', () => {
		const z = derive3(INITIAL);
		expect(z.dot).toBe(3); expect(z.na2).toBe(6); expect(z.nb2).toBe(6); close(z.th, 60, 1e-12);
		expect(z.p).toEqual([2, 2, -1]); close(z.shadow, 3 / Math.sqrt(6)); close(z.rect, 3);
	});
	test('oblique fixture (1, 2, 3)·(−2, 1, 4) = 12, θ ≈ 45.5846914028°', () => {
		const z = pair([1, 2, 3], [-2, 1, 4]);
		expect(z.dot).toBe(12); close(z.th, 45.5846914028, 1e-10); close(z.shadow, 3.207134902949, 1e-11);
	});
	test('rectangle stays in the plane of a and b, is a rectangle, and its normal is perpendicular to a', () => {
		for (const [a, b] of [[[2, 1, 1], [1, 2, -1]], [[2, 1, 1], [4, 2, 2]], [[2, 1, 1], [-2, -1, -1]], [[0, 3, 0], [0, -1, 0]], [[1, 2, 3], [-2, 1, 4]]]) {
			const z = pair(a, b), r = z.rectPts!;
			const e1 = sub(r[1], r[0]), e2 = sub(r[3], r[0]);
			close(dotI(e1, e2), 0);
			close(len(e2), len(a));
			close(dotI(e2, a), 0);
			const n = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
			close(dotI(n, a), 0, 1e-10);
			close(len(n), Math.abs(z.dot), 1e-10);
			// Collinear arrows: a display convention (least parallel axis), never along a.
			if (Math.abs(Math.abs(z.cosT) - 1) < 1e-12) close(Math.abs(dotI(z.h!, a)), 0);
		}
	});
	test('zero 3D arrows: no invented angle', () => {
		const z = pair([0, 0, 0], [1, 2, 3]);
		expect(Number.isNaN(z.th)).toBe(true); expect(z.rectPts).toBeNull(); expect(z.dot).toBe(0);
		const w = pair([1, 2, 3], [0, 0, 0]);
		expect(w.rect).toBe(0); expect(Number.isNaN(w.th)).toBe(true);
	});
	test('antiparallel arc reference is never along a (a along y uses another axis)', () => {
		const z = derive3({ ...INITIAL, a3: [0, 2, 0], b3: [0, -1, 0] });
		expect(z.arcRef[1]).toBe(0);
	});
});

describe('DP-4 lamp: per surface point, horizon circle, radial inputs (rulings 5–7)', () => {
	const z = lamp(INITIAL);
	test('initial probe θ ≈ 33.4858046595°, N·L ≈ 0.834022541857 (not the old 38° / 0.800)', () => {
		close(len(INITIAL.nl_pt), SPHERE_R);
		close(z.ndotl, 0.834022541857, 1e-11); close(z.thNL, 33.4858046595, 1e-9);
		close(z.P[0], 1.06066017178, 1e-10); close(z.L[1], 0.852410915752, 1e-11);
	});
	test('L changes per surface point: the probe value is not the sphere’s brightness', () => {
		close(ndotlAt([1.5, 0, 0], INITIAL.nl_light), 0.169030850946, 1e-11);
		const dir = INITIAL.nl_light.map((x) => x / len(INITIAL.nl_light));
		close(ndotlAt(dir.map((x) => x * SPHERE_R), INITIAL.nl_light), 1);
		close(ndotlAt(dir.map((x) => -x * SPHERE_R), INITIAL.nl_light), -1);
	});
	test('horizon circle: centre (.36, .45, .27), radius 1.358307770721, every point has N·L = 0', () => {
		close(z.term.c[0], 0.36); close(z.term.c[1], 0.45); close(z.term.c[2], 0.27); close(z.term.r, 1.358307770721, 1e-11);
		const n = z.term.n, u = [n[1], -n[0], 0].map((x) => x / Math.hypot(n[0], n[1]));
		const w = [n[1] * u[2] - n[2] * u[1], n[2] * u[0] - n[0] * u[2], n[0] * u[1] - n[1] * u[0]];
		for (let k = 0; k < 6; k++) {
			const t = (k * Math.PI) / 3, p = z.term.c.map((c, i) => c + z.term.r * (Math.cos(t) * u[i] + Math.sin(t) * w[i]));
			close(len(p), SPHERE_R, 1e-12); close(ndotlAt(p, INITIAL.nl_light), 0, 1e-10);
		}
		// The lit set is a cap smaller than a hemisphere: the circle lies on the lamp's side of the centre.
		expect(dotI(z.term.c, n)).toBeGreaterThan(0);
	});
	test('probe rectangle: logical area N·L, drawn at 0.6 scale (drawn area 0.36·N·L)', () => {
		const r = z.rectPts, e1 = sub(r[1], r[0]), e2 = sub(r[3], r[0]);
		close(len(e1), RECT_NL * z.ndotl); close(len(e2), RECT_NL); close(dotI(e1, e2), 0);
	});
	test('radial inputs: lamp ≥ 1.8 along its own direction, zero keeps the previous direction, probe on the sphere', () => {
		expect(lightLink.resolve({ nl_light: [0.2, 0, 0] }, INITIAL).nl_light).toEqual([LAMP_MIN, 0, 0]);
		expect(lightLink.resolve({ nl_light: [3, 0, 0] }, INITIAL).nl_light).toEqual([3, 0, 0]);
		const zero = lightLink.resolve({ nl_light: [0, 0, 0] }, INITIAL).nl_light as number[];
		close(zero[0], 1.018233764909, 1e-11); close(zero[1], 1.272792206136, 1e-11); close(zero[2], 0.763675323681, 1e-11);
		const p = probeLink.resolve({ nl_pt: [0, 0, 0] }, INITIAL).nl_pt as number[];
		close(len(p), SPHERE_R); close(p[0] / p[2], 1 / 0.8);
		const moved = moveP(INITIAL, [5, 0, 0]).set.nl_pt as number[];
		close(len(moved), SPHERE_R);
		const lit = moveLight({ ...INITIAL }, [-2, -2.5, -1.5]).set.nl_light as number[];
		expect(len(lit)).toBeGreaterThanOrEqual(LAMP_MIN - 1e-12);
	});
});

describe('DP-2 magnet to the perpendicular (one-way on keys, keeps length and side)', () => {
	test('a drag within 3° is caught exactly perpendicular, off-grid, eased', () => {
		const v = v2([3, 1], [-0.9, 3]);
		const m = moveB(v, [0, 0], 'drag'), b = m.set.b as number[];
		expect(m.ease).toEqual(['b']);
		expect(Math.abs(dotI([3, 1], b))).toBeLessThan(1e-12);
		close(len(b), Math.hypot(-0.9, 3));
		expect(3 * b[1] - 1 * b[0]).toBeGreaterThan(0);
		expect(Math.abs(dotI([3, 1], [-0.9, 3]) / (len([3, 1]) * len([-0.9, 3])))).toBeLessThan(PERP_ZONE);
	});
	test('a key step leaving the line is not pulled back; a step toward it is caught', () => {
		const on = v2([3, 1], [-1, 3]);
		const away = moveB(on, [0.1, 0], 'key');
		expect(away.ease ?? []).toEqual([]); expect(away.set.b).toEqual([-0.9, 3]);
		const toward = moveB(v2([3, 1], [-0.9, 3]), [-0.1, 0], 'key');
		expect(toward.ease).toEqual(['b']);
		const far = moveB(v2([3, 1], [1, 2]), [0.1, 0], 'drag');
		expect(far.ease ?? []).toEqual([]);
	});
});

describe('DP-1 cancellation schedule and table reveal', () => {
	test('step 6 strikes the a pair now and the b pair later; other steps are final; reduced motion is immediate', () => {
		expect(strikeTarget(5, false)).toEqual({ now: 0, later: null });
		expect(strikeTarget(6, false)).toEqual({ now: 1, later: 2 });
		expect(strikeTarget(6, true)).toEqual({ now: 2, later: null });
		expect(strikeTarget(8, false)).toEqual({ now: 2, later: null });
	});
	test('the newest revealed column per step', () => {
		expect([0, 1, 2, 3, 4].map(newestColumn)).toEqual(['b', 'bsq', null, 'p', null]);
	});
});

describe('rows', () => {
	test('row id lists are the term ids of the spec', () => {
		expect(rowsFor(1, 'en').map((r) => r.id)).toEqual(['a', 'b', 'na', 'nb', 'th', 'dot', 'rhs', 'rect']);
		expect(rowsFor(2, 'en').map((r) => r.id)).toEqual(['a', 'b', 'na', 'nb', 'shadow', 'cosT', 'th', 'dot', 'rect', 'bright', 'aa']);
		expect(rowsFor(3, 'en').map((r) => r.id)).toEqual(['a3', 'b3', 'na3', 'nb3', 'th3', 'dot3', 'rhs3', 'rect3']);
		expect(rowsFor(4, 'en').map((r) => r.id)).toEqual(['light', 'P', 'nl_raw', 'N', 'L', 'NdotL', 'thNL', 'brightNL']);
		expect(facesFor('en').map((f) => f.row.id)).toEqual(['sum', 'rect', 'bright']);
	});
	test('the raw N·L label does not change the reserved chip lines (no hydration shift)', () => {
		for (const locale of ['en', 'ru'] as const) expect(chipLines(rowsFor(4, locale, 1))).toBe(chipLines(rowsFor(4, locale, 0)));
	});
	test('DP-1 and DP-2 declare one range for a and b; values are finite for every edge fixture', () => {
		const r1 = rowsFor(1, 'en').filter((r) => r.kind === 'vec').map((r) => r.spec);
		const r2 = rowsFor(2, 'en').filter((r) => r.kind === 'vec').map((r) => r.spec);
		expect(r1).toEqual(r2);
		for (const [a, b] of [[[0, 0], [0, 0]], [[3, 1], [0, 0]], [[0, 0], [1, 2]], [[2, 0], [-3, 0]]]) {
			for (const scene of [1, 2] as const) for (const r of rowsFor(scene, 'en')) {
				if (r.kind !== 'ro' || !r.value) continue;
				const v = r.value(v2(a, b)), text = typeof v === 'number' ? r.fmt!(v) : '';
				expect(text).not.toContain('NaN');
			}
		}
	});
});
