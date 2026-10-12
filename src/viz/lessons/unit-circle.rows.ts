import { t, type Locale } from '../../i18n/core';
import type { Row } from '../scene/NPanel';
import { derive, fmt2, fmt3, fmtDeg, SPEC, SPECIAL } from './unit-circle.math';

/**
 * N-panel rows of the four 0.4 cards. Three-free: the MDX page passes the same rows to the SceneIsland skeleton (chip
 * reservation) and the ViewportCard. Row ids are term ids shared with the formulas and scene objects; word labels come
 * from the `uc` dictionary, math labels (θ, cos θ) are the same in every language.
 */
export type Scene = 1 | 2 | 3 | 4;

type RoId = 'cos' | 'sin' | 'one' | 'rad' | 'arc' | 'mir';

export function rowsFor(scene: Scene, locale: Locale): readonly Row[] {
	const ro = (id: RoId, label: string, group: string, f: (v: number) => string, tip: string, value: (z: ReturnType<typeof derive>) => number): Row => ({
		id, label, kind: 'ro', group, fmt: f, chars: 6,
		value: (v) => value(derive(v)),
		tip: { text: tip },
	});
	const th: Row = { id: 'th', label: 'θ', kind: 'num', key: 'th', spec: SPEC.th, snap: [...SPECIAL, 270, 360], fmt: fmtDeg, tip: { text: t(locale, 'uc.tipTh') } };
	const point = t(locale, 'uc.groupPoint');
	const cos = ro('cos', 'cos θ', point, fmt2, t(locale, 'uc.tipCos'), (z) => z.cos);
	const sin = ro('sin', 'sin θ', point, fmt2, t(locale, 'uc.tipSin'), (z) => z.sin);
	if (scene === 1) {
		return [th, cos, sin, ro('one', 'cos²θ + sin²θ', point, fmt2, t(locale, 'uc.tipOne'), (z) => z.one)];
	}
	if (scene === 2) {
		const arc = t(locale, 'uc.groupArc');
		return [
			th,
			{ id: 'roll', label: t(locale, 'uc.rowRoll'), kind: 'num', key: 'roll', spec: SPEC.roll, snap: [0, 0.5, 1], fmt: (v) => v.toFixed(2), tip: { text: t(locale, 'uc.tipRoll') } },
			ro('rad', t(locale, 'uc.rowRad'), arc, fmt3, t(locale, 'uc.tipRad'), (z) => z.rad),
			ro('arc', t(locale, 'uc.rowTurn'), arc, fmt3, t(locale, 'uc.tipTurn'), (z) => z.turn),
		];
	}
	if (scene === 3) return [th, cos, sin];
	const mirror = t(locale, 'uc.groupMirror');
	return [th, cos, sin, ro('mir', '90° − θ', mirror, fmtDeg, t(locale, 'uc.tipMir'), (z) => z.mir)];
}
