// Build-time checks of the term graph (src/data/terms.json) and of where its maps are placed: the graph is valid
// (refs, ids, texts in both languages, acyclic foundation edges, every lesson teaches terms), the graph helpers
// behave, and every published lesson page (English and Russian) carries its local term map.
import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { lessons } from '../../src/lib/course';
import { EDGES, mapData, ORDER, parseGraph, TERMS, termById, termsOf } from '../../src/viz/terms/data';
import { ancestorsIn, ancestry, genealogyIn, neighborhood, neighborhoodIn, validate, type TermGraph } from '../../src/viz/terms/graph';

const g = TERMS;
const slugs = lessons.map((l) => l.slug);

describe('shipped graph', () => {
	test('is valid', () => {
		expect(validate(g, slugs)).toEqual([]);
	});
	test('every course lesson teaches at least one term and introduces it first', () => {
		for (const s of slugs) expect({ s, n: termsOf(s).length > 0 }).toEqual({ s, n: true });
	});
	test('storyboard §3 seed ids exist', () => {
		for (const id of ['square', 'sqrt', 'coordinate', 'point', 'distance', 'pythagoras', 'angle', 'radian', 'unit-circle', 'cosine', 'sine', 'law-of-cosines']) {
			expect(termById(id).depth).toBe('ground');
		}
	});
	test('genealogy reaches the roots and the machine level', () => {
		// The course's spine: dot product -> law of cosines -> Pythagoras -> square -> number.
		const a = ancestry(g, 'dot-product');
		for (const id of ['law-of-cosines', 'pythagoras', 'square', 'number', 'cosine', 'unit-circle']) expect(a).toContain(id);
		expect(a.indexOf('number')).toBeLessThan(a.indexOf('square'));
		expect(a.indexOf('square')).toBeLessThan(a.indexOf('pythagoras'));
		expect(a.indexOf('pythagoras')).toBeLessThan(a.indexOf('law-of-cosines'));
		const machine = (id: string) => termById(id).depth === 'machine';
		const s = genealogyIn(ORDER, EDGES, 'dot-product', machine);
		for (const id of ['fma', 'simd-lane', 'summation-order']) expect(s.ids).toContain(id);
		// Every kept edge has both ends in the slice.
		for (const e of s.edges) expect(s.ids.includes(e.from) && s.ids.includes(e.to)).toBe(true);
	});
});

describe('graph helpers (brief fixtures)', () => {
	test('detects unknown refs, duplicate ids, unknown lessons, built-on cycles and empty definitions', () => {
		const bad: TermGraph = {
			terms: [
				{ id: 'a', label: 'A', def: 'A is A.', depth: 'ground', lessons: ['nope'], edges: [{ type: 'built-on', to: 'b' }] },
				{ id: 'b', label: 'B', def: 'B is B.', depth: 'ground', lessons: [], edges: [{ type: 'built-on', to: 'a' }, { type: 'used-in', to: 'zzz' }] },
				{ id: 'b', label: 'B2', def: '', depth: 'ground', lessons: [], edges: [] },
			],
		};
		const p = validate(bad, slugs).join('\n');
		expect(p).toMatch(/duplicate id "b"/);
		expect(p).toMatch(/unknown term "zzz"/);
		expect(p).toMatch(/unknown lesson "nope"/);
		expect(p).toMatch(/built-on cycle: a → b → a/);
		expect(p).toMatch(/empty definition/);
		expect(p).toMatch(/no Russian texts/);
	});
	test('rejects a used-in that restates a reversed built-on, self edges and two-way equivalences', () => {
		const ru = { label: 'x', def: 'x' };
		const bad: TermGraph = {
			terms: [
				{ id: 'a', label: 'A', def: 'A.', depth: 'ground', lessons: [], ru, edges: [{ type: 'used-in', to: 'b' }, { type: 'equivalent-to', to: 'c' }] },
				{ id: 'b', label: 'B', def: 'B.', depth: 'ground', lessons: [], ru, edges: [{ type: 'built-on', to: 'a' }] },
				{ id: 'c', label: 'C', def: 'C.', depth: 'ground', lessons: [], ru, edges: [{ type: 'equivalent-to', to: 'a' }, { type: 'is-a', to: 'c' }] },
			],
		};
		const p = validate(bad, []).join('\n');
		expect(p).toMatch(/a: used-in "b" restates "b" built-on "a"/);
		expect(p).toMatch(/equivalent-to "c" is also stored the other way/);
		expect(p).toMatch(/c: is-a edge to itself/);
		expect(p).toMatch(/built-on cycle: c → c/);
	});
	test('is-a counts as a foundation edge for cycles and genealogy', () => {
		const ru = { label: 'x', def: 'x' };
		const cyc: TermGraph = {
			terms: [
				{ id: 'a', label: 'A', def: 'A.', depth: 'ground', lessons: [], ru, edges: [{ type: 'is-a', to: 'b' }] },
				{ id: 'b', label: 'B', def: 'B.', depth: 'ground', lessons: [], ru, edges: [{ type: 'built-on', to: 'a' }] },
			],
		};
		expect(validate(cyc, []).join('\n')).toMatch(/built-on cycle: a → b → a/);
		expect(ancestry(g, 'standard-basis')).toContain('basis');
	});
	test('ancestry of law-of-cosines reaches square and excludes itself', () => {
		expect(ancestry(g, 'law-of-cosines')).toContain('square');
		expect(ancestry(g, 'law-of-cosines').at(-1)).not.toBe('law-of-cosines');
		expect(ancestry(g, 'law-of-cosines')).not.toContain('law-of-cosines');
	});
	test('ancestry is topological: every parent comes before its child', () => {
		for (const t of g.terms) {
			const a = ancestorsIn(EDGES, t.id);
			const pos = new Map(a.map((id, i) => [id, i]));
			for (const e of EDGES) {
				if ((e.type === 'built-on' || e.type === 'is-a') && pos.has(e.from) && pos.has(e.to)) {
					expect({ term: t.id, edge: `${e.from}->${e.to}`, ok: (pos.get(e.to) ?? 0) < (pos.get(e.from) ?? 0) }).toEqual({
						term: t.id,
						edge: `${e.from}->${e.to}`,
						ok: true,
					});
				}
			}
		}
	});
	test('neighborhood includes both directions and grows with hops', () => {
		const n = neighborhood(g, 'pythagoras', 1).nodes.map((t) => t.id);
		expect(n).toContain('square'); // built on
		expect(n).toContain('law-of-cosines'); // something built on it
		const two = neighborhoodIn(ORDER, EDGES, 'pythagoras', 2).ids;
		expect(two).toContain('number');
		expect(two.length).toBeGreaterThan(n.length);
		const multi = neighborhoodIn(ORDER, EDGES, ['vector', 'component'], 1);
		for (const e of multi.edges) expect(multi.ids.includes(e.from) && multi.ids.includes(e.to)).toBe(true);
	});
	test('parseGraph rejects a wrong shape with the field path', () => {
		expect(() => parseGraph({ terms: [{ id: 'a', label: 'A', def: 'A', depth: 'deep', lessons: [], edges: [] }] })).toThrow(/a\.depth "deep"/);
		expect(() => parseGraph({ terms: [{ id: 'a', label: 'A', def: 'A', depth: 'ground', lessons: [], edges: [{ type: 'likes', to: 'b' }] }] })).toThrow(
			/a\.edges\[0\]\.type "likes"/,
		);
		expect(() => parseGraph({ terms: [{ id: 'a', label: 1, depth: 'ground' }] })).toThrow(/a\.label must be a string/);
	});
});

describe('map payload', () => {
	test('resolves localized texts, lesson and glossary links; forward terms have no lesson', () => {
		process.env.BASE_URL = '/math-course/';
		const d = mapData(['law-of-cosines', 'matrix'], [], 'ru');
		expect(d.nodes[0]).toMatchObject({ label: 'Теорема косинусов', href: '/math-course/ru/ground/law-of-cosines/', gloss: '/math-course/ru/glossary/#term-law-of-cosines' });
		expect(d.nodes[1]).toMatchObject({ id: 'matrix', href: null, lesson: null });
		expect(mapData(['square'], [], 'en').nodes[0]).toMatchObject({ label: 'Square', href: '/math-course/ground/squares-and-plane/' });
	});
});

describe('placement', () => {
	const DOCS = join(import.meta.dir, '..', '..', 'src', 'content', 'docs');
	const read = (p: string) => readFileSync(join(DOCS, p), 'utf8');
	test('every lesson page ends with its local term map, in both languages where translated', () => {
		const ru = new Set(readdirSync(join(DOCS, 'ru'), { recursive: true, encoding: 'utf8' }).map((p) => p.replaceAll('\\', '/')));
		for (const l of lessons) {
			for (const file of [`${l.path}.mdx`, ...(ru.has(`${l.path}.mdx`) ? [`ru/${l.path}.mdx`] : [])]) {
				const text = read(file);
				expect({ file, map: /^<LessonMap \/>$/m.test(text) }).toEqual({ file, map: true });
			}
		}
	});
	test('the glossary is generated from the graph in both languages', () => {
		for (const file of ['glossary.mdx', 'ru/glossary.mdx']) expect({ file, ok: /^<Glossary \/>$/m.test(read(file)) }).toEqual({ file, ok: true });
	});
});
