// Lesson 3.1 math (src/viz/lessons/tensor-road.math.ts) against independent references: the tensor core's own
// strides/positions, brute-force index enumeration, lesson 1.7's embedding file and hand-checked NumPy facts.
import { describe, expect, test } from 'bun:test';
import raw from '../../src/data/embeddings-small.json';
import { arange, cStrides, sizeOf, transpose } from '../../src/core';
import { lessonStore } from '../../src/viz/store';
import {
	CHECKS, EXAMPLES, fmtShape, grow, INITIAL, indexOf, layout, LINKS, memView, model, roadDirs, roadShape, secondOfTranspose, selectPatch, SPEC,
} from '../../src/viz/lessons/tensor-road.math';
import { GLOVE8, ICON, luma, SENTENCE } from '../../src/viz/lessons/tensor-road.data';
import { roadRows } from '../../src/viz/lessons/tensor-road.rows';
import '../../src/i18n/ui';

const vals = (over: Record<string, number> = {}) => ({ ...INITIAL, ...over }) as Record<string, number>;

/** Every index tuple of a shape in row-major order (independent of the layout's own decoding). */
function* indices(shape: readonly number[]): Generator<number[]> {
	if (shape.length === 0) {
		yield [];
		return;
	}
	for (let i = 0; i < shape[0]; i++) for (const rest of indices(shape.slice(1))) yield [i, ...rest];
}

describe('nested layout', () => {
	const shapes = [[], [4], [3, 4], [2, 3, 4], [2, 2, 3, 4], [3, 2, 2, 3, 4], [2, 3, 2, 2, 3, 4], [4, 4, 4, 4, 4, 4], [1, 1, 1], [4, 1, 3, 1]];
	for (const shape of shapes) {
		test(`every index has exactly one cell: ${fmtShape(shape)}`, () => {
			const L = layout(shape, roadDirs(shape.length));
			expect(L.pos.length).toBe(3 * sizeOf(shape));
			const seen = new Set<string>();
			for (let i = 0; i < sizeOf(shape); i++) seen.add([0, 1, 2].map((w) => L.pos[3 * i + w].toFixed(3)).join());
			expect(seen.size).toBe(sizeOf(shape));
		});
	}

	test('cell centres follow the nesting rule (cells pitch 1, blocks = block size + one empty cell)', () => {
		const shape = [2, 2, 3, 4];
		const L = layout(shape, roadDirs(4));
		const st = cStrides(shape);
		for (const [b, z, y, x] of indices(shape)) {
			const i = b * st[0] + z * st[1] + y * st[2] + x * st[3];
			// Level 3 (b) repeats the 4-wide row along x with a pitch of 4 + 1.
			expect([L.pos[3 * i], L.pos[3 * i + 1], L.pos[3 * i + 2]]).toEqual([x + 5 * b, 0 - y, 0 - z]);
		}
		expect(L.max).toEqual([3 + 5, 2, 1]);
	});

	test('the index-0 copy of a new axis is exactly the smaller tensor (extrusion starts there)', () => {
		for (let r = 1; r <= 6; r++) {
			const sizes = [4, 3, 2, 2, 3, 2];
			const big = layout(roadShape(r, sizes), roadDirs(r));
			const small = layout(roadShape(r - 1, sizes), roadDirs(r - 1));
			expect(Array.from(big.pos.subarray(0, small.pos.length))).toEqual(Array.from(small.pos));
		}
	});

	test('grow: u = 0 stacks every copy on index 0 (copies hidden), u = 1 is the layout', () => {
		const shape = [3, 2, 4];
		const L = layout(shape, roadDirs(3));
		const out = [0, 0, 0];
		for (let i = 0; i < 24; i++) {
			const base = i % 8;
			expect(grow(L, shape, i, 0, out)).toBe(i < 8 ? 1 : 0);
			expect(out).toEqual([L.pos[3 * base], L.pos[3 * base + 1], L.pos[3 * base + 2]]);
			expect(grow(L, shape, i, 1, out)).toBe(1);
			expect(out).toEqual([L.pos[3 * i], L.pos[3 * i + 1], L.pos[3 * i + 2]]);
		}
	});
});

describe('model: shape, strides, address', () => {
	test('defaults: rank 3, shape (2, 3, 4), strides (12, 4, 1), 24 elements', () => {
		const m = model(vals());
		expect(m.shape).toEqual([2, 3, 4]);
		expect(m.strides).toEqual([12, 4, 1]);
		expect(m.size).toBe(24);
		expect(m.offset).toBe(0);
	});

	test('address = Σ index · stride equals the core tensor position and the arange value', () => {
		for (let r = 0; r <= 6; r++) {
			const shape = roadShape(r, [4, 3, 2, 2, 3, 2]);
			const t = arange(sizeOf(shape), shape);
			for (const idx of indices(shape)) {
				const patch: Record<string, number> = {};
				idx.forEach((j, a) => (patch['k' + (r - 1 - a)] = j));
				const m = model(vals({ rank: r, ...patch }));
				expect(m.idx).toEqual(idx);
				expect(m.offset).toBe(t.pos(idx));
				expect(m.value).toBe(t.get(idx));
				expect(indexOf(m, m.offset)).toEqual(idx);
				const back = selectPatch(m, m.offset);
				expect(model(vals({ rank: r, ...back })).idx).toEqual(idx);
			}
		}
	});

	test('rank 0 is a scalar: shape (), one element, empty product', () => {
		const m = model(vals({ rank: 0 }));
		expect(m.shape).toEqual([]);
		expect(m.size).toBe(1);
		expect(fmtShape(m.shape)).toBe('()');
		expect(fmtShape([4])).toBe('(4,)');
		expect(fmtShape([2, 3, 4])).toBe('(2, 3, 4)');
	});
});

describe('link group', () => {
	test('shrinking a size clamps the selected index into the tensor', () => {
		const s = lessonStore('tensor-road-test-clamp', INITIAL, LINKS);
		s.getState().setMany({ k0: 3, k1: 2, k2: 1 });
		expect(model(s.getState().vals).idx).toEqual([1, 2, 3]);
		s.getState().set('g0', 2);
		expect(model(s.getState().vals).idx).toEqual([1, 2, 1]);
		s.getState().set('rank', 9);
		expect(s.getState().vals.rank).toBe(SPEC.rank.max);
	});

	test('data mode switches to the example of the rank and clamps the index to its shape', () => {
		const s = lessonStore('tensor-road-test-data', INITIAL, LINKS);
		s.getState().setMany({ rank: 2, k0: 3, k1: 2 });
		s.getState().set('data', 1);
		const m = model(s.getState().vals);
		expect(m.ex?.id).toBe('gray');
		expect(m.idx).toEqual([2, 3]);
		s.getState().set('pick', 1);
		expect(model(s.getState().vals).ex?.id).toBe('verts');
		expect(model(s.getState().vals).idx).toEqual([2, 2]);
		s.getState().set('rank', 6);
		const six = model(s.getState().vals);
		expect(six.ex).toBeNull();
		expect(six.noExample).toBe(true);
	});
});

describe('real data', () => {
	test('every example has its rank, its size and finite values', () => {
		EXAMPLES.forEach((list, r) => list.forEach((e) => {
			expect(e.shape.length).toBe(r);
			expect(e.data.length).toBe(sizeOf(e.shape));
			expect(e.dirs.length).toBe(r);
			expect(e.names.length).toBe(r);
			expect(e.data.every(Number.isFinite)).toBe(true);
			if (e.dtype === 'uint8') expect(e.data.every((x) => Number.isInteger(x) && x >= 0 && x <= 255)).toBe(true);
			// Each example's layout gives one cell per index too.
			const L = layout(e.shape, e.dirs);
			const seen = new Set<string>();
			for (let i = 0; i < e.data.length; i++) seen.add([0, 1, 2].map((w) => L.pos[3 * i + w].toFixed(3)).join());
			expect(seen.size).toBe(e.data.length);
		}));
	});

	test('the images agree cell by cell: pixel = sun of the icon, gray = luma, batch/video derived from the icon', () => {
		const ex = (id: string) => EXAMPLES.flat().find((e) => e.id === id)!;
		expect(Array.from(ex('pixel').data)).toEqual([255, 217, 51]);
		const rgb = ex('rgb'), gray = ex('gray'), batch = ex('batch'), video = ex('video');
		for (let h = 0; h < 8; h++) for (let w = 0; w < 8; w++) {
			const p = [0, 1, 2].map((c) => rgb.data[h * 24 + w * 3 + c]);
			expect(p).toEqual([...ICON[h][w]]);
			expect(gray.data[h * 8 + w]).toBe(luma(p));
			// Batch element 0 = the icon, 1 = mirrored, 2 = dusk (halved).
			for (let c = 0; c < 3; c++) {
				expect(batch.data[0 * 192 + h * 24 + w * 3 + c]).toBe(p[c]);
				expect(batch.data[1 * 192 + h * 24 + (7 - w) * 3 + c]).toBe(p[c]);
				expect(batch.data[2 * 192 + h * 24 + w * 3 + c]).toBe(Math.floor(p[c] / 2));
				// Clip 0, frame t: rolled t pixels left, like np.roll(img, -t, axis=1).
				for (let t = 0; t < 3; t++) expect(video.data[(0 * 3 + t) * 192 + h * 24 + w * 3 + c]).toBe(ICON[h][(w + t) % 8][c]);
			}
		}
		// Row 2, column 5 is sky: its green channel is 156 (challenge 3's answer).
		expect(rgb.data[2 * 24 + 5 * 3 + 1]).toBe(156);
	});

	test('the GloVe slices are the first 8 numbers of lesson 1.7\'s vectors', () => {
		const words = (raw as { words: { word: string; v: number[] }[] }).words;
		for (const w of SENTENCE) expect(GLOVE8[w]).toEqual(words.find((x) => x.word === w)!.v.slice(0, 8));
		const text = EXAMPLES[3].find((e) => e.id === 'text')!;
		expect(text.shape).toEqual([1, 6, 8]);
		expect(text.of).toEqual([1, 6, 300]);
		// "the" is tokens 0 and 4: the same row twice.
		expect(Array.from(text.data.subarray(0, 8))).toEqual(Array.from(text.data.subarray(32, 40)));
	});
});

describe('memory view', () => {
	test('the transposed view reverses shape and strides, reads the same buffer, and is not contiguous', () => {
		const m = model(vals());
		const plain = memView(m, false), tr = memView(m, true);
		expect(plain.contiguous).toBe(true);
		expect(Array.from(plain.walk)).toEqual(Array.from({ length: 24 }, (_, i) => i));
		expect(tr.shape).toEqual([4, 3, 2]);
		expect(tr.strides).toEqual([1, 4, 12]);
		expect(tr.contiguous).toBe(false);
		// NumPy: x.T of arange(24).reshape(2, 3, 4) reads 0, 12, 4, 16, 8, 20, 1, 13, ...
		expect(Array.from(tr.walk.subarray(0, 8))).toEqual([0, 12, 4, 16, 8, 20, 1, 13]);
		const core = transpose(arange(24, [2, 3, 4]));
		expect(Array.from(tr.walk)).toEqual(Array.from({ length: 24 }, (_, k) => core.pos([...indexOf({ shape: [4, 3, 2], strides: cStrides([4, 3, 2]) }, k)])));
		tr.walk.forEach((p, k) => expect(tr.step[p]).toBe(k));
		expect(secondOfTranspose(m)).toBe(12);
	});

	test('a vector and a scalar are contiguous both ways (transposing one axis changes nothing)', () => {
		expect(memView(model(vals({ rank: 1 })), true).contiguous).toBe(true);
		expect(memView(model(vals({ rank: 0 })), true).contiguous).toBe(true);
	});
});

describe('challenges', () => {
	test('none is solved by the defaults', () => {
		for (const check of Object.values(CHECKS)) expect(check(vals())).toBe(false);
	});
	test('each is solved by its intended state', () => {
		expect(CHECKS.rank4of24(vals({ rank: 4, g0: 3, g1: 2, g2: 2, g3: 2 }))).toBe(true);
		expect(CHECKS.rank4of24(vals({ rank: 4, g0: 3, g1: 2, g2: 2, g3: 2, data: 1 }))).toBe(false);
		expect(CHECKS.offset17(vals({ k0: 1, k1: 1, k2: 1 }))).toBe(true);
		expect(CHECKS.green25(vals({ rank: 3, data: 1, pick: 0, k2: 2, k1: 5, k0: 1 }))).toBe(true);
		expect(CHECKS.green25(vals({ rank: 3, data: 1, pick: 0, k2: 2, k1: 5, k0: 2 }))).toBe(false);
		expect(CHECKS.transposeSecond(vals({ tr: 1, k2: 1 }))).toBe(true);
		expect(CHECKS.transposeSecond(vals({ tr: 0, k2: 1 }))).toBe(false);
	});
});

test('rows: size row k is shown from rank k + 1 on, only for the abstract road', () => {
	const rows = roadRows('en');
	const g2 = rows.find((r) => r.id === 'g2')!;
	expect(g2.visible!(vals({ rank: 2 }), {})).toBe(false);
	expect(g2.visible!(vals({ rank: 3 }), {})).toBe(true);
	expect(g2.visible!(vals({ rank: 3, data: 1 }), {})).toBe(false);
});
