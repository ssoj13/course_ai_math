import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive, deriveEpi, fmt2, fmtInt, SPEC } from './waves-dft.math';

/**
 * N-panel rows of the four 1.9 scenes. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard. Row ids are term ids shared with the formulas and scene objects; word labels come
 * from the `wd` dictionary, math labels (k, x·r) are the same in every language.
 * Scenes: 1 = signal, ruler and their products; 2 = spectrum; 3 = rebuild; 4 = epicycles.
 */
export type Scene = 1 | 2 | 3 | 4;

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	const k: Row = { id: 'k', label: 'k', kind: 'num', key: 'k', spec: SPEC.k, fmt: fmtInt, tip: { text: t(locale, 'wd.tipK') } };
	if (scene === 1) {
		const g = t(locale, 'wd.groupDot');
		return [
			k,
			{ id: 'sine', label: 'sin', kind: 'bool', key: 'sine', tip: { text: t(locale, 'wd.tipSine') } },
			{ id: 'dot', label: 'x·r', kind: 'ro', group: g, value: (v) => derive(v).dot, fmt: fmt2, tip: { text: t(locale, 'wd.tipDot') } },
			{ id: 'rr', label: 'r·r', kind: 'ro', group: g, value: (v) => derive(v).rr, fmt: fmtInt, chars: 3, tip: { text: t(locale, 'wd.tipRR') } },
			{ id: 'coef', label: t(locale, 'wd.rowCoef'), kind: 'ro', group: g, value: (v) => derive(v).coef, fmt: fmt2, tip: { text: t(locale, 'wd.tipCoef') } },
		];
	}
	if (scene === 2) {
		const g = t(locale, 'wd.groupEnergy');
		return [
			k,
			{ id: 'amp', label: t(locale, 'wd.rowAmp'), kind: 'ro', value: (v) => derive(v).amp, fmt: fmt2, tip: { text: t(locale, 'wd.tipAmp') } },
			{ id: 'energy', label: 'Σ x²', kind: 'ro', group: g, value: (v) => derive(v).energy, fmt: fmt2, tip: { text: t(locale, 'wd.tipEnergy') } },
			{ id: 'specEnergy', label: t(locale, 'wd.rowSpecEnergy'), kind: 'ro', group: g, value: (v) => derive(v).specEnergy, fmt: fmt2, tip: { text: t(locale, 'wd.tipSpecEnergy') } },
		];
	}
	if (scene === 3) {
		return [
			{ id: 'K', label: 'K', kind: 'num', key: 'K', spec: SPEC.K, fmt: fmtInt, tip: { text: t(locale, 'wd.tipKK') } },
			{ id: 'err', label: t(locale, 'wd.rowErr'), kind: 'ro', value: (v) => derive(v).err, fmt: fmt2, tip: { text: t(locale, 'wd.tipErr') } },
		];
	}
	return [
		{ id: 'S', label: t(locale, 'wd.rowSticks'), kind: 'num', key: 'S', spec: SPEC.S, fmt: fmtInt, tip: { text: t(locale, 'wd.tipS') } },
		{ id: 'epiErr', label: t(locale, 'wd.rowErr'), kind: 'ro', value: (v) => deriveEpi(v).err, fmt: fmt2, tip: { text: t(locale, 'wd.tipEpiErr') } },
		{ id: 'rel', label: '%', kind: 'ro', value: (v) => 100 * deriveEpi(v).rel, fmt: fmt2, tip: { text: t(locale, 'wd.tipRel') } },
	];
}
