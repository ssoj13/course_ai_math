import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import type { V } from '../geom';
import { RANGE2, RANGE3, derive, derive2, fmt, fmt1, fmtDeg, tri } from './cross-product.math';

/**
 * N-panel rows of the three CP scenes. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard. Row ids are term ids shared with the scene objects, formula terms and table
 * cells; word labels come from the `cp` dictionary, math labels (`|a × b|`, `θ`) are the same in every language.
 */
export type Scene = 1 | 2 | 3;

type Vals = Readonly<Record<string, number | V>>;

function ro(id: string, label: string, value: (v: Vals) => number | V, extra: Partial<Row> = {}): Row {
	return { id, label, kind: 'ro', chars: 7, fmt, value, tip: { text: label }, ...extra };
}

const vec3 = (id: string, label: string, tip: string): Row => ({ id, label, kind: 'vec', key: id, dims: 3, spec: RANGE3, snap: 0.5, tip: { text: tip } });

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	if (scene === 1) {
		const result = t(locale, 'cp.groupResult');
		return [
			vec3('a', 'a', t(locale, 'cp.tipA')),
			vec3('b', 'b', t(locale, 'cp.tipB')),
			{ id: 'swap', label: t(locale, 'cp.rowSwap'), kind: 'bool', key: 'swap', tip: { text: t(locale, 'cp.tipSwap') } },
			ro('c1', 'c.x', (v) => derive(v).c[0], { group: result, tip: { text: t(locale, 'cp.tipC1'), tex: String.raw`a_y b_z - a_z b_y` } }),
			ro('c2', 'c.y', (v) => derive(v).c[1], { tip: { text: t(locale, 'cp.tipC2'), tex: String.raw`a_z b_x - a_x b_z` } }),
			ro('c3', 'c.z', (v) => derive(v).c[2], { tip: { text: t(locale, 'cp.tipC3'), tex: String.raw`a_x b_y - a_y b_x` } }),
			ro('len', '|c|', (v) => derive(v).len, { tip: { text: t(locale, 'cp.tipLen'), tex: String.raw`\lvert\mathbf{a}\times\mathbf{b}\rvert` } }),
			ro('area', t(locale, 'cp.rowArea'), (v) => derive(v).area, { group: t(locale, 'cp.groupArea'), tip: { text: t(locale, 'cp.tipArea'), tex: String.raw`\lvert\mathbf{a}\rvert\,\lvert\mathbf{b}\rvert\sin\theta` } }),
			ro('th', 'θ', (v) => derive(v).th, { fmt: fmtDeg, chars: 6, tip: { text: t(locale, 'cp.tipTh') } }),
		];
	}
	if (scene === 2) {
		return [
			{ id: 'a2', label: 'a', kind: 'vec', key: 'a2', dims: 2, spec: RANGE2, snap: 0.5, tip: { text: t(locale, 'cp.tipA') } },
			{ id: 'b2', label: 'b', kind: 'vec', key: 'b2', dims: 2, spec: RANGE2, snap: 0.5, tip: { text: t(locale, 'cp.tipB') } },
			ro('sa', t(locale, 'cp.rowSigned'), (v) => derive2(v).sa, { group: t(locale, 'cp.groupArea'), tip: { text: t(locale, 'cp.tipSigned'), tex: String.raw`a_x b_y - a_y b_x` } }),
			ro('th2', 'θ', (v) => derive2(v).th, { fmt: fmtDeg, chars: 6, tip: { text: t(locale, 'cp.tipTh2') } }),
		];
	}
	const face = t(locale, 'cp.groupFace');
	return [
		vec3('tA', 'A', t(locale, 'cp.tipVertex')),
		vec3('tB', 'B', t(locale, 'cp.tipVertex')),
		vec3('tC', 'C', t(locale, 'cp.tipVertex')),
		{ id: 'rev', label: t(locale, 'cp.rowRev'), kind: 'bool', key: 'rev', tip: { text: t(locale, 'cp.tipRev') } },
		{ id: 'cull', label: t(locale, 'cp.rowCull'), kind: 'bool', key: 'cull', tip: { text: t(locale, 'cp.tipCull') } },
		ro('N', 'N', (v) => tri(v).N ?? [NaN, NaN, NaN], { group: face, fmt: fmt1, chars: 18, tip: { text: t(locale, 'cp.tipN') } }),
		ro('triArea', t(locale, 'cp.rowTriArea'), (v) => tri(v).area, { tip: { text: t(locale, 'cp.tipTriArea'), tex: String.raw`\tfrac{1}{2}\lvert\mathbf{e}_1\times\mathbf{e}_2\rvert` } }),
		ro('NdotL', 'N·L', (v) => tri(v).ndotl, { tip: { text: t(locale, 'cp.tipNdotL') } }),
		ro('bright', 'max(0, N·L)', (v) => tri(v).bright, { tip: { text: t(locale, 'cp.tipBright') } }),
	];
}
