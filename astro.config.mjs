// @ts-check
import { defineConfig, fontProviders } from 'astro/config';
import starlight from '@astrojs/starlight';
import react from '@astrojs/react';
import { readFileSync } from 'node:fs';
import { satteri } from '@astrojs/markdown-satteri';
import courseLinks from './src/lib/remark-lesson-links.mjs';
import katexMath from './src/lib/satteri-math.mjs';
import { fontsource } from './src/lib/fonts.mjs';

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
		// The scene chunk (three + r3f + drei, ~1013 kB raw / 270 kB gz) cannot shrink: r3f registers all of THREE
		// (`import *`), so three core is not tree-shaken. Task 6b added drei's Line (three-stdlib Line2/LineMaterial,
		// ~32 kB) for pixel-stable lines; Task 6c's angle/area/label primitives and demo added ~9 kB raw; Task 6d's
		// viewport chrome (ViewportCard, N-panel, gizmo, status bar, motion) and drei's shader Grid added ~31 kB raw
		// (+10 kB gz; KaTeX for tooltips stays in the shared Formula chunk). The limit is set just above it so any
		// further growth warns; the real gate is the per-page gzip budget of scripts/size-report.ts (Task 11, viz core
		// <= 300 kB gz). The chunk loads only on scene pages.
		build: { chunkSizeWarningLimit: 1015 },
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
				Head: './src/components/overrides/Head.astro',
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
