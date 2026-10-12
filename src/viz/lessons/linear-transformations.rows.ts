import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import type { V } from '../geom';
import { RANGE_A, RANGE_V, SPEC_MODE, SPEC_S, SPEC_T, asMode, derive1, derive2, fmt1, fmt2 } from './linear-transformations.math';

/**
 * N-panel rows of the two 2.1 scenes. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard, and the tests lock the row id lists. Row ids are term ids (hover <-> scene object
 * <-> formula term); word labels come from the `lt` dictionary, math labels (T(e₁), v) are the same in every language.
 */
export type Scene = 1 | 2;

type Vals = Readonly<Record<string, number | V>>;

/** Read-only derived row. */
function ro(id: string, label: string, value: (v: Vals) => number | V, extra: Partial<Row> = {}): Row {
	return { id, label, kind: 'ro', chars: 7, fmt: fmt2, value, ...extra };
}

/** The three maps of LT-2 by name (the `mode` row prints the name, not 0/1/2). */
export function modeName(locale: Locale): (m: number) => string {
	return (m) => t(locale, (['lt.mode0', 'lt.mode1', 'lt.mode2'] as const)[asMode(Math.round(m))]);
}

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	if (scene === 1) {
		return [
			{ id: 'a1', label: 'T(e₁)', kind: 'vec', key: 'a1', dims: 2, spec: RANGE_A, snap: 0.5, tip: { text: t(locale, 'lt.tipA1') } },
			{ id: 'a2', label: 'T(e₂)', kind: 'vec', key: 'a2', dims: 2, spec: RANGE_A, snap: 0.5, tip: { text: t(locale, 'lt.tipA2') } },
			{ id: 'v', label: 'v', kind: 'vec', key: 'v', dims: 2, spec: RANGE_V, snap: 0.5, tip: { text: t(locale, 'lt.tipV') } },
			{ id: 't', label: t(locale, 'lt.rowMorph'), kind: 'num', key: 't', spec: SPEC_T, snap: 0.25, fmt: fmt2, tip: { text: t(locale, 'lt.tipT') } },
			ro('av', 'T(v)', (v) => derive1(v).avT, { group: t(locale, 'lt.groupImage'), fmt: fmt1, chars: 12, tip: { text: t(locale, 'lt.tipAv') } }),
			ro('area', t(locale, 'lt.rowArea'), (v) => derive1(v).detT, { tip: { text: t(locale, 'lt.tipArea'), tex: String.raw`a_{1x}a_{2y}-a_{1y}a_{2x}` } }),
		];
	}
	return [
		{ id: 'mode', label: t(locale, 'lt.rowMode'), kind: 'num', key: 'mode', spec: SPEC_MODE, fmt: modeName(locale), tip: { text: t(locale, 'lt.tipMode') } },
		{ id: 'v', label: 'v', kind: 'vec', key: 'v', dims: 2, spec: RANGE_V, snap: 0.5, tip: { text: t(locale, 'lt.tipV2') } },
		{ id: 'w', label: 'w', kind: 'vec', key: 'w', dims: 2, spec: RANGE_V, snap: 0.5, tip: { text: t(locale, 'lt.tipW') } },
		{ id: 's', label: 's', kind: 'num', key: 's', spec: SPEC_S, snap: 0.5, fmt: fmt1, tip: { text: t(locale, 'lt.tipS') } },
		ro('gapAdd', t(locale, 'lt.rowGapAdd'), (v) => derive2(v).gapAdd, { group: t(locale, 'lt.groupTests'), tip: { text: t(locale, 'lt.tipGapAdd') } }),
		ro('gapScale', t(locale, 'lt.rowGapScale'), (v) => derive2(v).gapScale, { tip: { text: t(locale, 'lt.tipGapScale') } }),
		ro('origin', 'T(0)', (v) => derive2(v).origin, { fmt: fmt1, chars: 12, tip: { text: t(locale, 'lt.tipOrigin') } }),
	];
}
