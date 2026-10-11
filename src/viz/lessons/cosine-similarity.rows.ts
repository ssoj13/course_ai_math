import { t, type Locale, type TextKey } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import type { V } from '../geom';
import { RANGE_AB, RANGE_S, RANGE_TOY, angleDeg, cosine, fmt2, fmtCos, fmtDeg, fmtPct, scaled, toy } from './cosine-similarity.math';
import { PROV, VECS, pairOf, picture, type Group } from './cosine-similarity.data';

/**
 * N-panel rows of the three 1.7 scenes. Three-free: the MDX page passes the same rows to the SceneIsland skeleton
 * (chip reservation) and the ViewportCard. Row ids are term ids shared with the formulas and scene objects; word
 * labels come from the `cs` dictionary, math labels are the same in every language.
 */
export type Scene = 1 | 2 | 3;

/**
 * Dictionary keys picked by data (a toy word, a group, a document, a query, an analogy preset). Written out as literals
 * because the i18n tests require every key to appear verbatim in code (no keys built from template strings).
 */
export const TOY_KEY = { cat: 'cs.cat', dog: 'cs.dog', car: 'cs.car' } as const satisfies Record<string, TextKey>;
export const GROUP_KEY: Readonly<Record<Group, TextKey>> = {
	animals: 'cs.group_animals', vehicles: 'cs.group_vehicles', people: 'cs.group_people', places: 'cs.group_places',
	food: 'cs.group_food', graphics: 'cs.group_graphics', qualities: 'cs.group_qualities',
};
export const DOC_KEY: readonly TextKey[] = [
	'cs.doc0', 'cs.doc1', 'cs.doc2', 'cs.doc3', 'cs.doc4', 'cs.doc5', 'cs.doc6', 'cs.doc7', 'cs.doc8', 'cs.doc9', 'cs.doc10', 'cs.doc11',
];
export const Q_KEY: readonly TextKey[] = ['cs.q0', 'cs.q1', 'cs.q2', 'cs.q3', 'cs.q4'];
export const NOTE_KEY: readonly TextKey[] = ['cs.anNote0', 'cs.anNote1', 'cs.anNote2', 'cs.anNote3', 'cs.anNote4', 'cs.anNote5', 'cs.anNote6', 'cs.anNote7'];

type Vals = Readonly<Record<string, number | V>>;

function ro(id: string, label: string, value: (v: Vals) => number, extra: Partial<Row> = {}): Row {
	return { id, label, kind: 'ro', chars: 6, fmt: fmtCos, value, tip: { text: label }, ...extra };
}

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	if (scene === 1) {
		const sim = t(locale, 'cs.groupSim');
		const vecRow = (id: 'cat' | 'dog' | 'car'): Row => ({
			id, label: t(locale, TOY_KEY[id]), kind: 'vec', key: id, dims: 2, spec: RANGE_TOY, snap: 0.5, fmt: fmt2, tip: { text: t(locale, 'cs.tipToy') },
		});
		return [
			vecRow('cat'), vecRow('dog'), vecRow('car'),
			ro('cosCD', t(locale, 'cs.rowCosCD'), (v) => toy(v).cosCD, { group: sim, tip: { text: t(locale, 'cs.tipCos') } }),
			ro('cosCC', t(locale, 'cs.rowCosCC'), (v) => toy(v).cosCC, { tip: { text: t(locale, 'cs.tipCos') } }),
			ro('cosDC', t(locale, 'cs.rowCosDC'), (v) => toy(v).cosDC, { tip: { text: t(locale, 'cs.tipCos') } }),
			ro('thCD', t(locale, 'cs.rowThCD'), (v) => toy(v).thCD, { fmt: fmtDeg, chars: 4, tip: { text: t(locale, 'cs.tipTheta') } }),
		];
	}
	if (scene === 2) {
		const out = t(locale, 'cs.groupCompare');
		return [
			{ id: 'sa', label: 'a', kind: 'vec', key: 'sa', dims: 2, spec: RANGE_AB, snap: 1, fmt: fmt2, tip: { text: t(locale, 'cs.tipA') } },
			{ id: 'sb', label: 'b', kind: 'vec', key: 'sb', dims: 2, spec: RANGE_AB, snap: 1, fmt: fmt2, tip: { text: t(locale, 'cs.tipB') } },
			{ id: 's', label: 's', kind: 'num', key: 's', spec: RANGE_S, snap: [0, 0.5, 1, 1.5, 2], fmt: fmt2, tip: { text: t(locale, 'cs.tipS') } },
			ro('na', '|a|', (v) => scaled(v).na, { fmt: fmt2 }),
			ro('nsb', '|s·b|', (v) => scaled(v).nsb, { fmt: fmt2 }),
			ro('cos', 'cos θ', (v) => scaled(v).cos, { group: out, tip: { text: t(locale, 'cs.tipCosScale') } }),
			ro('th', 'θ', (v) => scaled(v).th, { fmt: fmtDeg, chars: 4, tip: { text: t(locale, 'cs.tipTheta') } }),
			ro('dist', '|a − s·b|', (v) => scaled(v).dist, { fmt: fmt2, tip: { text: t(locale, 'cs.tipDist') } }),
		];
	}
	const real = t(locale, 'cs.groupReal');
	return [
		ro('cosReal', t(locale, 'cs.rowCosReal'), (v) => { const [i, j] = pairOf(v); return cosine(VECS[i], VECS[j]); }, { group: real, tip: { text: t(locale, 'cs.tipCosReal') } }),
		ro('thReal', t(locale, 'cs.rowThReal'), (v) => { const [i, j] = pairOf(v); return angleDeg(VECS[i], VECS[j]); }, { fmt: fmtDeg, chars: 4, tip: { text: t(locale, 'cs.tipThReal') } }),
		ro('thPic', t(locale, 'cs.rowThPic'), (v) => picture(pairOf(v)).th, { fmt: fmtDeg, chars: 4, group: t(locale, 'cs.groupPic'), tip: { text: t(locale, 'cs.tipThPic') } }),
		ro('kept', t(locale, 'cs.rowKept'), () => PROV.pca.explained.reduce((s, x) => s + x, 0), { fmt: fmtPct, chars: 5, tip: { text: t(locale, 'cs.tipKept') } }),
	];
}
