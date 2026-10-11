import { describe, expect, test } from 'bun:test';
import { cross, dot, norm } from '../../src/viz/geom';
import {
	CHECKS, FRAME0, INITIAL, LATTICE_CAP, clipLine, combine, derive1, derive2, derive3, derive4, det2, det3, family, fmt, fmtIn,
	frame2, gramSchmidt, lattice, moveTh, moveW, orthoLink, rank, rulerTicks, solve,
} from '../../src/viz/lessons/basis.math';

/** Independent pure checks of lesson 1.6's model (numbers from the prose, edge cases: zero, dependent, sign). */

const vals = (patch: Record<string, number | number[]> = {}) => ({ ...INITIAL, ...patch });
const close = (a: readonly number[], b: readonly number[], eps = 1e-9) => {
	expect(a.length).toBe(b.length);
	a.forEach((x, i) => expect(Math.abs(x - b[i])).toBeLessThan(eps));
};

describe('2D basis algebra', () => {
	test('prose example: (3, 4) = −1·(1, 0) + 4·(1, 1)', () => {
		close(solve([[1, 0], [1, 1]], [3, 4])!, [-1, 4]);
		close(combine([[1, 0], [1, 1]], [-1, 4]), [3, 4]);
	});
	test('order matters: (2, 3) in the basis (0, 1), (1, 0) is (3, 2)', () => {
		close(solve([[0, 1], [1, 0]], [2, 3])!, [3, 2]);
	});
	test('dependent and zero rulers have no coordinates', () => {
		expect(solve([[1, 2], [2, 4]], [1, 1])).toBeNull();
		expect(solve([[0, 0], [1, 0]], [1, 1])).toBeNull();
	});
	test('signed area and rank', () => {
		expect(det2([1, 0], [1, 1])).toBe(1);
		expect(det2([1, 1], [1, 0])).toBe(-1);
		expect(rank([[1, 2], [2, 4]])).toBe(1);
		expect(rank([[1, 0], [1, 1]])).toBe(2);
		expect(rank([[0, 0], [0, 0]])).toBe(0);
		// A tiny but honest basis is still a basis (relative tolerance).
		expect(rank([[1e-6, 0], [0, 1e-6]])).toBe(2);
	});
});

describe('BS-1 lattice', () => {
	test('initial values reproduce the prose', () => {
		const z = derive1(vals());
		expect(z.dim).toBe(2);
		expect(z.area).toBe(1);
		close([z.c1, z.c2], [-1, 4]);
		close(z.leg!, [-1, 0]);
	});
	test('collapse: (1, 2), (2, 4) span a line; v on it is reachable, v off it is not', () => {
		const off = derive1(vals({ u1: [1, 2], u2: [2, 4] }));
		expect(off.dim).toBe(1);
		expect(off.c).toBeNull();
		expect(Number.isNaN(off.c1)).toBe(true);
		expect(off.onLine).toBe(false);
		expect(off.spanLine).not.toBeNull();
		const on = derive1(vals({ u1: [1, 2], u2: [2, 4], v: [1.5, 3] }));
		expect(on.onLine).toBe(true);
		expect(CHECKS.collapse(off as never) || CHECKS.collapse(vals({ u1: [1, 2], u2: [2, 4] }))).toBe(true);
	});
	test('two zero rulers span only the origin', () => {
		const z = derive1(vals({ u1: [0, 0], u2: [0, 0] }));
		expect(z.dim).toBe(0);
		expect(z.zeroBoth).toBe(true);
		expect(lattice([0, 0], [0, 0]).s).toEqual([]);
		expect(CHECKS.collapse(vals({ u1: [0, 0], u2: [1, 1] }))).toBe(false);
	});
	test('lattice lines pass through k·u₁ parallel to u₂ and stay inside the square', () => {
		const { s, t } = lattice([1, 0], [1, 1], 5);
		expect(s.length).toBeGreaterThan(5);
		expect(t.length).toBe(11); // lines y = k for k = −5..5
		for (const [a, b] of [...s, ...t]) for (const p of [a, b]) for (const x of p) expect(Math.abs(x)).toBeLessThanOrEqual(5 + 1e-9);
		// Each s-line is parallel to u₂ = (1, 1).
		for (const [a, b] of s) expect(Math.abs(det2([b[0] - a[0], b[1] - a[1]], [1, 1]))).toBeLessThan(1e-9);
	});
	test('dependent rulers draw one line per family; nearly dependent ones are capped', () => {
		expect(family([2, 4], [1, 2], 5).length).toBe(1);
		expect(family([1, 0], [1, 1e-7], 5).length).toBeLessThanOrEqual(2 * LATTICE_CAP + 1);
	});
	test('clipLine misses and hits', () => {
		expect(clipLine([0, 10], [1, 0], 5)).toBeNull();
		const seg = clipLine([0, 0], [1, 1], 5)!;
		close(seg[0], [-5, -5]);
		close(seg[1], [5, 5]);
		expect(clipLine([0, 0], [0, 0], 5)).toBeNull();
	});
	test('challenge: v = (3, 4) reads (2, 1) in u₁ = (1, 1), u₂ = (1, 2)', () => {
		expect(CHECKS.readsTwoOne(vals({ u1: [1, 1], u2: [1, 2] }))).toBe(true);
		expect(CHECKS.readsTwoOne(vals())).toBe(false);
	});
});

describe('BS-2 span in 3D', () => {
	test('prose: (1,0,0), (0,1,0), (1,1,0) span the plane z = 0', () => {
		const z = derive2(vals());
		expect(z.dim).toBe(2);
		expect(z.vol).toBe(0);
		close(z.normal!.map(Math.abs), [0, 0, 1]);
		expect(CHECKS.otherPlane(vals())).toBe(false);
	});
	test('lifting w3 gives all of space; a different plane passes the second challenge', () => {
		expect(CHECKS.space(vals({ w3: [1, 1, 1] }))).toBe(true);
		expect(det3([1, 0, 0], [0, 1, 0], [1, 1, 1])).toBe(1);
		expect(CHECKS.otherPlane(vals({ w1: [1, 0, 1], w2: [0, 1, 0], w3: [1, 1, 1] }))).toBe(true);
	});
	test('a line and a point', () => {
		expect(derive2(vals({ w1: [1, 1, 0], w2: [2, 2, 0], w3: [0, 0, 0] })).dim).toBe(1);
		expect(derive2(vals({ w1: [0, 0, 0], w2: [0, 0, 0], w3: [0, 0, 0] })).dim).toBe(0);
	});
	test('drag magnet drops a near-plane tip into the plane; keys stay on the grid', () => {
		const v0 = vals({ w3: [1, 1, 0.5] });
		const m = moveW(2)(v0, [0, 0, -0.4], 'drag');
		expect(m.ease).toEqual(['w3']);
		close(m.set.w3 as number[], [1, 1, 0]);
		const k = moveW(2)(v0, [0, 0, -0.1], 'key');
		close(k.set.w3 as number[], [1, 1, 0.4]);
		expect(k.ease).toBeUndefined();
		const far = moveW(2)(v0, [0, 0, 1], 'drag');
		close(far.set.w3 as number[], [1, 1, 1.5]);
	});
});

describe('BS-3 rotated axes', () => {
	test('prose: P = (3, 1) at 45° reads (2.828, 1.414) and keeps its length', () => {
		const z = derive3(vals());
		expect(Math.abs(z.c1 - 2 * Math.SQRT2)).toBeLessThan(1e-12);
		expect(Math.abs(z.c2 - Math.SQRT2)).toBeLessThan(1e-12);
		expect(Math.abs(z.len2 - z.clen2)).toBeLessThan(1e-12);
		close(combine([z.u1, z.u2], [z.c1, z.c2]), [3, 1]);
	});
	test('the pair is orthonormal at every angle', () => {
		for (const th of [-180, -37, 0, 45, 90, 133]) {
			const [a, b] = frame2(th);
			expect(Math.abs(norm(a) - 1)).toBeLessThan(1e-12);
			expect(Math.abs(dot(a, b))).toBeLessThan(1e-12);
			expect(det2(a, b)).toBeLessThan(0); // clockwise pair, as the prose's (1, 1)/√2, (1, −1)/√2
		}
	});
	test('angle drag snaps to 15° near a target, keys leave a target', () => {
		const at = (th: number) => vals({ th });
		const [c, s] = [Math.cos((44 * Math.PI) / 180), Math.sin((44 * Math.PI) / 180)];
		const m = moveTh(at(30), [c - Math.cos(Math.PI / 6), s - 0.5], 'drag');
		expect(m.set.th).toBe(45);
		expect(m.ease).toEqual(['th']);
		const key = moveTh(at(45), [-0.0123, 0.0123], 'key');
		expect(key.set.th).toBe(46);
	});
	test('challenges: on the axis near 18°, (1, 3) at 90° where the pair is (0, 1), (1, 0)', () => {
		expect(CHECKS.onAxis(vals({ th: 18 }))).toBe(true);
		expect(CHECKS.onAxis(vals({ th: 45 }))).toBe(false);
		expect(CHECKS.swapped(vals({ th: 90 }))).toBe(true);
		close(frame2(90).flat(), [0, 1, 1, 0], 1e-12);
		expect(CHECKS.swapped(vals({ th: -90 }))).toBe(false);
	});
});

describe('BS-4 rulers', () => {
	test('the starting frame is orthonormal and right-handed; v = (3, 3, 0) reads (4, −1, −1)', () => {
		const z = derive4(vals());
		expect(z.orthonormal).toBe(true);
		expect(z.hand).toBe(1);
		close(z.rd, [4, -1, -1]);
		close(z.c!, [4, -1, -1]);
		expect(z.gap).toBeLessThan(1e-12);
		close(cross(FRAME0[0], FRAME0[1]), FRAME0[2]);
	});
	test('oblique rulers: readings are not coordinates and the ghost misses v', () => {
		const z = derive4(vals({ ortho: 0, r1: [1, 0, 0], r2: [1, 1, 0], r3: [0, 0, 1], v3: [3, 4, 0] }));
		close(z.c!, [-1, 4, 0]);
		close(z.rd, [3, 7, 0]);
		close(z.ghost, [10, 7, 0]);
		expect(z.gap).toBeCloseTo(Math.hypot(7, 3), 12);
		expect(z.perp).toEqual([false, true, true]);
		expect(z.unit).toEqual([true, false, true]);
		expect(z.orthonormal).toBe(false);
		// Feet are the perpendicular drops: v − foot ⟂ ruler.
		const f = z.feet[1]!;
		expect(Math.abs(dot([3 - f[0], 4 - f[1], 0 - f[2]], [1, 1, 0]))).toBeLessThan(1e-12);
		expect(CHECKS.ghostMiss(vals({ ortho: 0, r2: [0, 1, 1] }))).toBe(true);
	});
	test('dependent rulers: no coordinates, flat handedness', () => {
		const z = derive4(vals({ ortho: 0, r1: [1, 0, 0], r2: [0, 1, 0], r3: [1, 1, 0] }));
		expect(z.c).toBeNull();
		expect(z.hand).toBe(0);
		expect(z.walk).toBeNull();
	});
	test('a left-handed orthonormal frame reports −1', () => {
		expect(derive4(vals({ ortho: 0, r3: FRAME0[2].map((x) => -x) })).hand).toBe(-1);
	});
	test('Gram–Schmidt keeps the leader direction and completes degenerate slots', () => {
		const f = gramSchmidt([[2, 0, 0], [1, 1, 0], [5, 5, 0]])!;
		close(f[0], [1, 0, 0]);
		close(f[1], [0, 1, 0]);
		close(f[2], [0, 0, 1]); // completed by u₁ × u₂: right-handed
		const g = gramSchmidt([[1, 0, 0], [0, 2, 0], [0, 0, 3]], [1, 0, 2])!;
		close(g[1], [0, 1, 0]);
		expect(gramSchmidt([[0, 0, 0], [0, 1, 0], [0, 0, 1]])).toBeNull();
	});
	test('orthoLink: flag on orthonormalizes the written ruler first; flag off leaves rulers free', () => {
		const link = orthoLink();
		const cur = vals();
		const out = link.resolve({ r2: [0, 0, 2] }, cur);
		close(out.r2 as number[], [0, 0, 1]);
		const rs = ['r1', 'r2', 'r3'].map((k) => out[k] as number[]);
		for (let i = 0; i < 3; i++) {
			expect(Math.abs(norm(rs[i]) - 1)).toBeLessThan(1e-12);
			for (let j = i + 1; j < 3; j++) expect(Math.abs(dot(rs[i], rs[j]))).toBeLessThan(1e-12);
		}
		const free = link.resolve({ ortho: 0, r2: [0, 0, 2] }, cur);
		expect(free.r2).toEqual([0, 0, 2]);
		const back = link.resolve({ ortho: 1 }, { ...cur, ortho: 0, r1: [2, 0, 0], r2: [1, 1, 0], r3: [0, 1, 1] });
		close(back.r1 as number[], [1, 0, 0]);
		close(back.r2 as number[], [0, 1, 0]);
		close(back.r3 as number[], [0, 0, 1]);
		// A zero leader is refused: the previous frame stays.
		const zero = link.resolve({ r1: [0, 0, 0] }, cur);
		close(zero.r1 as number[], FRAME0[0]);
		expect(() => link.resolve({ ortho: 2 }, cur)).toThrow();
		expect(link.shown(cur)).toEqual({});
	});
	test('ruler ticks stay within the ruler and skip zero', () => {
		expect(rulerTicks([1, 0, 0]).length).toBe(6);
		expect(rulerTicks([0, 0, 0])).toEqual([]);
		expect(rulerTicks([0.1, 0, 0]).length).toBe(24);
	});
});

describe('formatting', () => {
	test('no negative zero, dashes for missing values', () => {
		expect(fmt(-0.001)).toBe('0.00');
		expect(fmt(NaN)).toBe('–');
		expect(fmtIn(0.3)).toBe('0.3');
		expect(fmtIn(2 / 3)).toBe('0.67');
	});
});
