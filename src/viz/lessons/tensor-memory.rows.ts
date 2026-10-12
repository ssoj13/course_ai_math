import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { cacheModel, chain, fmtInt, N, selected } from './tensor-memory.math';

/**
 * N-panel rows of the 3.3 cards. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the card. The shape, the view and the reshape are chosen with the code buttons under the card.
 */
export function viewRows(locale: Locale): readonly Row[] {
	const read = t(locale, 'mem.groupRead');
	return [
		{ id: 'ord', label: t(locale, 'mem.rowOrd'), kind: 'bool', key: 'ord', tip: { text: t(locale, 'mem.tipOrd') } },
		{ id: 'cont', label: t(locale, 'mem.rowCont'), kind: 'bool', key: 'cont', tip: { text: t(locale, 'mem.tipCont') } },
		{ id: 'sel', label: t(locale, 'mem.rowSel'), kind: 'num', key: 'sel', spec: { min: 0, max: 23, step: 1 }, fmt: fmtInt, group: read, tip: { text: t(locale, 'mem.tipSel') } },
		{ id: 'addr', label: t(locale, 'mem.rowAddr'), kind: 'ro', group: read, value: (v) => selected(chain(v).res, v.sel as number).addr, fmt: fmtInt, chars: 3, tip: { text: t(locale, 'mem.tipAddr') } },
	];
}

export function cacheRows(locale: Locale): readonly Row[] {
	const run = t(locale, 'mem.groupRun');
	return [
		{ id: 'col', label: t(locale, 'mem.rowCol'), kind: 'bool', key: 'col', tip: { text: t(locale, 'mem.tipCol') } },
		{ id: 'lines', label: t(locale, 'mem.rowLines'), kind: 'num', key: 'lines', spec: { min: 1, max: N, step: 1 }, snap: 1, fmt: fmtInt, tip: { text: t(locale, 'mem.tipLines') } },
		{ id: 'step', label: t(locale, 'mem.rowStep'), kind: 'num', key: 'step', spec: { min: 0, max: N * N - 1, step: 1 }, fmt: fmtInt, tip: { text: t(locale, 'mem.tipStep') } },
		{ id: 'misses', label: t(locale, 'mem.rowMisses'), kind: 'ro', group: run, value: (v) => { const m = cacheModel(v); return m.run.misses[m.step]; }, fmt: fmtInt, chars: 3, tip: { text: t(locale, 'mem.tipMisses') } },
		{ id: 'hits', label: t(locale, 'mem.rowHits'), kind: 'ro', group: run, value: (v) => { const m = cacheModel(v); return m.step + 1 - m.run.misses[m.step]; }, fmt: fmtInt, chars: 3, tip: { text: t(locale, 'mem.tipHits') } },
	];
}
