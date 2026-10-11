/**
 * Resolves course links at build time from `src/data/course.json`, in the locale of the page that contains them.
 *   lesson:<slug>[#anchor]                      -> /<base>/[<locale>/]<module>/<slug>/[#anchor]
 *   page:<glossary|roadmap|home>[#anchor]
 * The locale of an MDX file comes from its path: `src/content/docs/ru/...` -> ru, anything else -> en (Starlight's
 * root locale). Anchors are passed through unchanged: headings differ between languages, so a translated page uses
 * its own anchors. Unknown slugs/pages throw (with the file name), so a broken link fails the build.
 * Also used by the Starlight route middleware (route-links.ts) for hero action links.
 */
import { fileURLToPath } from 'node:url';
import course from '../data/course.json' with { type: 'json' };
import { localizedUrl, splitLocale } from './course-rules.mjs';

/** @typedef {import('./course-rules.mjs').Locale} Locale */

// A JSON import (not fs) so the file is bundled when route-links.ts runs inside the built site.

/** slug -> `<module>/<slug>` path (same rule as `Lesson.path` in course.ts) */
const LESSON_PATHS = new Map(course.modules.flatMap((m) => m.lessons.map((l) => [l.slug, `${m.slug}/${l.slug}`])));

/** Non-lesson pages: name -> URL path below the base and locale. */
const PAGE_PATHS = new Map([
	['glossary', 'glossary'],
	['roadmap', 'course/roadmap'],
	['home', ''],
]);

/**
 * Resolves `lesson:`/`page:` URLs; returns null for any other URL.
 * @param {string} url
 * @param {string} base site base path, e.g. `/math-course`
 * @param {Locale} locale locale of the page the link is on
 * @param {string} [where] file name for error messages
 * @returns {string | null}
 */
export function resolveCourseLink(url, base, locale, where = 'unknown file') {
	const m = /^(lesson|page):([^#]*)(#.*)?$/.exec(url);
	if (!m) return null;
	const [, scheme, name, hash = ''] = m;
	const table = scheme === 'lesson' ? LESSON_PATHS : PAGE_PATHS;
	const path = table.get(name);
	if (path === undefined) {
		throw new Error(`${where}: unknown course link "${scheme}:${name}" (known: ${[...table.keys()].join(', ')})`);
	}
	return `${localizedUrl(base, path, locale)}${hash}`;
}

const DOCS_DIR = '/src/content/docs/';

/**
 * Locale of a content file from its absolute path (`.../src/content/docs/ru/vectors/x.mdx` -> ru).
 * @param {string} file
 * @returns {Locale}
 */
export function fileLocale(file) {
	const p = file.replaceAll('\\', '/');
	const i = p.lastIndexOf(DOCS_DIR);
	return i < 0 ? 'en' : splitLocale(p.slice(i + DOCS_DIR.length)).locale;
}

/**
 * Sätteri mdast plugin (Astro 7's default Markdown/MDX processor; the file name keeps the "remark"
 * wording of the course docs). Plain object, so no `satteri` import is needed (it is only a
 * transitive dependency). Resolves markdown links and link definitions; JSX `href`s are not touched.
 * @param {{ base?: string }} [options]
 */
export default function courseLinks({ base = '' } = {}) {
	/** @param {{ url: string }} node @param {any} ctx */
	const rewrite = (node, ctx) => {
		const where = ctx.fileURL ? fileURLToPath(ctx.fileURL) : 'unknown file';
		const next = resolveCourseLink(node.url, base, fileLocale(where), where);
		if (next) ctx.setProperty(node, 'url', next);
	};
	return { name: 'course-links', link: rewrite, definition: rewrite };
}
