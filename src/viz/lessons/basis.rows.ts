import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import type { V } from '../geom';
import { RANGE2, RANGE3, RANGE_R, SPEC_TH, derive1, derive2, derive3, derive4, fmt, fmt3, fmtDeg } from './basis.math';

/**
 * N-panel rows of the four 1.6 scenes. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard, and the tests lock the row id lists. Row ids are term ids (hover <-> scene object
 * <-> formula term <-> table cell); word labels come from the `bas` dictionary, math labels (u₁, c₁, u₁·v) are the same
 * in every language.
 */
export type Scene = 1 | 2 | 3 | 4;

type Vals = Readonly<Record<string, number | V>>;

/** Read-only derived row. */
function ro(id: string, label: string, value: (v: Vals) => number | V, extra: Partial<Row> = {}): Row {
	return { id, label, kind: 'ro', chars: 7, fmt, value, ...extra };
}

/** Three decimals, or a dash: unit-vector components in a vector readout. */
const fmtVec = (x: number) => (Number.isFinite(x) ? fmt3(x) : '–');
/** One decimal, or a dash: a 3-vector of coordinates must fit the 200 px panel (18 characters); the table has 2. */
const fmtVec1 = (x: number) => (Number.isFinite(x) ? (+x.toFixed(1) + 0).toFixed(1) : '–');

/** Dimension of a span as a word ("2 · plane"); the rows of BS-1 and BS-2 share it. */
function dimText(locale: Locale): (d: number) => string {
	return (d) => d + ' · ' + (d === 3 ? t(locale, 'bas.dimSpace') : d === 2 ? t(locale, 'bas.dimPlane') : d === 1 ? t(locale, 'bas.dimLine') : t(locale, 'bas.dimPoint'));
}

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	if (scene === 1) {
		const coords = t(locale, 'bas.groupCoords');
		return [
			{ id: 'u1', label: 'u₁', kind: 'vec', key: 'u1', dims: 2, spec: RANGE2, snap: 0.5, tip: { text: t(locale, 'bas.tipU1') } },
			{ id: 'u2', label: 'u₂', kind: 'vec', key: 'u2', dims: 2, spec: RANGE2, snap: 0.5, tip: { text: t(locale, 'bas.tipU2') } },
			{ id: 'v', label: 'v', kind: 'vec', key: 'v', dims: 2, spec: RANGE2, snap: 0.5, tip: { text: t(locale, 'bas.tipV') } },
			ro('area', t(locale, 'bas.rowArea'), (v) => derive1(v).area, { group: t(locale, 'bas.groupSpan'), tip: { text: t(locale, 'bas.tipArea'), tex: String.raw`u_{1x}u_{2y}-u_{1y}u_{2x}` } }),
			ro('dim', t(locale, 'bas.rowDim'), (v) => derive1(v).dim, { fmt: dimText(locale), chars: 13, tip: { text: t(locale, 'bas.tipDim') } }),
			ro('c1', 'c₁', (v) => derive1(v).c1, { group: coords, tip: { text: t(locale, 'bas.tipC1') } }),
			ro('c2', 'c₂', (v) => derive1(v).c2, { tip: { text: t(locale, 'bas.tipC2') } }),
		];
	}
	if (scene === 2) {
		return [
			...(['w1', 'w2', 'w3'] as const).map((id, i): Row => ({
				id, label: ['a', 'b', 'c'][i], kind: 'vec', key: id, dims: 3, spec: RANGE3, snap: 0.5, tip: { text: t(locale, 'bas.tipW') },
			})),
			ro('vol', t(locale, 'bas.rowVolume'), (v) => derive2(v).vol, { group: t(locale, 'bas.groupSpan'), tip: { text: t(locale, 'bas.tipVolume'), tex: String.raw`\mathbf{a}\cdot(\mathbf{b}\times\mathbf{c})` } }),
			ro('dim3', t(locale, 'bas.rowDim'), (v) => derive2(v).dim, { fmt: dimText(locale), chars: 13, tip: { text: t(locale, 'bas.tipDim') } }),
		];
	}
	if (scene === 3) {
		const fresh = t(locale, 'bas.groupNew');
		return [
			{ id: 'th', label: 'θ', kind: 'num', key: 'th', spec: SPEC_TH, snap: 15, fmt: (x) => fmtDeg(x) + '°', tip: { text: t(locale, 'bas.tipTh') } },
			{ id: 'p', label: 'P', kind: 'vec', key: 'p', dims: 2, spec: RANGE2, snap: 0.5, tip: { text: t(locale, 'bas.tipP') } },
			ro('ru1', 'u₁', (v) => derive3(v).u1, { group: fresh, fmt: fmtVec, chars: 16, tip: { text: t(locale, 'bas.tipRu1') } }),
			ro('ru2', 'u₂', (v) => derive3(v).u2, { fmt: fmtVec, chars: 16, tip: { text: t(locale, 'bas.tipRu2') } }),
			ro('sc1', 'c₁ = P·u₁', (v) => derive3(v).c1, { tip: { text: t(locale, 'bas.tipSc1') } }),
			ro('sc2', 'c₂ = P·u₂', (v) => derive3(v).c2, { tip: { text: t(locale, 'bas.tipSc2') } }),
			ro('len2', 'x² + y²', (v) => derive3(v).len2, { group: t(locale, 'bas.groupCheck'), tip: { text: t(locale, 'bas.tipLen2') } }),
			ro('clen2', 'c₁² + c₂²', (v) => derive3(v).clen2, { tip: { text: t(locale, 'bas.tipClen2') } }),
		];
	}
	const readings = t(locale, 'bas.groupReadings');
	return [
		{ id: 'v3', label: 'v', kind: 'vec', key: 'v3', dims: 3, spec: RANGE3, snap: 0.5, tip: { text: t(locale, 'bas.tipV3') } },
		{ id: 'ortho', label: t(locale, 'bas.rowOrtho'), kind: 'bool', key: 'ortho', tip: { text: t(locale, 'bas.tipOrtho') } },
		...(['r1', 'r2', 'r3'] as const).map((id, i): Row => ({
			id, label: ['u₁', 'u₂', 'u₃'][i], kind: 'vec', key: id, dims: 3, spec: RANGE_R, snap: 0.5, tip: { text: t(locale, 'bas.tipR') },
		})),
		...[0, 1, 2].map((i) => ro(`rd${i + 1}`, ['u₁·v', 'u₂·v', 'u₃·v'][i], (v) => derive4(v).rd[i], { group: i === 0 ? readings : undefined, tip: { text: t(locale, 'bas.tipRd') } })),
		ro('coords', 'c', (v) => derive4(v).c ?? [NaN, NaN, NaN], { group: t(locale, 'bas.groupCoords'), fmt: fmtVec1, chars: 18, tip: { text: t(locale, 'bas.tipCoords') } }),
		ro('gap', t(locale, 'bas.rowGap'), (v) => derive4(v).gap, { tip: { text: t(locale, 'bas.tipGap') } }),
		ro('hand', t(locale, 'bas.rowHand'), (v) => derive4(v).hand, {
			chars: 11,
			fmt: (h) => (h > 0 ? t(locale, 'bas.handRight') : h < 0 ? t(locale, 'bas.handLeft') : t(locale, 'bas.handFlat')),
			tip: { text: t(locale, 'bas.tipHand') },
		}),
	];
}
