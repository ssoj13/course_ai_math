import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import {
	audioModel, FACES, fmt3, fmtInt, IDS, IMG_C, IMG_H, IMG_W, imageModel, lookupAt, LOOKUPS, N_FACES, RATE, SAMPLES,
} from './data-as-tensors.math';
import { flat, slice } from '../../core';

/**
 * N-panel rows of the four 3.2 cards. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the card. Row ids are the store keys they edit (read-only rows: what they show).
 */
export function imageRows(locale: Locale): readonly Row[] {
	const sel = t(locale, 'dat.groupSel');
	return [
		{ id: 'chw', label: t(locale, 'dat.rowChw'), kind: 'bool', key: 'chw', tip: { text: t(locale, 'dat.tipChw') } },
		{ id: 'peel', label: t(locale, 'dat.rowPeel'), kind: 'num', key: 'peel', spec: { min: 0, max: 1, step: 0.1 }, fmt: (x) => x.toFixed(1), tip: { text: t(locale, 'dat.tipPeel') } },
		{ id: 'h', label: 'H', kind: 'num', key: 'h', spec: { min: 0, max: IMG_H - 1, step: 1 }, fmt: fmtInt, group: sel, tip: { text: t(locale, 'dat.tipH') } },
		{ id: 'w', label: 'W', kind: 'num', key: 'w', spec: { min: 0, max: IMG_W - 1, step: 1 }, fmt: fmtInt, group: sel, tip: { text: t(locale, 'dat.tipW') } },
		{ id: 'c', label: 'C', kind: 'num', key: 'c', spec: { min: 0, max: IMG_C - 1, step: 1 }, fmt: fmtInt, group: sel, tip: { text: t(locale, 'dat.tipC') } },
		{ id: 'addr', label: t(locale, 'dat.rowAddr'), kind: 'ro', group: sel, value: (v) => imageModel(v).addr, fmt: fmtInt, chars: 4, tip: { text: t(locale, 'dat.tipAddr') } },
		{ id: 'byte', label: t(locale, 'dat.rowByte'), kind: 'ro', group: sel, value: (v) => imageModel(v).value, fmt: fmtInt, chars: 4, tip: { text: t(locale, 'dat.tipByte') } },
	];
}

/** The token id of the most recent lookup (0 before the first). */
const lastId = (look: number): number => (look < 1 ? 0 : IDS.get(lookupAt(Math.min(LOOKUPS, Math.round(look)) - 1)));

export function textRows(locale: Locale): readonly Row[] {
	return [
		{ id: 'look', label: t(locale, 'dat.rowLook'), kind: 'num', key: 'look', spec: { min: 0, max: LOOKUPS, step: 1 }, snap: 1, fmt: fmtInt, tip: { text: t(locale, 'dat.tipLook') } },
		{ id: 'tokenId', label: t(locale, 'dat.rowId'), kind: 'ro', value: (v) => lastId(v.look as number), fmt: fmtInt, chars: 6, tip: { text: t(locale, 'dat.tipId') } },
	];
}

export function audioRows(locale: Locale): readonly Row[] {
	return [
		{ id: 'fl', label: t(locale, 'dat.rowFl'), kind: 'num', key: 'fl', spec: { min: 2, max: 24, step: 1 }, snap: [2, 3, 4, 6, 8, 12, 16, 24], fmt: fmtInt, tip: { text: t(locale, 'dat.tipFl') } },
		{ id: 's', label: t(locale, 'dat.rowS'), kind: 'num', key: 's', spec: { min: 0, max: SAMPLES - 1, step: 1 }, fmt: fmtInt, tip: { text: t(locale, 'dat.tipS') } },
		{ id: 'frames', label: t(locale, 'dat.rowFrames'), kind: 'ro', value: (v) => audioModel(v).t.shape[0], fmt: fmtInt, chars: 3, tip: { text: t(locale, 'dat.tipFrames') } },
		{ id: 'ms', label: t(locale, 'dat.rowMs'), kind: 'ro', value: (v) => (1000 * audioModel(v).s) / RATE, fmt: (x) => x.toFixed(3), chars: 6, tip: { text: t(locale, 'dat.tipMs') } },
		{ id: 'sample', label: t(locale, 'dat.rowSample'), kind: 'ro', value: (v) => audioModel(v).value, fmt: fmt3, chars: 6, tip: { text: t(locale, 'dat.tipSample') } },
	];
}

export function meshRows(locale: Locale): readonly Row[] {
	return [
		{ id: 'face', label: t(locale, 'dat.rowFace'), kind: 'num', key: 'face', spec: { min: 0, max: N_FACES - 1, step: 1 }, fmt: fmtInt, tip: { text: t(locale, 'dat.tipFace') } },
		{ id: 'tri', label: 'F[f]', kind: 'ro', value: (v) => flat(slice(FACES, [Math.round(v.face as number)])), fmt: fmtInt, chars: 9, tip: { text: t(locale, 'dat.tipTri') } },
	];
}
