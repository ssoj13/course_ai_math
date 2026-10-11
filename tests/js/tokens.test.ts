/**
 * Token layer: every palette role is defined in lab.css as a literal #rrggbb (WebGL needs concrete colors), exactly
 * once (the viewport is always dark, Ruling R), and the calibrated colors meet the contrast rules of
 * docs/design/visual-language.md: strokes >= 3:1 on both ends of the viewport gradient, chrome text >= 4.5:1.
 */
import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
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
	expect(lc([p.vecA, p.vecB, p.vecC, p.light, p.hot])).toEqual(['#56b4e9', '#e07bd0', '#3dd6b0', '#ffe27a', '#ffaa40']);
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
