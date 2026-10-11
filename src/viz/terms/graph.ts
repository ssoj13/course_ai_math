/**
 * The term graph (`src/data/terms.json`): one typed graph that every mind map of the course is drawn from (lesson maps,
 * genealogy, glossary; later the roadmap overlay, depth stacks and flashcards). Pure and DOM-free: the build-time
 * validation (tests/js/terms.test.ts), the server wrappers and the map islands all import it.
 *
 * Edge semantics, read as a sentence from the term that stores the edge (`X --type--> Y`):
 *   built-on        X is built on Y (Y is needed to understand X),
 *   is-a            X is a kind of Y,
 *   special-case-of X is Y with something fixed (Pythagoras = law of cosines at 90°),
 *   equivalent-to   X and Y are the same thing seen two ways (stored once, symmetric),
 *   used-in         X is applied in / implements Y without being a prerequisite (FMA is used in the dot product).
 * `built-on` and `is-a` are the FOUNDATION edges: they define the genealogy ("down to its roots") and must be acyclic.
 * A `used-in` edge that merely restates a reversed `built-on` edge is rejected, so each fact is stored once.
 */

export type EdgeType = 'built-on' | 'is-a' | 'special-case-of' | 'equivalent-to' | 'used-in';
export const EDGE_TYPES: readonly EdgeType[] = ['built-on', 'is-a', 'special-case-of', 'equivalent-to', 'used-in'];

/**
 * Level of a term: the course layer it belongs to (`matrices`/`ai` are forward terms of later parts) or `machine`,
 * the hardware/number-format layer every concept eventually lands on.
 */
export type Depth = 'ground' | 'vectors' | 'matrices' | 'ai' | 'machine';
export const DEPTHS: readonly Depth[] = ['ground', 'vectors', 'matrices', 'ai', 'machine'];

/** Reader-facing texts of a term in one language: a short label and a one-line "X is Y built on Z" definition. */
export interface TermText {
	label: string;
	def: string;
}

export interface Term extends TermText {
	id: string;
	depth: Depth;
	/** Lessons (course.json slugs) that teach the term, first = where it is introduced; [] = a forward term. */
	lessons: string[];
	edges: { type: EdgeType; to: string }[];
	/** Russian texts; English ones are the top-level fields (the course.json convention). */
	ru?: TermText;
}

export interface TermGraph {
	terms: Term[];
}

export interface MapEdge {
	from: string;
	to: string;
	type: EdgeType;
}

/** Foundation edges: the ones genealogy follows and that must stay acyclic. */
export const isFoundation = (t: EdgeType): boolean => t === 'built-on' || t === 'is-a';

const ID = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** All edges of the graph as a flat list, in term order. */
export function edgesOf(g: TermGraph): MapEdge[] {
	return g.terms.flatMap((t) => t.edges.map((e) => ({ from: t.id, to: e.to, type: e.type })));
}

/**
 * Every problem of the graph as a readable line; [] = valid. Checks: ids (format, duplicates), texts (English and
 * Russian, non-empty), depth and edge types, edge targets, self/duplicate/symmetric-duplicate edges, a `used-in`
 * that restates a reversed `built-on`, lesson slugs (known, and every known lesson teaches at least one term) and
 * cycles over the foundation edges (`built-on cycle: a → b → a`).
 */
export function validate(g: TermGraph, lessonSlugs: readonly string[]): string[] {
	const out: string[] = [];
	const ids = new Set<string>();
	for (const t of g.terms) {
		if (!ID.test(t.id)) out.push(`bad id "${t.id}" (lowercase words joined by "-")`);
		if (ids.has(t.id)) out.push(`duplicate id "${t.id}"`);
		ids.add(t.id);
	}
	const lessons = new Set(lessonSlugs);
	const taught = new Set<string>();
	const pairs = new Set<string>();
	for (const t of g.terms) {
		if (!t.label.trim()) out.push(`${t.id}: empty label`);
		if (!t.def.trim()) out.push(`${t.id}: empty definition`);
		if (!t.ru) out.push(`${t.id}: no Russian texts ("ru": { label, def })`);
		else {
			if (!t.ru.label.trim()) out.push(`${t.id}: empty Russian label`);
			if (!t.ru.def.trim()) out.push(`${t.id}: empty Russian definition`);
		}
		if (!DEPTHS.includes(t.depth)) out.push(`${t.id}: unknown depth "${t.depth}"`);
		for (const l of t.lessons) {
			if (!lessons.has(l)) out.push(`${t.id}: unknown lesson "${l}"`);
			taught.add(l);
		}
		if (new Set(t.lessons).size !== t.lessons.length) out.push(`${t.id}: a lesson is listed twice`);
		for (const e of t.edges) {
			if (!EDGE_TYPES.includes(e.type)) out.push(`${t.id}: unknown edge type "${e.type}"`);
			if (!ids.has(e.to)) out.push(`${t.id}: ${e.type} → unknown term "${e.to}"`);
			if (e.to === t.id) out.push(`${t.id}: ${e.type} edge to itself`);
			const key = `${t.id}|${e.to}`;
			if (pairs.has(key)) out.push(`${t.id}: more than one edge to "${e.to}"`);
			pairs.add(key);
		}
	}
	for (const e of edgesOf(g)) {
		if (e.type === 'equivalent-to' && pairs.has(`${e.to}|${e.from}`)) out.push(`${e.from}: equivalent-to "${e.to}" is also stored the other way`);
		if (e.type === 'used-in' && g.terms.find((t) => t.id === e.to)?.edges.some((b) => b.type === 'built-on' && b.to === e.from)) {
			out.push(`${e.from}: used-in "${e.to}" restates "${e.to}" built-on "${e.from}"`);
		}
	}
	for (const l of lessonSlugs) if (!taught.has(l)) out.push(`lesson "${l}" has no terms`);
	out.push(...cycles(g));
	return out;
}

/** Cycles over foundation edges, one line per back edge found by a colored DFS: `built-on cycle: a → b → a`. */
function cycles(g: TermGraph): string[] {
	const out: string[] = [];
	const parents = foundationParents(edgesOf(g));
	const color = new Map<string, 1 | 2>(); // 1 = on the DFS stack, 2 = done
	const stack: string[] = [];
	const visit = (id: string) => {
		color.set(id, 1);
		stack.push(id);
		for (const p of parents.get(id) ?? []) {
			const c = color.get(p);
			if (c === 1) out.push(`built-on cycle: ${[...stack.slice(stack.indexOf(p)), p].join(' → ')}`);
			else if (c === undefined) visit(p);
		}
		stack.pop();
		color.set(id, 2);
	};
	for (const t of g.terms) if (!color.has(t.id)) visit(t.id);
	return out;
}

/** id → foundation parents (the terms it is built on or is a kind of), in edge order. */
function foundationParents(edges: readonly MapEdge[]): Map<string, string[]> {
	const m = new Map<string, string[]>();
	for (const e of edges) if (isFoundation(e.type)) m.set(e.from, [...(m.get(e.from) ?? []), e.to]);
	return m;
}

/**
 * Every foundation ancestor of `id` (what it is built on, transitively), topologically ordered with the roots first;
 * `id` itself is not included. Assumes a validated (acyclic) graph.
 */
export function ancestry(g: TermGraph, id: string): string[] {
	return ancestorsIn(edgesOf(g), id);
}

/** `ancestry` over a plain edge list (the map islands hold edges, not the whole TermGraph). */
export function ancestorsIn(edges: readonly MapEdge[], id: string): string[] {
	const parents = foundationParents(edges);
	const seen = new Set<string>([id]);
	const out: string[] = [];
	const visit = (n: string) => {
		for (const p of parents.get(n) ?? []) {
			if (seen.has(p)) continue;
			seen.add(p);
			visit(p);
			out.push(p);
		}
	};
	visit(id);
	return out;
}

/** A subgraph to draw: node ids in a stable order and the edges between them. */
export interface Slice {
	ids: string[];
	edges: MapEdge[];
}

/** Edges whose both ends are in `ids`. */
const within = (edges: readonly MapEdge[], ids: ReadonlySet<string>): MapEdge[] => edges.filter((e) => ids.has(e.from) && ids.has(e.to));

/**
 * The terms within `hops` edges of the focus term(s), following every edge type in both directions, and all edges
 * among them. Order = the order of `order` (graph order), so layouts are deterministic.
 */
export function neighborhoodIn(order: readonly string[], edges: readonly MapEdge[], focus: string | readonly string[], hops: 1 | 2): Slice {
	const start = typeof focus === 'string' ? [focus] : [...focus];
	const set = new Set(start);
	let frontier = start;
	for (let h = 0; h < hops; h++) {
		const next: string[] = [];
		for (const e of edges) {
			for (const [a, b] of [[e.from, e.to], [e.to, e.from]] as const) {
				if (frontier.includes(a) && !set.has(b)) {
					set.add(b);
					next.push(b);
				}
			}
		}
		frontier = next;
	}
	return { ids: order.filter((id) => set.has(id)), edges: within(edges, set) };
}

/** `neighborhoodIn` over a TermGraph, returning the terms themselves (the interface of the brief). */
export function neighborhood(g: TermGraph, focus: string | readonly string[], hops: 1 | 2): { nodes: Term[]; edges: MapEdge[] } {
	const s = neighborhoodIn(g.terms.map((t) => t.id), edgesOf(g), focus, hops);
	const ids = new Set(s.ids);
	return { nodes: g.terms.filter((t) => ids.has(t.id)), edges: s.edges };
}

/**
 * Genealogy of `id`: the term, all its foundation ancestors with the foundation edges between them, plus the
 * machine-level terms (`isMachine`) that are `used-in` any of them ("down to the machine"). Order follows `order`.
 */
export function genealogyIn(order: readonly string[], edges: readonly MapEdge[], id: string, isMachine: (id: string) => boolean): Slice {
	const chain = new Set([id, ...ancestorsIn(edges, id)]);
	const machine = edges.filter((e) => e.type === 'used-in' && chain.has(e.to) && isMachine(e.from));
	const ids = new Set([...chain, ...machine.map((e) => e.from)]);
	const kept = edges.filter((e) => (isFoundation(e.type) && chain.has(e.from) && chain.has(e.to)) || machine.includes(e));
	return { ids: order.filter((x) => ids.has(x)), edges: kept.filter((e) => ids.has(e.from) && ids.has(e.to)) };
}

/** The related terms of one kind: `out` = stored on the term (X built-on Y), `in` = pointing at it (Y is built on X). */
export interface Relation {
	type: EdgeType;
	dir: 'out' | 'in';
	ids: string[];
}

/**
 * Relations of `id` among `edges`, grouped by type and direction in a fixed reading order (outgoing first). The
 * symmetric `equivalent-to` is reported once, as `out`, whichever term stores it.
 */
export function relations(edges: readonly MapEdge[], id: string): Relation[] {
	const out: Relation[] = [];
	for (const dir of ['out', 'in'] as const) {
		for (const type of EDGE_TYPES) {
			if (type === 'equivalent-to' && dir === 'in') continue;
			const ids = edges
				.filter((e) => e.type === type && (dir === 'out' ? e.from === id : e.to === id))
				.map((e) => (dir === 'out' ? e.to : e.from));
			if (type === 'equivalent-to') ids.push(...edges.filter((e) => e.type === type && e.to === id).map((e) => e.from));
			if (ids.length) out.push({ type, dir, ids });
		}
	}
	return out;
}

/**
 * One term as a map island receives it: texts already in the page locale and links already resolved by the server
 * (islands never load terms.json or course.json).
 */
export interface MapNode extends TermText {
	id: string;
	depth: Depth;
	/** URL of the lesson that introduces the term; null for a forward term (not taught yet). */
	href: string | null;
	/** Title of that lesson in the page locale (link text in the list view). */
	lesson: string | null;
	/** URL of the term's glossary entry. */
	gloss: string;
}

/** Everything a map island draws from: the nodes and edges of its slice (or the whole graph for genealogy). */
export interface MapData {
	nodes: MapNode[];
	edges: MapEdge[];
}

/** Texts of a term in a locale; throws when the Russian texts are missing (validate() reports it at build time). */
export function termText(t: Term, locale: 'en' | 'ru'): TermText {
	if (locale === 'en') return { label: t.label, def: t.def };
	if (!t.ru) throw new Error(`terms.json: "${t.id}" has no Russian texts`);
	return t.ru;
}
