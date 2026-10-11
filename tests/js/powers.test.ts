import { describe, expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { checkTiles, layoutTiles, tileBuf, tileCount } from '../../src/viz/scene/tiles';
import { powersLinks } from '../../src/viz/links';
import { lessonStore } from '../../src/viz/store';
import {
	bandMid, CHECKS, cubeBox, derive, fmt4, fmtAmt, fmtSci, fmtVal, fmtX, INITIAL, LADDER_TOP, powerBox, PRESETS, RUNG_GAP, RUNGS, rungBands, rungId,
	rungShape, rungTop, SPEC, squareBox, sup, WRAP,
} from '../../src/viz/lessons/powers.math';
import { rowsFor } from '../../src/viz/lessons/powers.rows';

/** Independent numbers for lesson 0.2 (storyboard §7.6-§7.11): counts, linked writes, ladder geometry, curves. */

const vals = (over: Record<string, number> = {}) => {
	const v: Record<string, number> = { ...(INITIAL as Record<string, number>), ...over };
	// Keep the x/A/V triple coherent like the store does.
	if ('x' in over) Object.assign(v, { A: v.x ** 2, V: v.x ** 3 });
	return v;
};

describe('x^n bodies count x^n unit pieces', () => {
	test('x = 3: a stick of 3, a slab of 9, a cube of 27 = 3 layers of 9', () => {
		expect(tileCount(powerBox(1, 3))).toEqual({ whole: 3, partial: 0, pieces: 3, total: 3 });
		expect(tileCount(powerBox(2, 3))).toEqual({ whole: 9, partial: 0, pieces: 9, total: 9 });
		expect(tileCount(powerBox(3, 3))).toEqual({ whole: 27, partial: 0, pieces: 27, total: 27 });
		expect(derive(vals({ x: 3, n: 3 })).layers).toBe(3);
	});
	test('x = 2.5: 8 whole cubes and 7.625 in cut pieces (15.625); 6.25 = 4 whole tiles + 2.25', () => {
		const c = tileCount(powerBox(3, 2.5));
		expect(c.whole).toBe(8);
		expect(c.total).toBe(15.625);
		expect(c.partial).toBeCloseTo(7.625, 12);
		expect(c.pieces).toBe(27);
		const s = tileCount(squareBox(2.5));
		expect([s.whole, s.partial, s.pieces]).toEqual([4, 2.25, 9]);
	});
	test('thin axes are display-only: a 0.1 stick is one 0.1 piece, never 0.12 of display thickness', () => {
		const c = tileCount(powerBox(1, 0.1));
		expect(c.total).toBeCloseTo(0.1, 12);
		expect(c.whole).toBe(0);
		expect(c.pieces).toBe(1);
	});
	test('x = 0 draws nothing for every n; n outside 1..3 is an authoring error', () => {
		for (const n of [1, 2, 3]) expect(tileCount(powerBox(n, 0)).pieces).toBe(0);
		expect(() => powerBox(4, 1)).toThrow();
		expect(() => powerBox(0, 1)).toThrow();
	});
	test('roots: A = 2 gives 1 whole tile + 1.00 of cut area; V = 2 gives 1 whole cube + 1.00 cut', () => {
		const sq = tileCount(squareBox(Math.sqrt(2)));
		expect(sq.whole).toBe(1);
		expect(sq.partial).toBeCloseTo(1, 12);
		const cb = tileCount(cubeBox(Math.cbrt(2)));
		expect(cb.whole).toBe(1);
		expect(cb.pieces).toBe(8);
		expect(cb.partial).toBeCloseTo(1, 12);
		expect(fmt4(Math.cbrt(2))).toBe('1.2599');
	});
});

describe('linked x / A / V store writes (powersLinks)', () => {
	const fresh = (name: string) => lessonStore('powers-test-' + name, INITIAL, powersLinks());
	test('initial snapshot is coherent and n / k mirror into the timelines', () => {
		const s = fresh('init').getState();
		expect([s.vals.x, s.vals.A, s.vals.V, s.vals.from]).toEqual([3, 9, 27, 0]);
		expect(s.step.power).toBe(1);
		expect(s.step.ladder).toBe(0);
	});
	test('x = 2.5 -> A = 6.25, V = 15.625, from x; A = 20 clamps to 16 -> x = 4, V = 64, from A; V = 8 -> x = 2, A = 4', () => {
		const st = fresh('xav');
		st.getState().set('x', 2.5);
		expect([st.getState().vals.A, st.getState().vals.V, st.getState().vals.from]).toEqual([6.25, 15.625, 0]);
		st.getState().set('A', 20);
		expect([st.getState().vals.x, st.getState().vals.A, st.getState().vals.V, st.getState().vals.from]).toEqual([4, 16, 64, 1]);
		st.getState().set('V', 8);
		expect([st.getState().vals.x, st.getState().vals.A, st.getState().vals.from]).toEqual([2, 4, 2]);
		expect(CHECKS.edgeTwo({ ...st.getState().vals, d3: 1 })).toBe(true);
	});
	test('a negative side clamps to 0 (negative bases are prose-only here)', () => {
		const st = fresh('neg');
		st.getState().set('x', -2);
		expect([st.getState().vals.x, st.getState().vals.A, st.getState().vals.V]).toEqual([0, 0, 0]);
	});
	test('n and k are authoritative; the timelines write them through the adapters', () => {
		const st = fresh('steps');
		st.getState().set('n', 3);
		expect(st.getState().step.power).toBe(2);
		st.getState().setStep('power', 0);
		expect(st.getState().vals.n).toBe(1);
		st.getState().setStep('ladder', 3);
		expect(st.getState().vals.k).toBe(0);
		st.getState().set('k', -3);
		expect(st.getState().step.ladder).toBe(6);
	});
});

describe('ladder geometry (§7.9)', () => {
	test('seven fixed slots stack from y = 7 down to y = -7 without overlap', () => {
		expect(rungTop(3)).toBe(LADDER_TOP);
		let prevBottom = Infinity;
		for (const j of RUNGS) {
			const top = rungTop(j);
			expect(top).toBeLessThanOrEqual(prevBottom - RUNG_GAP + 1e-12);
			prevBottom = top - (j === 3 ? 4 : j === 2 ? 2 : 1);
		}
		expect(prevBottom).toBe(-7);
		expect(() => rungTop(4)).toThrow();
	});
	test('every rung fits its slot at every base of the range, and every rung is a valid TileGrid row', () => {
		const buf = tileBuf();
		for (let base = SPEC.base.min; base <= SPEC.base.max; base += SPEC.base.step) {
			for (const k of RUNGS) {
				for (const j of RUNGS) {
					const { shape, fill } = rungShape(base, j, k);
					const opts = { origin: [0, rungTop(j) - 1], fill };
					checkTiles(shape, opts);
					layoutTiles(shape, opts, buf);
					for (let i = 0; i < buf.n; i++) {
						expect(buf.hi[3 * i + 1]).toBeLessThanOrEqual(rungTop(j) + 1e-12);
						expect(buf.lo[3 * i]).toBeGreaterThanOrEqual(0);
						expect(buf.hi[3 * i]).toBeLessThanOrEqual(WRAP);
					}
				}
			}
		}
	});
	test('base 3: 3³ = 27 pieces in 4 bands (8 + 8 + 8 + 3); base 2.5: 15.625 in 2 bands; fractions are one band', () => {
		expect(rungBands(3, 3)).toBe(4);
		expect(rungBands(3, 2)).toBe(2);
		expect(rungBands(2.5, 3)).toBe(2);
		expect(rungBands(2, 3)).toBe(1);
		expect(rungBands(2, -2)).toBe(1);
		const c = tileCount(rungShape(2.5, 3, 3).shape);
		expect([c.whole, c.pieces, c.total]).toEqual([15, 16, 15.625]);
		expect(bandMid(3, 0)).toBe(6.5);
	});
	test('rungs below the current one are empty; fractions fill one unit outline to b^j', () => {
		expect(rungShape(2, -1, 0)).toEqual({ shape: { kind: 'row', len: 0, wrap: WRAP }, fill: undefined });
		const r = rungShape(2, -2, -3);
		expect(r.fill).toBe(0.25);
		expect(tileCount(r.shape, { fill: r.fill })).toEqual({ whole: 0, partial: 0.25, pieces: 1, total: 0.25 });
		expect(tileCount(rungShape(3, 0, 0).shape, { fill: rungShape(3, 0, 0).fill })).toEqual({ whole: 1, partial: 0, pieces: 1, total: 1 });
	});
	test('every step down divides by the base: base 2 gives 8, 4, 2, 1, 0.5, 0.25, 0.125', () => {
		const d = derive(vals({ base: 2 }));
		expect(RUNGS.map((j) => d[rungId(j) as 'p3'])).toEqual([8, 4, 2, 1, 0.5, 0.25, 0.125]);
		for (let i = 1; i < RUNGS.length; i++) {
			const b = derive(vals({ base: 1.5 }));
			expect(b[rungId(RUNGS[i]) as 'p3']).toBeCloseTo(b[rungId(RUNGS[i - 1]) as 'p3'] / 1.5, 12);
		}
		const z = derive(vals({ base: 3, k: -1 }));
		expect([z.cur, z.prevval, z.next]).toEqual([3 ** -1, 1, 3 ** -2]);
	});
	test('labels: base 1.5 rungs read 3.375 2.25 1.5 1 0.667 0.444 0.296', () => {
		expect(RUNGS.map((j) => fmtVal(1.5 ** j))).toEqual(['3.375', '2.25', '1.5', '1', '0.667', '0.444', '0.296']);
		expect(RUNGS.map(sup).join(' ')).toBe('³ ² ¹ ⁰ ⁻¹ ⁻² ⁻³');
	});
});

describe('pow curves (§7.11)', () => {
	test('g = 32, t = 0.5: 2.3e-10 reads 0.0000 at 4 decimals and 2.3e-10 in scientific form', () => {
		const d = derive(vals({ g: 32, t: 0.5 }));
		expect(d.y).toBeCloseTo(2.3283e-10, 13);
		expect(fmt4(d.y)).toBe('0.0000');
		expect(fmtSci(d.y)).toBe('2.3e-10');
		expect(fmtSci(0)).toBe('0.0000');
	});
	test('(t^g)^(1/g) reads t back for every preset; t = 1 is 1 on every curve; 0^0.2 = 0', () => {
		for (const p of PRESETS) {
			for (const t of [0.1, 0.5, 0.9, 1]) expect(derive(vals({ g: p.g, t })).yinv).toBeCloseTo(t, 9);
			expect(derive(vals({ g: p.g, t: 1 })).y).toBe(1);
		}
		expect(derive(vals({ g: 0.2, t: 0 })).y).toBe(0);
		expect(derive(vals({ g: 2.2, t: 0.5 })).y ** (1 / 2.2)).toBeCloseTo(0.5, 12);
		expect(0.5 ** (1 / 2.2)).toBeCloseTo(0.7297, 4);
	});
	test('presets are inside the field range and exact (1/2.2 is not rounded to the 0.1 step)', () => {
		for (const p of PRESETS) {
			expect(p.g).toBeGreaterThanOrEqual(SPEC.g.min);
			expect(p.g).toBeLessThanOrEqual(SPEC.g.max);
		}
		expect(PRESETS[1].g).toBe(1 / 2.2);
	});
});

describe('challenges read store values only (§7.7-§7.11)', () => {
	test('0.2-1', () => {
		expect(CHECKS.fourTiles(vals({ x: 2 }))).toBe(true);
		expect(CHECKS.fourTiles(vals({ x: 2.1 }))).toBe(false);
		expect(CHECKS.typedRoot({ ...vals(), x: Math.SQRT2, A: 2, V: 2 * Math.SQRT2, from: 1 })).toBe(true);
		expect(CHECKS.typedRoot({ ...vals(), x: 2, A: 4, V: 8, from: 1 })).toBe(false);
		expect(CHECKS.typedRoot(vals({ x: 2.5 }))).toBe(false);
		expect(CHECKS.notTyped(vals({ x: 2.5 }))).toBe(true);
		expect(CHECKS.notTyped({ ...vals({ x: 2.5 }), from: 1 })).toBe(false);
	});
	test('0.2-2', () => {
		expect(CHECKS.cube27(vals({ x: 3, n: 3 }))).toBe(true);
		expect(CHECKS.cube27(vals({ x: 3, n: 2 }))).toBe(false);
		expect(CHECKS.cubeSmaller(vals({ x: 0.5, n: 3 }))).toBe(true);
		expect(CHECKS.cubeSmaller(vals({ x: 0, n: 3 }))).toBe(false);
		expect(CHECKS.cubeSmaller(vals({ x: 1, n: 3 }))).toBe(false);
		expect(CHECKS.twoLayers(vals({ x: 2, n: 3 }))).toBe(true);
	});
	test('0.2-3', () => {
		expect(CHECKS.oneTile(vals({ k: 0 }))).toBe(true);
		expect(CHECKS.quarter(vals({ k: -2, base: 2 }))).toBe(true);
		expect(CHECKS.quarter(vals({ k: -2, base: 3 }))).toBe(false);
		expect(CHECKS.otherBase(vals({ k: 0, base: 2.5 }))).toBe(true);
		expect(CHECKS.otherBase(vals({ k: 0, base: 2 }))).toBe(false);
	});
	test('0.2-4', () => {
		expect(CHECKS.wholeEdge({ ...vals({ x: 2 }), d3: 1 })).toBe(true);
		expect(CHECKS.wholeEdge({ ...vals({ x: 3 }), d3: 1 })).toBe(false);
		expect(CHECKS.wholeEdge({ ...vals({ x: 2 }), d3: 0 })).toBe(false);
		expect(CHECKS.wholeEdge({ ...vals({ x: 0 }), d3: 1 })).toBe(false);
		expect(CHECKS.edgeTwo({ ...vals({ x: 2 }), d3: 1 })).toBe(true);
	});
	test('0.2-5', () => {
		expect(CHECKS.diagonal(vals({ g: 1 }))).toBe(true);
		expect(CHECKS.quarterOut(vals({ g: 2, t: 0.5 }))).toBe(true);
		expect(CHECKS.highlight(vals({ g: 32, t: 0.5 }))).toBe(true);
		expect(CHECKS.highlight(vals({ g: 8, t: 0.5 }))).toBe(false);
		// The initial state solves nothing.
		for (const check of Object.values(CHECKS)) expect(check(vals())).toBe(false);
	});
});

test('number formats: a root-written side keeps its precision in every derived amount', () => {
	expect([fmtX(3), fmtX(2.5), fmtX(Math.SQRT2), fmtX(Math.cbrt(2)), fmtX(0)]).toEqual(['3.0', '2.5', '1.4142', '1.2599', '0.0']);
	expect([fmtAmt(9), fmtAmt(15.625), fmtAmt(Math.cbrt(2) ** 2), fmtAmt(Math.SQRT2 ** 2)]).toEqual(['9', '15.625', '1.5874', '2']);
});

test('N-panel rows: unique ids per card, every read-only row evaluates', () => {
	for (const scene of [1, 2, 3, 4, 5] as const) {
		const rows = rowsFor(scene, 'en');
		expect(new Set(rows.map((r) => r.id)).size).toBe(rows.length);
		for (const r of rows) if (r.kind === 'ro') expect(Number.isFinite(r.value!(vals()) as number)).toBe(true);
	}
	expect(rowsFor(3, 'en').filter((r) => r.kind === 'ro').map((r) => r.id)).toEqual(RUNGS.map(rungId));
});
