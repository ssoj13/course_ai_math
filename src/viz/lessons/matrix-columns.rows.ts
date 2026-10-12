import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import type { V } from '../geom';
import { RANGE_M, RANGE_P, RANGE_R, RANGE_X, SPEC_TR, derive1, derive2, fmt1, fmt2 } from './matrix-columns.math';

/**
 * N-panel rows of the two 2.2 scenes. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard, and the tests lock the row id lists. Row ids are term ids (hover <-> scene object
 * <-> formula term <-> table cell); word labels come from the `mc` dictionary, math labels are the same everywhere.
 */
export type Scene = 1 | 2;

type Vals = Readonly<Record<string, number | V>>;

function ro(id: string, label: string, value: (v: Vals) => number | V, extra: Partial<Row> = {}): Row {
	return { id, label, kind: 'ro', chars: 7, fmt: fmt2, value, ...extra };
}

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	if (scene === 1) {
		return [
			{ id: 'm1', label: 'col₁', kind: 'vec', key: 'm1', dims: 2, spec: RANGE_M, snap: 0.5, tip: { text: t(locale, 'mc.tipM1') } },
			{ id: 'm2', label: 'col₂', kind: 'vec', key: 'm2', dims: 2, spec: RANGE_M, snap: 0.5, tip: { text: t(locale, 'mc.tipM2') } },
			{ id: 'x', label: 'x', kind: 'vec', key: 'x', dims: 2, spec: RANGE_X, snap: 0.5, tip: { text: t(locale, 'mc.tipX') } },
			ro('y1', 'row₁·x', (v) => derive1(v).yRows[0], { group: t(locale, 'mc.groupRows'), fmt: fmt2, tip: { text: t(locale, 'mc.tipY1') } }),
			ro('y2', 'row₂·x', (v) => derive1(v).yRows[1], { fmt: fmt2, tip: { text: t(locale, 'mc.tipY2') } }),
			ro('y', 'Mx', (v) => derive1(v).yCols, { group: t(locale, 'mc.groupCols'), fmt: fmt1, chars: 12, tip: { text: t(locale, 'mc.tipY') } }),
		];
	}
	return [
		{ id: 'p', label: 'v', kind: 'vec', key: 'p', dims: 3, spec: RANGE_P, snap: 0.5, tip: { text: t(locale, 'mc.tipP') } },
		{ id: 'ortho', label: t(locale, 'mc.rowOrtho'), kind: 'bool', key: 'ortho', tip: { text: t(locale, 'mc.tipOrtho') } },
		...(['q1', 'q2', 'q3'] as const).map((id, i): Row => ({
			id, label: ['u₁', 'u₂', 'u₃'][i], kind: 'vec', key: id, dims: 3, spec: RANGE_R, snap: 0.5, tip: { text: t(locale, 'mc.tipQ') },
		})),
		{ id: 'tr', label: t(locale, 'mc.rowTransfer'), kind: 'num', key: 'tr', spec: SPEC_TR, snap: 0.25, fmt: fmt2, tip: { text: t(locale, 'mc.tipTr') } },
		ro('yv', 'Rv', (v) => derive2(v).y, { group: t(locale, 'mc.groupOut'), fmt: fmt1, chars: 18, tip: { text: t(locale, 'mc.tipYv') } }),
		ro('gap', t(locale, 'mc.rowGap'), (v) => derive2(v).gap, { tip: { text: t(locale, 'mc.tipGap') } }),
		ro('hand', t(locale, 'mc.rowHand'), (v) => derive2(v).hand, {
			chars: 11,
			fmt: (h) => (h > 0 ? t(locale, 'mc.handRight') : h < 0 ? t(locale, 'mc.handLeft') : t(locale, 'mc.handFlat')),
			tip: { text: t(locale, 'mc.tipHand') },
		}),
	];
}
