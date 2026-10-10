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
	// Vite workers are ES modules: the default iife format cannot code-split the Pyodide / elk workers.
	vite: { worker: { format: 'es' } },
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
				PageFrame: './src/components/overrides/PageFrame.astro',
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
