import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive2, derive3, deriveDis, deriveR, fmt1, fmt2, SPEC } from './pythagoras.math';

/**
 * N-panel rows of the four viewport scenes of lesson 0.3 (0.3-4, the n-D table, is page-side). Three-free: the MDX
 * page passes the same rows to the SceneIsland skeleton (chip reservation) and the ViewportCard. Row ids are term ids
 * shared with the formulas and scene objects; word labels come from the `pt` dictionary, math labels (dx², d) are the
 * same in every language.
 */
export type Scene = 1 | 2 | 3 | 5;

type Vals = Parameters<typeof derive2>[0];

/** A read-only row: value from a derive function, two decimals, 7 characters in the phone chip. */
const ro = (id: string, label: string, group: string, value: (v: Vals) => number, tip: string): Row => ({
	id, label, kind: 'ro', group, fmt: fmt2, chars: 7, value, tip: { text: tip },
});

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	if (scene === 1) {
		const sq = t(locale, 'pt.groupSquares'), len = t(locale, 'pt.groupLength');
		return [
			{ id: 'P', label: 'P', kind: 'vec', key: 'P', dims: 2, spec: SPEC.P, fmt: fmt1, tip: { text: t(locale, 'pt.tipP') } },
			{ id: 'Q', label: 'Q', kind: 'vec', key: 'Q', dims: 2, spec: SPEC.P, fmt: fmt1, tip: { text: t(locale, 'pt.tipQ') } },
			ro('dx2', 'dx²', sq, (v) => derive2(v).dx2, t(locale, 'pt.tipDx2')),
			ro('dy2', 'dy²', sq, (v) => derive2(v).dy2, t(locale, 'pt.tipDy2')),
			ro('d2', 'd²', sq, (v) => derive2(v).d2, t(locale, 'pt.tipD2')),
			ro('d', 'd', len, (v) => derive2(v).d, t(locale, 'pt.tipD')),
			ro('walk', t(locale, 'pt.rowWalk'), len, (v) => derive2(v).walk, t(locale, 'pt.tipWalk')),
		];
	}
	if (scene === 2) {
		const sides = t(locale, 'pt.groupSides'), areas = t(locale, 'pt.groupAreas');
		return [
			{ id: 'a', label: 'a', kind: 'num', key: 'a', spec: SPEC.ab, snap: 0.5, fmt: fmt1, group: sides, tip: { text: t(locale, 'pt.tipA') } },
			{ id: 'b', label: 'b', kind: 'num', key: 'b', spec: SPEC.ab, snap: 0.5, fmt: fmt1, group: sides, tip: { text: t(locale, 'pt.tipB') } },
			ro('big', '(a + b)²', areas, (v) => deriveDis(v).big, t(locale, 'pt.tipBig')),
			ro('tris', '4 · ½ab', areas, (v) => deriveDis(v).tris, t(locale, 'pt.tipTris')),
			ro('c2', 'c²', areas, (v) => deriveDis(v).c2, t(locale, 'pt.tipC2')),
			ro('ab2', 'a² + b²', areas, (v) => deriveDis(v).a2 + deriveDis(v).b2, t(locale, 'pt.tipAb2')),
		];
	}
	if (scene === 3) {
		const sq = t(locale, 'pt.groupSquares'), len = t(locale, 'pt.groupLength');
		return [
			{ id: 'P3', label: 'P', kind: 'vec', key: 'P3', dims: 3, spec: SPEC.P3, fmt: fmt1, tip: { text: t(locale, 'pt.tipP3') } },
			{ id: 'Q3', label: 'Q', kind: 'vec', key: 'Q3', dims: 3, spec: SPEC.P3, fmt: fmt1, tip: { text: t(locale, 'pt.tipQ3') } },
			ro('f2', 'f²', sq, (v) => derive3(v).f2, t(locale, 'pt.tipF2')),
			ro('s2', 'd²', sq, (v) => derive3(v).d2, t(locale, 'pt.tipD2_3')),
			ro('s', 'd', len, (v) => derive3(v).d, t(locale, 'pt.tipD')),
		];
	}
	const sq = t(locale, 'pt.groupSquares'), len = t(locale, 'pt.groupLength');
	return [
		{ id: 'X', label: 'X', kind: 'vec', key: 'X', dims: 2, spec: SPEC.X, fmt: fmt1, tip: { text: t(locale, 'pt.tipX') } },
		{ id: 'r', label: 'r', kind: 'num', key: 'r', spec: SPEC.r, snap: 0.5, fmt: fmt1, tip: { text: t(locale, 'pt.tipR') } },
		ro('xd2', 'd²', sq, (v) => deriveR(v).d2, t(locale, 'pt.tipD2_5')),
		ro('r2', 'r²', sq, (v) => deriveR(v).r2, t(locale, 'pt.tipR2')),
		ro('xd', 'd', len, (v) => deriveR(v).d, t(locale, 'pt.tipD5')),
	];
}
