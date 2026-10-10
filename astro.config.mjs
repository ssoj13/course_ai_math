// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import react from '@astrojs/react';
import { readFileSync } from 'node:fs';
import { satteri } from '@astrojs/markdown-satteri';
import courseLinks from './src/lib/remark-lesson-links.mjs';
import katexMath from './src/lib/satteri-math.mjs';

const BASE = '/math-course';

// Course structure comes from one manifest (see src/lib/course.ts); numbering rules must match it.
/** @type {{ modules: import('./src/lib/course').ModuleJson[] }} */
const course = JSON.parse(readFileSync(new URL('./src/data/course.json', import.meta.url), 'utf8'));

let lessonNum = 0;
/**
 * One sidebar group per module with the lessons numbered by the GLOBAL lesson number. Long titles use the lesson's
 * optional `short` label so they fit the sidebar (same rule as `Lesson.label` in course.ts); side-branch lessons get a
 * `↳` prefix. Every group stays expanded so the whole course is visible at a glance.
 */
const moduleGroups = course.modules.map((m, i) => ({
	label: `${i + 1}. ${m.title}${m.optional ? ' (optional)' : ''}`,
	items: m.lessons.map((l) => ({
		label: `${l.branch ? '↳ ' : ''}${++lessonNum}. ${l.short ?? l.title}`,
		slug: `${m.slug}/${l.slug}`,
	})),
}));

// Published as a GitHub Pages project site: https://ssoj13.github.io/math-course/
export default defineConfig({
	site: 'https://ssoj13.github.io',
	base: BASE,
	markdown: { processor: satteri({ features: { math: true }, mdastPlugins: [courseLinks({ base: BASE }), katexMath()] }) },
	vite: {
		// Vite workers are ES modules: the default iife format cannot code-split the Pyodide / elk workers.
		worker: { format: 'es' },
		// The scene chunk (three + r3f + drei, ~978 kB raw / 258 kB gz) cannot shrink: r3f registers all of THREE
		// (`import *`), so three core is not tree-shaken. Task 6b added drei's Line (three-stdlib Line2/LineMaterial,
		// ~32 kB) for pixel-stable lines; Task 6c's angle/area/label primitives and demo added ~9 kB raw (no new deps).
		// The limit is set just above it so any further growth warns; the real gate is the per-page gzip budget of
		// scripts/size-report.ts (Task 11, viz core <= 300 kB gz). The chunk loads only on scene pages.
		build: { chunkSizeWarningLimit: 985 },
	},
	integrations: [
		starlight({
			title: 'Math for AI',
			description:
				'An interactive, visualization-first course: from the law of cosines and the dot product to tensors, calculus, statistics and attention.',
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/ssoj13/math-course' }],
			editLink: { baseUrl: 'https://github.com/ssoj13/math-course/edit/main/' },
			customCss: ['katex/dist/katex.min.css', './src/styles/lab.css'],
			disable404Route: true, // replaced by src/pages/404.astro
			components: {
				Sidebar: './src/components/overrides/Sidebar.astro',
				PageTitle: './src/components/overrides/PageTitle.astro',
				Footer: './src/components/overrides/Footer.astro',
			},
			routeMiddleware: './src/lib/route-links.ts',
			lastUpdated: true,
			sidebar: [
				{ label: 'Home', link: '/' },
				{ label: 'Course roadmap', slug: 'course/roadmap' },
				...moduleGroups,
				{ label: 'Reference', items: [{ label: 'Glossary', slug: 'glossary' }] },
			],
		}),
		react(),
	],
});
