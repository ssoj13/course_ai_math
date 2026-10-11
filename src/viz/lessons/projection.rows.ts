import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive, fmt1, fmt2, fmtDeg, SPEC } from './projection.math';

/**
 * N-panel rows of the four 1.5 cards. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard. Row ids are term ids shared with the formulas, tables and scene objects; word
 * labels come from the `pj` dictionary, math labels are the same in every language.
 *
 * Scenes: 1 = PJ-1 (signed shadow), 2 = PJ-2 (projection, remainder, tiles), 3 = PJ-3 (coordinates are shadows),
 * 4 = PJ-4 (3D: shadow on a plane).
 */
export type Scene = 1 | 2 | 3 | 4;

type Vals = Parameters<typeof derive>[0];

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	const ro = (id: string, label: string, group: string, value: (v: Vals) => number | readonly number[], f: (x: number) => string, tip: string, chars = 7): Row =>
		({ id, label, kind: 'ro', group, value, fmt: f, chars, tip: { text: tip } });
	const vec2 = (id: 'a' | 'b' | 'c', tip: string): Row =>
		// PJ-3's arrow is its own key `c` (so it does not move with PJ-1/PJ-2) but the reader knows it as a.
		({ id, label: id === 'c' ? 'a' : id, kind: 'vec', key: id, dims: 2, spec: SPEC[id], snap: 0.5, fmt: fmt1, tip: { text: tip } });
	const shadowGroup = t(locale, 'pj.groupShadow');
	if (scene === 1) {
		return [
			vec2('a', t(locale, 'pj.tipA')), vec2('b', t(locale, 'pj.tipB')),
			ro('shadow', 'a·b̂', shadowGroup, (v) => derive(v).p.s, fmt2, t(locale, 'pj.tipShadow')),
			ro('th', 'θ', shadowGroup, (v) => derive(v).p.th, fmtDeg, t(locale, 'pj.tipTh')),
		];
	}
	if (scene === 2) {
		const nan = [NaN, NaN];
		return [
			vec2('a', t(locale, 'pj.tipA')), vec2('b', t(locale, 'pj.tipB')),
			ro('proj', 'proj', shadowGroup, (v) => derive(v).p.proj ?? nan, fmt2, t(locale, 'pj.tipProj'), 14),
			ro('perp', 'perp', shadowGroup, (v) => derive(v).p.perp ?? nan, fmt2, t(locale, 'pj.tipPerp'), 14),
			ro('perpDot', 'perp·b', shadowGroup, (v) => derive(v).p.perpDotB, fmt2, t(locale, 'pj.tipPerpDot')),
		];
	}
	if (scene === 3) {
		const axesGroup = t(locale, 'pj.groupAxes');
		return [
			vec2('c', t(locale, 'pj.tipC')),
			{ id: 'phi', label: 'φ', kind: 'num', key: 'phi', spec: SPEC.phi, snap: 15, fmt: fmtDeg, group: axesGroup, tip: { text: t(locale, 'pj.tipPhi') } },
			{ id: 'gap', label: t(locale, 'pj.rowGap'), kind: 'num', key: 'gap', spec: SPEC.gap, snap: [60, 90, 120], fmt: fmtDeg, group: axesGroup, tip: { text: t(locale, 'pj.tipGap') } },
			ro('sh1', 'a·u₁', shadowGroup, (v) => derive(v).q.sh[0], fmt2, t(locale, 'pj.tipSh')),
			ro('sh2', 'a·u₂', shadowGroup, (v) => derive(v).q.sh[1], fmt2, t(locale, 'pj.tipSh')),
			ro('miss', t(locale, 'pj.rowMiss'), shadowGroup, (v) => derive(v).q.miss, fmt2, t(locale, 'pj.tipMiss')),
		];
	}
	return [
		{ id: 'a3', label: 'a', kind: 'vec', key: 'a3', dims: 3, spec: SPEC.a3, snap: 0.5, fmt: fmt1, tip: { text: t(locale, 'pj.tipA3') } },
		{ id: 'n3', label: 'n', kind: 'vec', key: 'n3', dims: 3, spec: SPEC.n3, fmt: fmt2, tip: { text: t(locale, 'pj.tipN') } },
		ro('an', 'a·n', shadowGroup, (v) => derive(v).g.an, fmt2, t(locale, 'pj.tipAn')),
		ro('ground', t(locale, 'pj.rowGround'), shadowGroup, (v) => derive(v).g.ground, fmt1, t(locale, 'pj.tipGround'), 18),
	];
}
