/**
 * A filled polygon with a fixed vertex count whose placement can move along a lesson timeline (shears, rigid turns,
 * pieces sliding into place): a glass face (Glass.tsx), an optional hatch (negative area), a glowing edge (solid,
 * dashed or none) and an optional centre label.
 *
 * - `pts(vals, step)` gives the target vertices; `path(vals, from, to, progress)` (optional) the in-between
 *   placements, so area-preserving motions stay exact instead of a straight vertex lerp.
 * - A step change springs over `phases` spring periods (reduced motion cuts); a value change cuts, except during a
 *   motion with a `path`, where it reshapes the current placement without restarting the clock. `position` (a
 *   reader-controlled fractional placement) bypasses the internal clock entirely.
 * - Vertices must stay coplanar and their count fixed (both throw); a degenerate polygon hides fill and edge.
 */
import { lazy, Suspense, useContext, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import { DoubleSide, Float32BufferAttribute, ShapeUtils, Vector2 } from 'three';
import { to3, sub, dot, cross, normalize, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useVals, useStep, useHot, type LessonState } from '../store';
import { useLabel, useHoverable, HOVER_RANK } from './handles';
import { useDynGeo, writeGeo, Stroke } from './view';
import { SPRING_MS, reducedMotion, springProgress } from './motion';
import { LookCtx, assertSubject } from './look';
import { Hatch } from './hatchMaterial';
import { lerpVertices, polygonInfo } from './polygon';

type Vals = LessonState['vals'];

const Glass = lazy(() => import('./Glass').then((m) => ({ default: m.Glass })));

/** Smallest area drawn; below it the polygon is degenerate (fill and edge hidden, the label stays). */
const MIN_AREA = 1e-9;

/** First in-plane unit direction of a polygon: from vertex 0 to the first vertex that differs from it. */
function firstEdge(coords: number[][]): number[] {
	const other = coords.find((q) => q.some((x, j) => x !== coords[0][j]));
	if (!other) throw new Error('Poly: all vertices coincide');
	return normalize(sub(other, coords[0]));
}

/**
 * Glass coordinates of a polygon: `local` = position in world units along the first edge `u` and its in-plane normal
 * `v` (the glass grid then counts unit cells aligned with that edge: squares and strips show their own cells), `uv` =
 * the same normalised to the polygon's extent in those directions (rim glow; exact for rectangles).
 */
function glassCoords(coords: number[][], u: number[], v: number[]): { local: number[]; uv: number[] } {
	const o = coords[0];
	const l = coords.map((p) => [dot(sub(p, o), u), dot(sub(p, o), v)]);
	const lo = [Math.min(...l.map((q) => q[0])), Math.min(...l.map((q) => q[1]))];
	const hi = [Math.max(...l.map((q) => q[0])), Math.max(...l.map((q) => q[1]))];
	const span = [Math.max(hi[0] - lo[0], 1e-9), Math.max(hi[1] - lo[1], 1e-9)];
	return {
		local: l.flatMap((q) => [q[0] - lo[0], q[1] - lo[1]]),
		uv: l.flatMap((q) => [(q[0] - lo[0]) / span[0], (q[1] - lo[1]) / span[1]]),
	};
}

export interface PolyProps {
	lesson: string;
	id: string;
	timeline?: string;
	step?: number;
	/** Reader-controlled fractional placement; supplied positions bypass the internal clock. */
	position?: number;
	pts: (vals: Vals, step: number) => V[];
	/** Explicit pure path, including fractional placements for area-preserving shears/rigid turns. */
	path?: (vals: Vals, from: number, to: number, progress: number) => V[];
	phases?: number;
	role: SceneRole;
	fill?: number;
	hatched?: (vals: Vals) => boolean;
	edge?: 'solid' | 'dashed' | 'none';
	label?: (vals: Vals) => string;
}

interface Motion {
	start: number;
	from: number;
	to: number;
	pts: V[];
	duration: number;
}

export function Poly(p: PolyProps): React.JSX.Element {
	assertSubject(useContext(LookCtx), true);
	const vals = useVals(p.lesson);
	const timeline = useStep(p.lesson, p.timeline ?? 'cut');
	const step = p.position ?? p.step ?? timeline;
	if (!Number.isFinite(step)) throw new Error('Poly position must be finite');
	const timelinePhases = useStep(p.lesson, (p.timeline ?? 'cut') + ':phases');
	const countPhases = p.phases ?? Math.max(1, timelinePhases);
	if (!Number.isInteger(countPhases) || countPhases < 1) throw new Error('Poly phases must be a positive integer');
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const invalidate = useThree((s) => s.invalidate);
	const target = p.pts(vals, step);
	const count = useRef(target.length);
	const last = useRef({ vals, step });
	const [points, setPoints] = useState(target);
	const shown = useRef(target);
	const position = useRef(step);
	const motion = useRef<Motion | null>(null);
	if (target.length !== count.current) throw new Error('Poly ' + p.id + ': vertex count changed');
	polygonInfo(target);

	useLayoutEffect(() => {
		const old = last.current;
		last.current = { vals, step };
		const cut = () => {
			motion.current = null;
			shown.current = target;
			position.current = step;
			setPoints(target);
		};
		if (p.position !== undefined) cut();
		else if (old.step !== step && !reducedMotion()) {
			motion.current = { start: performance.now(), from: position.current, to: step, pts: shown.current, duration: SPRING_MS * countPhases };
			invalidate();
		} else if (old.vals !== vals && motion.current && p.path && !reducedMotion()) {
			// A direct value write reshapes the current placement without restarting its clock.
			const next = p.path(vals, position.current, position.current, 0);
			shown.current = next;
			setPoints(next);
			invalidate();
		} else if (old.vals !== vals || old.step !== step) cut();
	}, [vals, step, p.position]);

	useFrame(() => {
		const m = motion.current;
		if (!m) return;
		const elapsed = performance.now() - m.start;
		const n = m.duration / SPRING_MS;
		// One spring per phase: a multi-phase step (shear, then turn) reads as distinct movements.
		const phase = Math.floor(elapsed / SPRING_MS);
		const t = reducedMotion() || elapsed >= m.duration ? 1 : (phase + springProgress(elapsed % SPRING_MS, SPRING_MS)) / n;
		const next = p.path ? p.path(vals, m.from, m.to, t) : lerpVertices(m.pts, target, t);
		if (next.length !== count.current) throw new Error('Poly path changed vertex count');
		polygonInfo(next);
		shown.current = next;
		position.current = m.from + (m.to - m.from) * t;
		setPoints(next);
		if (t === 1) motion.current = null;
		else invalidate();
	});

	const info = polygonInfo(points);
	const coords = points.map(to3);
	const indices = useMemo(() => {
		if (info.area < MIN_AREA) return [];
		const u = firstEdge(coords);
		const v = cross(info.normal, u);
		return ShapeUtils.triangulateShape(coords.map((q) => new Vector2(dot(q, u), dot(q, v))), []).flat();
	}, [points]);
	const geo = useDynGeo(count.current, indices);
	useLayoutEffect(() => {
		geo.setIndex(indices);
		writeGeo(geo, coords);
		if (info.area < MIN_AREA) return;
		const u = firstEdge(coords);
		const g = glassCoords(coords, u, cross(info.normal, u));
		geo.setAttribute('local', new Float32BufferAttribute(g.local, 2));
		geo.setAttribute('uv', new Float32BufferAttribute(g.uv, 2));
	}, [geo, points, indices]);
	useLabel(p.label ? { lesson: p.lesson, text: p.label(vals), role: p.role, hot: p.id, at: () => info.center } : null);
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.fill);
	const color = pal[p.role];
	const alpha = p.fill ?? STYLE.fillA;
	return (
		<group {...hover}>
			{info.area >= MIN_AREA && (
				<>
					<mesh geometry={geo} frustumCulled={false}>
						<Suspense fallback={<meshBasicMaterial color={color} side={DoubleSide} transparent opacity={alpha} depthWrite={false} polygonOffset polygonOffsetFactor={1} />}>
							<Glass color={color} hot={hot} alpha={alpha} />
						</Suspense>
					</mesh>
					{p.hatched?.(vals) && <Hatch geometry={geo} role={p.role} />}
					{p.edge !== 'none' && <Stroke points={[...coords, coords[0]]} color={color} hot={hot} width={STYLE.edge} dashPx={p.edge === 'dashed' ? [5, 4] : undefined} />}
				</>
			)}
		</group>
	);
}
