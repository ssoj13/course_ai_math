/**
 * Typed accessors over `src/data/course.json`, the single source of truth for the course structure:
 * lesson/module numbers, URLs, prev/next, progress ids and every generated list derive from it.
 * astro.config.mjs and remark-lesson-links.mjs read the same JSON directly (they run outside Vite's
 * TS pipeline), so keep the derivation rules (numbering, `<module>/<lesson>/` URLs) identical.
 */
import data from '../data/course.json';

export interface LessonJson {
	slug: string;
	title: string;
	/** Optional compact label for navigation (sidebar, prev/next); the page keeps the full `title`. */
	short?: string;
	summary: string;
	/** Side branch: shown dashed on the roadmap; still numbered in course order. */
	branch?: boolean;
}

export interface ModuleJson {
	slug: string;
	title: string;
	summary: string;
	optional?: boolean;
	lessons: LessonJson[];
}

export interface Lesson extends LessonJson {
	/** Global lesson number, 1..N in course order. */
	num: number;
	moduleSlug: string;
	moduleTitle: string;
	/** Module number, 1..M. */
	moduleNum: number;
	/** True for lessons of an optional module. */
	optional: boolean;
	/** Content id / URL path without base: `<module>/<lesson>`. */
	path: string;
	/** Navigation label: `short ?? title` (astro.config.mjs applies the same rule to the sidebar). */
	label: string;
}

export interface Module extends Omit<ModuleJson, 'lessons' | 'optional'> {
	num: number;
	optional: boolean;
	lessons: Lesson[];
}

let counter = 0;
export const modules: readonly Module[] = (data.modules as ModuleJson[]).map((m, i) => ({
	slug: m.slug,
	title: m.title,
	summary: m.summary,
	num: i + 1,
	optional: m.optional === true,
	lessons: m.lessons.map((l) => ({
		...l,
		num: ++counter,
		moduleSlug: m.slug,
		moduleTitle: m.title,
		moduleNum: i + 1,
		optional: m.optional === true,
		path: `${m.slug}/${l.slug}`,
		label: l.short ?? l.title,
	})),
}));

/** All lessons in course order. */
export const lessons: readonly Lesson[] = modules.flatMap((m) => m.lessons);

const bySlug = new Map(lessons.map((l) => [l.slug, l]));
const byPath = new Map(lessons.map((l) => [l.path, l]));

const base = (): string => import.meta.env.BASE_URL.replace(/\/$/, '');

export function lessonBySlug(slug: string): Lesson | undefined {
	return bySlug.get(slug);
}

/**
 * Finds a lesson by content id (`vectors/dot-product`) or URL path (`/math-course/vectors/dot-product/`).
 * The base path, slashes and an `.mdx` suffix are ignored; returns undefined for non-lesson pages.
 */
export function lessonByPath(path: string): Lesson | undefined {
	let p = path.replace(/\.mdx?$/, '').replace(/^\/+|\/+$/g, '');
	const b = base().replace(/^\//, '');
	if (b && p.startsWith(`${b}/`)) p = p.slice(b.length + 1);
	return byPath.get(p.replace(/\/index$/, ''));
}

/** Absolute, base-prefixed URL of a lesson page; throws on an unknown slug. */
export function href(slug: string, hash?: string): string {
	const l = bySlug.get(slug);
	if (!l) throw new Error(`course.json has no lesson "${slug}"`);
	return `${base()}/${l.path}/${hash ? `#${hash}` : ''}`;
}

/** Absolute, base-prefixed URL of a top-level page (`course/roadmap`, `glossary`, ...; '' = home). */
export function pageHref(path: string): string {
	return `${base()}/${path ? `${path.replace(/\/$/, '')}/` : ''}`;
}

/** Previous / next lesson in course order (crosses module boundaries). */
export function adjacent(slug: string): { prev?: Lesson; next?: Lesson } {
	const i = lessons.findIndex((l) => l.slug === slug);
	return i < 0 ? {} : { prev: lessons[i - 1], next: lessons[i + 1] };
}
