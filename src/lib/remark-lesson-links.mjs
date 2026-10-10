/**
 * Resolves course links at build time from `src/data/course.json`.
 *   lesson:<slug>[#anchor]                      -> /<base>/<module>/<slug>/[#anchor]
 *   page:<glossary|roadmap|home>[#anchor]
 * Unknown slugs/pages throw (with the file name), so a broken link fails the build.
 * Also used by the Starlight route middleware (route-links.ts) for hero action links.
 */
import { fileURLToPath } from 'node:url';
import course from '../data/course.json' with { type: 'json' };

// A JSON import (not fs) so the file is bundled when route-links.ts runs inside the built site.

/** slug -> `<module>/<slug>` path */
const LESSON_PATHS = new Map(course.modules.flatMap((m) => m.lessons.map((l) => [l.slug, `${m.slug}/${l.slug}`])));

/** Non-lesson pages: name -> URL path below the base. */
const PAGE_PATHS = new Map([
	['glossary', 'glossary'],
	['roadmap', 'course/roadmap'],
	['home', ''],
]);

/**
 * Resolves `lesson:`/`page:` URLs; returns null for any other URL.
 * @param {string} url
 * @param {string} base site base path, e.g. `/math-course`
 * @param {string} [where] file name for error messages
 * @returns {string | null}
 */
export function resolveCourseLink(url, base, where = 'unknown file') {
	const m = /^(lesson|page):([^#]*)(#.*)?$/.exec(url);
	if (!m) return null;
	const [, scheme, name, hash = ''] = m;
	const table = scheme === 'lesson' ? LESSON_PATHS : PAGE_PATHS;
	const path = table.get(name);
	if (path === undefined) {
		throw new Error(`${where}: unknown course link "${scheme}:${name}" (known: ${[...table.keys()].join(', ')})`);
	}
	return `${base.replace(/\/$/, '')}/${path ? `${path}/` : ''}${hash}`;
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
		const next = resolveCourseLink(node.url, base, where);
		if (next) ctx.setProperty(node, 'url', next);
	};
	return { name: 'course-links', link: rewrite, definition: rewrite };
}
