import { describe, expect, test } from 'bun:test';
import { en } from '../../src/i18n/ui';
import {
	INITIAL, CHECKS, analogy, angleDeg, cosMatrix, cosine, dist, dot, fmtCos, fmtDeg, fmtInt, nearest, products, rank, scaled, toy, unitRows,
} from '../../src/viz/lessons/cosine-similarity.math';
import { ANALOGIES, DOCS, GROUPS, LLM, PROV, QUERIES, SENTENCE, VECS, WORDS, indexOf } from '../../src/viz/lessons/cosine-similarity.data';
import { DOC_KEY, GROUP_KEY, NOTE_KEY, Q_KEY, rowsFor } from '../../src/viz/lessons/cosine-similarity.rows';

/**
 * Lesson 1.7 model: every number the prose states is recomputed here from first principles (hand arithmetic), the
 * zero-vector guard is exercised on every path, and the shipped embedding file is checked for internal consistency.
 */

const close = (x: number, y: number, eps = 1e-12) => expect(Math.abs(x - y)).toBeLessThan(eps);

describe('cosine', () => {
	test('prose examples', () => {
		close(cosine([1, 1], [2, 2]), 1);
		close(cosine([1, 0], [0, 1]), 0);
		close(cosine([1, 2], [2, 1]), 0.8);
		close(cosine([3, 4], [6, 8]), 1);
		close(cosine([3, 4], [4, 3]), 0.96);
		close(cosine([1, 0], [-1, 0]), -1);
		close(dist([3, 4], [6, 8]), 5);
		close(dist([3, 4], [4, 3]), Math.SQRT2);
	});

	test('toy words of the prose', () => {
		// cat·dog = 0.72 + 0.02 = 0.74; |cat| = √0.82, |dog| = √0.68.
		close(cosine([0.9, 0.1], [0.8, 0.2]), 0.74 / Math.sqrt(0.82 * 0.68));
		expect(fmtCos(cosine([0.9, 0.1], [0.8, 0.2]))).toBe('0.991');
		// cat·car = 0.09 + 0.09 = 0.18, both lengths √0.82.
		close(cosine([0.9, 0.1], [0.1, 0.9]), 0.18 / 0.82);
		expect(fmtCos(cosine([0.9, 0.1], [0.1, 0.9]))).toBe('0.220');
	});

	test('scale invariance: any positive factor leaves the cosine, a negative one flips its sign', () => {
		for (const s of [0.05, 0.5, 2, 1e3]) close(cosine([3, 4], [4 * s, 3 * s]), 0.96, 1e-12);
		close(cosine([3, 4], [-4, -3]), -0.96);
	});

	test('a zero vector has no direction: cosine and angle undefined, dot and distance defined', () => {
		expect(cosine([0, 0], [1, 2])).toBeNaN();
		expect(cosine([1, 2], [0, 0])).toBeNaN();
		expect(angleDeg([0, 0], [0, 0])).toBeNaN();
		expect(dot([0, 0], [1, 2])).toBe(0);
		close(dist([0, 0], [3, 4]), 5);
		expect(fmtCos(NaN)).toBe('–');
		expect(fmtDeg(NaN)).toBe('–');
	});

	test('rounding never leaves [−1, 1]', () => {
		const a = [0.1, 0.2, 0.3];
		expect(cosine(a, a)).toBeLessThanOrEqual(1);
		expect(cosine(a, a.map((x) => -x))).toBeGreaterThanOrEqual(-1);
	});

	test('mismatched lengths throw', () => {
		expect(() => dot([1, 2], [1, 2, 3])).toThrow();
		expect(() => products([1], [1, 2])).toThrow();
	});
});

describe('tables and search', () => {
	test('unit rows keep zero rows zero; cosMatrix marks them undefined', () => {
		expect(unitRows([[0, 0], [3, 4]])).toEqual([[0, 0], [0.6, 0.8]]);
		const m = cosMatrix([[1, 0], [0, 0], [1, 1]]);
		close(m[0][0], 1);
		expect(m[0][1]).toBeNaN();
		close(m[0][2], Math.SQRT1_2);
	});

	test('products sum to the dot product', () => {
		const p = products([1, 2, 3], [4, -5, 6]);
		expect(p).toEqual([4, -10, 18]);
		expect(p.reduce((s, x) => s + x, 0)).toBe(dot([1, 2, 3], [4, -5, 6]));
	});

	test('nearest: best first, skips, ignores zero rows, empty for a zero query', () => {
		const rows = [[1, 0], [0, 1], [1, 1], [0, 0]];
		expect(nearest(rows, [1, 0.1], 2).map((h) => h.i)).toEqual([0, 2]);
		expect(nearest(rows, [1, 0.1], 2, new Set([0])).map((h) => h.i)).toEqual([2, 1]);
		expect(nearest(rows, [0, 0], 3)).toEqual([]);
		expect(nearest(rows, [1, 1], 10).map((h) => h.i)).toEqual([2, 0, 1]);
	});

	test('analogy on a toy space: king − man + woman lands on queen; without exclusion the input wins', () => {
		// Axes: royalty, maleness.
		const rows = [[1, 1], [0, 1], [0, -1], [1, -1]]; // king, man, woman, queen
		expect(analogy(rows, 0, 1, 2, 1, true)[0].i).toBe(3);
		const q = analogy(rows, 0, 1, 2, 4, false);
		expect(q[0].i).toBe(3);
		// Here queen is exact; the real table (below) shows the input-wins failure.
	});

	test('rank orders every document', () => {
		const r = rank([[1, 0], [0, 1], [1, 1]], [0, 1]);
		expect(r.map((h) => h.i)).toEqual([1, 2, 0]);
	});
});

describe('scene models and challenges', () => {
	test('initial toy state', () => {
		const z = toy(INITIAL);
		expect(fmtCos(z.cosCD)).toBe('0.991');
		expect(z.zero).toEqual([]);
		expect(toy({ ...INITIAL, dog: [0, 0] }).zero).toEqual(['dog']);
		expect(toy({ ...INITIAL, dog: [0, 0] }).cosCD).toBeNaN();
	});

	test('scaled: cosine constant for s > 0, undefined at s = 0, distance changes', () => {
		const at = (s: number) => scaled({ ...INITIAL, s });
		close(at(1).cos, 0.96);
		close(at(2).cos, 0.96);
		close(at(0.25).cos, 0.96);
		expect(at(0).cos).toBeNaN();
		close(at(1).dist, Math.SQRT2);
		close(at(2).dist, Math.hypot(5, 2));
		close(at(0).dist, 5);
		expect(CHECKS.zero({ ...INITIAL, s: 0 })).toBe(true);
		expect(CHECKS.farSame({ ...INITIAL, s: 2 })).toBe(true);
		expect(CHECKS.farSame(INITIAL)).toBe(false);
		expect(CHECKS.farSame({ ...INITIAL, s: 2, sb: [4, 2] })).toBe(false);
	});

	test('toy challenges', () => {
		expect(CHECKS.catCar(INITIAL)).toBe(false);
		expect(CHECKS.catCar({ ...INITIAL, car: [0.9, 0.1] })).toBe(true);
		expect(CHECKS.perp({ ...INITIAL, dog: [-0.1, 0.9] })).toBe(true);
		expect(CHECKS.perp(INITIAL)).toBe(false);
	});

	test('thousands grouping', () => {
		expect(fmtInt(400001)).toBe('400 001');
		expect(fmtInt(2048)).toBe('2 048');
		expect(fmtInt(785)).toBe('785');
	});
});

describe('the shipped embedding file', () => {
	test('shape and provenance', () => {
		expect(WORDS.length).toBe(62);
		expect(PROV.words.dims).toBe(300);
		for (const w of WORDS) {
			expect(w.v.length).toBe(300);
			expect(w.pca?.length).toBe(3);
			expect(GROUPS as readonly string[]).toContain(w.group);
			expect(w.row).toBeGreaterThan(0);
			expect(w.row).toBeLessThan(PROV.words.table_rows);
		}
		expect(new Set(WORDS.map((w) => w.row)).size).toBe(WORDS.length);
		expect(PROV.words.max_cos_rounding_error).toBeLessThan(1e-3);
		expect(SENTENCE.map((w) => w.word)).toEqual(['the', 'cat', 'sat', 'on', 'the', 'mat']);
		expect(PROV.pca.explained.length).toBe(3);
		expect(PROV.pca.origin.length).toBe(3);
	});

	test('the frequency order of GloVe rows: "the" is row 1', () => {
		expect(SENTENCE[0].row).toBe(1);
	});

	test('cosine in the full 300 dimensions: cat is nearer to dog than to car', () => {
		const cat = VECS[indexOf('cat')];
		expect(cosine(cat, VECS[indexOf('dog')])).toBeGreaterThan(cosine(cat, VECS[indexOf('car')]));
		// The best neighbour of cat inside the table is dog (GloVe's full vocabulary agrees: dog, cats, pet).
		expect(WORDS[nearest(VECS, cat, 1, new Set([indexOf('cat')]))[0].i].word).toBe('dog');
	});

	test('analogies inside the table: king − man + woman → queen; with the inputs kept, king wins', () => {
		const [k, m, w] = ['king', 'man', 'woman'].map(indexOf);
		expect(WORDS[analogy(VECS, k, m, w, 1, true)[0].i].word).toBe('queen');
		expect(WORDS[analogy(VECS, k, m, w, 1, false)[0].i].word).toBe('king');
		const full = ANALOGIES.find((a) => a.a === 'king')!;
		expect(full.full_vocab_top[0].word).toBe('queen');
		expect(full.full_vocab_top_with_inputs[0].word).toBe('king');
	});

	test('search vectors are unit length and the five queries find their passages', () => {
		for (const d of [...DOCS, ...QUERIES]) {
			expect(d.v.length).toBe(384);
			close(Math.hypot(...d.v), 1, 2e-3);
		}
		const best = QUERIES.map((q) => rank(DOCS.map((d) => d.v), q.v)[0].i);
		expect(best).toEqual([0, 5, 6, 9, 10]);
	});

	test('LLM facts', () => {
		expect(LLM.rows).toBe(151936);
		expect(LLM.dims).toBe(2048);
		expect(LLM.tokens.length).toBe(6);
	});
});

describe('dictionary', () => {
	const dict: Readonly<Record<string, string>> = en;
	test('the English search strings are exactly the texts the model embedded', () => {
		DOCS.forEach((d, i) => expect(dict[`cs.doc${i}`]).toBe(d.text));
		QUERIES.forEach((q, i) => expect(dict[`cs.q${i}`]).toBe(q.text));
		expect(dict['cs.doc12']).toBeUndefined();
		expect(dict['cs.q5']).toBeUndefined();
	});

	test('key tables cover the data exactly', () => {
		expect(DOC_KEY.length).toBe(DOCS.length);
		expect(Q_KEY.length).toBe(QUERIES.length);
		expect(NOTE_KEY.length).toBe(ANALOGIES.length);
		expect(Object.keys(GROUP_KEY)).toEqual([...GROUPS]);
	});

	test('one analogy note per preset', () => {
		ANALOGIES.forEach((_, i) => expect(dict[`cs.anNote${i}`]).toBeString());
		expect(dict[`cs.anNote${ANALOGIES.length}`]).toBeUndefined();
	});
});

describe('rows', () => {
	test('row ids per scene', () => {
		expect(rowsFor(1, 'en').map((r) => r.id)).toEqual(['cat', 'dog', 'car', 'cosCD', 'cosCC', 'cosDC', 'thCD']);
		expect(rowsFor(2, 'en').map((r) => r.id)).toEqual(['sa', 'sb', 's', 'na', 'nsb', 'cos', 'th', 'dist']);
		expect(rowsFor(3, 'en').map((r) => r.id)).toEqual(['cosReal', 'thReal', 'thPic', 'kept']);
	});
});
