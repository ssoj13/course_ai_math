/**
 * N-panel rows of the einsum cards (three-free; the same arrays go to SceneIsland for the phone chip reservation).
 */
import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { mvData, MV_J, playData, SEED } from './einsum.store';
import { fmtShort } from './model';

type Vals = Parameters<typeof playData>[0];
const int = (x: number) => (Number.isFinite(x) ? String(Math.round(x)) : '–');

/** Playground (spec §3.5): seed, then what the spec gives: cells, shaft length, level of detail, the selected cell. */
export function playRows(locale: Locale): readonly Row[] {
	const of = (f: (d: Extract<ReturnType<typeof playData>, { ok: true }>['d'], v: Vals) => number) => (v: Vals) => {
		const b = playData(v);
		return b.ok ? f(b.d, v) : NaN;
	};
	return [
		{ id: 'seed', label: t(locale, 'du.seed'), kind: 'num', key: 'seed', spec: { ...SEED }, fmt: int, tip: { text: t(locale, 'du.seedTip') } },
		{ id: 'cells', label: t(locale, 'du.cells'), kind: 'ro', chars: 6, fmt: int, value: of((d) => d.L.cells.n), tip: { text: t(locale, 'du.cellsTip') } },
		{ id: 'shaft', label: t(locale, 'du.shaft'), kind: 'ro', chars: 4, fmt: int, value: of((d) => d.L.plan.sumLen), tip: { text: t(locale, 'du.shaftTip') } },
		{ id: 'lod', label: 'LOD', kind: 'ro', chars: 2, fmt: int, value: of((d) => d.L.lod), tip: { text: t(locale, 'du.lodTip') } },
		{ id: 'out', label: t(locale, 'du.selCell'), kind: 'ro', chars: 7, fmt: fmtShort, value: of((d, v) => d.out[Math.min(d.out.length - 1, Math.max(0, Math.round(v.sel as number)))]), tip: { text: t(locale, 'du.selTip') } },
	];
}

/** Matrix x vector (spec §3.1/§3.2): the column count of W, the seed and the two results. */
export function mvRows(locale: Locale): readonly Row[] {
	const y = (i: number) => (v: Vals) => {
		const b = mvData(v);
		return b.ok ? b.d.out[i] : NaN;
	};
	return [
		{ id: 'mv_j', label: 'j', kind: 'num', key: 'mv_j', spec: { ...MV_J }, fmt: int, tip: { text: t(locale, 'du.mvJTip') } },
		{ id: 'seed', label: t(locale, 'du.seed'), kind: 'num', key: 'seed', spec: { ...SEED }, fmt: int, tip: { text: t(locale, 'du.seedTip') } },
		{ id: 'c0', label: 'y₁', kind: 'ro', chars: 6, fmt: fmtShort, value: y(0), tip: { text: t(locale, 'du.yTip') } },
		{ id: 'c1', label: 'y₂', kind: 'ro', chars: 6, fmt: fmtShort, value: y(1), tip: { text: t(locale, 'du.yTip') } },
	];
}
