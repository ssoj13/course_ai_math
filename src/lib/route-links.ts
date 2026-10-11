/**
 * Starlight route middleware, two course-specific fixes to the route data:
 * 1. Frontmatter is not processed by the Markdown pipeline, so `lesson:`/`page:` links in hero actions
 *    (index.mdx, ru/index.mdx) are resolved here with the same resolver as the link plugin, in the page's locale.
 * 2. Lesson pages get prev/next links in COURSE order (course.json). The stock links follow the sidebar,
 *    which would send the first lesson back to the roadmap and the last one on to the Reference group.
 *    Labels match the sidebar (`short` label when the lesson has one) in the page's locale; untranslated Russian
 *    pages (Starlight fallback routes, English body) still link to the Russian URLs, so the reader stays in Russian.
 */
import { defineRouteMiddleware } from '@astrojs/starlight/route-data';
import { adjacent, fromStarlight, href, lessonByPath, loc, type Lesson, type Locale } from './course';
import { resolveCourseLink } from './remark-lesson-links.mjs';

const link = (locale: Locale, l?: Lesson) =>
	l &&
	({ type: 'link', label: `${l.num}. ${loc(l, locale).label}`, href: href(l.slug, undefined, locale), isCurrent: false, badge: undefined, attrs: {} } as const);

export const onRequest = defineRouteMiddleware((context) => {
	const route = context.locals.starlightRoute;
	const locale = fromStarlight(route.locale);
	const hero = route.entry.data.hero;
	if (hero?.actions.length) {
		// Copy instead of mutating: a fallback route shares its entry object with the English route, so an in-place
		// rewrite would leak one locale's URLs into the other's render.
		const actions = hero.actions.map((a) => ({
			...a,
			link: resolveCourseLink(a.link, import.meta.env.BASE_URL, locale, route.entry.id) ?? a.link,
		}));
		route.entry = { ...route.entry, data: { ...route.entry.data, hero: { ...hero, actions } } };
	}
	const lesson = lessonByPath(route.id);
	if (lesson) {
		const { prev, next } = adjacent(lesson.slug);
		route.pagination = { prev: link(locale, prev), next: link(locale, next) };
	}
});
