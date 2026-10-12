import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { arrow2, fmt1, fmt2, RANGE_P, RANGE_P3, RANGE_T, RANGE_TAIL, RANGE_V, RANGE_V3, RANGE_W, translate, trip } from './vector.math';

/**
 * N-panel rows of the six 1.1 scenes. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard. Row ids are term ids shared with the scene objects, formula terms and table
 * cells; word labels come from the `ve` dictionary, math labels (v, |v|, B − A) are the same in every language.
 */
export type Scene = 1 | 2 | 3 | 4 | 5 | 6;

/** A vector printed in a read-only row: "(−3.0, −4.0)" fits the 200 px panel. */
const ro = (id: string, label: string, value: Row['value'], extra: Partial<Row> = {}): Row => ({ id, label, kind: 'ro', value, fmt: fmt1, chars: 7, ...extra });

/** @param rev VE-5 only: the "trip back" state renames the difference row (same width, so the chips do not move). */
export function rowsFor(scene: Scene, locale: Locale, rev = 0): readonly Row[] {
	if (scene === 1) {
		return [{ id: 'v', label: 'v', kind: 'vec', key: 'v', dims: 2, spec: RANGE_V, snap: 0.5, tip: { text: t(locale, 've.tipV'), tex: String.raw`\mathbf{v} = (x, y)` } }];
	}
	if (scene === 2) {
		return [
			{ id: 'w', label: 'v', kind: 'vec', key: 'w', dims: 2, spec: RANGE_W, snap: 0.5, tip: { text: t(locale, 've.tipW') } },
			// While anchored the arrow is drawn from the origin and its stored start is not used, so the field is hidden.
			{ id: 'tail', label: t(locale, 've.rowStart'), kind: 'vec', key: 'tail', dims: 2, spec: RANGE_TAIL, snap: 0.5, visible: (v) => v.anchor === 0, tip: { text: t(locale, 've.tipTail') } },
			ro('head', t(locale, 've.rowEnd'), (v) => arrow2(v).to, { chars: 12, tip: { text: t(locale, 've.tipHead') } }),
			{ id: 'anchor', label: t(locale, 've.rowAnchor'), kind: 'bool', key: 'anchor', tip: { text: t(locale, 've.tipAnchor') } },
		];
	}
	if (scene === 3) {
		return [
			{ id: 'T', label: 'T', kind: 'vec', key: 'T', dims: 2, spec: RANGE_T, snap: 0.5, tip: { text: t(locale, 've.tipT') } },
			ro('N2', "N'", (v) => translate(v).N2, { fmt: fmt2, chars: 12, tip: { text: t(locale, 've.tipN2') } }),
			{ id: 'asPoint', label: t(locale, 've.rowAsPoint'), kind: 'bool', key: 'asPoint', tip: { text: t(locale, 've.tipAsPoint') } },
		];
	}
	if (scene === 4) {
		return [{ id: 'v3', label: 'v', kind: 'vec', key: 'v3', dims: 3, spec: RANGE_V3, snap: 0.5, tip: { text: t(locale, 've.tipV3'), tex: String.raw`\mathbf{v} = (x, y, z)` } }];
	}
	if (scene === 5) {
		return [
			{ id: 'A', label: 'A', kind: 'vec', key: 'A', dims: 2, spec: RANGE_P, snap: 0.5, tip: { text: t(locale, 've.tipA') } },
			{ id: 'B', label: 'B', kind: 'vec', key: 'B', dims: 2, spec: RANGE_P, snap: 0.5, tip: { text: t(locale, 've.tipB') } },
			ro('d', rev ? 'A − B' : 'B − A', (v) => trip(v).d, { chars: 12, tip: { text: t(locale, rev ? 've.tipDBack' : 've.tipD') } }),
			ro('len', t(locale, 've.rowDistance'), (v) => trip(v).len, { fmt: fmt2, tip: { text: t(locale, 've.tipLen'), tex: String.raw`\sqrt{d_x^2 + d_y^2}` } }),
			{ id: 'rev', label: t(locale, 've.rowRev'), kind: 'bool', key: 'rev', tip: { text: t(locale, 've.tipRev') } },
		];
	}
	return [
		{ id: 'A3', label: 'A', kind: 'vec', key: 'A3', dims: 3, spec: RANGE_P3, snap: 0.5, tip: { text: t(locale, 've.tipA') } },
		{ id: 'B3', label: 'B', kind: 'vec', key: 'B3', dims: 3, spec: RANGE_P3, snap: 0.5, tip: { text: t(locale, 've.tipB') } },
		ro('d3', 'B − A', (v) => trip(v, true).d, { chars: 18, tip: { text: t(locale, 've.tipD') } }),
		ro('len3', t(locale, 've.rowDistance'), (v) => trip(v, true).len, { fmt: fmt2, tip: { text: t(locale, 've.tipLen'), tex: String.raw`\sqrt{d_x^2 + d_y^2 + d_z^2}` } }),
	];
}
