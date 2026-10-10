/**
 * Palette roles read from --viz-* custom properties (src/styles/lab.css).
 * Theme attribute: Starlight 0.42.6 sets `document.documentElement.dataset.theme` to 'light' | 'dark' (always present)
 * at node_modules/@astrojs/starlight/dist/components/ThemeProvider.astro:13 (and ThemeSelect.astro:52 on toggle).
 * Dark values live under `:root`, light ones under `:root[data-theme='light']`.
 * The getter is injected, so these tests stay DOM-free; the live hook is covered by palette-dom.test.tsx.
 */
import { expect, test } from 'bun:test';
import { readPalette } from '../../src/viz/palette';

test('reads every role from --viz-* custom properties', () => {
	const css: Record<string, string> = { '--viz-vec-a': ' #1f77b4 ', '--viz-vec-b': '#ff7f0e' };
	const p = readPalette((n) => css[n] ?? '#000000');
	expect(p.vecA).toBe('#1f77b4');
	expect(p.vecB).toBe('#ff7f0e');
	expect(Object.keys(p).length).toBe(13);
});

test('missing variable is an error, not a silent black', () => {
	expect(() => readPalette(() => '')).toThrow(/--viz-/);
});
