/**
 * Typed accessors over `src/data/course.json`, the single source of truth for the course structure:
 * lesson/module numbers, URLs, prev/next, progress ids and every generated list derive from it.
 * astro.config.mjs and remark-lesson-links.mjs read the same JSON directly (they run outside Vite's TS pipeline);
 * the rules they share with this file (locale prefixes, localized texts, `short ?? title` labels) live in
 * `course-rules.mjs`, and the numbering / `<module>/<lesson>/` URL rule is the same one-liner in each consumer.
 *
 * Texts are per locale: `loc(lesson | module, locale)` returns title/summary/label; URL helpers take an optional
 * locale (default English = Starlight's root locale, no prefix; Russian lives under `/ru/`).
 */
import data from '../data/course.json';
import { courseText, localizedUrl, splitLocale, type CourseItem, type Locale } from './course-rules.mjs';

export { LOCALES, asLocale, fromStarlight, type Locale } from './course-rules.mjs';

export interface LessonJson extends CourseItem {
	slug: string;
	branch?: boolean;
}

export interface ModuleJson extends CourseItem {
	slug: string;
	optional?: boolean;
	lessons: LessonJson[];
}

export interface Lesson {
	slug: string;
	branch?: boolean;
	/** Global lesson number, 1..N in course order. */
	num: number;
	moduleSlug: string;
	/** Module number, 1..M. */
	moduleNum: number;
	/** True for lessons of an optional module. */
	optional: boolean;
	/** Content id / URL path without base and locale: `<module>/<lesson>`. */
	path: string;
	/** Raw course.json entry; read texts through `loc()`. */
	readonly json: LessonJson;
}

export interface Module {
	slug: string;
	num: number;
	optional: boolean;
	lessons: Lesson[];
	/** Raw course.json entry; read texts through `loc()`. */
	readonly json: ModuleJson;
}

let counter = 0;
export const modules: readonly Module[] = (data.modules as ModuleJson[]).map((m, i) => ({
	slug: m.slug,
	num: i + 1,
	optional: m.optional === true,
	json: m,
	lessons: m.lessons.map((l) => ({
		slug: l.slug,
		...(l.branch ? { branch: true } : {}),
		num: ++counter,
		moduleSlug: m.slug,
		moduleNum: i + 1,
		optional: m.optional === true,
		path: `${m.slug}/${l.slug}`,
		json: l,
	})),
}));

/** All lessons in course order. */
export const lessons: readonly Lesson[] = modules.flatMap((m) => m.lessons);

const bySlug = new Map(lessons.map((l) => [l.slug, l]));
const byPath = new Map(lessons.map((l) => [l.path, l]));
const moduleBySlug = new Map(modules.map((m) => [m.slug, m]));

const base = (): string => import.meta.env.BASE_URL.replace(/\/$/, '');

/**
 * Localized texts of a lesson or module: `{ title, short?, summary, label }`, `label` = `short ?? title` (the same
 * rule astro.config.mjs applies to the sidebar). Throws when course.json lacks that locale's texts.
 */
export function loc(item: Lesson | Module, locale: Locale): ReturnType<typeof courseText> {
	return courseText(item.json, locale);
}

export function lessonBySlug(slug: string): Lesson | undefined {
	return bySlug.get(slug);
}

/** Module of a lesson (or by slug); throws on an unknown slug. */
export function moduleOf(slug: string): Module {
	const m = moduleBySlug.get(slug) ?? moduleBySlug.get(bySlug.get(slug)?.moduleSlug ?? '');
	if (!m) throw new Error(`course.json has no module or lesson "${slug}"`);
	return m;
}

/**
 * Finds a lesson by content id (`vectors/dot-product`, `ru/vectors/dot-product`) or URL path
 * (`/math-course/ru/vectors/dot-product/`). The base path, a leading locale segment, slashes and an `.mdx` suffix are
 * ignored; returns undefined for non-lesson pages.
 */
export function lessonByPath(path: string): Lesson | undefined {
	let p = path.replace(/\.mdx?$/, '').replace(/^\/+|\/+$/g, '');
	const b = base().replace(/^\//, '');
	if (b && p.startsWith(`${b}/`)) p = p.slice(b.length + 1);
	return byPath.get(splitLocale(p).rest.replace(/\/index$/, ''));
}

/** Absolute, base-prefixed URL of a lesson page in a locale; throws on an unknown slug. */
export function href(slug: string, hash?: string, locale: Locale = 'en'): string {
	const l = bySlug.get(slug);
	if (!l) throw new Error(`course.json has no lesson "${slug}"`);
	return `${localizedUrl(base(), l.path, locale)}${hash ? `#${hash}` : ''}`;
}

/** Absolute, base-prefixed URL of a top-level page (`course/roadmap`, `glossary`, ...; '' = home) in a locale. */
export function pageHref(path: string, locale: Locale = 'en'): string {
	return localizedUrl(base(), path, locale);
}

/** Absolute, base-prefixed URL of a module overview page (`src/pages/[module]/`, `src/pages/ru/[module]/`); throws on an unknown slug. */
export function moduleHref(slug: string, locale: Locale = 'en'): string {
	if (!moduleBySlug.has(slug)) throw new Error(`course.json has no module "${slug}"`);
	return localizedUrl(base(), slug, locale);
}

/** Previous / next lesson in course order (crosses module boundaries). */
export function adjacent(slug: string): { prev?: Lesson; next?: Lesson } {
	const i = lessons.findIndex((l) => l.slug === slug);
	return i < 0 ? {} : { prev: lessons[i - 1], next: lessons[i + 1] };
}
