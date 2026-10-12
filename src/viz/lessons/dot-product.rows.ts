import { fixed } from '../fmt';
import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import type { V } from '../geom';
import { RANGE2, RANGE3, derive, derive3, lamp, fmt, fmtCos, fmtDeg } from './dot-product.math';

/**
 * N-panel rows of the four DP scenes (spec §2.2–§2.5) and the three-faces strip cells of DP-2. Three-free: the MDX
 * page passes the same rows to the SceneIsland skeleton (chip reservation) and the ViewportCard, and the tests lock
 * the row id lists. Row ids are term ids (hover <-> scene object <-> formula term <-> table cell); labels that are words
 * come from the `dp` dictionary, math labels (`|a|`, `Σaᵢbᵢ`) are the same in every language.
 */

export type Scene = 1 | 2 | 3 | 4;

/** Unit vectors and the probe point at one decimal: "(−1.1, −0.6, −0.8)" fits the 200 px sidebar (18 characters). */
const fmt1 = (x: number) => (Number.isFinite(x) ? fixed(x, 1) : '–');

/** Read-only derived row; `start` is the DP-1 step that reveals it (same reveal as its formula term). */
function ro(id: string, label: string, value: (v: Readonly<Record<string, number | V>>) => number, extra: Partial<Row> = {}): Row {
	return { id, label, kind: 'ro', chars: 7, fmt, value, tip: { text: label }, ...extra };
}

/** Glyph values must be finite: an undefined face (a zero arrow) draws its glyph at zero while the value reads a dash. */
const finite = (x: number) => (Number.isFinite(x) ? x : 0);
const products = (v: Readonly<Record<string, number | V>>) => { const z = derive(v); return [z.p1, z.p2]; };
const rectGlyph = (v: Readonly<Record<string, number | V>>) => { const z = derive(v); return [finite(z.shadow), z.na]; };

const fromStep = (start: number) => ({ visible: (_v: unknown, s: Readonly<Record<string, number>>) => (s.derive ?? 0) >= start });

/** The two 2D input rows, shared by DP-1 and DP-2 (one range for one key, controller ruling 3). */
function inputs2(locale: Locale): Row[] {
	return (['a', 'b'] as const).map((id) => ({
		id, label: id, kind: 'vec', key: id, dims: 2, spec: RANGE2, snap: 0.5,
		tip: { text: t(locale, id === 'a' ? 'dp.aTip' : 'dp.bTip'), tex: id === 'a' ? String.raw`\mathbf{a} = (a_1, a_2)` : String.raw`\mathbf{b} = (b_1, b_2)` },
	}));
}

/** @param raw DP-4 only: the "show raw N·L" state renames the brightness row (spec DP-4 N-panel). */
export function rowsFor(scene: Scene, locale: Locale, raw = 0): readonly Row[] {
	if (scene === 1) {
		const group = t(locale, 'dp.oneNumber');
		return [
			...inputs2(locale),
			ro('na', '|a|', (v) => derive(v).na),
			ro('nb', '|b|', (v) => derive(v).nb),
			ro('th', 'θ', (v) => derive(v).th, { fmt: fmtDeg, chars: 6, tip: { text: t(locale, 'dp.thTip') } }),
			ro('dot', 'Σaᵢbᵢ', (v) => derive(v).dot, { group, ...fromStep(4), tip: { text: t(locale, 'dp.dotTip'), tex: String.raw`\sum_i a_i b_i` } }),
			ro('rhs', '|a||b| cos θ', (v) => derive(v).rhs, { group, ...fromStep(7), tip: { text: t(locale, 'dp.rhsTip'), tex: String.raw`\lvert\mathbf{a}\rvert\,\lvert\mathbf{b}\rvert\cos\theta` } }),
			ro('rect', t(locale, 'dp.rectLabel'), (v) => derive(v).rect, { group, ...fromStep(8), tip: { text: t(locale, 'dp.rectTip') } }),
		];
	}
	if (scene === 2) {
		const faces = t(locale, 'dp.threeFaces');
		return [
			...inputs2(locale),
			ro('na', '|a|', (v) => derive(v).na),
			ro('nb', '|b|', (v) => derive(v).nb),
			ro('shadow', t(locale, 'dp.shadow'), (v) => derive(v).shadow, { tip: { text: t(locale, 'dp.shadowTip'), tex: String.raw`\lvert\mathbf{b}\rvert\cos\theta` } }),
			ro('cosT', 'cos θ', (v) => derive(v).cosT, { fmt: fmtCos }),
			ro('th', 'θ', (v) => derive(v).th, { fmt: fmtDeg, chars: 6, tip: { text: t(locale, 'dp.thTip') } }),
			ro('dot', 'Σaᵢbᵢ', (v) => derive(v).dot, { group: faces, tip: { text: t(locale, 'dp.dotTip'), tex: String.raw`\sum_i a_i b_i` } }),
			ro('rect', t(locale, 'dp.rectLabel'), (v) => derive(v).rect, { group: faces, tip: { text: t(locale, 'dp.rectTip') } }),
			ro('bright', 'max(0,\u00a0cos\u00a0θ)', (v) => derive(v).bright, { group: faces, fmt: fmtCos, tip: { text: t(locale, 'dp.brightTip') } }),
			ro('aa', 'a·a', (v) => derive(v).aa, { group: t(locale, 'dp.check'), tip: { text: t(locale, 'dp.aaTip') } }),
		];
	}
	if (scene === 3) {
		const group = t(locale, 'dp.oneNumber');
		return [
			...(['a3', 'b3'] as const).map((id): Row => ({
				id, label: id === 'a3' ? 'a' : 'b', kind: 'vec', key: id, dims: 3, spec: RANGE3, snap: 0.5,
				tip: { text: t(locale, id === 'a3' ? 'dp.aTip' : 'dp.bTip') },
			})),
			ro('na3', '|a|', (v) => derive3(v).na),
			ro('nb3', '|b|', (v) => derive3(v).nb),
			ro('th3', 'θ', (v) => derive3(v).th, { fmt: fmtDeg, chars: 6, tip: { text: t(locale, 'dp.thTip') } }),
			ro('dot3', 'Σaᵢbᵢ', (v) => derive3(v).dot, { group, tip: { text: t(locale, 'dp.dotTip'), tex: String.raw`\sum_i a_i b_i` } }),
			ro('rhs3', '|a||b| cos θ', (v) => derive3(v).rhs, { group, tip: { text: t(locale, 'dp.rhsTip') } }),
			ro('rect3', t(locale, 'dp.rectLabel'), (v) => derive3(v).rect, { group, tip: { text: t(locale, 'dp.rectTip') } }),
		];
	}
	const probe = t(locale, 'dp.probe');
	return [
		{ id: 'light', label: t(locale, 'dp.light'), kind: 'vec', key: 'nl_light', dims: 3, spec: { min: -4, max: 4, step: 0.1 }, snap: 0.5, tip: { text: t(locale, 'dp.lightTip') } },
		{ id: 'P', label: 'P', kind: 'ro', value: (v) => v.nl_pt as V, fmt: fmt1, chars: 18, tip: { text: t(locale, 'dp.pTip') } },
		{ id: 'nl_raw', label: t(locale, 'dp.rawRow'), kind: 'bool', key: 'nl_raw', tip: { text: t(locale, 'dp.rawTip') } },
		{ id: 'N', label: 'N', kind: 'ro', value: (v) => lamp(v).N, fmt: fmt1, chars: 18, group: probe, tip: { text: t(locale, 'dp.nTip') } },
		{ id: 'L', label: 'L', kind: 'ro', value: (v) => lamp(v).L, fmt: fmt1, chars: 18, tip: { text: t(locale, 'dp.lTip') } },
		ro('NdotL', 'N·L', (v) => lamp(v).ndotl, { fmt: fmtCos, tip: { text: t(locale, 'dp.ndotlTip'), tex: String.raw`\mathbf{N}\cdot\mathbf{L}` } }),
		ro('thNL', 'θ', (v) => lamp(v).thNL, { fmt: fmtDeg, chars: 6, tip: { text: t(locale, 'dp.thNLTip') } }),
		ro('brightNL', t(locale, raw ? 'dp.rawNL' : 'dp.brightness'), (v) => (v.nl_raw ? lamp(v).ndotl : lamp(v).brightNL), { fmt: fmtCos, glyph: 'disc', glyphValue: (v) => lamp(v).brightNL, tip: { text: t(locale, 'dp.brightNLTip') } }),
	];
}

/**
 * DP-2's three-faces strip (spec DP-2): the same number as a sum of products, an area and a brightness, as large field
 * cells with glyphs.
 */
export function facesFor(locale: Locale): { row: Row; caption: string }[] {
	return [
		{ row: { id: 'sum', label: 'Σaᵢbᵢ', kind: 'ro', chars: 7, fmt, glyph: 'sum', value: (v) => derive(v).dot, glyphValue: products }, caption: t(locale, 'dp.faceSum') },
		{ row: { id: 'rect', label: '|a|·(b·â)', kind: 'ro', chars: 7, fmt, glyph: 'rect', value: (v) => derive(v).rect, glyphValue: rectGlyph }, caption: t(locale, 'dp.faceRect') },
		{ row: { id: 'bright', label: 'max(0,\u00a0cos\u00a0θ)', kind: 'ro', chars: 7, fmt: fmtCos, glyph: 'disc', value: (v) => derive(v).bright, glyphValue: (v) => finite(derive(v).bright) }, caption: t(locale, 'dp.faceBright') },
	];
}
