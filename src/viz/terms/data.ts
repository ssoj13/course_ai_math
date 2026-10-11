/**
 * Server side of the term graph: loads `src/data/terms.json` with a structural check and turns slices of it into the
 * `MapData` a map island receives (texts in the page locale, lesson and glossary URLs resolved here). Used by the
 * Astro wrappers (LessonMap, Glossary) and the tests; client code imports only `graph.ts`.
 */
import raw from '../../data/terms.json';
import { href, lessonBySlug, loc, pageHref, type Locale } from '../../lib/course';
import { DEPTHS, EDGE_TYPES, edgesOf, termText, type Depth, type EdgeType, type MapData, type MapEdge, type Term, type TermGraph } from './graph';

const fail = (msg: string): never => {
	throw new Error(`terms.json: ${msg}`);
};
const str = (v: unknown, where: string): string => (typeof v === 'string' ? v : fail(`${where} must be a string`));
const obj = (v: unknown, where: string): Record<string, unknown> =>
	v && typeof v === 'object' && !Array.isArray(v) ? (v as Record<string, unknown>) : fail(`${where} must be an object`);
const arr = (v: unknown, where: string): unknown[] => (Array.isArray(v) ? v : fail(`${where} must be an array`));

/**
 * Checks the SHAPE of the parsed JSON (types of every field, known depth and edge type) and returns it typed; the
 * graph rules (refs, cycles, lessons) are `validate()`'s job.
 */
export function parseGraph(json: unknown): TermGraph {
	const terms = arr(obj(json, 'root').terms, 'terms').map((x, i): Term => {
		const t = obj(x, `terms[${i}]`);
		const id = str(t.id, `terms[${i}].id`);
		const depth = str(t.depth, `${id}.depth`);
		if (!DEPTHS.includes(depth as Depth)) fail(`${id}.depth "${depth}" is not one of ${DEPTHS.join(', ')}`);
		const ru = t.ru === undefined ? undefined : obj(t.ru, `${id}.ru`);
		return {
			id,
			label: str(t.label, `${id}.label`),
			def: str(t.def, `${id}.def`),
			depth: depth as Depth,
			lessons: arr(t.lessons, `${id}.lessons`).map((l, j) => str(l, `${id}.lessons[${j}]`)),
			edges: arr(t.edges, `${id}.edges`).map((e, j) => {
				const o = obj(e, `${id}.edges[${j}]`);
				const type = str(o.type, `${id}.edges[${j}].type`);
				if (!EDGE_TYPES.includes(type as EdgeType)) fail(`${id}.edges[${j}].type "${type}" is not one of ${EDGE_TYPES.join(', ')}`);
				return { type: type as EdgeType, to: str(o.to, `${id}.edges[${j}].to`) };
			}),
			...(ru ? { ru: { label: str(ru.label, `${id}.ru.label`), def: str(ru.def, `${id}.ru.def`) } } : {}),
		};
	});
	return { terms };
}

/** The course's term graph. */
export const TERMS: TermGraph = parseGraph(raw);
/** All its edges. */
export const EDGES: readonly MapEdge[] = edgesOf(TERMS);
/** Term ids in graph order. */
export const ORDER: readonly string[] = TERMS.terms.map((t) => t.id);

const byId = new Map(TERMS.terms.map((t) => [t.id, t]));

export function termById(id: string): Term {
	return byId.get(id) ?? fail(`unknown term "${id}"`);
}

/** Terms taught by a lesson (its slug listed in `lessons`), in graph order. */
export function termsOf(lessonSlug: string): Term[] {
	return TERMS.terms.filter((t) => t.lessons.includes(lessonSlug));
}

/** Terms a lesson introduces (its slug is their first lesson): the focus of its term map. */
export function introducedIn(lessonSlug: string): Term[] {
	return TERMS.terms.filter((t) => t.lessons[0] === lessonSlug);
}

/** Anchor id of a term's glossary entry. */
export const glossId = (id: string): string => `term-${id}`;

/** The map island payload for the given term ids (graph order kept) and edges, in a locale. */
export function mapData(ids: readonly string[], edges: readonly MapEdge[], locale: Locale): MapData {
	const glossary = pageHref('glossary', locale);
	return {
		nodes: ids.map((id) => {
			const t = termById(id);
			const first = t.lessons[0];
			const lesson = first ? lessonBySlug(first) : undefined;
			if (first && !lesson) fail(`${id}: unknown lesson "${first}"`);
			return {
				id,
				depth: t.depth,
				...termText(t, locale),
				href: lesson ? href(lesson.slug, undefined, locale) : null,
				lesson: lesson ? loc(lesson, locale).label : null,
				gloss: `${glossary}#${glossId(id)}`,
			};
		}),
		edges: [...edges],
	};
}
