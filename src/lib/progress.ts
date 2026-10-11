/**
 * Course progress: the completion record shared by <LessonDone> and the sidebar
 * override (which marks completed lessons on every page). The lesson list comes from course.json;
 * the record is keyed by lesson slug.
 */
import { href, lessons, LOCALES, type Locale } from './course';
import { t } from '../i18n/core';
import { KEYS, load, watch } from './store';

export const LESSONS = lessons;
export const LESSON_SLUGS: readonly string[] = lessons.map((l) => l.slug);

export type Progress = Record<string, boolean>;

/** Normalizes a stored/imported record: unknown keys and non-`true` values are dropped. */
export function normalize(raw: unknown): Progress {
	const progress: Progress = {};
	if (raw && typeof raw === 'object' && !Array.isArray(raw)) {
		const rec = raw as Record<string, unknown>;
		for (const slug of LESSON_SLUGS) if (rec[slug] === true) progress[slug] = true;
	}
	return progress;
}

/** Reads progress defensively: storage may hold anything (hand edits, another build of the course). */
export function readProgress(): Progress {
	return normalize(load<unknown>(KEYS.progress, {}));
}

export interface Counts {
	core: number;
	coreTotal: number;
	optional: number;
	optionalTotal: number;
}

/** Completed/total lessons, core (non-optional modules) and optional counted separately. */
export function counts(progress: Progress): Counts {
	const c: Counts = { core: 0, coreTotal: 0, optional: 0, optionalTotal: 0 };
	for (const l of lessons) {
		const done = progress[l.slug] === true ? 1 : 0;
		if (l.optional) {
			c.optionalTotal++;
			c.optional += done;
		} else {
			c.coreTotal++;
			c.core += done;
		}
	}
	return c;
}

/** "12/15 core" (+ " · 1/3 optional" when the course has optional modules) */
export function describe(progress: Progress, locale: Locale = 'en'): string {
	const c = counts(progress);
	const core = t(locale, 'progress.core', { done: c.core, total: c.coreTotal });
	return c.optionalTotal ? `${core} · ${t(locale, 'progress.optional', { done: c.optional, total: c.optionalTotal })}` : core;
}

/** Sidebar link path (base-prefixed, trailing slash) -> lesson slug. */
const SLUG_BY_PATH = new Map(LOCALES.flatMap(locale => lessons.map((l) => [href(l.slug, undefined, locale), l.slug])));

/**
 * Adds `.lab-nav-complete` to the Starlight sidebar links of completed lessons. Matching is by the
 * full base-prefixed path from the manifest.
 */
export function markSidebar(progress: Progress = readProgress()): void {
	document.querySelectorAll<HTMLAnchorElement>('#starlight__sidebar a[href]').forEach((a) => {
		const path = a.pathname.endsWith('/') ? a.pathname : `${a.pathname}/`;
		const slug = SLUG_BY_PATH.get(path);
		if (slug) a.classList.toggle('lab-nav-complete', progress[slug] === true);
	});
}

/** Marks the sidebar now and whenever progress changes (this tab or another). */
export function trackSidebar(): void {
	markSidebar();
	watch(KEYS.progress, () => markSidebar());
}
