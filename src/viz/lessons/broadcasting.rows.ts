import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { aKey, bKey, model, OPS, SPEC } from './broadcasting.math';
import { fmtInt } from './tensor-road.math';

/**
 * N-panel rows of the 3.4 cards. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the card. Size rows are labelled by their axis counted from the right (-1 = last), the way the
 * broadcasting rule reads shapes; a size row shows while its axis exists.
 */
export function ruleRows(locale: Locale): readonly Row[] {
	const size = (name: 'A' | 'B', key: (k: number) => string, rank: string, k: number): Row => ({
		id: key(k), label: t(locale, 'bc.rowAxis', { name, k: -(k + 1) }), kind: 'num', key: key(k), spec: { ...SPEC.size }, snap: 1,
		fmt: fmtInt, group: name, visible: (v) => (v[rank] as number) > k, tip: { text: t(locale, 'bc.tipSize', { name, k: -(k + 1) }) },
	});
	return [
		{ id: 'op', label: t(locale, 'bc.rowOp'), kind: 'num', key: 'op', spec: { ...SPEC.op }, snap: 1, fmt: (x) => OPS[Math.round(x)] ?? '?', tip: { text: t(locale, 'bc.tipOp') } },
		{ id: 'ar', label: t(locale, 'bc.rowRank', { name: 'A' }), kind: 'num', key: 'ar', spec: { ...SPEC.ar }, snap: 1, fmt: fmtInt, group: 'A', tip: { text: t(locale, 'bc.tipRank', { name: 'A' }) } },
		...[2, 1, 0].map((k) => size('A', aKey, 'ar', k)),
		{ id: 'br', label: t(locale, 'bc.rowRank', { name: 'B' }), kind: 'num', key: 'br', spec: { ...SPEC.br }, snap: 1, fmt: fmtInt, group: 'B', tip: { text: t(locale, 'bc.tipRank', { name: 'B' }) } },
		...[2, 1, 0].map((k) => size('B', bKey, 'br', k)),
		{
			id: 'stored', label: t(locale, 'bc.rowStored'), kind: 'ro', group: t(locale, 'bc.groupMem'), chars: 3,
			value: (v) => { const m = model(v); return m.A.size + m.B.size; }, fmt: fmtInt, tip: { text: t(locale, 'bc.tipStored') },
		},
		{
			id: 'cells', label: t(locale, 'bc.rowCells'), kind: 'ro', group: t(locale, 'bc.groupMem'), chars: 3,
			value: (v) => model(v).C?.size ?? 0, fmt: fmtInt, tip: { text: t(locale, 'bc.tipCells') },
		},
	];
}

export function tintRows(locale: Locale): readonly Row[] {
	return [
		{ id: 'tint', label: t(locale, 'bc.rowTint'), kind: 'vec', key: 'tint', dims: 3, spec: { ...SPEC.tint }, snap: [0, 0.5, 1, 1.5], fmt: (x) => x.toFixed(2), tip: { text: t(locale, 'bc.tipTint') } },
	];
}
