import { describe, expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { lessonStore } from '../../src/viz/store';
import {
	CHECKS, FRAME0, INITIAL, LESSON, LINKS, PRESETS, byCols, byRows, derive1, derive2, isIdentity, matOfQuat, mul, products, quatOf, slerpFromId,
	transferMat, transpose,
} from '../../src/viz/lessons/matrix-columns.math';
import { rowsFor } from '../../src/viz/lessons/matrix-columns.rows';

/** Independent pure checks of lesson 2.2's model: prose numbers, row = column, transfer paths, degenerate frames. */

const vals = (patch: Record<string, number | readonly number[]> = {}) => ({ ...INITIAL, ...patch });
const close = (a: readonly number[], b: readonly number[], eps = 1e-12) => {
	expect(a.length).toBe(b.length);
	a.forEach((x, i) => expect(Math.abs(x - b[i])).toBeLessThan(eps));
};
const det3 = (m: number[][]) =>
	m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0]) + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
const I3 = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];

describe('MC-1: two readings of Mx', () => {
	test('prose: rows (4 − 1, 2 − 2) = columns (4, 2) + (−1, −2) = (3, 0)', () => {
		const z = derive1(vals());
		close(z.yRows, [3, 0]);
		close(z.yCols, [3, 0]);
		close(z.prods.flat(), [4, -1, 2, -2]);
		close(z.leg, [4, 2]);
	});
	test('rows and columns agree for arbitrary matrices (same products, other grouping)', () => {
		for (const [m, x] of [[[[1.5, -2], [0.3, 4]], [-1.2, 0.7]], [[[0, 0], [0, 0]], [3, -3]], [[[3, 1, 2], [1, 0, 2]], [1, 2, -1]]] as const) {
			const M = m.map((r) => [...r]);
			close(byRows(M, x), byCols(M, x), 1e-12);
			close(products(M, x).map((r) => r.reduce((s, p) => s + p, 0)), byRows(M, x), 1e-12);
		}
		close(byRows([[3, 1, 2], [1, 0, 2]], [1, 2, -1]), [3, -1]);
	});
	test('a column is the image of a basis vector; a squashing matrix can lose a non-zero input', () => {
		expect(CHECKS.column2(vals({ x: [0, 1] }))).toBe(true);
		close(derive1(vals({ x: [0, 1] })).y, [1, 2]);
		expect(CHECKS.lost(vals({ m1: [1, 1], m2: [2, 2], x: [2, -1] }))).toBe(true);
		expect(CHECKS.lost(vals({ x: [0, 0] }))).toBe(false);
	});
});

describe('MC-2: rulers as rows', () => {
	test('prose frame: Rv = (3, −4, 2), Gram = I, columns rebuild Rv, rulers rebuild v', () => {
		const z = derive2(vals());
		close(z.y, [3, -4, 2]);
		expect(isIdentity(z.g)).toBe(true);
		expect(z.orthonormal && z.rotation).toBe(true);
		close(z.colWalk[3], [3, -4, 2]);
		close(z.cols.flat(), [0.6, -0.8, 0, 0.8, 0.6, 0, 0, 0, 1]);
		close(z.c!, [3, -4, 2]);
		close(z.ghost, [5, 0, 2]);
		expect(z.gap).toBeLessThan(1e-12);
		close(z.feet[0]!, [1.8, 2.4, 0]);
	});
	test('oblique frame: c = (1, 1, 3), readings (2, 3, 3), ghost (5, 3, 3), gap √13, d = Gc', () => {
		const z = derive2(vals(PRESETS.oblique));
		close(z.c!, [1, 1, 3]);
		close(z.y, [2, 3, 3]);
		close(z.ghost, [5, 3, 3]);
		expect(z.gap).toBeCloseTo(Math.sqrt(13), 12);
		close(z.g.flat(), [1, 1, 0, 1, 2, 0, 0, 0, 1]);
		close(byRows(z.g, z.c!), z.y);
		expect(z.orthonormal).toBe(false);
		expect(CHECKS.ghostMiss(vals(PRESETS.oblique))).toBe(true);
		// Projection vector onto the non-unit ruler u₂ = (1, 1, 0): (u₂·v / u₂·u₂) u₂ = (1.5, 1.5, 0).
		close(z.feet[1]!, [1.5, 1.5, 0]);
	});
	test('unit but oblique: c = (1.4, 1, 3), readings (2, 1.84, 3), ghost (3.104, 1.472, 3)', () => {
		const z = derive2(vals(PRESETS.unitOblique));
		close(z.c!, [1.4, 1, 3], 1e-12);
		close(z.y, [2, 1.84, 3], 1e-12);
		close(z.ghost, [3.104, 1.472, 3], 1e-12);
		expect(z.unit.every(Boolean)).toBe(true);
		expect(z.orthonormal).toBe(false);
	});
	test('mirror: orthonormal, left-handed, the transpose still undoes R, the transfer is a blend through a flat picture', () => {
		const z = derive2(vals(PRESETS.mirror));
		expect(z.orthonormal).toBe(true);
		expect(z.vol).toBe(-1);
		expect(z.hand).toBe(-1);
		expect(z.rotation).toBe(false);
		close(byRows(transpose(z.R), z.y), z.v);
		expect(det3(transferMat(z.R, false, 0.5))).toBeCloseTo(0, 12);
		expect(CHECKS.mirror(vals(PRESETS.mirror))).toBe(true);
		expect(CHECKS.mirror(vals())).toBe(false);
	});
	test('duplicate rulers: no coordinates, flat; a zero ruler has no shadow', () => {
		const z = derive2(vals(PRESETS.duplicate));
		expect(z.c).toBeNull();
		expect(z.hand).toBe(0);
		expect(z.walk).toBeNull();
		const zero = derive2(vals({ ortho: 0, q2: [0, 0, 0] }));
		expect(zero.feet[1]).toBeNull();
		expect(zero.c).toBeNull();
	});
});

describe('transfer', () => {
	test('quaternion round trip and slerp end points', () => {
		const R = FRAME0.map((r) => [...r]);
		const q = quatOf(R);
		close(matOfQuat(q).flat(), R.flat(), 1e-12);
		close(slerpFromId(q, 0).flat(), I3.flat(), 1e-12);
		close(slerpFromId(q, 1).flat(), R.flat(), 1e-12);
	});
	test('every in-between of a rotation is a rotation (orthonormal, det +1)', () => {
		const R = FRAME0.map((r) => [...r]);
		for (const tau of [0.25, 0.5, 0.75]) {
			const T = transferMat(R, true, tau);
			close(mul(T, transpose(T)).flat(), I3.flat(), 1e-12);
			expect(det3(T)).toBeCloseTo(1, 12);
		}
	});
	test('quatOf handles every branch: half turns about x, y, z and a general frame', () => {
		const frames = [
			[[1, 0, 0], [0, -1, 0], [0, 0, -1]],
			[[-1, 0, 0], [0, 1, 0], [0, 0, -1]],
			[[-1, 0, 0], [0, -1, 0], [0, 0, 1]],
			[[2 / 3, 2 / 3, 1 / 3], [-2 / 3, 1 / 3, 2 / 3], [1 / 3, -2 / 3, 2 / 3]],
		];
		for (const R of frames) {
			close(slerpFromId(quatOf(R), 1).flat(), R.flat(), 1e-12);
			close(transferMat(R, true, 1).flat(), R.flat(), 1e-12);
		}
	});
	test('at τ = 1 the rulers land on the axes and v on Rv; the world v is unchanged at τ = 0', () => {
		const z1 = derive2(vals({ tr: 1 }));
		z1.u.forEach((u, i) => close(z1.at(u), I3[i], 1e-12));
		close(z1.at(z1.v), [3, -4, 2], 1e-12);
		const z0 = derive2(vals());
		close(z0.at(z0.v), [5, 0, 2]);
		// A bent frame lands its rulers on the columns of the Gram matrix, not on the axes.
		const zb = derive2(vals({ ...PRESETS.oblique, tr: 1 }));
		close(zb.at(zb.u[1]), [1, 2, 0], 1e-12);
	});
});

describe('store and rows', () => {
	test('the orthonormal link re-squares the rulers while on, leaves them free while off', () => {
		// A test-only store id: the real lesson id belongs to the lesson module (AGENTS.md, Tests).
		const s = lessonStore(LESSON + ':test', INITIAL, LINKS);
		s.getState().setMany({ q1: [1, 1, 0] });
		const z = derive2(s.getState().vals);
		expect(z.orthonormal).toBe(true);
		close(z.u[0], [Math.SQRT1_2, Math.SQRT1_2, 0], 1e-12);
		s.getState().setMany(PRESETS.oblique);
		expect(derive2(s.getState().vals).orthonormal).toBe(false);
		s.getState().setMany(PRESETS.ortho);
		close(derive2(s.getState().vals).y, [3, -4, 2], 1e-12);
	});
	test('row ids are locked (term ids shared with tables, formulas and hover links)', () => {
		expect(rowsFor(1, 'en').map((r) => r.id)).toEqual(['m1', 'm2', 'x', 'y1', 'y2', 'y']);
		expect(rowsFor(2, 'en').map((r) => r.id)).toEqual(['p', 'ortho', 'q1', 'q2', 'q3', 'tr', 'yv', 'gap', 'hand']);
		expect(rowsFor(2, 'ru').map((r) => r.id)).toEqual(rowsFor(2, 'en').map((r) => r.id));
	});
});
