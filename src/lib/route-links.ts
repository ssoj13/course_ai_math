/**
 * Starlight route middleware, two course-specific fixes to the route data:
 * 1. Frontmatter is not processed by the Markdown pipeline, so `lesson:`/`page:` links in hero actions
 *    (index.mdx) are resolved here with the same resolver as the link plugin.
 * 2. Lesson pages get prev/next links in COURSE order (course.json). The stock links follow the sidebar,
 *    which would send the first lesson back to the roadmap and the last one on to the Reference group.
 *    Labels match the sidebar (`short` label when the lesson has one).
 */
import { defineRouteMiddleware } from '@astrojs/starlight/route-data';
import { adjacent, href, lessonByPath, type Lesson } from './course';
import { resolveCourseLink } from './remark-lesson-links.mjs';

const link = (l?: Lesson) =>
	l && ({ type: 'link', label: `${l.num}. ${l.label}`, href: href(l.slug), isCurrent: false, badge: undefined, attrs: {} } as const);

export const onRequest = defineRouteMiddleware((context) => {
	const route = context.locals.starlightRoute;
	for (const action of route.entry.data.hero?.actions ?? []) {
		action.link = resolveCourseLink(action.link, import.meta.env.BASE_URL, route.entry.id) ?? action.link;
	}
	const lesson = lessonByPath(route.id);
	if (lesson) {
		const { prev, next } = adjacent(lesson.slug);
		route.pagination = { prev: link(prev), next: link(next) };
	}
});
