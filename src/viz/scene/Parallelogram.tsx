/**
 * The parallelogram spanned by vectors a and b from `at` (default: the origin), filled. In 2D its signed area
 * a.x*b.y - a.y*b.x is shown by orientation: counter-clockwise (positive) is a plain tint, clockwise (negative) adds a
 * 45-degree hatch, so the sign reads in grayscale too (visual-language.md: "sign is never color-only"). In 3D it is a
 * plain quad in the plane of a and b (area |a x b| has no sign). Collinear or zero vectors span no area and draw
 * nothing (no throw: a reader can drag into that). `Quad` here is also the drawing of AreaTile.
 */
import { lazy, Suspense, useContext, useLayoutEffect, useMemo, useRef } from 'react';
import { Float32BufferAttribute, type Group } from 'three';
import { add, cross, hatch, norm, scale, signedArea2, to3, worldPlaneBasis, dot, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useHoverable, useLabel, usePos } from './handles';
import { SLine, Stroke, useDynGeo, useReveal, useWorldPx, writeGeo } from './view';

import { Point } from './Point';
import { LookCtx, assertSubject } from './look';
const Glass = lazy(()=>import('./Glass').then(m=>({default:m.Glass})));
const QUAD_INDEX = [0, 1, 2, 0, 2, 3];
const O2: V = [0, 0];
const O3: V = [0, 0, 0];

export interface QuadProps {
	lesson: string;
	id: string;
	o: V;
	u: V;
	v: V;
	role: SceneRole;
	/** Negative signed area: adds the hatch. */
	neg: boolean;
	/** Text at the center (overlay label), if any. */
	text?: string;
}

/**
 * The parallelogram o + s*u + t*v: an 18% fill with a 1.5 px edge (visual-language.md, STYLE), plus the 45-degree
 * hatch when `neg`. The caller guarantees a non-degenerate shape. The fill is pushed back in depth (polygonOffset) so
 * coplanar arrows and lines keep their exact color instead of being tinted. Hover lights `id`: the edge gets the hot
 * outline, the fill keeps its color. Draw-in: the whole shape fades in with the reveal.
 */
export function Quad(p: QuadProps): React.JSX.Element {
 assertSubject(useContext(LookCtx),true);
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const { o, u, v } = p;
	const g = useMemo(() => {
		const c = [o, add(o, u), add(add(o, u), v), add(o, v)].map(to3);
		return { c, edge: [...c, c[0]], center: add(o, scale(add(u, v), 0.5)) };
	}, [o, u, v]);
	const geo = useDynGeo(4, QUAD_INDEX);
	useLayoutEffect(() => { writeGeo(geo, g.c); geo.setAttribute('uv', new Float32BufferAttribute([0,0,1,0,1,1,0,1],2)); const basis=worldPlaneBasis(u,v); geo.setAttribute('plane',new Float32BufferAttribute(g.c.flatMap(p=>basis.map(axis=>dot(p,axis))),2)); }, [geo, g]);
	const step = useWorldPx(STYLE.hatchPx, g.center);
	const lines = useMemo(() => (p.neg && step ? hatch(o, u, v, step).map(to3) : null), [p.neg, step, o, u, v]);
	useLabel(p.text === undefined ? null : { lesson: p.lesson, text: p.text, role: p.role, hot: p.id, at: () => g.center });
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.fill);
	const group = useRef<Group>(null);
	useReveal((r) => {
		if (group.current) group.current.visible = r > 0;
	});
	const color = pal[p.role];
	return (
		<group ref={group} {...hover}>
			<mesh geometry={geo} frustumCulled={false} renderOrder={0}>
				<Suspense fallback={<meshBasicMaterial color={color} transparent opacity={STYLE.glassBottomA} depthWrite={false}/>}><Glass color={color} hot={hot} px={step ? STYLE.hatchPx/step : 30}/></Suspense>
			</mesh>
			<Stroke points={g.edge} color={color} hot={hot} width={STYLE.edge} innerAlpha={.30} halo order={1} />
			{g.c.map((at,i)=><Point key={i} lesson={p.lesson} id={p.id} size={5} at={at} drag={false} role={p.role}/>)}
			{lines && lines.length > 0 && (
				<SLine points={lines} segments color={color} lineWidth={STYLE.hatchW} transparent depthWrite={false} renderOrder={1} />
			)}
		</group>
	);
}

export interface ParallelogramProps {
	lesson: string;
	id: string;
	a: V | string;
	b: V | string;
	/** Corner the vectors start from (default: the origin). */
	at?: V | string;
	role: SceneRole;
	/** Center text: fixed, or computed from the area (signed in 2D, |a x b| in 3D). */
	label?: string | ((area: number) => string);
}

export function Parallelogram(p: ParallelogramProps): React.JSX.Element | null {
 assertSubject(useContext(LookCtx),true);
	const a = usePos(p.lesson, p.a);
	const b = usePos(p.lesson, p.b);
	const o = usePos(p.lesson, p.at ?? (a.length === 3 ? O3 : O2));
	const area = a.length === 2 ? signedArea2(a, b) : norm(cross(a, b));
	// Relative test: collinear vectors leave only rounding noise as "area"; zero vectors give 0 <= 0.
	if (Math.abs(area) <= 1e-9 * norm(a) * norm(b)) return null;
	const text = typeof p.label === 'function' ? p.label(area) : p.label;
	return <Quad lesson={p.lesson} id={p.id} o={o} u={a} v={b} role={p.role} neg={a.length === 2 && area < 0} text={text} />;
}
