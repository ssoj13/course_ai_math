/**
 * The right-angle mark: a small square in the corner `at` between directions `u` and `v`, `size` CSS px on a side at
 * any zoom (the marker is a symbol, not a measured length). By default it shows only while u and v are within `tol`
 * degrees of perpendicular (default 0.5: drags snap to 0.1-unit grid values, so exactly perpendicular grid vectors
 * read 90 degrees to rounding, while a near miss such as (1, 0) vs (0.1, 10) = 89.4 degrees is not marked); `show`
 * forces it on or off. A zero direction has no angle, so the mark is hidden even when forced.
 */
import { useMemo, useRef } from 'react';
import type { Group } from 'three';
import { useStore } from 'zustand';
import { add, isRight, norm, normalize, to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { lessonStore } from '../store';
import { usePos } from './handles';
import { Stroke, usePxScale } from './view';

export interface RightAngleProps {
	lesson: string;
	/** Scene object id for the hot highlight (optional: the mark usually belongs to a projection). */
	id?: string;
	at: V | string;
	u: V | string;
	v: V | string;
	/** Side length in CSS px (default 12). */
	size?: number;
	/** Force on/off; default: on when the angle is within `tol` of 90 degrees. */
	show?: boolean;
	/** Tolerance in degrees for the automatic mode (default 0.5). */
	tol?: number;
	role?: SceneRole;
}

export function RightAngle(p: RightAngleProps): React.JSX.Element {
	const pal = usePalette();
	const hot = useStore(lessonStore(p.lesson), (s) => p.id !== undefined && s.hot.has(p.id));
	const at = usePos(p.lesson, p.at);
	const u = usePos(p.lesson, p.u);
	const v = usePos(p.lesson, p.v);
	const on = norm(u) > 0 && norm(v) > 0 && (p.show ?? isRight(u, v, p.tol));
	// Unit-sized corner path u^ -> u^ + v^ -> v^; the group scales it to `size` px every frame.
	const pts = useMemo(() => {
		if (!on) return null;
		const a = normalize(u);
		const b = normalize(v);
		return [a, add(a, b), b].map(to3);
	}, [on, u, v]);
	const ref = useRef<Group>(null);
	// Draw-in: the mark appears in the second half of the reveal (after the lines it sits between).
	usePxScale(ref, (r) => (r < 0.5 ? 0 : (p.size ?? 12)));
	return (
		<group ref={ref} position={to3(at)} visible={pts !== null}>
			{pts && <Stroke points={pts} color={pal[p.role ?? 'muted']} hot={hot} width={STYLE.edge} halo={false} order={1} />}
		</group>
	);
}
