import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive, fmt1, fmt2, fmt3, SPEC } from './length.math';

/**
 * N-panel rows of the five 1.3 cards. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard. Row ids are term ids shared with the formulas, tables and scene objects; word
 * labels come from the `ln` dictionary, math labels (|v|², v̂) are the same in every language.
 *
 * Scenes: 1 = LN-1 (2D, Σ timeline), 2 = LN-1-3D, 3 = LN-2 (normalize, 2D), 4 = LN-2-3D (unit sphere), 5 = LN-3 (8 bars).
 */
export type Scene = 1 | 2 | 3 | 4 | 5;

type Vals = Parameters<typeof derive>[0];

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	const ro = (id: string, label: string, group: string, value: (v: Vals) => number | readonly number[], f: (x: number) => string, tip: string, chars = 7): Row =>
		({ id, label, kind: 'ro', group, value, fmt: f, chars, tip: { text: tip } });
	const sum = t(locale, 'ln.groupSum');
	if (scene === 1 || scene === 2) {
		const three = scene === 2;
		const key = three ? 'v3' : 'v';
		const s = (v: Vals) => (three ? derive(v).s3 : derive(v).s);
		return [
			{ id: key, label: 'v', kind: 'vec', key, dims: three ? 3 : 2, spec: three ? SPEC.v3 : SPEC.v, snap: 0.5, fmt: fmt1, tip: { text: t(locale, 'ln.tipV') } },
			ro(three ? 's2_3' : 's2', '|v|²', sum, (v) => s(v).s2, fmt2, t(locale, 'ln.tipS2')),
			ro(three ? 'len3' : 'len', '|v|', sum, (v) => s(v).len, fmt2, t(locale, 'ln.tipLen')),
		];
	}
	if (scene === 3 || scene === 4) {
		const three = scene === 4;
		const key = three ? 'w3' : 'w';
		const n = (v: Vals) => (three ? derive(v).n3 : derive(v).n);
		const unitGroup = t(locale, 'ln.groupUnit');
		return [
			{ id: key, label: 'v', kind: 'vec', key, dims: three ? 3 : 2, spec: three ? SPEC.w3 : SPEC.w, snap: 0.5, fmt: fmt1, tip: { text: t(locale, 'ln.tipW') } },
			{ id: 'k', label: t(locale, 'ln.rowBlend'), kind: 'num', key: 'k', spec: SPEC.k, snap: [0, 0.5, 1], fmt: fmt2, tip: { text: t(locale, 'ln.tipK') } },
			ro(three ? 'nw3' : 'nw', '|v|', unitGroup, (v) => n(v).n, fmt2, t(locale, 'ln.tipNw')),
			// The 3D unit vector would not fit the 200 px panel; LN-2-3D shows it in its formula instead.
			...(three ? [] : [ro('hat', 'v̂', unitGroup, (v) => n(v).hatC, fmt3, t(locale, 'ln.tipHat'), 16)]),
			ro(three ? 'hatLen3' : 'hatLen', '|v̂|', unitGroup, (v) => n(v).hatLen, fmt3, t(locale, 'ln.tipHatLen')),
		];
	}
	return [
		ro('su2', 'Σ uᵢ²', sum, (v) => derive(v).su.s2, fmt2, t(locale, 'ln.tipSu2')),
		ro('lenu', '|u|', sum, (v) => derive(v).su.len, fmt2, t(locale, 'ln.tipLenu')),
	];
}
