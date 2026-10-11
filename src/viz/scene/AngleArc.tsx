/**
 * The angle between two directions at a corner: a 1.5 px arc of world radius `r` plus a 10% filled sector
 * (visual-language.md, STYLE), with an optional label on the bisector just outside the arc. Hot = outline on the arc. `from` and `to` are DIRECTIONS
 * from `at` (vectors, e.g. the store keys of a and b with `at` the origin), not points; the arc always takes the
 * shorter way, so it shows the unsigned angle in [0, 180] degrees.
 *
 * Robustness (a reader can drag anything anywhere): 0 degrees is a zero-length arc; 180 degrees is a semicircle (2D:
 * counter-clockwise from `from`; 3D: in the plane of `from` and `normal`); a zero direction draws nothing and hides
 * the label instead of throwing. Because any 3D pair can be dragged antiparallel, a 3D arc needs `normal` up front
 * (a loud authoring error at the first render rather than a crash at the reader's first 180-degree drag).
 */
import { useLayoutEffect, useMemo, useRef } from 'react';
import { DoubleSide, type Group, type MeshBasicMaterial } from 'three';
import { angleArc, arcTicks, to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useHoverable, useLabel, usePos, useStage } from './handles';
import { SLine, Stroke, useDynGeo, useReveal, useWorldPx, writeGeo } from './view';

export interface AngleArcProps {
	lesson: string;
	id: string;
	/** Corner (literal or store key). */
	at: V | string;
	/** Direction of the first side (literal or store key). */
	from: V | string;
	/** Direction of the second side (literal or store key). */
	to: V | string;
	/** Radius in world units. */
	r: number;
	/** Bisector text: fixed, or computed from the angle in degrees. */
	label?: string | ((deg: number) => string);
	/** 3D only (required there): picks the plane of a 180-degree arc; any vector not along the sides. */
	normal?: V;
	role?: SceneRole;
	/**
	 * Magnet targets in degrees (e.g. 0, 30, 45, 60, 90, ...): short radial marks just outside the arc, measured from
	 * `from` toward the arc's side, so the reader sees where a dragged angle will stick (see `magnet` in drag.ts).
	 */
	ticks?: readonly number[];
}

const N = 32;
// Triangle fan over [center, arc0 .. arcN]: N triangles.
const FAN = Array.from({ length: N }, (_, i) => [0, i + 1, i + 2]).flat();
// Clearance between the arc and the near edge of its label, in CSS px.
const LABEL_GAP = 6;
// Length of a magnet tick beyond the arc, in CSS px (pixel-stable: a world length vanishes on a phone).
const TICK_PX = 6;

export function AngleArc(p: AngleArcProps): React.JSX.Element | null {
	const { mode } = useStage();
	if (mode === '3d' && !p.normal) throw new Error(`AngleArc "${p.id}": a 3D arc needs \`normal\` (the plane of a 180-degree angle)`);
	if (!(p.r > 0) || !Number.isFinite(p.r)) throw new Error(`AngleArc "${p.id}": r must be finite and > 0, got ${p.r}`);
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const c = usePos(p.lesson, p.at);
	const f = usePos(p.lesson, p.from);
	const t = usePos(p.lesson, p.to);
	const nKey = p.normal?.join();
	const arc = useMemo(() => angleArc(c, f, t, p.r, N, p.normal), [c, f, t, p.r, nKey]);
	const line = useMemo(() => arc?.pts.map(to3), [arc]);
	const geo = useDynGeo(N + 2, FAN);
	useLayoutEffect(() => {
		if (arc) writeGeo(geo, [c, ...arc.pts]);
	}, [geo, arc, c]);
	const deg = arc ? (arc.theta * 180) / Math.PI : 0;
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.arc);
	const text = typeof p.label === 'function' ? p.label(deg) : p.label;
	useLabel(
		text === undefined
			? null
			: { lesson: p.lesson, text, role: p.role ?? 'ink', hot: p.id, at: () => arc?.mid ?? null, away: () => c, gap: LABEL_GAP + (p.ticks ? TICK_PX : 0), sides: () => (arc ? [arc.pts[0], arc.pts[N]] : null) },
	);
	if (!arc || !line) return null;
	const color = pal[p.role ?? 'ink'];
	return (
		<>
			<ArcBody hover={hover} geo={geo} line={line} color={color} hot={hot} />
			{p.ticks && <Ticks c={c} f={f} t={t} r={p.r} ticks={p.ticks} normal={p.normal} />}
		</>
	);
}

/** Magnet ticks of an arc: muted 1 px radial marks TICK_PX long (re-measured when the zoom changes, see useWorldPx). */
function Ticks(p: { c: V; f: V; t: V; r: number; ticks: readonly number[]; normal?: V }): React.JSX.Element | null {
	const pal = usePalette();
	const len = useWorldPx(TICK_PX, p.c);
	const key = p.ticks.join();
	const nKey = p.normal?.join();
	const pts = useMemo(() => (len === null ? [] : arcTicks(p.c, p.f, p.t, p.r, p.ticks, len, p.normal).map(to3)), [p.c, p.f, p.t, p.r, key, len, nKey]);
	if (!pts.length) return null;
	return <SLine points={pts} segments color={pal.muted} lineWidth={1} transparent depthWrite={false} renderOrder={1} />;
}

/** Sector + arc of an AngleArc; a separate component so the draw-in hook only exists while the arc is drawn. */
function ArcBody(p: { hover: ReturnType<typeof useHoverable>; geo: React.ComponentProps<'mesh'>['geometry']; line: [number, number, number][]; color: string; hot: boolean }): React.JSX.Element {
	const fill = useRef<MeshBasicMaterial>(null);
	const group = useRef<Group>(null);
	useReveal((r) => {
		if (fill.current) fill.current.opacity = STYLE.sectorA * r;
		if (group.current) group.current.visible = r > 0;
	});
	return (
		<group ref={group} {...p.hover}>
			<mesh geometry={p.geo} frustumCulled={false} renderOrder={0}>
				<meshBasicMaterial
					ref={fill}
					color={p.color}
					transparent
					opacity={STYLE.sectorA}
					depthWrite={false}
					side={DoubleSide}
					polygonOffset
					polygonOffsetFactor={1}
					polygonOffsetUnits={1}
				/>
			</mesh>
			<Stroke points={p.line} color={p.color} hot={p.hot} width={STYLE.edge} halo={false} order={1} />
		</group>
	);
}
