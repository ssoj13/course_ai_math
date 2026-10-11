/**
 * A full circle as a 64-segment loop: 2D in the plane z = 0, 3D in the plane perpendicular to `normal` (orthonormal
 * basis from `planeBasis`; `arcPoints` sweeps at most 180 degrees, so a loop has its own parametrization). Center,
 * radius and normal are literals or pure functions of the lesson values (LC-3 reach rings follow the bone lengths,
 * the DP-4 terminator follows the light). A radius below 1e-9 draws nothing (a ring that collapsed to its center, e.g.
 * min reach with equal bones). A secondary line (1.5 px, no halo; dashed with `dashPx`); hover lights it.
 */
import { useMemo } from 'react';
import { circlePoints, to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useHot, useVals, type LessonState } from '../store';
import { HOVER_RANK, useHoverable, useStage } from './handles';
import { Stroke } from './view';

type Vals = LessonState['vals'];

export interface CircleProps {
	lesson: string;
	id: string;
	center: V | ((vals: Vals) => V);
	r: number | ((vals: Vals) => number);
	/** 3D only (required there): the circle's plane normal. */
	normal?: V | ((vals: Vals) => V);
	role: SceneRole;
	/** Dash and gap in CSS px (e.g. STYLE.dash). */
	dashPx?: readonly [number, number];
}

const SEG = 64;
const get = <T,>(x: T | ((v: Vals) => T), vals: Vals): T => (typeof x === 'function' ? (x as (v: Vals) => T)(vals) : x);

export function Circle(p: CircleProps): React.JSX.Element | null {
	const { mode } = useStage();
	if (mode === '3d' && !p.normal) throw new Error(`Circle "${p.id}": a 3D circle needs \`normal\``);
	if (mode === '2d' && p.normal) throw new Error(`Circle "${p.id}": a 2D circle takes no \`normal\``);
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const vals = useVals(p.lesson);
	const c = get(p.center, vals);
	const r = get(p.r, vals);
	const n = p.normal && get(p.normal, vals);
	const key = `${c.join()}|${r}|${n?.join() ?? ''}`;
	const pts = useMemo(() => (r < 1e-9 ? null : circlePoints(c, r, SEG, n)?.map(to3)), [key]);
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.guide);
	if (!pts) return null;
	return (
		<group {...hover}>
			<Stroke points={pts} color={pal[p.role]} hot={hot} width={STYLE.edge} halo={false} dashPx={p.dashPx} order={1} />
		</group>
	);
}
