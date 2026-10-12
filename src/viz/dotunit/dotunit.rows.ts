/**
 * N-panel rows of the DotUnit cards (three-free: the MDX page passes the same rows to SceneIsland for the phone chip
 * reservation and to the ViewportCard). Row ids are term ids shared with the scene, the formula and the table.
 */
import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { COMP, dotOf, N_MAX, N_MIN } from './dotunit.store';
import { fmt } from './model';

/** The DotUnit card (spec §2.1, §2.6): n, then the numbers both faces end on. */
export function unitRows(locale: Locale): readonly Row[] {
	const ro = (id: string, label: string, value: (v: Parameters<typeof dotOf>[0]) => number, tip: string): Row =>
		({ id, label, kind: 'ro', chars: 7, fmt, value, tip: { text: tip } });
	return [
		{ id: 'n', label: 'n', kind: 'num', key: 'n', spec: { min: N_MIN, max: N_MAX, step: 1 }, fmt: (x) => String(Math.round(x)), tip: { text: t(locale, 'du.nTip') } },
		ro('len_a', '|a|', (v) => dotOf(v).lenA, t(locale, 'du.lenTip')),
		ro('shadow', t(locale, 'du.shadow'), (v) => dotOf(v).shadow, t(locale, 'du.shadowTip')),
		ro('proj', '|a|·' + t(locale, 'du.shadowShort'), (v) => dotOf(v).proj, t(locale, 'du.projTip')),
		ro('dot', 'Σ aᵢbᵢ', (v) => dotOf(v).acc.value, t(locale, 'du.dotTip')),
		// The printed arithmetic and order (spec §1.3); geometry stays fp64, the difference is a number, not a height.
		{ id: 'fp32', label: 'fp32', kind: 'bool', key: 'du_fp32', group: t(locale, 'du.machine'), tip: { text: t(locale, 'du.fp32Tip') } },
		{ id: 'pairwise', label: t(locale, 'du.pairwise'), kind: 'bool', key: 'du_pair', group: t(locale, 'du.machine'), tip: { text: t(locale, 'du.pairTip') } },
		{ id: 'err', label: t(locale, 'du.err'), kind: 'ro', chars: 9, fmt: (x) => (x === 0 ? '0' : x.toExponential(1)), value: (v) => dotOf(v).acc.err, group: t(locale, 'du.machine'), tip: { text: t(locale, 'du.errTip') } },
	];
}

/** Component fields of a and b for the DOM controls (one field per component of the current n). */
export function compRows(locale: Locale, n: number): readonly Row[] {
	return (['a', 'b'] as const).map((v) => ({
		id: v, label: v, kind: 'vec', key: 'du_' + v, dims: n, spec: { ...COMP }, snap: 0.5,
		tip: { text: t(locale, v === 'a' ? 'du.aTip' : 'du.bTip') },
	}));
}
