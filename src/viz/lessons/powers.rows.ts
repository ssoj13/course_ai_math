import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive, fmt2, fmt4, fmtG, fmtAmt, fmtInt, fmtVal, fmtX, RUNGS, rungId, SNAP_A, SNAP_V, SNAP_X, SPEC, sup, type Derived } from './powers.math';

/**
 * N-panel rows of the five 0.2 scenes (storyboard §7.7-§7.11). Three-free: the MDX page passes the same rows to the
 * SceneIsland skeleton (chip reservation) and the ViewportCard. Row ids are term ids shared with the formulas and the
 * scene objects; word labels come from the `pw` dictionary, math labels (x², A^½) are the same in every language.
 */
export type Scene = 1 | 2 | 3 | 4 | 5;

type RoId = { [K in keyof Derived]: Derived[K] extends number ? K : never }[keyof Derived];

const POW_LABEL = ['x¹', 'x²', 'x³'] as const;

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	const ro = (id: RoId, label: string, group: string, f: (v: number) => string, tip: string, chars = 7): Row => ({
		id, label, kind: 'ro', group, fmt: f, chars,
		value: (v) => derive(v)[id],
		tip: { text: tip },
	});
	const xRow: Row = { id: 'x', label: 'x', kind: 'num', key: 'x', spec: SPEC.x, snap: SNAP_X, fmt: fmtX, tip: { text: t(locale, 'pw.tipX') } };
	if (scene === 1) {
		const tiles = t(locale, 'pw.groupTiles');
		return [
			xRow,
			{ id: 'A', label: 'A = x²', kind: 'num', key: 'A', spec: SPEC.A, snap: SNAP_A, fmt: fmtAmt, tip: { text: t(locale, 'pw.tipA') } },
			ro('whole', t(locale, 'pw.rowWhole'), tiles, fmtInt, t(locale, 'pw.tipWhole')),
			// Shown only while there is a cut area (spec: `visible: partial > 0`).
			{ ...ro('partial', t(locale, 'pw.rowPartial'), tiles, fmtAmt, t(locale, 'pw.tipPartial')), visible: (v) => derive(v).partial > 0 },
		];
	}
	if (scene === 2) {
		const count = t(locale, 'pw.groupCount');
		return [
			xRow,
			{ id: 'n', label: 'n', kind: 'num', key: 'n', spec: SPEC.n, fmt: fmtInt, tip: { text: t(locale, 'pw.tipN') } },
			ro('xn', 'xⁿ', count, fmtAmt, t(locale, 'pw.tipXn')),
			ro('wholeN', t(locale, 'pw.rowWholeN'), count, fmtInt, t(locale, 'pw.tipWholeN')),
			{ ...ro('partialN', t(locale, 'pw.rowPartialN'), count, fmtAmt, t(locale, 'pw.tipPartialN')), visible: (v) => derive(v).partialN > 0 },
			{ ...ro('layers', t(locale, 'pw.rowLayers'), count, fmtInt, t(locale, 'pw.tipLayers')), visible: (v) => derive(v).n === 3 },
		];
	}
	if (scene === 3) {
		const rungs = t(locale, 'pw.groupRungs');
		return [
			{ id: 'base', label: 'b', kind: 'num', key: 'base', spec: SPEC.base, fmt: fmtVal, tip: { text: t(locale, 'pw.tipBase') } },
			{ id: 'k', label: 'k', kind: 'num', key: 'k', spec: SPEC.k, fmt: fmtInt, tip: { text: t(locale, 'pw.tipK') } },
			// A rung's value is the answer of a later step: it appears once the ladder reached it (j >= k).
			...RUNGS.map((j): Row => ({
				...ro(rungId(j) as RoId, 'b' + sup(j), rungs, fmtVal, t(locale, j < 0 ? 'pw.tipRungNeg' : j === 0 ? 'pw.tipRungZero' : 'pw.tipRung')),
				visible: (v) => j >= (v.k as number),
			})),
		];
	}
	if (scene === 4) {
		const side = t(locale, 'pw.groupSide');
		const edge = t(locale, 'pw.groupEdge');
		const is3 = (v: Parameters<NonNullable<Row['visible']>>[0]) => v.d3 === 1;
		return [
			{ id: 'd3', label: '3D', kind: 'bool', key: 'd3', tip: { text: t(locale, 'pw.tipD3') } },
			{ id: 'A', label: 'A', kind: 'num', key: 'A', spec: SPEC.A, snap: SNAP_A, fmt: fmtAmt, visible: (v) => !is3(v), tip: { text: t(locale, 'pw.tipA4') } },
			{ ...ro('sqrtA', 'A^½', side, fmt4, t(locale, 'pw.tipSqrtA')), visible: (v) => !is3(v) },
			{ ...ro('check2', '(A^½)²', side, fmtAmt, t(locale, 'pw.tipCheck2')), visible: (v) => !is3(v) },
			{ id: 'V', label: 'V', kind: 'num', key: 'V', spec: SPEC.V, snap: SNAP_V, fmt: fmtAmt, visible: is3, tip: { text: t(locale, 'pw.tipV') } },
			{ ...ro('cbrtV', 'V^⅓', edge, fmt4, t(locale, 'pw.tipCbrtV')), visible: is3 },
			{ ...ro('check3', '(V^⅓)³', edge, fmtAmt, t(locale, 'pw.tipCheck3')), visible: is3 },
		];
	}
	const values = t(locale, 'pw.groupValues');
	return [
		{ id: 'g', label: 'g', kind: 'num', key: 'g', spec: SPEC.g, fmt: fmtG, tip: { text: t(locale, 'pw.tipG') } },
		{ id: 't', label: 't', kind: 'num', key: 't', spec: SPEC.t, fmt: fmt2, tip: { text: t(locale, 'pw.tipT') } },
		ro('y', 'tᵍ', values, fmt4, t(locale, 'pw.tipY')),
		ro('yinv', '(tᵍ)^(1/g)', values, fmt4, t(locale, 'pw.tipYinv'), 10),
	];
}

/** Power chip labels of 0.2-2 (x¹ x² x³), indexed by n - 1. */
export const powLabel = (n: number): string => POW_LABEL[n - 1] ?? 'xⁿ';
