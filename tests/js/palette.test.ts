/**
 * Palette roles read from --viz-* custom properties (src/styles/lab.css; always dark, Ruling R).
 * Theme attribute: Starlight 0.42.6 sets `document.documentElement.dataset.theme` to 'light' | 'dark' (always present)
 * at node_modules/@astrojs/starlight/dist/components/ThemeProvider.astro:13 (and ThemeSelect.astro:52 on toggle); the
 * palette still re-reads on a flip (palette-dom.test.tsx).
 * The getter is injected, so these tests stay DOM-free.
 */
import { expect, test } from 'bun:test';
import { fadeAlphas, readPalette, STYLE } from '../../src/viz/palette';

test('reads every role from --viz-* custom properties', () => {
	const css: Record<string, string> = { '--viz-vec-a': ' #1f77b4 ', '--viz-vec-b': '#ff7f0e' };
	const p = readPalette((n) => css[n] ?? '#000000');
	expect(p.vecA).toBe('#1f77b4');
	expect(p.vecB).toBe('#ff7f0e');
	expect(Object.keys(p).length).toBe(18);
});

test('missing variable is an error, not a silent black', () => {
	expect(() => readPalette(() => '')).toThrow(/--viz-/);
});

test('fadeAlphas scales core, halo and outline alike; 1 is the plain recipe', () => {
	expect(fadeAlphas(1, false)).toEqual({ core: 1, halo: STYLE.haloA, outline: 1 });
	expect(fadeAlphas(1, true).halo).toBe(STYLE.hotHaloA);
	const f = fadeAlphas(0.35, false);
	expect(f.core).toBe(0.35);
	expect(f.halo).toBeCloseTo(STYLE.haloA * 0.35, 12);
	expect(f.outline).toBe(0.35);
	expect(() => fadeAlphas(1.2, false)).toThrow();
	expect(() => fadeAlphas(Number.NaN, false)).toThrow();
});
