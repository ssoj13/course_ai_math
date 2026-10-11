// @ts-check
import { defineConfig, fontProviders } from 'astro/config';
import starlight from '@astrojs/starlight';
import react from '@astrojs/react';
import { readFileSync } from 'node:fs';
import { satteri } from '@astrojs/markdown-satteri';
import courseLinks from './src/lib/remark-lesson-links.mjs';
import katexMath from './src/lib/satteri-math.mjs';
import { compactThreeShaders } from './src/lib/compact-glsl.mjs';
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
        // Compact runtime GLSL string whitespace while retaining every token and preprocessor newline.
        plugins: [{
            name: 'compact-three-glsl',
            transform(code, id) {
                if (!id.endsWith('/three/build/three.module.js')) return;
                return { code: compactThreeShaders(code), map: null };
            },
        }],
		// Keep the original raw warning threshold. Lazy look/probe boundaries split actual chunks;
		// token-preserving GLSL compaction reduces Three's emitted shader strings. Hidden maps
		// support exact source-byte ownership audits of shared chunks (including palette/controls).
		build: { chunkSizeWarningLimit: 1015, sourcemap: 'hidden' },
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
