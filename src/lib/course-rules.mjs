/**
 * Locale and course-text derivation rules shared by astro.config.mjs (sidebar), remark-lesson-links.mjs (content
 * links, build time), course.ts (typed accessors) and the i18n tests. Plain JS with JSDoc types because
 * astro.config.mjs and the Sätteri plugin run outside Vite's TS pipeline; keeping the rules here (instead of
 * re-implementing them per consumer) is what keeps sidebar labels, links and page chrome in sync.
 *
 * Locales: `en` is Starlight's ROOT locale (URLs without a prefix, content in `src/content/docs/`); every other
 * locale lives under `/<code>/` (content in `src/content/docs/<code>/`, same relative paths). course.json carries the
 * English texts at the top level and the translations in a per-locale object (`ru: { title, short?, summary }`).
 */

/** @typedef {'en' | 'ru'} Locale */

/** All course locales; the first one is the default (Starlight root) locale. */
export const LOCALES = /** @type {const} */ (['en', 'ru']);

/** Starlight `locales` config: root = English (unprefixed URLs), `ru` under `/ru/`. */
export const STARLIGHT_LOCALES = {
	root: { label: 'English', lang: 'en' },
	ru: { label: 'Русский', lang: 'ru' },
};

/**
 * Narrows a string to a course locale; throws on anything else (no silent fallback to English, so a typo in a
 * locale code fails the build instead of rendering the wrong language).
 * @param {string} code
 * @returns {Locale}
 */
export function asLocale(code) {
	if (/** @type {readonly string[]} */ (LOCALES).includes(code)) return /** @type {Locale} */ (code);
	throw new Error(`unknown course locale "${code}" (known: ${LOCALES.join(', ')})`);
}

/**
 * Course locale of a Starlight route locale (`Astro.locals.starlightRoute.locale`): undefined = the root locale.
 * @param {string | undefined} starlightLocale
 * @returns {Locale}
 */
export function fromStarlight(starlightLocale) {
	return starlightLocale === undefined || starlightLocale === 'root' ? 'en' : asLocale(starlightLocale);
}

/**
 * URL/content-id prefix of a locale: '' for the root locale, `ru/` for Russian.
 * @param {Locale} locale
 */
export function localePrefix(locale) {
	return asLocale(locale) === 'en' ? '' : `${locale}/`;
}

/**
 * Splits a leading locale segment off a content id or base-less URL path: `ru/vectors/dot-product` ->
 * { locale: 'ru', rest: 'vectors/dot-product' }; `ru` -> { locale: 'ru', rest: '' }; no prefix -> English.
 * @param {string} path content id or URL path without the base, no leading slash
 * @returns {{ locale: Locale, rest: string }}
 */
export function splitLocale(path) {
	for (const l of LOCALES) {
		if (l === 'en') continue;
		if (path === l) return { locale: l, rest: '' };
		if (path.startsWith(`${l}/`)) return { locale: l, rest: path.slice(l.length + 1) };
	}
	return { locale: 'en', rest: path };
}

/**
 * Absolute, base-prefixed URL of a page path in a locale: (`/math-course`, 'vectors/dot-product', 'ru') ->
 * `/math-course/ru/vectors/dot-product/`; an empty path is the locale's home.
 * @param {string} base site base, with or without a trailing slash
 * @param {string} path page path without base and locale, no leading slash ('' = home)
 * @param {Locale} locale
 */
export function localizedUrl(base, path, locale) {
	const p = `${localePrefix(locale)}${path.replace(/^\/+|\/+$/g, '')}`.replace(/\/$/, '');
	return `${base.replace(/\/$/, '')}/${p ? `${p}/` : ''}`;
}

/**
 * @typedef {{ title: string, short?: string, summary: string }} CourseText
 * @typedef {CourseText & { ru?: CourseText }} CourseItem a course.json module or lesson
 */

/**
 * @typedef {{ unnumbered?: boolean, part?: number, lessons: readonly unknown[] }} NumberedModuleJson
 * @typedef {{ part: number | null, ids: (string | null)[] }} ModuleNumbers
 */

/**
 * CURRICULUM.md numbering: modules are "Part P", lessons are "P.k" (k = 1-based position in the module, side branches
 * included). The first numbered module is Part 0 (Ground); each next one is the previous part + 1 unless it declares
 * `part` explicitly, which is how a module keeps its curriculum number while an earlier part is not built yet
 * (Tensors = Part 3 before Matrices = Part 2 exists). `unnumbered: true` (the `start` module) takes no number and
 * does not shift the parts after it. Parts must strictly increase, so a stale override fails the build.
 * @param {readonly NumberedModuleJson[]} modules course.json modules in course order
 * @returns {ModuleNumbers[]}
 */
export function numberCourse(modules) {
	let prev = -1;
	return modules.map((m, i) => {
		if (m.unnumbered) {
			if (m.part !== undefined) throw new Error(`course.json module ${i}: "unnumbered" and "part" exclude each other`);
			return { part: null, ids: m.lessons.map(() => null) };
		}
		const part = m.part ?? prev + 1;
		if (!Number.isInteger(part) || part <= prev) throw new Error(`course.json module ${i}: part ${part} must be an integer above ${prev}`);
		prev = part;
		return { part, ids: m.lessons.map((_, k) => `${part}.${k + 1}`) };
	});
}

/**
 * Navigation label with the curriculum number in front (`0.3 Pythagoras`, `0. Ground`); unnumbered items keep the
 * bare label. One rule for the sidebar, prev/next links and the course map.
 * @param {string | number | null} num lesson id or part number
 * @param {string} label
 */
export function numbered(num, label) {
	if (num === null) return label;
	return typeof num === 'number' ? `${num}. ${label}` : `${num} ${label}`;
}

/**
 * Localized texts of a course.json module or lesson. English = the top-level fields; other locales = the per-locale
 * object, which must exist (a missing translation is a build error, not a silent English fallback). `label` is the
 * compact navigation label: `short ?? title` (sidebar, prev/next).
 * @param {CourseItem} item
 * @param {Locale} locale
 * @returns {CourseText & { label: string }}
 */
export function courseText(item, locale) {
	const src = asLocale(locale) === 'en' ? item : item[locale];
	if (!src) throw new Error(`course.json: "${item.title}" has no "${locale}" texts`);
	const { title, short, summary } = src;
	if (!title || !summary) throw new Error(`course.json: "${item.title}" (${locale}) needs a title and a summary`);
	return { title, ...(short ? { short } : {}), summary, label: short ?? title };
}
