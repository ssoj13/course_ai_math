// Lesson 3.4 math and choreography (src/viz/lessons/broadcasting.*) against independent references: hand-written
// broadcasting loops (no tensor core), NumPy's printed results for the page's examples, and the frame the renderer draws.
import { describe, expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { lessonStore, type LessonState } from '../../src/viz/store';
import { Frame } from '../../src/viz/dotunit/choreo';
import {
	align, CHECKS, CROP, INITIAL, LINKS, model, PRESETS, shapesPatch, SPEC, tintModel, values,
} from '../../src/viz/lessons/broadcasting.math';
import { CAP, geo, linked, phases, REPEAT, writeFrame, writeIds } from '../../src/viz/lessons/broadcasting.scene';
import { ruleRows, tintRows } from '../../src/viz/lessons/broadcasting.rows';
import { flatDirs, fmtNum, place, unravel } from '../../src/viz/lessons/tensor-ops';

type Vals = LessonState['vals'];
const vals = (patch: Record<string, number | number[]> = {}): Vals => ({ ...INITIAL, ...patch });
const shapes = (a: number[], b: number[]) => vals(shapesPatch(a, b));

/** Broadcast by hand: pad with leading 1s, then each output index reads index 0 along size-1 axes. */
function reference(sa: number[], sb: number[], f: (x: number, y: number) => number): { out: number[]; c: number[] } | null {
	const n = Math.max(sa.length, sb.length);
	const pa = [...Array(n - sa.length).fill(1), ...sa], pb = [...Array(n - sb.length).fill(1), ...sb];
	const out: number[] = [];
	for (let d = 0; d < n; d++) {
		if (pa[d] !== pb[d] && pa[d] !== 1 && pb[d] !== 1) return null;
		out.push(Math.max(pa[d], pb[d]));
	}
	const size = out.reduce((p, x) => p * x, 1);
	const flatOf = (s: number[], idx: number[]) => idx.reduce((k, i, d) => k * s[d] + (s[d] === 1 ? 0 : i), 0);
	const c: number[] = [];
	for (let k = 0; k < size; k++) {
		const idx = unravel(out, k);
		const a = flatOf(pa, idx);
		const b = 10 * (flatOf(pb, idx) + 1);
		c.push(f(a, b));
	}
	return { out, c };
}

describe('the rule, column by column', () => {
	test('right alignment and the verdict of each axis', () => {
		expect(align([3, 4], [4]).map((c) => c.kind)).toEqual(['newB', 'same']);
		expect(align([3, 4], [3, 1]).map((c) => c.kind)).toEqual(['same', 'stretchB']);
		expect(align([3, 1], [1, 4]).map((c) => c.kind)).toEqual(['stretchB', 'stretchA']);
		expect(align([3, 4], [3]).map((c) => [c.axis, c.kind])).toEqual([[-2, 'newB'], [-1, 'bad']]);
		expect(align([4], [2, 3, 4]).map((c) => c.kind)).toEqual(['newA', 'newA', 'same']);
		expect(align([3, 4], []).map((c) => c.out)).toEqual([3, 4]);
	});
});

describe('model: every preset against a hand-written loop', () => {
	const ops = [(x: number, y: number) => x + y, (x: number, y: number) => x - y, (x: number, y: number) => x * y];
	for (const [id, p] of Object.entries(PRESETS))
		for (let op = 0; op < 3; op++)
			test(`${id}, op ${op}`, () => {
				const m = model(vals({ ...shapesPatch(p.a, p.b), op }));
				const ref = reference([...p.a], [...p.b], ops[op]);
				if (!ref) {
					expect(m.ok).toBe(false);
					expect(m.C).toBeNull();
					return;
				}
				expect(m.ok).toBe(true);
				expect(m.out).toEqual(ref.out);
				expect(values(m.C!)).toEqual(ref.c);
			});

	test('the default: (3, 4) + (4,) as NumPy prints it', () => {
		const m = model(vals());
		expect(values(m.C!)).toEqual([10, 21, 32, 43, 14, 25, 36, 47, 18, 29, 40, 51]);
		expect([...m.opB!.view.strides]).toEqual([0, 1]);
		expect(m.opB!.repeats).toBe(8);
		expect(m.opA!.repeats).toBe(0);
		expect(m.opB!.view.writable).toBe(false);
		// The real elements are row 0; every other row repeats the element above its column.
		expect(Array.from(m.opB!.real)).toEqual([1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0]);
		expect(Array.from(m.opB!.from)).toEqual([0, 1, 2, 3, 0, 1, 2, 3, 0, 1, 2, 3]);
	});

	test('a scalar stretches along every axis with stride 0', () => {
		const m = model(shapes([3, 4], []));
		expect([...m.opB!.view.strides]).toEqual([0, 0]);
		expect(m.opB!.repeats).toBe(11);
	});

	test('a mismatch is refused by the core with its own message, and the bad axis is named', () => {
		const m = model(shapes([3, 4], [3]));
		expect(m.ok).toBe(false);
		expect(m.bad).toEqual([-1]);
		expect(m.coreMsg).toContain('do not broadcast');
		expect(m.coreMsg).toContain('(3, 4)');
	});

	test('the tint: picture (4, 4, 3) times (3,), every pixel times the same three numbers', () => {
		const m = tintModel(vals());
		expect(m.out).toEqual([4, 4, 3]);
		expect([...m.opB!.view.strides]).toEqual([0, 0, 1]);
		expect(m.opB!.repeats).toBe(45);
		const c = values(m.C!);
		// NumPy: np.array([92, 156, 240]) * [1.0, 0.8, 0.5] -> [ 92. 124.8 120. ]
		expect(c.slice(0, 3).map((x) => fmtNum(x, 6))).toEqual(['92', '124.8', '120']);
		CROP.flat().forEach((p, k) => p.forEach((x, ch) => expect(c[3 * k + ch]).toBeCloseTo(x * (INITIAL.tint as number[])[ch], 12)));
	});
});

describe('link group', () => {
	test('ranks, sizes and the operator are rounded and clamped in one write', () => {
		const s = lessonStore('broadcasting-test-clamp', INITIAL, LINKS);
		s.getState().setMany({ ar: 7, a0: 9, b1: 0.4, op: 2.6 });
		const v = s.getState().vals;
		expect([v.ar, v.a0, v.b1, v.op]).toEqual([SPEC.ar.max, SPEC.size.max, SPEC.size.min, SPEC.op.max]);
	});
});

describe('choreography', () => {
	test('the largest shapes fit the frame capacity', () => {
		const big = model(shapes([4, 4, 4], [4, 4, 4]));
		expect(geo(big, 'num').n).toBeLessThanOrEqual(CAP);
		expect(geo(tintModel(vals()), 'image').n).toBeLessThanOrEqual(CAP);
	});

	test('step 0 shows only stored numbers; the stretch slides repeats out of their element; then C fills', () => {
		const m = model(vals());
		const g = geo(m, 'num');
		const f = new Frame(CAP);
		writeIds(g, f);
		const none = new Set<string>();
		const posA = place([3, 4], flatDirs(2), [0, 0]).pos;
		writeFrame(m, g, 'num', 0, 1, none, f);
		const b = g.base[1], c = g.base[2];
		expect(f.alpha[b + 0]).toBe(1);
		expect(f.alpha[b + 5]).toBe(0);
		// Repeat b5 (row 1, column 1) waits on top of its element b1 (row 0, column 1).
		expect(f.c[3 * (b + 5)]).toBeCloseTo(f.c[3 * (b + 1)], 6);
		expect(f.c[3 * (b + 5) + 1]).toBeCloseTo(f.c[3 * (b + 1) + 1], 6);
		expect(f.alpha[c]).toBe(0);
		writeFrame(m, g, 'num', 1, 1, none, f);
		expect(f.alpha[b + 5]).toBeCloseTo(REPEAT, 6);
		expect(f.c[3 * (b + 5) + 1]).toBeCloseTo(posA[3 * 5 + 1], 6);
		writeFrame(m, g, 'num', 2, 1, none, f);
		expect(f.alpha[c + 11]).toBe(1);
		expect(f.id[c + 11]).toBe('c11');
		expect(phases(2, 0.5)).toEqual({ stretch: 1, fill: 0.5 });
	});

	test('hovering a cell of C lights the two cells it combines and the stored elements behind them', () => {
		const m = model(vals());
		expect([...linked(m, new Set(['c6']))].sort()).toEqual(['a6', 'b2', 'b6', 'c6']);
		expect([...linked(m, new Set(['b9']))].sort()).toEqual(['a9', 'b1', 'b9', 'c9']);
		expect(linked(model(shapes([3, 4], [3])), new Set(['a1'])).size).toBe(0);
	});

	test('negative results are hatched', () => {
		const m = model(vals({ op: 1 }));
		const g = geo(m, 'num');
		const f = new Frame(CAP);
		writeIds(g, f);
		writeFrame(m, g, 'num', 2, 1, new Set(), f);
		expect(f.hatch[g.base[2]]).toBe(1);
		expect(m.C!.data[0]).toBe(-10);
	});
});

describe('challenges and rows', () => {
	test('no challenge holds on the initial values; each holds on its target', () => {
		for (const check of Object.values(CHECKS)) expect(check(vals())).toBe(false);
		expect(CHECKS.column(shapes([3, 4], [3, 1]))).toBe(true);
		expect(CHECKS.error(shapes([3, 4], [3]))).toBe(true);
		expect(CHECKS.outer(shapes([3, 1], [1, 4]))).toBe(true);
		expect(CHECKS.outer(shapes([3, 1], [4]))).toBe(true);
		expect(CHECKS.outer(shapes([3, 4], [4]))).toBe(false);
		expect(CHECKS.noRed(vals({ tint: [0, 1, 1] }))).toBe(true);
	});

	test('every editable row writes a key of the store', () => {
		for (const r of [...ruleRows('en'), ...tintRows('ru')]) if (r.key) expect(Object.hasOwn(INITIAL, r.key)).toBe(true);
	});
});
