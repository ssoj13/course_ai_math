import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { blend, combo, combo3, fmt1, fmt2, RANGE_3, RANGE_AB, RANGE_C, RANGE_L, RANGE_S, RANGE_ST, RANGE_W, scaled, sum } from './vector-arithmetic.math';

/**
 * N-panel rows of the five 1.2 scenes. Three-free: the MDX page passes the same rows to the SceneIsland skeleton
 * (chip reservation) and the ViewportCard. Row ids are term ids shared with the scene objects, formula terms and table
 * cells; word labels come from the `va` dictionary, math labels (a + b, s, |a|) are the same in every language.
 */
export type Scene = 1 | 2 | 3 | 4 | 5;

const ro = (id: string, label: string, value: Row['value'], extra: Partial<Row> = {}): Row => ({ id, label, kind: 'ro', value, fmt: fmt1, chars: 12, ...extra });
const vec = (id: string, label: string, key: string, dims: 2 | 3, spec: Row['spec'], tip: string): Row => ({ id, label, kind: 'vec', key, dims, spec, snap: 0.5, tip: { text: tip } });
const scalar = (id: string, label: string, key: string, spec: Row['spec'], tip: string, snap: number): Row => ({ id, label, kind: 'num', key, spec, snap, fmt: fmt1, tip: { text: tip } });

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	if (scene === 1) {
		const lengths = t(locale, 'va.groupLengths');
		return [
			vec('a', 'a', 'a', 2, RANGE_AB, t(locale, 'va.tipA')),
			vec('b', 'b', 'b', 2, RANGE_AB, t(locale, 'va.tipB')),
			ro('sum', 'a + b', (v) => sum(v).ab, { tip: { text: t(locale, 'va.tipSum') } }),
			ro('na', '|a|', (v) => sum(v).na, { fmt: fmt2, chars: 7, group: lengths, tip: { text: t(locale, 'va.tipLen') } }),
			ro('nb', '|b|', (v) => sum(v).nb, { fmt: fmt2, chars: 7, tip: { text: t(locale, 'va.tipLen') } }),
			ro('nsum', '|a + b|', (v) => sum(v).nab, { fmt: fmt2, chars: 7, tip: { text: t(locale, 'va.tipLenSum') } }),
		];
	}
	if (scene === 2) {
		return [
			vec('c', 'a', 'c', 2, RANGE_C, t(locale, 'va.tipA')),
			scalar('s', 's', 's', RANGE_S, t(locale, 'va.tipS'), 0.5),
			ro('sc', 's·a', (v) => scaled(v).sc, { tip: { text: t(locale, 'va.tipSc') } }),
		];
	}
	if (scene === 3) {
		return [
			vec('la', 'a', 'la', 2, RANGE_L, t(locale, 'va.tipA')),
			vec('lb', 'b', 'lb', 2, RANGE_L, t(locale, 'va.tipB')),
			scalar('ls', 's', 'ls', RANGE_ST, t(locale, 'va.tipCoef'), 0.5),
			scalar('lt', 't', 'lt', RANGE_ST, t(locale, 'va.tipCoef'), 0.5),
			ro('r', 's·a + t·b', (v) => combo(v).r, { tip: { text: t(locale, 'va.tipR') } }),
			{ id: 'paint', label: t(locale, 'va.rowPaint'), kind: 'bool', key: 'paint', tip: { text: t(locale, 'va.tipPaint') } },
		];
	}
	if (scene === 4) {
		return [
			vec('a3', 'a', 'a3', 3, RANGE_3, t(locale, 'va.tipA')),
			vec('b3', 'b', 'b3', 3, RANGE_3, t(locale, 'va.tipB')),
			vec('c3', 'c', 'c3', 3, RANGE_3, t(locale, 'va.tipC')),
			scalar('s3', 's', 's3', RANGE_ST, t(locale, 'va.tipCoef'), 0.5),
			scalar('t3', 't', 't3', RANGE_ST, t(locale, 'va.tipCoef'), 0.5),
			scalar('u3', 'u', 'u3', RANGE_ST, t(locale, 'va.tipCoef'), 0.5),
			ro('r3', 'r', (v) => combo3(v).r, { chars: 18, tip: { text: t(locale, 'va.tipR3') } }),
		];
	}
	return [
		{ ...scalar('w1', 'w₁', 'w1', RANGE_W, t(locale, 'va.tipW1'), 0.25), fmt: fmt2 },
		{ ...scalar('w2', 'w₂', 'w2', RANGE_W, t(locale, 'va.tipW2'), 0.25), fmt: fmt2 },
		ro('corner', "P'", (v) => blend(v).corner, { fmt: fmt2, chars: 14, tip: { text: t(locale, 'va.tipCorner') } }),
	];
}
