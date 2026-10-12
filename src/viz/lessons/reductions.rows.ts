import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { model, OP_NAMES, SPEC } from './reductions.math';
import { fmtInt } from './tensor-road.math';

/**
 * N-panel rows of the 3.5 card. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the card. The data itself is chosen with the buttons under the card.
 */
export function reduceRows(locale: Locale): readonly Row[] {
	const read = t(locale, 'rd.groupRead');
	return [
		{ id: 'op', label: t(locale, 'rd.rowOp'), kind: 'num', key: 'op', spec: { ...SPEC.op }, snap: 1, fmt: (x) => OP_NAMES[Math.round(x)] ?? '?', tip: { text: t(locale, 'rd.tipOp') } },
		{ id: 'ax', label: t(locale, 'rd.rowAxis'), kind: 'num', key: 'ax', spec: { ...SPEC.ax }, snap: 1, fmt: fmtInt, visible: (v) => v.all !== 1, tip: { text: t(locale, 'rd.tipAxis') } },
		{ id: 'all', label: t(locale, 'rd.rowAll'), kind: 'bool', key: 'all', tip: { text: t(locale, 'rd.tipAll') } },
		{ id: 'kd', label: t(locale, 'rd.rowKeep'), kind: 'bool', key: 'kd', tip: { text: t(locale, 'rd.tipKeep') } },
		{ id: 'count', label: t(locale, 'rd.rowCount'), kind: 'ro', group: read, chars: 3, value: (v) => model(v).count[0], fmt: fmtInt, tip: { text: t(locale, 'rd.tipCount') } },
		{ id: 'cells', label: t(locale, 'rd.rowCells'), kind: 'ro', group: read, chars: 3, value: (v) => model(v).r.size, fmt: fmtInt, tip: { text: t(locale, 'rd.tipCells') } },
	];
}
