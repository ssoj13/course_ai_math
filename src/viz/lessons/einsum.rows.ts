import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { flops, lab, SEED } from './einsum.math';

/**
 * N-panel rows of the 3.6 light-box card. Three-free: the MDX page passes the same rows to the SceneIsland skeleton
 * (chip reservation) and the card. The op itself and the index lengths are edited under the card (their number and
 * names change with the op, the N-panel rows do not).
 */
type Vals = Parameters<typeof lab>[0];
const int = (x: number) => (Number.isFinite(x) ? String(Math.round(x)) : '–');
const of = (f: (b: Extract<ReturnType<typeof lab>, { ok: true }>) => number) => (v: Vals) => {
	const b = lab(v);
	return b.ok ? f(b) : NaN;
};

export function labRows(locale: Locale): readonly Row[] {
	const read = t(locale, 'es.groupRead');
	return [
		{ id: 'seed', label: t(locale, 'du.seed'), kind: 'num', key: 'seed', spec: { ...SEED }, fmt: int, tip: { text: t(locale, 'du.seedTip') } },
		{ id: 'cells', label: t(locale, 'du.cells'), kind: 'ro', group: read, chars: 4, fmt: int, value: of((b) => b.plan.cells), tip: { text: t(locale, 'es.tipCells') } },
		{ id: 'shaft', label: t(locale, 'du.shaft'), kind: 'ro', group: read, chars: 3, fmt: int, value: of((b) => b.plan.sumLen), tip: { text: t(locale, 'du.shaftTip') } },
		{ id: 'flops', label: 'FLOPs', kind: 'ro', group: read, chars: 5, fmt: int, value: of((b) => flops(b.plan)), tip: { text: t(locale, 'es.tipFlops') } },
	];
}
