// @ts-check
import { defineConfig, fontProviders } from 'astro/config';
import starlight from '@astrojs/starlight';
import react from '@astrojs/react';
import { readFileSync } from 'node:fs';
import { satteri } from '@astrojs/markdown-satteri';
import courseLinks from './src/lib/remark-lesson-links.mjs';
import katexMath from './src/lib/satteri-math.mjs';
import { STARLIGHT_LOCALES, courseText, numberCourse, numbered } from './src/lib/course-rules.mjs';
import { fontsource } from './src/lib/fonts.mjs';

const BASE = '/course_ai_math';

// Course structure comes from one manifest (see src/lib/course.ts); numbering rules must match it.
/** @type {{ modules: import('./src/lib/course').ModuleJson[] }} */
const course = JSON.parse(readFileSync(new URL('./src/data/course.json', import.meta.url), 'utf8'));

const numbers = numberCourse(course.modules);
/**
 * One sidebar group per module, numbered like CURRICULUM.md (`numberCourse`: Part P, lessons P.k; the `start` module
 * is unnumbered). Long titles use the lesson's optional `short` label so they fit the sidebar (same rule as
 * `Lesson.label` in course.ts); side-branch lessons get a `↳` prefix. Every group stays expanded so the whole course
 * is visible at a glance.
 */
const moduleGroups = course.modules.map((m, i) => ({
	label: `${numbered(numbers[i].part, m.title)}${m.optional ? ' (optional)' : ''}`,
	translations: { ru: `${numbered(numbers[i].part, courseText(m, 'ru').title)}${m.optional ? ' (необязательно)' : ''}` },
	items: m.lessons.map((l, k) => ({
		label: `${l.branch ? '↳ ' : ''}${numbered(numbers[i].ids[k], l.short ?? l.title)}`,
		translations: { ru: `${l.branch ? '↳ ' : ''}${numbered(numbers[i].ids[k], courseText(l, 'ru').label)}` },
		slug: `${m.slug}/${l.slug}`,
	})),
}));

// Published as a GitHub Pages project site: https://ssoj13.github.io/course_ai_math/
export default defineConfig({
	site: 'https://ssoj13.github.io',
	base: BASE,
	// Self-hosted fonts (visual-language.md "Type") from the @fontsource packages, through Astro's Fonts API: local
	// files (no network), per-subset unicode-range faces and metric-matched fallbacks, emitted by <Font> in the Head
	// override. `display: 'optional'`: a face not ready within the browser's ~100 ms block period is not swapped in
	// later, so text never moves (with `swap` a late face re-wrapped the intro paragraph: CLS 0.049 measured); the
	// cached face is used from the next page on. Inter's opsz axis turns into Inter Display at heading sizes by
	// itself; only the prose face ships italics (emphasis). Families are exposed as CSS vars used by lab.css.
	fonts: [
		{
			provider: fontProviders.local(),
			name: 'Atkinson Hyperlegible Next',
			cssVariable: '--lab-font-prose',
			display: 'optional',
			fallbacks: ['system-ui', 'sans-serif'],
			options: {
				variants: fontsource('@fontsource-variable/atkinson-hyperlegible-next', 'wght.css', 'wght-italic.css'),
			},
		},
		{
			provider: fontProviders.local(),
			name: 'Inter',
			cssVariable: '--lab-font-ui',
			display: 'optional',
			fallbacks: ['system-ui', 'sans-serif'],
			options: { variants: fontsource('@fontsource-variable/inter', 'opsz.css') },
		},
		{
			provider: fontProviders.local(),
			name: 'JetBrains Mono',
			cssVariable: '--lab-font-mono',
			display: 'optional',
			fallbacks: ['ui-monospace', 'monospace'],
			options: { variants: fontsource('@fontsource-variable/jetbrains-mono', 'wght.css') },
		},
	],
	markdown: { processor: satteri({ features: { math: true }, mdastPlugins: [courseLinks({ base: BASE }), katexMath()] }) },
	vite: {
		// Vite workers are ES modules: the default iife format cannot code-split the Pyodide / elk workers.
		worker: { format: 'es' },
		// Keep the original raw warning threshold (Task 11: the real gate is the physical renderer chunk <= 300 kB gz).
		build: { chunkSizeWarningLimit: 1015 },
	},
	integrations: [
		starlight({
			title: { en: 'Math for AI', ru: 'Математика для ИИ' },
			locales: STARLIGHT_LOCALES,
			defaultLocale: 'root',
			description:
				'An interactive, visualization-first course: from the law of cosines and the dot product to tensors, calculus, statistics and attention.',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/ssoj13/course_ai_math' }],
			editLink: { baseUrl: 'https://github.com/ssoj13/course_ai_math/edit/main/' },
			customCss: ['katex/dist/katex.min.css', './src/styles/lab.css'],
			disable404Route: true, // replaced by src/pages/404.astro
			components: {
				Head: './src/components/overrides/Head.astro',
				Sidebar: './src/components/overrides/Sidebar.astro',
				PageTitle: './src/components/overrides/PageTitle.astro',
				Footer: './src/components/overrides/Footer.astro',
			},
			routeMiddleware: './src/lib/route-links.ts',
			lastUpdated: true,
			sidebar: [
				{ label: 'Home', translations: { ru: 'Главная' }, link: '/' },
				{ label: 'Course roadmap', translations: { ru: 'План курса' }, slug: 'course/roadmap' },
				...moduleGroups,
				{ label: 'Reference', translations: { ru: 'Справочник' }, items: [{ label: 'Glossary', translations: { ru: 'Словарь' }, slug: 'glossary' }] },
			],
		}),
		react(),
	],
});
