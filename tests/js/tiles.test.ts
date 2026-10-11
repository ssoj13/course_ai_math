import { describe, expect, test } from 'bun:test';
import {
	boxMatrix, checkTiles, layoutTiles, MAX_TILES, retarget, stepMotion, stubBox, THIN, tileBounds, tileBuf, tileCount, tileMotion, tileTotal,
	type TileBuf, type TileOpts, type TileShape,
} from '../../src/viz/scene/tiles';
import { omegaFor, SPRING_MS, springStep } from '../../src/viz/scene/motion';

// Finite comparator: NaN/Infinity never compare equal to anything (a NaN layout must fail, not pass vacuously).
const near = (a: number, b: number, tol = 1e-12) => Number.isFinite(a) && Number.isFinite(b) && Math.abs(a - b) <= tol * Math.max(1, Math.abs(a), Math.abs(b));
const sq = (s: number): TileShape => ({ kind: 'box', size: [s, s, 0], thin: [false, false, true] });
const cube = (s: number): TileShape => ({ kind: 'box', size: [s, s, s], thin: [false, false, false] });

/** Every piece of a layout as {lo, hi, whole}. */
function pieces(b: TileBuf) {
	return Array.from({ length: b.n }, (_, n) => ({ lo: [...b.lo.slice(3 * n, 3 * n + 3)], hi: [...b.hi.slice(3 * n, 3 * n + 3)], whole: b.whole[n] === 1, slot: b.slot[n] }));
}
const measure = (p: { lo: number[]; hi: number[] }, active: boolean[]) => p.lo.reduce((m, lo, a) => m * (active[a] ? p.hi[a] - lo : 1), 1);

/** Checks a layout against its count: piece number, whole flags, union measure, disjoint interiors, finite sizes. */
function verify(shape: TileShape, opts: TileOpts, active: boolean[]) {
	const c = tileCount(shape, opts);
	const ps = pieces(layoutTiles(shape, opts, tileBuf()));
	expect(ps.length).toBe(c.pieces);
	expect(ps.filter((p) => p.whole).length).toBe(c.whole);
	expect(near(ps.reduce((s, p) => s + measure(p, active), 0), c.total)).toBe(true);
	expect(near(ps.filter((p) => !p.whole).reduce((s, p) => s + measure(p, active), 0), c.partial)).toBe(true);
	for (const p of ps) for (let a = 0; a < 3; a++) expect(p.hi[a] - p.lo[a]).toBeGreaterThan(0);
	for (let i = 0; i < ps.length; i++)
		for (let j = i + 1; j < ps.length; j++) {
			const overlap = active.every((on, a) => !on || Math.min(ps[i].hi[a], ps[j].hi[a]) - Math.max(ps[i].lo[a], ps[j].lo[a]) > 1e-12);
			expect(overlap).toBe(false);
		}
	expect(new Set(ps.map((p) => p.slot)).size).toBe(ps.length);
	return { c, ps };
}

describe('TileGrid counts (storyboard 7.1)', () => {
	test('squares of side 0, 0.1, 0.5, 1, 2.5, 3, 4 and their mirrors', () => {
		const table: [number, number, number, number][] = [[0, 0, 0, 0], [0.1, 0, 1, 0.01], [0.5, 0, 1, 0.25], [1, 1, 1, 1], [2.5, 4, 9, 6.25], [3, 9, 9, 9], [4, 16, 16, 16]];
		for (const [s, whole, pieces, total] of table)
			for (const sign of [1, -1]) {
				const { c } = verify(sq(sign * s), { negative: 'mirror' }, [true, true, false]);
				expect(c.whole).toBe(whole);
				expect(c.pieces).toBe(pieces);
				expect(near(c.total, total)).toBe(true);
				expect(near(c.partial, total - whole)).toBe(true);
			}
	});
	test('x = 2.5: 4 whole + 2.25 partial area in 5 clipped pieces, each clipped exactly (no scaled unit body)', () => {
		const { c, ps } = verify(sq(2.5), {}, [true, true, false]);
		expect(c).toEqual({ whole: 4, partial: 2.25, pieces: 9, total: 6.25 });
		expect(ps.filter((p) => !p.whole).length).toBe(5);
		const sizes = ps.filter((p) => !p.whole).map((p) => [p.hi[0] - p.lo[0], p.hi[1] - p.lo[1]].sort().join());
		expect(sizes.sort()).toEqual(['0.5,0.5', '0.5,1', '0.5,1', '0.5,1', '0.5,1']);
	});
	test('areas 0, 2, 2.25, 9, 16 through the root (0.G-3)', () => {
		const table: [number, number, number, number][] = [[0, 0, 0, 0], [2, 1, 3, 1], [2.25, 1, 3, 1.25], [9, 9, 0, 0], [16, 16, 0, 0]];
		for (const [A, whole, partialPieces, partial] of table) {
			const { c } = verify(sq(Math.sqrt(A)), {}, [true, true, false]);
			expect(c.whole).toBe(whole);
			expect(c.pieces - c.whole).toBe(partialPieces);
			expect(c.partial.toFixed(2)).toBe(partial.toFixed(2));
		}
	});
	test('explicit thin axis: active 0.1 square has area 0.01, a 0.1 cube volume 0.001; thin axes are never counted', () => {
		expect(tileCount(sq(0.1))).toEqual({ whole: 0, partial: 0.1 * 0.1, pieces: 1, total: 0.1 * 0.1 });
		const v = tileCount(cube(0.1));
		expect(v.pieces).toBe(1);
		expect(v.whole).toBe(0);
		expect(near(v.total, 0.001)).toBe(true);
		// The display-only axis is THIN thick whatever its size, and a 0.1 active side stays 0.1 (no size threshold).
		for (const display of [0, 0.1, 4]) {
			const p = pieces(layoutTiles({ kind: 'box', size: [0.1, 0.1, display], thin: [false, false, true] }, {}, tileBuf()))[0];
			expect(near(p.hi[2] - p.lo[2], THIN)).toBe(true);
			expect(near(p.hi[0] - p.lo[0], 0.1)).toBe(true);
		}
	});
	test('3D cubes and heterogeneous boxes: 64 bodies at 4, 27 at 3, seams between unit bodies', () => {
		expect(verify(cube(4), {}, [true, true, true]).c.pieces).toBe(64);
		expect(verify(cube(3), {}, [true, true, true]).c).toEqual({ whole: 27, partial: 0, pieces: 27, total: 27 });
		const { c } = verify(cube(2.5), {}, [true, true, true]);
		expect(c.whole).toBe(8);
		expect(c.pieces).toBe(27);
		expect(near(c.partial, 2.5 ** 3 - 8)).toBe(true);
		for (let mask = 0; mask < 8; mask++) {
			const size = [0.1, 2.5, 4].map((s, a) => (mask >> a & 1 ? -s : s)) as unknown as readonly [number, number, number];
			verify({ kind: 'box', size, thin: [false, false, false] }, { negative: 'mirror' }, [true, true, true]);
		}
	});
	test('an active zero side is empty; a slab and a stick count only their active axes', () => {
		expect(tileCount({ kind: 'box', size: [3, 0, 2], thin: [false, false, false] }).pieces).toBe(0);
		expect(tileCount({ kind: 'box', size: [3, 0, 2], thin: [false, true, false] })).toEqual({ whole: 6, partial: 0, pieces: 6, total: 6 });
		expect(tileCount({ kind: 'box', size: [2.5, 0, 0], thin: [false, true, true] })).toEqual({ whole: 2, partial: 0.5, pieces: 3, total: 2.5 });
	});
});

describe('TileGrid geometry', () => {
	test('mirrored square: same pieces through the anchor; union [-3, 0]^2 at x = -3 (anchored low corner)', () => {
		const b = layoutTiles(sq(-3), { negative: 'mirror', origin: [0, 0] }, tileBuf());
		const lo = [0, 0, 0], hi = [0, 0, 0];
		expect(tileBounds(b, lo, hi)).toBe(true);
		expect([lo[0], lo[1], hi[0], hi[1]]).toEqual([-3, -3, 0, 0]);
		const o = [1.25, -2, 3.5];
		const m = layoutTiles(sq(-2.5), { negative: 'mirror', origin: o }, tileBuf());
		tileBounds(m, lo, hi);
		expect([lo[0], lo[1], hi[0], hi[1]]).toEqual([o[0] - 2.5, o[1] - 2.5, o[0], o[1]]);
		expect(near(hi[2] - lo[2], THIN)).toBe(true);
	});
	test('rows: ceil(len) pieces, wrap bands toward -y (xy) and +z (xz), last piece clipped', () => {
		for (const plane of ['xy', 'xz'] as const)
			for (const len of [0, 0.25, 1, 2.5, 16, 27])
				for (const wrap of [1, 2, 3, 16]) {
					const shape: TileShape = { kind: 'row', len, wrap };
					const active = [true, plane === 'xy', plane === 'xz'];
					const { c, ps } = verify(shape, { plane }, active);
					expect(c.pieces).toBe(Math.ceil(len));
					ps.forEach((p, n) => {
						const band = Math.floor(n / wrap);
						expect(p.lo[0]).toBe(n % wrap);
						expect((plane === 'xy' ? p.lo[1] : p.lo[2]) + 0).toBe((plane === 'xy' ? -band : band) + 0);
						expect(near(p.hi[0] - p.lo[0], Math.min(1, len - n))).toBe(true);
					});
				}
	});
	test('fill: one body `fill` wide in an uncounted unit outline', () => {
		for (const fill of [0, 0.25, 1]) {
			const b = layoutTiles({ kind: 'row', len: 1, wrap: 3 }, { fill }, tileBuf());
			expect(tileCount({ kind: 'row', len: 1, wrap: 3 }, { fill })).toEqual({ whole: fill === 1 ? 1 : 0, partial: fill - (fill === 1 ? 1 : 0), pieces: fill > 0 ? 1 : 0, total: fill });
			expect(b.n).toBe(fill > 0 ? 1 : 0);
			expect(b.hasOutline).toBe(true);
			expect([...b.outline.lo, ...b.outline.hi]).toEqual([0, 0, 0, 1, 1, THIN]);
			if (b.n) expect(near(b.hi[0] - b.lo[0], fill)).toBe(true);
		}
	});
	test('instance matrices written into the actual Float32Array layout three uses (column-major, unit body)', () => {
		const b = layoutTiles(sq(2.5), {}, tileBuf());
		const m = tileMotion();
		retarget(m, b, false, [0, 0, 0], true);
		const out = new Float32Array(16 * MAX_TILES);
		let k = 0;
		for (let s = 0; s < MAX_TILES; s++) if (m.state[s]) boxMatrix(m.cur, 6 * s, 1, out, 16 * k++);
		expect(k).toBe(9);
		// Slot 2 (i = 2, j = 0) is the clipped piece [2, 2.5] x [0, 1] x [0, 0.12].
		const at = [...Array(9).keys()].find((i) => near(out[16 * i + 12], 2.25) && near(out[16 * i + 13], 0.5))!;
		const mat = [...out.slice(16 * at, 16 * at + 16)];
		expect(mat.map((x) => Math.round(x * 1e6) / 1e6)).toEqual([0.5, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0.12, 0, 2.25, 0.5, 0.06, 1]);
	});
	test('invalid input is rejected before anything is laid out', () => {
		const bad: [TileShape, TileOpts][] = [
			[sq(4.1), {}], [sq(-1), {}], [sq(NaN), {}], [sq(Infinity), { negative: 'mirror' }],
			[{ kind: 'box', size: [1, 1, 1], thin: [true, true, true] }, {}],
			[sq(1), { fill: 0.5 }], [sq(1), { origin: [NaN, 0] }],
			[{ kind: 'row', len: 2, wrap: 3 }, { fill: 0.25 }], [{ kind: 'row', len: 1, wrap: 3 }, { fill: 1.1 }],
			[{ kind: 'row', len: 1, wrap: 0 }, {}], [{ kind: 'row', len: 1, wrap: 17 }, {}], [{ kind: 'row', len: 1, wrap: 1.5 }, {}],
			[{ kind: 'row', len: -1, wrap: 3 }, {}], [{ kind: 'row', len: 65, wrap: 16 }, {}],
		];
		for (const [shape, opts] of bad) {
			expect(() => checkTiles(shape, opts)).toThrow();
			expect(() => layoutTiles(shape, opts, tileBuf())).toThrow();
			expect(() => tileCount(shape, opts)).toThrow();
		}
	});
	test('plate total: integer when every active size is, else 2 decimals', () => {
		expect(tileTotal(sq(3), 9)).toBe('9');
		expect(tileTotal(sq(2.5), 6.25)).toBe('6.25');
		expect(tileTotal(sq(Math.SQRT2), 2.0000000000000004)).toBe('2.00');
		expect(tileTotal(sq(0), 0)).toBe('0');
	});
});

describe('TileGrid transitions (no pop, counts are target truth)', () => {
	test('born pieces start as stubs no larger than min(THIN, size), spring monotonically, and land exactly', () => {
		const m = tileMotion();
		retarget(m, layoutTiles(sq(2), {}, tileBuf()), false, [0, 0, 0], true);
		const target = layoutTiles(sq(3), {}, tileBuf());
		retarget(m, target, false, [0, 0, 0], false);
		// Slot of piece (2, 2): born along x and y; it starts at [2, 2.12] x [2, 2.12].
		const s = 2 + 4 * 2;
		expect([m.cur[6 * s], m.cur[6 * s + 1], m.cur[6 * s + 3], m.cur[6 * s + 4]].map((x) => Math.round(x * 1e9) / 1e9)).toEqual([2, 2, 2.12, 2.12]);
		let last = m.cur[6 * s + 3];
		let frames = 0;
		while (stepMotion(m, 16)) {
			expect(m.cur[6 * s + 3]).toBeGreaterThanOrEqual(last - 1e-12);
			expect(m.cur[6 * s + 3]).toBeLessThanOrEqual(3 + 1e-12);
			last = m.cur[6 * s + 3];
			frames++;
		}
		expect(frames).toBeGreaterThan(3);
		expect(m.cur[6 * s + 3]).toBe(3);
		expect(tileCount(sq(3)).pieces).toBe(9);
	});
	test('a 0.1 piece is never enlarged to THIN; dying pieces shrink into a stub and are then hidden', () => {
		const lo = new Float64Array([0, 0, 0, 0.1, 0.1, THIN]);
		const out = new Float64Array(6);
		stubBox(lo, 0, 0, false, [0, 0, 0], [0, 0, 0], out, 0);
		expect([...out].map((x) => Math.round(x * 1e9) / 1e9)).toEqual([0, 0, 0, 0.1, 0.1, THIN]);
		const m = tileMotion();
		retarget(m, layoutTiles(sq(3), {}, tileBuf()), false, [0, 0, 0], true);
		retarget(m, layoutTiles(sq(1), {}, tileBuf()), false, [0, 0, 0], false);
		const dying = [...m.state].filter((x) => x === 2).length;
		expect(dying).toBe(8);
		while (stepMotion(m, 16));
		expect([...m.state].filter((x) => x !== 0).length).toBe(1);
	});
	test('mirrored dying pieces shrink toward the anchor', () => {
		const m = tileMotion();
		retarget(m, layoutTiles(sq(-2), { negative: 'mirror' }, tileBuf()), false, [0, 0, 0], true);
		retarget(m, layoutTiles(sq(-1), { negative: 'mirror' }, tileBuf()), false, [0, 0, 0], false);
		// Slot 1 = piece (1, 0) at x in [-2, -1]: its stub keeps the end nearer the anchor, [-1.12, -1].
		expect([m.to[6], m.to[9]].map((x) => Math.round(x * 1e9) / 1e9)).toEqual([-1.12, -1]);
	});
	test('the inlined spring is the shared springStep', () => {
		const m = tileMotion();
		retarget(m, layoutTiles(sq(1), {}, tileBuf()), false, [0, 0, 0], true);
		retarget(m, layoutTiles(sq(2), {}, tileBuf()), false, [0, 0, 0], false);
		const s = 1;
		const before = m.cur[6 * s + 3];
		stepMotion(m, 16);
		const ref = springStep({ x: before, v: 0 }, 2, omegaFor(SPRING_MS), 0.016);
		expect(near(m.cur[6 * s + 3], ref.x, 1e-12)).toBe(true);
	});
});
