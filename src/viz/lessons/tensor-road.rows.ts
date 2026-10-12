import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { fmtInt, gKey, MAX_RANK, model, SPEC } from './tensor-road.math';

/**
 * N-panel rows of the tensor road card (3.1-1). Three-free: the MDX page passes the same rows to the SceneIsland
 * skeleton (chip reservation) and the card. A size row is shown while its level is part of the abstract road
 * (level k exists from rank k + 1 on; examples bring their own sizes).
 */
const SIZE_KEYS = ['tr.size0', 'tr.size1', 'tr.size2', 'tr.size3', 'tr.size4', 'tr.size5'] as const;

export function roadRows(locale: Locale): readonly Row[] {
	const sizes = t(locale, 'tr.groupSizes');
	const read = t(locale, 'tr.groupRead');
	return [
		{ id: 'rank', label: t(locale, 'tr.rowRank'), kind: 'num', key: 'rank', spec: { ...SPEC.rank }, snap: 1, fmt: fmtInt, tip: { text: t(locale, 'tr.tipRank') } },
		{ id: 'data', label: t(locale, 'tr.rowData'), kind: 'bool', key: 'data', tip: { text: t(locale, 'tr.tipData') } },
		...Array.from({ length: MAX_RANK }, (_, k): Row => ({
			id: gKey(k), label: t(locale, SIZE_KEYS[k]), kind: 'num', key: gKey(k), spec: { ...SPEC.size }, snap: 1, fmt: fmtInt, group: sizes,
			visible: (v) => v.data !== 1 && (v.rank as number) > k,
			tip: { text: t(locale, 'tr.tipSize') },
		})),
		{ id: 'elements', label: t(locale, 'tr.rowElements'), kind: 'ro', group: read, value: (v) => model(v).size, fmt: fmtInt, chars: 5, tip: { text: t(locale, 'tr.tipElements') } },
		{ id: 'offset', label: t(locale, 'tr.rowOffset'), kind: 'ro', group: read, value: (v) => model(v).offset, fmt: fmtInt, chars: 5, tip: { text: t(locale, 'tr.tipOffset') } },
	];
}
