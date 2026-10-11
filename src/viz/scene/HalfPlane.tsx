/**
 * The half of space where `normal . v` has one sign, as a faint fill (DP-2 "a·v > 0 here", DP-3's ⊥ disc). 2D: the
 * half-plane through the origin, a polygon clipped from a square far larger than anything the camera can show (pan and
 * zoom-out included), so its edge is only ever the boundary line. 3D: a single-sided disc of radius `extent` through
 * the origin, perpendicular to `normal`: `pos` is the face seen from the +normal side, `neg` the face seen from the
 * other side, so two HalfPlanes make the two-coloured disc of DP-3. Drawn below every line (render order, no depth
 * writes); hot (hover, or a linked view) doubles the opacity. A zero normal has no line or plane: nothing is drawn.
 */
import { useLayoutEffect, useMemo, useRef } from 'react';
import { BackSide, CircleGeometry, DoubleSide, FrontSide, Quaternion, Vector3, type Group, type MeshBasicMaterial } from 'three';
import { halfPlanePoly, norm, to3, type V } from '../geom';
import { usePalette, type SceneRole } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useHoverable, usePos, useStage } from './handles';
import { useDynGeo, useReveal, writeGeo } from './view';

export interface HalfPlaneProps {
	lesson: string;
	id: string;
	/** Normal of the boundary through the origin (literal or store key), e.g. the key of vector a. */
	normal: V | string;
	/** `pos`: normal . v > 0; `neg`: normal . v < 0. */
	side: 'pos' | 'neg';
	role: SceneRole;
	/** Fill alpha (default 0.1); doubled while hot. */
	opacity?: number;
}

// 2D clip square as a multiple of the stage extent: zoom-out (0.5x), the 16:10 aspect and panning stay inside it.
const FAR = 50;
// Up to 5 vertices after one clip of a square; a 5-vertex fan, short polygons repeat their last vertex.
const FAN5 = [0, 1, 2, 0, 2, 3, 0, 3, 4];
// Unit disc in the xy plane facing +z, shared by every 3D half-plane.
const DISC = new CircleGeometry(1, 64);
const Z = new Vector3(0, 0, 1);

const AUX = { aux: true };

export function HalfPlane(p: HalfPlaneProps): React.JSX.Element | null {
	const reg = useStage();
	const n = usePos(p.lesson, p.normal);
	if (reg.mode === '2d' && n.length !== 2) throw new Error(`HalfPlane "${p.id}": a 2D half-plane needs a 2D normal`);
	if (reg.mode === '3d' && n.length !== 3) throw new Error(`HalfPlane "${p.id}": a 3D half-space needs a 3D normal`);
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.fill);
	// 0.18 (was 0.1 on the flat 6d background): over the holographic nebulae a 10 % tint no longer read as a side.
	const base = p.opacity ?? 0.18;
	const alpha = hot ? 2 * base : base;
	const fill = useRef<MeshBasicMaterial>(null);
	const group = useRef<Group>(null);
	useReveal((r) => {
		if (fill.current) fill.current.opacity = alpha * r;
		if (group.current) group.current.visible = r > 0;
	});
	const sgn = p.side === 'pos' ? 1 : -1;
	const nKey = n.join();
	const poly = useMemo(() => (n.length === 2 ? halfPlanePoly(n, sgn, FAR * reg.extent) : null), [nKey, sgn, reg.extent]);
	const geo = useDynGeo(5, FAN5);
	useLayoutEffect(() => {
		if (poly && poly.length >= 3) writeGeo(geo, Array.from({ length: 5 }, (_, i) => poly[Math.min(i, poly.length - 1)]));
	}, [geo, poly]);
	const q = useMemo(() => (n.length === 3 && norm(n) > 0 ? new Quaternion().setFromUnitVectors(Z, new Vector3(...to3(n)).normalize()) : null), [nKey]);
	if (norm(n) === 0 || (poly && poly.length < 3)) return null;
	const color = pal[p.role];
	const mat = (
		<meshBasicMaterial
			ref={fill}
			color={color}
			transparent
			opacity={alpha}
			depthWrite={false}
			side={poly ? DoubleSide : sgn > 0 ? FrontSide : BackSide}
			polygonOffset
			polygonOffsetFactor={2}
			polygonOffsetUnits={2}
		/>
	);
	return (
		<group ref={group} {...hover}>
			{poly ? (
				// A half-plane is 50 x extent wide: scene furniture for framing and the floor reflection (`aux`).
				<mesh geometry={geo} frustumCulled={false} renderOrder={-1} userData={AUX}>
					{mat}
				</mesh>
			) : (
				<mesh geometry={DISC} quaternion={q ?? undefined} scale={reg.extent} renderOrder={-1} userData={AUX}>
					{mat}
				</mesh>
			)}
		</group>
	);
}
