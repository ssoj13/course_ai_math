/**
 * Token layer: every palette role is defined in lab.css as a literal #rrggbb (WebGL needs concrete colors), exactly
 * once (the viewport is always dark, Ruling R), and the calibrated colors meet the contrast rules of
 * docs/design/visual-language.md: strokes >= 3:1 on both ends of the viewport gradient, chrome text >= 4.5:1.
 */
import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { contrast, readPalette, ROLE_VARS, STROKE_ROLES, STYLE, type Role } from '../../src/viz/palette';

const css = readFileSync(new URL('../../src/styles/lab.css', import.meta.url), 'utf8');
const defs = [...css.matchAll(/(--viz-[a-z0-9-]+)\s*:\s*([^;]+);/g)].map((m) => [m[1], m[2].trim()] as const);
const map = new Map(defs);
const pal = () => readPalette((n) => map.get(n) ?? '');

test('lab.css defines every role var once, as a literal #rrggbb', () => {
	const p = pal();
	for (const [role, name] of Object.entries(ROLE_VARS) as [Role, string][]) {
		expect(defs.filter(([n]) => n === name).length).toBe(1);
		expect(p[role]).toMatch(/^#[0-9a-f]{6}$/i);
	}
});

test('visual-language.md values (spec table)', () => {
	const p = pal();
	const lc = (xs: string[]) => xs.map((c) => c.toLowerCase());
	expect(lc([p.axisX, p.axisY, p.axisZ])).toEqual(['#ff3352', '#8bdc00', '#2890ff']);
	expect(lc([p.vecA, p.vecB, p.vecC, p.light, p.hot])).toEqual(['#56b4e9', '#e06bc8', '#3dd6b0', '#ffe27a', '#ffaa40']);
	expect(lc([p.bgTop, p.bgBottom, p.chrome, p.chrome2, p.field, p.fieldHover])).toEqual(['#3a3a3a', '#282828', '#2b2b2b', '#232323', '#545454', '#656565']);
	expect(lc([p.grid, p.gridMinor, p.ink, p.muted])).toEqual(['#4a4a4a', '#444444', '#e6e6e6', '#9a9a9a']);
});

test('contrast: WCAG ratio', () => {
	expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 6);
	expect(contrast('#777777', '#777777')).toBe(1);
	expect(contrast('#ffffff', '#000000')).toBe(contrast('#000000', '#ffffff'));
	expect(() => contrast('red', '#000000')).toThrow();
});

test('every stroke role is >= 3:1 on both ends of the viewport gradient', () => {
	const p = pal();
	for (const r of STROKE_ROLES) {
		expect({ r, top: contrast(p[r], p.bgTop) >= 3, bottom: contrast(p[r], p.bgBottom) >= 3 }).toEqual({ r, top: true, bottom: true });
	}
});

test('chrome text (ink, muted) is >= 4.5:1 on chrome and chrome-2; field text (ink) >= 4.5:1 on field and field-hover', () => {
	const p = pal();
	for (const t of ['ink', 'muted'] as const) for (const bg of ['chrome', 'chrome2'] as const) expect(contrast(p[t], p[bg])).toBeGreaterThanOrEqual(4.5);
	for (const bg of ['field', 'fieldHover'] as const) expect(contrast(p.ink, p[bg])).toBeGreaterThanOrEqual(4.5);
});

test('grid: minor lines visible but quiet on both ends of the gradient, majors stronger than minors', () => {
	const p = pal();
	for (const bg of [p.bgTop, p.bgBottom]) expect(contrast(p.gridMinor, bg)).toBeGreaterThanOrEqual(1.15);
	expect(contrast(p.gridMinor, p.bgTop)).toBeLessThan(1.3);
	expect(contrast(p.grid, p.bgTop)).toBeGreaterThan(contrast(p.gridMinor, p.bgTop));
});

test('style tokens: the stroke recipe of visual-language.md', () => {
	expect(STYLE.core).toBe(2.5);
	expect(STYLE.halo).toBe(9);
	expect(STYLE.haloA).toBe(0.14);
	expect(STYLE.hotPad).toBe(2);
	expect(STYLE.hotHaloA).toBe(0.3);
	expect(STYLE.fillA).toBe(0.18);
	expect(STYLE.edge).toBe(1.5);
	expect(STYLE.hatchPx).toBe(6);
	expect(STYLE.sectorA).toBe(0.1);
	expect([STYLE.pointR * 2, STYLE.rim]).toEqual([6, 1.5]);
});

/** `hsl(h, s%, l%)` -> `#rrggbb` (Starlight writes its grays as hsl). */
function hslHex(v: string): string {
	const m = /^hsl\((\d+(?:\.\d+)?),\s*(\d+(?:\.\d+)?)%,\s*(\d+(?:\.\d+)?)%\)$/.exec(v.trim());
	if (!m) throw new Error(`not an hsl() color: ${v}`);
	const [h, s, l] = [Number(m[1]), Number(m[2]) / 100, Number(m[3]) / 100];
	const f = (n: number) => {
		const k = (n + h / 30) % 12;
		const c = l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k - 3, 9 - k, 1));
		return Math.round(c * 255).toString(16).padStart(2, '0');
	};
	return `#${f(0)}${f(8)}${f(4)}`;
}

test('page-side N-panel text (FieldCell labels, read-only values) is >= 4.5:1 on the page and on lab cards in both themes', () => {
	// The engine routes label/read-only color through --lab-np-ink; a FieldCell sets it to the page body text.
	expect(css).toMatch(/\.lab-np-label \{[^}]*color: var\(--lab-np-ink, var\(--viz-ink\)\);/);
	expect(css).toMatch(/\.lab-np-ro \{[^}]*color: var\(--lab-np-ink, var\(--viz-ink\)\);/);
	expect(css).toMatch(/\.lab-field-cell \{ --lab-np-ink: var\(--sl-color-text\);/);
	// Starlight's own theme values: --sl-color-text = gray-2, page = black, lab card surface = gray-6 (both themes).
	// props.css is not in the package exports; markdown.css is, and lives next to it.
	const sl = readFileSync(join(dirname(Bun.resolveSync('@astrojs/starlight/style/markdown.css', import.meta.dir)), 'props.css'), 'utf8');
	const dark = sl.slice(0, sl.indexOf(":root[data-theme='light']")), light = sl.slice(sl.indexOf(":root[data-theme='light']"));
	expect(dark).toMatch(/--sl-color-text: var\(--sl-color-gray-2\);/);
	expect(light).not.toMatch(/--sl-color-text:/);
	expect(css).toMatch(/--lab-surface: var\(--sl-color-gray-6\);/);
	for (const block of [dark, light]) {
		const v = (name: string) => hslHex(new RegExp(`${name}: (hsl\\([^)]*\\))`).exec(block)![1]);
		for (const bg of ['--sl-color-black', '--sl-color-gray-6']) expect(contrast(v('--sl-color-gray-2'), v(bg))).toBeGreaterThanOrEqual(4.5);
	}
});
