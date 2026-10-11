import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive, fmt1, fmt2, fmt4, fmtInt, SNAP_A, SNAP_X, SPEC } from './squares-and-plane.math';

/**
 * N-panel rows of the three 0.G scenes (storyboard §7.3-§7.5). Three-free: the MDX page passes the same rows to the
 * SceneIsland skeleton (chip reservation) and the ViewportCard. Row ids are term ids shared with the formulas and
 * scene objects; word labels come from the `sq` dictionary, math labels (x², √A) are the same in every language.
 */
export type Scene = 1 | 2 | 3;

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	const ro = (id: 'x2' | 'whole' | 'partial' | 'px2' | 'py2' | 'sum' | 'sqrtA' | 'check', label: string, group: string, f: (v: number) => string, tip: string): Row => ({
		id, label, kind: 'ro', group, fmt: f, chars: 7,
		value: (v) => derive(v)[id],
		tip: { text: tip },
	});
	if (scene === 1) {
		const area = t(locale, 'sq.groupArea');
		return [
			{ id: 'x', label: 'x', kind: 'num', key: 'x', spec: SPEC.x, snap: SNAP_X, fmt: fmt1, tip: { text: t(locale, 'sq.tipX') } },
			ro('x2', 'x²', area, fmt2, t(locale, 'sq.tipX2')),
			ro('whole', t(locale, 'sq.rowWhole'), area, fmtInt, t(locale, 'sq.tipWhole')),
			// Shown only while there is a partial area (spec: `visible: partial > 0`).
			{ ...ro('partial', t(locale, 'sq.rowPartial'), area, fmt2, t(locale, 'sq.tipPartial')), visible: (v) => derive(v).partial > 0 },
		];
	}
	if (scene === 2) {
		const squares = t(locale, 'sq.groupSquares');
		return [
			{ id: 'P', label: 'P', kind: 'vec', key: 'P', dims: 2, spec: SPEC.P, fmt: fmt1, tip: { text: t(locale, 'sq.tipP') } },
			ro('px2', 'x²', squares, fmt2, t(locale, 'sq.tipPx2')),
			ro('py2', 'y²', squares, fmt2, t(locale, 'sq.tipPy2')),
			ro('sum', 'x² + y²', squares, fmt2, t(locale, 'sq.tipSum')),
		];
	}
	const side = t(locale, 'sq.groupSide');
	return [
		{ id: 'A', label: 'A', kind: 'num', key: 'A', spec: SPEC.A, snap: SNAP_A, fmt: fmt2, tip: { text: t(locale, 'sq.tipA') } },
		ro('sqrtA', '√A', side, fmt4, t(locale, 'sq.tipSqrtA')),
		ro('check', '(√A)²', side, fmt2, t(locale, 'sq.tipCheck')),
	];
}
