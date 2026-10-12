// Build-time checks of the term graph (src/data/terms.json) and of where its maps are placed: the graph is valid
// (refs, ids, texts in both languages, acyclic foundation edges, every lesson teaches terms), the graph helpers
// behave, and every published lesson page (English and Russian) carries its local term map.
import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { lessons } from '../../src/lib/course';
import { EDGES, introducedIn, mapData, ORDER, parseGraph, TERMS, termById, termsOf } from '../../src/viz/terms/data';
import { ancestorsIn, ancestry, flow, foundationsIn, genealogyIn, layerCount, mapHeight, neighborhood, wrapLabel, neighborhoodIn, spineIn, validate, type TermGraph } from '../../src/viz/terms/graph';
import { exitPoint, openView, splinePath, straightPath } from '../../src/viz/terms/layout';

const g = TERMS;
const slugs = lessons.map((l) => l.slug);

describe('shipped graph', () => {
	test('is valid', () => {
		expect(validate(g, slugs)).toEqual([]);
	});
	test('every course lesson introduces at least one term (its map focus)', () => {
		for (const s of slugs) expect({ s, introduced: introducedIn(s).length > 0, taught: termsOf(s).length >= introducedIn(s).length }).toEqual({ s, introduced: true, taught: true });
	});
	test('lesson cards stay small: the introduced terms and their direct foundations, at most 12', () => {
		for (const s of slugs) {
			const own = introducedIn(s).map((t) => t.id);
			const c = foundationsIn(ORDER, EDGES, own, introducedIn(s).filter((t) => t.depth !== 'machine').map((t) => t.id));
			expect({ s, small: c.ids.length <= 12, hasOwn: own.every((id) => c.ids.includes(id)) }).toEqual({ s, small: true, hasOwn: true });
			// Only foundation hops: every non-focus node is something a focus term is built on.
			for (const id of c.ids.filter((x) => !own.includes(x))) {
				expect(EDGES.some((e) => (e.type === 'built-on' || e.type === 'is-a') && own.includes(e.from) && e.to === id)).toBe(true);
			}
		}
	});
	test('spine maps of the Start-here pages are one connected chain of the backbone terms', () => {
		const s = spineIn(ORDER, EDGES, ['multiply-add', 'distance', 'angle', 'dot-product', 'matrix', 'tensor', 'embedding', 'attention']);
		expect(s.ids.length).toBe(8);
		const has = (from: string, to: string) => s.edges.some((e) => e.from === from && e.to === to);
		expect(has('dot-product', 'distance') || has('dot-product', 'angle')).toBe(true);
		expect(has('matrix', 'dot-product') && has('tensor', 'matrix') && has('attention', 'tensor')).toBe(true);
		expect(has('multiply-add', 'dot-product')).toBe(true);
		// Every term is reached: no isolated node in the card.
		for (const id of s.ids) expect(s.edges.some((e) => e.from === id || e.to === id)).toBe(true);
		expect(() => spineIn(ORDER, EDGES, ['no-such-term'])).toThrow();
	});
	test('lesson 1.7 introduces its AI terms, and attention rests on softmax', () => {
		const own = introducedIn('cosine-similarity').map((t) => t.id);
		for (const id of ['embedding', 'embedding-table', 'nearest-neighbour', 'semantic-search', 'rag', 'softmax']) expect(own).toContain(id);
		const a = ancestry(g, 'attention');
		for (const id of ['softmax', 'power', 'embedding', 'dot-product']) expect(a).toContain(id);
		expect(ancestry(g, 'rag')).toEqual(expect.arrayContaining(['semantic-search', 'nearest-neighbour', 'cosine-similarity', 'model']));
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
		// ... and below the machine terms their own foundations: FMA -> rounding -> floating point.
		for (const id of ['rounding', 'float']) expect(s.ids).toContain(id);
		expect(s.edges).toContainEqual({ from: 'rounding', to: 'float', type: 'built-on' });
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
	test('a foundation edge may not point to a term introduced in a later lesson', () => {
		const ru = { label: 'x', def: 'x' };
		const [first, second] = slugs as [string, string];
		const g2: TermGraph = {
			terms: [
				{ id: 'early', label: 'E', def: 'E.', depth: 'ground', lessons: [first], ru, edges: [{ type: 'built-on', to: 'late' }, { type: 'used-in', to: 'late2' }] },
				{ id: 'late', label: 'L', def: 'L.', depth: 'ground', lessons: [second], ru, edges: [] },
				{ id: 'late2', label: 'L2', def: 'L2.', depth: 'ground', lessons: [second], ru, edges: [{ type: 'built-on', to: 'fwd' }] },
				{ id: 'fwd', label: 'F', def: 'F.', depth: 'ground', lessons: [], ru, edges: [] },
			],
		};
		const p = validate(g2, slugs);
		expect(p.filter((x) => x.includes('later lesson'))).toEqual([`early: built-on "late", which is introduced in a later lesson (${second} after ${first})`]);
	});
	test('layer count and box height of a slice', () => {
		const s = foundationsIn(ORDER, EDGES, ['law-of-cosines']);
		expect(s.ids.sort()).toEqual(['cosine', 'law-of-cosines', 'pythagoras']);
		expect(layerCount(s)).toBe(2);
		expect(layerCount({ ids: ['a'], edges: [] })).toBe(1);
		expect(mapHeight(4)).toBe(Math.ceil((4 * 28 + 3 * 34 + 16) * 1.12));
		expect(mapHeight(2)).toBe(200); // clamped up
		expect(mapHeight(4, true)).toBe(Math.ceil((4 * 42 + 3 * 34 + 16) * 1.12));
		expect(wrapLabel('Square')).toEqual(['Square']);
		expect(wrapLabel('Angle between vectors')).toEqual(['Angle between', 'vectors']);
		expect(wrapLabel('Скалярное произведение')).toEqual(['Скалярное', 'произведение']);
		expect(wrapLabel('Linearindependence')).toEqual(['Linearindependence']);
		expect(mapHeight(1)).toBe(200);
		expect(mapHeight(40)).toBe(680);
		expect(flow({ from: 'x', to: 'y', type: 'built-on' })).toEqual(['y', 'x']);
		expect(flow({ from: 'x', to: 'y', type: 'used-in' })).toEqual(['x', 'y']);
	});
	test('parseGraph rejects a wrong shape with the field path', () => {
		expect(() => parseGraph({ terms: [{ id: 'a', label: 'A', def: 'A', depth: 'deep', lessons: [], edges: [] }] })).toThrow(/a\.depth "deep"/);
		expect(() => parseGraph({ terms: [{ id: 'a', label: 'A', def: 'A', depth: 'ground', lessons: [], edges: [{ type: 'likes', to: 'b' }] }] })).toThrow(
			/a\.edges\[0\]\.type "likes"/,
		);
		expect(() => parseGraph({ terms: [{ id: 'a', label: 1, depth: 'ground' }] })).toThrow(/a\.label must be a string/);
	});
});

describe('layout helpers', () => {
	test('splinePath: 3k+2 bend points are a cubic chain, anything else a polyline', () => {
		const p = (x: number, y: number) => ({ x, y });
		expect(splinePath(p(0, 0), [p(1, 0), p(2, 0)], p(3, 0))).toBe('M0,0 C1,0 2,0 3,0');
		expect(splinePath(p(0, 0), [p(1, 0), p(2, 0), p(3, 0), p(4, 0), p(5, 0)], p(6, 0))).toBe('M0,0 C1,0 2,0 3,0 C4,0 5,0 6,0');
		expect(splinePath(p(0, 0), [], p(3, 4))).toBe('M0,0 L3,4');
		expect(splinePath(p(0, 0), [p(1, 1)], p(2, 0))).toBe('M0,0 L1,1 L2,0');
		expect(splinePath(p(0.04, 0.06), [], p(1.25, 2))).toBe('M0,0.1 L1.3,2');
	});
	test('exitPoint / straightPath: border to border along the centre line', () => {
		const a = { x: 0, y: 0, w: 100, h: 20 };
		expect(exitPoint(a, { x: 250, y: 10 })).toEqual({ x: 100, y: 10 });
		expect(exitPoint(a, { x: 50, y: -90 })).toEqual({ x: 50, y: 0 });
		expect(exitPoint(a, { x: 50, y: 10 })).toEqual({ x: 50, y: 10 });
		expect(straightPath(a, { x: 200, y: 0, w: 100, h: 20 })).toBe('M100,10 L200,10');
	});
	test('openView: everything when readable, otherwise the focus at the floor zoom', () => {
		const box = { width: 600, height: 400 };
		expect(openView({ width: 500, height: 300 }, box, ['a'], 0.92)).toEqual({ padding: 0.06, maxZoom: 1.25 });
		expect(openView({ width: 1500, height: 300 }, box, ['a', 'b'], 0.92)).toEqual({ padding: 0.06, nodes: [{ id: 'a' }, { id: 'b' }], minZoom: 0.92, maxZoom: 0.92 });
		expect(openView({ width: 1500, height: 300 }, box, ['a'], 0)).toEqual({ padding: 0.06, maxZoom: 1.25 });
		expect(openView({ width: 1500, height: 300 }, box, [], 0.92)).toEqual({ padding: 0.06, maxZoom: 1.25 });
	});
});

describe('map payload', () => {
	test('resolves localized texts, lesson and glossary links; forward terms have no lesson', () => {
		process.env.BASE_URL = '/math-course/';
		const d = mapData(['law-of-cosines', 'determinant', 'matrix'], [], 'ru');
		expect(d.nodes[0]).toMatchObject({ label: 'Теорема косинусов', href: '/math-course/ru/ground/law-of-cosines/', gloss: '/math-course/ru/glossary/#term-law-of-cosines' });
		expect(d.nodes[1]).toMatchObject({ id: 'determinant', href: null, lesson: null });
		expect(d.nodes[2]).toMatchObject({ id: 'matrix', href: '/math-course/ru/matrices/linear-transformations/' });
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
				expect({ file, map: /^<LessonMap( spine=\{\[[^\]]*\]\})? \/>$/m.test(text) }).toEqual({ file, map: true });
			}
		}
	});
	test('the glossary is generated from the graph in both languages', () => {
		for (const file of ['glossary.mdx', 'ru/glossary.mdx']) expect({ file, ok: /^<Glossary \/>$/m.test(read(file)) }).toEqual({ file, ok: true });
	});
});
