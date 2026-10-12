// Lessons 3.2 (data-as-tensors) and 3.3 (tensor-memory) and their shared tensor-cells helpers, against independent
// references: brute-force index enumeration, hand-checked NumPy facts (strides printed by NumPy 2.4.6 for the same
// arrays), lesson 1.7's embedding file, closed-form sine samples and mesh geometry.
import { describe, expect, test } from 'bun:test';
import raw from '../../src/data/embeddings-small.json';
import { cStrides, isContiguous, sizeOf } from '../../src/core';
import { Frame } from '../../src/viz/dotunit/choreo';
import { lessonStore } from '../../src/viz/store';
import { chipLines } from '../../src/viz/scene/chips';
import { flatDirs, layout, maxAbsOf, paintCell, stripWindow, unravel } from '../../src/viz/lessons/tensor-cells';
import { GLOVE8, GLOVE_ROW, ICON, SENTENCE, SENTENCE2 } from '../../src/viz/lessons/tensor-road.data';
import * as D from '../../src/viz/lessons/data-as-tensors.math';
import * as M from '../../src/viz/lessons/tensor-memory.math';
import { audioRows, imageRows, meshRows, textRows } from '../../src/viz/lessons/data-as-tensors.rows';
import { cacheRows, viewRows } from '../../src/viz/lessons/tensor-memory.rows';
import '../../src/i18n/ui';

/** Every index tuple of a shape in row-major order. */
function* indices(shape: readonly number[]): Generator<number[]> {
	if (shape.length === 0) {
		yield [];
		return;
	}
	for (let i = 0; i < shape[0]; i++) for (const rest of indices(shape.slice(1))) yield [i, ...rest];
}

describe('tensor-cells', () => {
	test('unravel is the row-major enumeration', () => {
		for (const shape of [[24], [4, 6], [2, 3, 4], [2, 2, 2, 3], [1, 5, 1]]) {
			let k = 0;
			for (const idx of indices(shape)) expect(unravel(k++, shape)).toEqual(idx);
		}
		expect(() => unravel(24, [2, 3, 4])).toThrow();
		expect(() => unravel(-1, [4])).toThrow();
	});

	test('flat layouts keep every cell distinct and facing the viewer (z = 0)', () => {
		for (const shape of [[24], [4, 6], [2, 3, 4], [2, 2, 2, 3], []]) {
			const L = layout(shape, flatDirs(shape.length));
			const seen = new Set<string>();
			for (let i = 0; i < sizeOf(shape); i++) {
				expect(L.pos[3 * i + 2]).toBe(0);
				seen.add(L.pos[3 * i] + ',' + L.pos[3 * i + 1]);
			}
			expect(seen.size).toBe(sizeOf(shape));
		}
		expect(() => flatDirs(5)).toThrow();
	});

	test('stripWindow keeps the centre inside a window of at most max cells', () => {
		expect(stripWindow(24, 5, 48)).toEqual({ from: 0, to: 24 });
		expect(stripWindow(192, 0, 48)).toEqual({ from: 0, to: 48 });
		expect(stripWindow(192, 100, 48)).toEqual({ from: 76, to: 124 });
		expect(stripWindow(192, 191, 48)).toEqual({ from: 144, to: 192 });
	});

	test('paintCell: Hinton squares grow with |v|, negatives are hatched; bytes set brightness', () => {
		const f = new Frame(3);
		paintCell(f, 0, [0, 0, 0], 'signed', 0.5, 1, { maxAbs: 1 });
		paintCell(f, 1, [1, 0, 0], 'signed', -1, 1, { maxAbs: 1 });
		paintCell(f, 2, [2, 0, 0], 'rgb', 255, 1, { role: 'axisY' });
		expect(f.s[0]).toBeCloseTo(0.5, 6);
		expect(f.s[3]).toBeCloseTo(0.85, 6);
		expect([f.hatch[0], f.hatch[1]]).toEqual([0, 1]);
		expect(f.alpha[2]).toBeCloseTo(1, 6);
		expect(maxAbsOf([0.2, -0.7, 0.1])).toBe(0.7);
	});
});

describe('3.2 image', () => {
	test('both layouts hold the icon bytes; addresses follow the NumPy strides (24, 3, 1) and (64, 8, 1)', () => {
		expect(cStrides(D.IMG_HWC.shape)).toEqual([24, 3, 1]);
		expect(D.IMG_CHW.strides).toEqual([64, 8, 1]);
		expect(isContiguous(D.IMG_CHW)).toBe(true);
		for (const [h, w, c] of indices([8, 8, 3])) {
			expect(D.IMG_HWC.data[24 * h + 3 * w + c]).toBe(ICON[h][w][c]);
			expect(D.IMG_CHW.data[64 * c + 8 * h + w]).toBe(ICON[h][w][c]);
			expect(D.imgAddr(false, h, w, c)).toBe(24 * h + 3 * w + c);
			expect(D.imgAddr(true, h, w, c)).toBe(64 * c + 8 * h + w);
			expect(D.imgAt(true, D.imgAddr(true, h, w, c))).toEqual([h, w, c]);
			expect(D.imgAt(false, D.imgAddr(false, h, w, c))).toEqual([h, w, c]);
			expect(D.sameBytes(h, w, c)).toBe(true);
		}
	});

	test('lesson numbers: the sun pixel, the green byte at 19 / 70, address 64 channels first', () => {
		const m = D.imageModel({ ...D.INITIAL, h: 0, w: 6, c: 1 });
		expect([m.value, m.addr, m.rgb]).toEqual([217, 19, [255, 217, 51]]);
		expect(D.imageModel({ ...D.INITIAL, h: 0, w: 6, c: 1, chw: 1 }).addr).toBe(70);
		expect(D.CHECKS.blueSun({ ...D.INITIAL, h: 0, w: 7, c: 2 })).toBe(true);
		expect(D.imageModel({ ...D.INITIAL, h: 0, w: 7, c: 2 }).value).toBe(51);
		expect(D.CHECKS.chw64({ ...D.INITIAL, chw: 1, h: 0, w: 0, c: 1 })).toBe(true);
		expect(D.CHECKS.chw64({ ...D.INITIAL, chw: 0, h: 0, w: 0, c: 1 })).toBe(false);
		expect(D.imageModel({ ...D.INITIAL, chw: 1, h: 0, w: 0, c: 1 }).value).toBe(156);
		// By hand 3: address 100 channels last is x[4, 1, 1].
		expect(D.imgAt(false, 100)).toEqual([4, 1, 1]);
	});
});

describe('3.2 text', () => {
	const words = raw.words as { word: string; row: number; v: number[] }[];
	test('GloVe rows and ids are copies of lesson 1.7 data', () => {
		for (const w of Object.keys(GLOVE8) as (keyof typeof GLOVE8)[]) {
			const src = words.find((x) => x.word === w)!;
			expect(src.v.slice(0, 8)).toEqual([...GLOVE8[w]]);
			expect(src.row).toBe(GLOVE_ROW[w]);
		}
	});

	test('ids (2, 6) and the gather X = E[ids] (2, 6, 8)', () => {
		expect(D.IDS.shape).toEqual([2, 6]);
		expect([...D.IDS.data]).toEqual([1, 5451, 3224, 14, 1, 14949, 1, 301, 3224, 14, 1, 1471]);
		expect(D.EMBEDDED.shape).toEqual([2, 6, 8]);
		[SENTENCE, SENTENCE2].forEach((s, b) => s.forEach((w, t) => {
			for (let d = 0; d < 8; d++) expect(D.EMBEDDED.get([b, t, d])).toBe(Math.fround(GLOVE8[w][d]));
		}));
		expect(D.VOCAB.map((w) => GLOVE_ROW[w])).toEqual([1, 14, 301, 1471, 3224, 5451, 14949]);
		expect(() => D.tableRow(2)).toThrow();
	});

	test('the fourth read of "the" is lookup 11', () => {
		const the = Array.from({ length: D.LOOKUPS }, (_, k) => D.IDS.get(D.lookupAt(k))).flatMap((id, k) => (id === 1 ? [k + 1] : []));
		expect(the).toEqual([1, 5, 7, 11]);
		expect(D.CHECKS.fourthThe({ ...D.INITIAL, look: 11 })).toBe(true);
		expect(D.CHECKS.fourthThe({ ...D.INITIAL, look: 12 })).toBe(false);
	});
});

describe('3.2 audio', () => {
	test('samples are 0.8 sin(2π 440 n / 8000) rounded to 3 decimals, stored as float32', () => {
		for (let n = 0; n < D.SAMPLES; n++) expect(D.SIGNAL.data[n]).toBe(Math.fround(Math.round(800 * Math.sin((2 * Math.PI * 440 * n) / 8000)) / 1000));
		expect(D.SIGNAL.data[1]).toBeCloseTo(0.271, 6);
	});

	test('every frame length reshapes the same buffer into whole frames', () => {
		for (const fl of D.FRAMES) {
			expect(48 % fl).toBe(0);
			expect(D.framesShare(fl)).toBe(true);
			const m = D.audioModel({ ...D.INITIAL, fl, s: 17 });
			expect(m.t.shape).toEqual([48 / fl, fl]);
			expect(m.idx).toEqual([Math.floor(17 / fl), 17 % fl]);
			expect(m.value).toBe(D.SIGNAL.data[17]);
		}
		expect(D.snapFrame(5)).toBe(4);
		expect(D.snapFrame(10)).toBe(8);
		expect(D.snapFrame(100)).toBe(24);
		expect(D.CHECKS.fourFrames({ ...D.INITIAL, fl: 12 })).toBe(true);
	});
});

describe('3.2 mesh', () => {
	const v = (i: number) => [0, 1, 2].map((k) => D.VERTS.get([i, k]));
	test('every triangle faces outward (counter-clockwise seen from outside)', () => {
		const centre = [0, 0.3, 0];
		for (let f = 0; f < D.N_FACES; f++) {
			const [a, b, c] = D.corners(f);
			const u = b.map((x, k) => x - a[k]), w = c.map((x, k) => x - a[k]);
			const n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]];
			const out = [0, 1, 2].map((k) => (a[k] + b[k] + c[k]) / 3 - centre[k]);
			expect(n[0] * out[0] + n[1] * out[1] + n[2] * out[2]).toBeGreaterThan(0);
			expect(D.corners(f)).toEqual([0, 1, 2].map((k) => v(D.FACES.get([f, k]))));
		}
	});

	test('Euler: 5 vertices - 9 edges + 6 faces = 2; base faces avoid the apex', () => {
		expect(D.edges().length).toBe(9);
		expect([0, 1, 2, 3, 4, 5].filter((f) => D.CHECKS.baseFace({ ...D.INITIAL, face: f }))).toEqual([4, 5]);
	});
});

describe('3.2 shape reader and store', () => {
	test('elements and bytes of the presets', () => {
		const facts = D.PRESETS.map((p) => [p.id, D.presetFacts(p).n, D.presetFacts(p).bytes]);
		expect(facts).toEqual([
			['imgLast', 4816896, 4816896], ['imgFirst', 4816896, 19267584], ['ids', 16384, 131072],
			['hidden', 67108864, 134217728], ['audio', 16000, 64000], ['verts', 30000, 120000],
		]);
		for (const p of D.PRESETS) expect(p.axes.length).toBe(p.shape.length);
		expect(D.fmtBytes(134217728)).toEqual({ v: '128.0', unit: 'MiB' });
		expect(D.fmtBytes(576)).toEqual({ v: '576', unit: 'B' });
		expect(D.fmtBig(4816896)).toBe('4 816 896');
	});

	test('link groups round and clamp every key', () => {
		const s = lessonStore('test-data-as-tensors', D.INITIAL, D.LINKS).getState();
		s.setMany({ h: 9.4, w: -3, c: 1.6, peel: 2 });
		s.setMany({ look: 40, fl: 7, s: 99, face: 10, preset: -1 });
		const v = lessonStore('test-data-as-tensors').getState().vals;
		expect([v.h, v.w, v.c, v.peel, v.look, v.fl, v.s, v.face, v.preset]).toEqual([7, 0, 2, 1, 12, 6, 47, 5, 0]);
	});

	test('rows keep their ids in both languages and fit the chip model', () => {
		for (const f of [imageRows, textRows, audioRows, meshRows, viewRows, cacheRows]) {
			expect(f('ru').map((r) => r.id)).toEqual(f('en').map((r) => r.id));
			expect(chipLines(f('en'))).toBeGreaterThan(0);
		}
	});
});

describe('3.3 view chain', () => {
	const at = (over: Record<string, number>) => M.chain({ ...M.INITIAL, cont: 0, re: 0, ...over });
	const id = (o: string) => M.OPS.findIndex((x) => x.id === o);
	const SHP234 = M.SHAPES.findIndex((s) => s.join() === '2,3,4');

	test('views of x = arange(24).reshape(2, 3, 4): NumPy strides / 8, offsets, shared buffer', () => {
		const want: Record<string, [number[], number[], number]> = {
			none: [[2, 3, 4], [12, 4, 1], 0],
			T: [[4, 3, 2], [1, 4, 12], 0],
			step: [[2, 3, 2], [12, 4, 2], 0],
			rev: [[2, 3, 4], [-12, 4, 1], 12],
			idx: [[3, 4], [4, 1], 12],
			crop: [[2, 3, 2], [12, 4, 1], 1],
		};
		for (const [o, [shape, strides, off]] of Object.entries(want)) {
			const c = at({ shp: SHP234, op: id(o) });
			expect([c.res.shape, c.res.strides, c.res.offset]).toEqual([shape, strides, off]);
			expect(c.res.data).toBe(M.DATA);
		}
		expect(M.walkOf(at({ shp: SHP234, op: id('T') }).res).pos.slice(0, 8)).toEqual(Int32Array.from([0, 12, 4, 16, 8, 20, 1, 13]));
	});

	test('column-major x has strides (1, 2, 6); its transpose is contiguous', () => {
		const c = at({ shp: SHP234, ord: 1, op: id('T') });
		expect(c.x.strides).toEqual([1, 2, 6]);
		expect(c.x.pos([1, 2, 3])).toBe(23);
		expect(isContiguous(c.view)).toBe(true);
		expect(M.CHECKS.fThenT({ ...M.INITIAL, shp: SHP234, ord: 1, op: id('T') })).toBe(true);
	});

	test('reshape: refused for size or contiguity, accepted after the one copy; a contiguous view is not copied', () => {
		const re = (s: string) => M.RESHAPES.findIndex((r) => r?.join() === s);
		expect(at({ shp: SHP234, op: id('T'), re: re('2,-1') }).refused).toBe('contig');
		expect(at({ shp: SHP234, op: id('none'), re: re('5,-1') }).refused).toBe('size');
		expect(at({ shp: SHP234, op: id('T'), re: re('5,-1') }).refused).toBe('size');
		const ok = at({ shp: SHP234, op: id('T'), cont: 1, re: re('2,-1') });
		expect([ok.copied, ok.refused, ok.res.shape]).toEqual([true, null, [2, 12]]);
		expect(Array.from(ok.res.data.slice(0, 6))).toEqual([0, 12, 4, 16, 8, 20]);
		expect(ok.res.data).not.toBe(M.DATA);
		const same = at({ shp: SHP234, op: id('none'), cont: 1 });
		expect([same.copied, same.res.data === M.DATA]).toEqual([false, true]);
		expect(M.CHECKS.copyThenView({ ...M.INITIAL, shp: SHP234, op: id('T'), cont: 1, re: re('2,-1') })).toBe(true);
		expect(M.CHECKS.copyThenView({ ...M.INITIAL, shp: SHP234, op: id('T'), cont: 0, re: re('2,-1') })).toBe(false);
	});

	test('every shape x op x order: the walk reads exactly the elements, addresses = offset + Σ i·s', () => {
		for (let shp = 0; shp < M.SHAPES.length; shp++) for (let op = 0; op < M.OPS.length; op++) for (const ord of [0, 1]) {
			const c = at({ shp, op, ord });
			const w = M.walkOf(c.res);
			let k = 0;
			for (const idx of indices(c.res.shape)) {
				const addr = c.res.offset + idx.reduce((s, i, a) => s + i * c.res.strides[a], 0);
				expect(w.pos[k]).toBe(addr);
				expect(M.selected(c.res, k).addr).toBe(addr);
				expect(w.step[addr]).toBe(k);
				k++;
			}
		}
	});

	test('negative stride challenge: x[::-1], the cell reading address 0', () => {
		const v = { ...M.INITIAL, shp: SHP234, op: id('rev'), cont: 0, re: 0, sel: 12 };
		expect(M.selected(M.chain(v).res, 12).addr).toBe(0);
		expect(M.CHECKS.negativeStride(v)).toBe(true);
	});

	test('the selection is clamped to the new result', () => {
		const s = lessonStore('test-tensor-memory', M.INITIAL, M.LINKS).getState();
		s.setMany({ shp: SHP234, op: id('none'), re: 0, cont: 0, sel: 23 });
		s.setMany({ op: id('crop') });
		expect(lessonStore('test-tensor-memory').getState().vals.sel).toBe(11);
	});
});

describe('3.3 cache', () => {
	test('row walk: 16 misses for any cache; column walk: 256 below 16 lines, 16 with 16', () => {
		for (let L = 1; L <= M.N; L++) {
			expect(M.runCache(false, L).misses[255]).toBe(16);
			expect(M.runCache(true, L).misses[255]).toBe(L < 16 ? 256 : 16);
		}
		expect(M.CHECKS.columnFits({ ...M.INITIAL, col: 1, lines: 16 })).toBe(true);
		expect(M.CHECKS.columnFits({ ...M.INITIAL, col: 1, lines: 15 })).toBe(false);
	});

	test('one row of float32 = one 64-byte line; the replayed cache agrees with the run', () => {
		expect(M.PER_LINE).toBe(16);
		for (let c = 0; c < 16; c++) expect(M.lineOf(3, c)).toBe(3);
		for (const col of [false, true]) for (const L of [1, 4, 16]) {
			const run = M.runCache(col, L);
			let prev: number[] = [];
			for (let k = 0; k < 256; k++) {
				const held = M.cacheAt(col, L, k);
				const line = M.lineOf(...M.cellAt(k, col));
				expect(held[0]).toBe(line);
				expect(held.length).toBeLessThanOrEqual(L);
				expect(run.hit[k]).toBe(prev.includes(line) ? 1 : 0);
				prev = held;
			}
		}
	});
});
