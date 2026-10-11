import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { readPalette, type Palette } from '../../src/viz/palette';

// Colour-vision-deficiency check for the scene palette: roles that must be told apart in the same scene keep a
// minimum CIE76 distance (ΔE) under full protanopia and deuteranopia (Machado, Oliveira & Fernandes 2009, severity
// 1.0, applied in linear RGB). Colour is never the only cue (sign = hatch, ids = labels), but the vectors a and b and
// the hot outline must still read as different strokes for a colour-blind reader.

const css = readFileSync(new URL('../../src/styles/lab.css', import.meta.url), 'utf8');
const vars = new Map([...css.matchAll(/(--viz-[a-z0-9-]+):\s*(#[0-9a-f]{6})/gi)].map((m) => [m[1], m[2]]));
const pal: Palette = readPalette((name) => vars.get(name) ?? '');

type M3 = readonly number[];
const NORMAL: M3 = [1, 0, 0, 0, 1, 0, 0, 0, 1];
const PROTAN: M3 = [0.152286, 1.052583, -0.204868, 0.114503, 0.786281, 0.099216, -0.003882, -0.048116, 1.051998];
const DEUTAN: M3 = [0.367322, 0.860646, -0.227968, 0.280085, 0.672501, 0.047413, -0.01182, 0.04294, 0.968881];
const VISIONS = { normal: NORMAL, protan: PROTAN, deutan: DEUTAN } as const;

/** sRGB hex -> CIELAB (D65) as seen through the CVD matrix `m`. */
export function labUnder(hex: string, m: M3): [number, number, number] {
	const lin = [1, 3, 5].map((i) => {
		const c = parseInt(hex.slice(i, i + 2), 16) / 255;
		return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
	});
	const [r, g, b] = [0, 1, 2].map((row) =>
		Math.min(1, Math.max(0, m[3 * row] * lin[0] + m[3 * row + 1] * lin[1] + m[3 * row + 2] * lin[2])),
	);
	const f = (v: number) => (v > 216 / 24389 ? Math.cbrt(v) : (v * (24389 / 27)) / 116 + 16 / 116);
	const x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047);
	const y = f(0.2126729 * r + 0.7151522 * g + 0.072175 * b);
	const z = f((0.0193339 * r + 0.119192 * g + 0.9503041 * b) / 1.08883);
	return [116 * y - 16, 500 * (x - y), 200 * (y - z)];
}

const deltaE = (p: number[], q: number[]) => Math.hypot(p[0] - q[0], p[1] - q[1], p[2] - q[2]);

// Roles that appear together in one scene and must stay distinguishable. The threshold is the same for every
// group: ΔE 15 is a clearly visible difference on a dark viewport at stroke widths of 2.5 px.
const GROUPS: readonly (readonly (keyof Palette)[])[] = [
	['vecA', 'vecB', 'vecC', 'hot', 'light'],
	['axisX', 'axisY', 'axisZ', 'vecA'],
];
const MIN_DE = 15;

for (const [vision, m] of Object.entries(VISIONS)) {
	test(`scene palette pairs stay apart under ${vision} vision (ΔE >= ${MIN_DE})`, () => {
		const failures: string[] = [];
		for (const group of GROUPS) {
			for (let i = 0; i < group.length; i++) {
				for (let j = i + 1; j < group.length; j++) {
					const d = deltaE(labUnder(pal[group[i]], m), labUnder(pal[group[j]], m));
					if (d < MIN_DE) failures.push(`${group[i]}/${group[j]} ΔE ${d.toFixed(1)}`);
				}
			}
		}
		expect(failures).toEqual([]);
	});
}
