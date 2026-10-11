import { expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { lessonStore } from '../../src/viz/store';
import { axes, ccw, CHECKS, coords, derive, INITIAL, LINKS, plane, shadow } from '../../src/viz/lessons/projection.math';
import { rowsFor } from '../../src/viz/lessons/projection.rows';
import { rowRanges } from '../../src/viz/scene/NPanel';

/** Lesson 1.5: values checked against the prose's hand computations and independent identities. */

const r = (v: readonly number[] | null, n = 9) => v!.map((x) => +x.toFixed(n) + 0);

test('signed shadow a·b̂: the prose examples, the sign, and independence of |b|', () => {
	expect(shadow([3, 4], [1, 0]).s).toBe(3);
	expect(shadow([3, 4], [1, 1]).s).toBeCloseTo(7 / Math.SQRT2, 14);
	expect(shadow([3, 4], [10, 10]).s).toBeCloseTo(7 / Math.SQRT2, 14);
	expect(shadow([-3, 1], [1, 0])).toMatchObject({ s: -3, sign: 'neg' });
	expect(shadow([0, 2], [1, 0])).toMatchObject({ s: 0, sign: 'zero' });
	expect(shadow([3, 4], [1, 0]).sign).toBe('pos');
	// |shadow| never exceeds |a|.
	for (let i = 0; i < 100; i++) {
		const a = [Math.sin(i) * 4, Math.cos(i * 1.3) * 4], b = [Math.cos(i * 0.7) * 3, Math.sin(i * 2.1) * 3];
		const z = shadow(a, b);
		expect(Math.abs(z.s)).toBeLessThanOrEqual(z.na + 1e-12);
	}
});

test('a zero axis has no shadow; a zero arrow has a zero shadow', () => {
	const z = shadow([3, 4], [0, 0]);
	expect(z).toMatchObject({ zeroB: true, sign: 'none', bh: null, proj: null, perp: null, ab: 0 });
	expect(Number.isNaN(z.s)).toBe(true);
	expect(Number.isNaN(z.perpDotB)).toBe(true);
	expect(shadow([0, 0], [1, 2])).toMatchObject({ s: 0, sign: 'zero', proj: [0, 0], perp: [0, 0] });
});

test('vector projection: both routes agree; the remainder is perpendicular; Pythagoras holds', () => {
	const z = shadow([3, 4], [1, 1]);
	expect(r(z.proj)).toEqual([3.5, 3.5]);
	expect(r(z.projAlt)).toEqual([3.5, 3.5]);
	expect(r(z.perp)).toEqual([-0.5, 0.5]);
	expect(z.perpDotB).toBeCloseTo(0, 14);
	expect(z.proj2 + z.perp2).toBeCloseTo(25, 12);
	expect(r(shadow([3, 4], [1, 0]).perp)).toEqual([0, 4]);
	expect(r(shadow([2, 1], [0, 1]).proj)).toEqual([0, 1]);
	for (let i = 1; i < 60; i++) {
		const a = [Math.sin(i) * 4, Math.cos(i * 1.3) * 4, Math.sin(i * 0.4)], b = [Math.cos(i * 0.7) * 3, Math.sin(i * 2.1) * 3, 1];
		const w = shadow(a, b);
		expect(Math.abs(w.perpDotB)).toBeLessThan(1e-12);
		expect(Math.abs(w.proj2 + w.perp2 - w.na2)).toBeLessThan(1e-12);
		w.proj!.forEach((x, k) => expect(x).toBeCloseTo(w.projAlt![k], 12));
	}
});

test('coordinates are shadows only on perpendicular unit axes', () => {
	const std = coords([3, 4], 0, 90);
	expect(r(std.sh)).toEqual([3, 4]);
	expect(r(std.co)).toEqual([3, 4]);
	expect(std.miss).toBeLessThan(1e-12);
	// Turned orthonormal axes at 45°: new numbers, still rebuild the arrow, still equal the coordinates.
	const turned = coords([3, 4], 45, 90);
	expect(turned.sh[0]).toBeCloseTo(7 / Math.SQRT2, 12);
	expect(turned.sh[1]).toBeCloseTo(1 / Math.SQRT2, 12);
	expect(turned.miss).toBeLessThan(1e-12);
	expect(r(turned.co, 9)).toEqual(r(turned.sh, 9));
	// Oblique axes u₁ = (1, 0), u₂ = (cos 60°, sin 60°): coordinates (s, t) solve s·u₁ + t·u₂ = c, shadows do not.
	const ob = coords([3, 4], 0, 60);
	const t = 4 / Math.sin(Math.PI / 3), s = 3 - t * 0.5;
	expect(ob.co[0]).toBeCloseTo(s, 12);
	expect(ob.co[1]).toBeCloseTo(t, 12);
	expect(ob.sh[0]).toBeCloseTo(3, 12);
	expect(ob.miss).toBeGreaterThan(1);
	const [u1, u2] = axes(30, 90);
	expect(Math.hypot(...u1)).toBeCloseTo(1, 15);
	expect(u1[0] * u2[0] + u1[1] * u2[1]).toBeCloseTo(0, 15);
});

test('projection onto a plane: slide, reflection and the two in-plane shadows', () => {
	const g = plane([2, -3, 1], [0, 1, 0]);
	expect(g.an).toBe(-3);
	expect(r(g.ground)).toEqual([2, 0, 1]);
	expect(r(plane([1, -1, 0], [0, 1, 0]).refl)).toEqual([1, 1, 0]);
	expect(r(g.rebuild)).toEqual([2, 0, 1]);
	// A tilted unit normal: the ground part is perpendicular to n and the rebuild from two shadows equals it.
	const n = [1, 2, 2].map((x) => x / 3);
	const h = plane([1.5, -2, 0.7], n);
	expect(h.ground.reduce((s, x, i) => s + x * n[i], 0)).toBeCloseTo(0, 14);
	h.rebuild.forEach((x, i) => expect(x).toBeCloseTo(h.ground[i], 12));
	expect(() => plane([1, 1, 1], [0, 2, 0])).toThrow();
});

test('the store keeps n on the unit sphere and falls back from a zero request', () => {
	const st = lessonStore('pj-test-n', { ...INITIAL }, LINKS);
	st.getState().set('n3', [0, 0, 3]);
	expect(st.getState().vals.n3).toEqual([0, 0, 1]);
	st.getState().set('n3', [0, 0, 0]);
	expect(st.getState().vals.n3).toEqual([0, 0, 1]);
});

test('triangle orientation and challenges', () => {
	expect(ccw([1, 0], [1, 1])).toBe(true);
	expect(ccw([1, 0], [1, -1])).toBe(false);
	expect(derive(INITIAL).p.s).toBeCloseTo(6 / Math.sqrt(10), 14);
	expect(CHECKS.negative(INITIAL)).toBe(false);
	expect(CHECKS.negative({ ...INITIAL, a: [-2, 1] })).toBe(true);
	expect(CHECKS.zero({ ...INITIAL, a: [-1, 3] })).toBe(true);
	expect(CHECKS.zero({ ...INITIAL, a: [0, 0] })).toBe(false);
	expect(CHECKS.full({ ...INITIAL, a: [1.5, 0.5] })).toBe(true);
	expect(CHECKS.equal(INITIAL)).toBe(false);
	expect(CHECKS.equal({ ...INITIAL, a: [1, 2] })).toBe(true);
	expect(CHECKS.turned({ ...INITIAL, phi: 45 })).toBe(true);
	expect(CHECKS.oblique({ ...INITIAL, gap: 60 })).toBe(true);
	expect(CHECKS.oblique(INITIAL)).toBe(false);
	expect(CHECKS.inPlane({ ...INITIAL, a3: [2, 0, 1] })).toBe(true);
});

test('N-panel rows: same ids in both locales; n is the radial key', () => {
	for (const scene of [1, 2, 3, 4] as const) {
		const en = rowsFor(scene, 'en'), ru = rowsFor(scene, 'ru');
		expect(ru.map((x) => x.id)).toEqual(en.map((x) => x.id));
		expect(rowRanges(ru)).toEqual(rowRanges(en));
	}
	expect(rowRanges(rowsFor(4, 'en')).n3).toEqual({ min: -1, max: 1 });
});
