import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive, solveIK, MAGNETS, fmt, fmtCos, fmtAngle } from './law-of-cosines.math';

/**
 * N-panel rows of the four LC scenes (spec §1.2–§1.5). Three-free: the MDX page passes the same rows to the
 * SceneIsland skeleton (chip reservation) and the ViewportCard, and the tests lock the row id lists. Labels that
 * are words come from the `lc` dictionary; math labels (`a·p`, `DB²`) are the same in every language.
 */

/** Scrub specs of the shared triangle keys (spec §1.1). */
export const SCRUB = { a: { min: .5, max: 4, step: .1 }, b: { min: .5, max: 4, step: .1 }, th: { min: 0, max: 180, step: 1 } };

type Derived = ReturnType<typeof derive>;

const deg = (x: number) => fmtAngle(x) + '°';

/** What each read-only row measures (its tooltip), as literal dictionary keys (the usage test scans for them). */
const TIPS = {
	c: 'lc.tipC', a2: 'lc.tipA2', b2: 'lc.tipB2', c2: 'lc.tipC2', strip_a: 'lc.tipStripA', strip_b: 'lc.tipStripB',
	R1: 'lc.tipR1', R2: 'lc.tipR2', R1c: 'lc.tipR1c', R2c: 'lc.tipR2c', ab: 'lc.tipAb', cos: 'lc.tipCos', corr: 'lc.tipCorr',
	p: 'lc.tipP', rise: 'lc.tipRise', rest: 'lc.tipRest', rest2: 'lc.tipRest2', rise2: 'lc.tipRise2',
	d: 'lc.tipD', cosraw: 'lc.tipCosRaw', cosc: 'lc.tipCosC', phi: 'lc.tipPhi',
} as const;
type TipId = keyof typeof TIPS & keyof Derived;

/** @param cut current cut step: from step 8 the `c·FB` row reads "= R1" (spec step 8). */
export function rowsFor(scene: 0 | 1 | 2 | 3, locale: Locale, cut = 0): readonly Row[] {
	const inputs: Row[] = (['a', 'b', 'th'] as const).map((id) => ({
		id, label: id === 'th' ? 'θ' : id, kind: 'num', key: id, spec: SCRUB[id],
		// Ctrl-snap targets: the magnet angles for θ, half units for the sides.
		snap: id === 'th' ? MAGNETS : .5,
		fmt: id === 'th' ? deg : fmt,
		tip: { text: t(locale, id === 'th' ? 'lc.angleTip' : 'lc.sideTip') },
	}));
	/** Read-only derived row, revealed by the step `start` of `timeline` (same reveal as the formula lines). */
	const ro = (id: TipId, label: string, start = 0, timeline = 'cut', group = t(locale, 'lc.areas')): Row => ({
		id, label, kind: 'ro', chars: 7, group, fmt,
		value: (v) => derive(v)[id] as number,
		visible: (_v, s) => (s[timeline] ?? 0) >= start,
		tip: { text: t(locale, TIPS[id]) },
	});
	if (scene === 0) {
		return [
			...inputs, { ...ro('c', 'c'), group: undefined },
			ro('a2', 'a²'), ro('b2', 'b²'), ro('strip_a', 'a·p', 2), ro('strip_b', 'b·q', 3),
			ro('R1', 'a·DB', 4), ro('R2', 'b·EA', 4),
			{ ...ro('R1c', 'c·FB', 5), chars: 13, fmt: (x) => fmt(x) + (cut >= 8 ? ' = R1' : '') },
			ro('R2c', 'c·AF', 5), ro('c2', 'c²'), ro('ab', 'a·b', 11),
		];
	}
	if (scene === 1) {
		return [
			...inputs, { ...ro('cos', 'cos θ'), group: undefined, fmt: fmtCos }, { ...ro('c', 'c'), group: undefined },
			ro('a2', 'a²'), ro('b2', 'b²'), ro('corr', '−2ab cos θ'), ro('c2', 'c²'),
		];
	}
	if (scene === 2) {
		const pieces = [['p', 'b cos θ', 2], ['rise', 'b sin θ', 2], ['rest', 'a − b cos θ', 3], ['rest2', 'DB²', 4], ['rise2', 'AD²', 4], ['c2', 'c²', 4]] as const;
		return [...inputs, ...pieces.map(([id, label, start]) => ro(id, label, start, 'proof', t(locale, 'lc.pieces')))];
	}
	const solveGroup = t(locale, 'lc.solve');
	const solve = (id: 'd' | 'cosraw' | 'cosc' | 'phi', label: string): Row => ({
		id, label, kind: 'ro', chars: 8, group: solveGroup,
		value: (v) => solveIK(v)[id],
		fmt: id.startsWith('cos') ? fmtCos : id === 'phi' ? deg : fmt,
		tip: { text: t(locale, TIPS[id]) },
	});
	return [
		{ id: 'ik_hand', label: t(locale, 'lc.target'), kind: 'vec', key: 'ik_hand', dims: 2, spec: { min: -6.5, max: 6.5, step: .1 } },
		{ id: 'ik_l1', label: 'L₁', kind: 'num', key: 'ik_l1', spec: { min: .5, max: 3, step: .1 } },
		{ id: 'ik_l2', label: 'L₂', kind: 'num', key: 'ik_l2', spec: { min: .5, max: 3, step: .1 } },
		{ id: 'ik_flip', label: t(locale, 'lc.elbowDown'), kind: 'bool', key: 'ik_flip' },
		solve('d', 'd'), solve('cosraw', t(locale, 'lc.cosRaw')), solve('cosc', t(locale, 'lc.clamped')), solve('phi', 'φ'),
		{ id: 'reach', label: t(locale, 'lc.reach'), kind: 'ro', chars: 13, group: solveGroup, fmt, value: (v) => { const ik = solveIK(v); return [ik.reachMin, ik.reachMax]; } },
	];
}
