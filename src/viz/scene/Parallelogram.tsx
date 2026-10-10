/**
 * The parallelogram spanned by vectors a and b from `at` (default: the origin), filled. In 2D its signed area
 * a.x*b.y - a.y*b.x is shown by orientation: counter-clockwise (positive) is a plain tint, clockwise (negative) adds a
 * 45-degree hatch, so the sign reads in grayscale too (visual-language.md: "sign is never color-only"). In 3D it is a
 * plain quad in the plane of a and b (area |a x b| has no sign). Collinear or zero vectors span no area and draw
 * nothing (no throw: a reader can drag into that). `Quad` here is also the drawing of AreaTile.
 */
import { useLayoutEffect, useMemo } from 'react';
import { DoubleSide } from 'three';
import { add, cross, hatch, norm, scale, signedArea2, to3, type V } from '../geom';
import { usePalette, type Role } from '../palette';
import { hotOff, hotOn, useHot } from '../store';
import { useLabel, usePos } from './handles';
import { SLine, useDynGeo, useWorldPx, writeGeo } from './view';

const QUAD_INDEX = [0, 1, 2, 0, 2, 3];
// visual-language.md and the 0.5/1.4 lesson design: 1 px hatch lines every 6 px.
const HATCH_PX = 6;
const O2: V = [0, 0];
const O3: V = [0, 0, 0];

export interface QuadProps {
	lesson: string;
	id: string;
	o: V;
	u: V;
	v: V;
	role: Role;
	/** Negative signed area: adds the hatch. */
	neg: boolean;
	/** Text at the center (overlay label), if any. */
	text?: string;
}

/**
 * The parallelogram o + s*u + t*v: a 16% fill with a 1.5 px edge (visual-language.md), plus the hatch when `neg`.
 * The caller guarantees a non-degenerate shape. The fill is pushed back in depth (polygonOffset) so coplanar arrows
 * and lines drawn earlier keep their exact color instead of being tinted. Hover lights `id`.
 */
export function Quad(p: QuadProps): React.JSX.Element {
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const { o, u, v } = p;
	const g = useMemo(() => {
		const c = [o, add(o, u), add(add(o, u), v), add(o, v)].map(to3);
		return { c, edge: [...c, c[0]], center: add(o, scale(add(u, v), 0.5)) };
	}, [o, u, v]);
	const geo = useDynGeo(4, QUAD_INDEX);
	useLayoutEffect(() => writeGeo(geo, g.c), [geo, g]);
	const step = useWorldPx(HATCH_PX, g.center);
	const lines = useMemo(() => (p.neg && step ? hatch(o, u, v, step).map(to3) : null), [p.neg, step, o, u, v]);
	useLabel(p.text === undefined ? null : { lesson: p.lesson, text: p.text, role: p.role, hot: p.id, at: () => g.center });
	const color = hot ? pal.hot : pal[p.role];
	return (
		<group onPointerOver={() => hotOn(p.lesson, p.id)} onPointerOut={() => hotOff(p.lesson, p.id)}>
			<mesh geometry={geo} frustumCulled={false} renderOrder={0}>
				<meshBasicMaterial
					color={color}
					transparent
					opacity={hot ? 0.32 : 0.16}
					depthWrite={false}
					side={DoubleSide}
					polygonOffset
					polygonOffsetFactor={1}
					polygonOffsetUnits={1}
				/>
			</mesh>
			<SLine points={g.edge} color={color} lineWidth={hot ? 2.5 : 1.5} renderOrder={1} />
			{lines && lines.length > 0 && <SLine points={lines} segments color={color} lineWidth={1} renderOrder={1} />}
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
	role: Role;
	/** Center text: fixed, or computed from the area (signed in 2D, |a x b| in 3D). */
	label?: string | ((area: number) => string);
}

export function Parallelogram(p: ParallelogramProps): React.JSX.Element | null {
	const a = usePos(p.lesson, p.a);
	const b = usePos(p.lesson, p.b);
	const o = usePos(p.lesson, p.at ?? (a.length === 3 ? O3 : O2));
	const area = a.length === 2 ? signedArea2(a, b) : norm(cross(a, b));
	// Relative test: collinear vectors leave only rounding noise as "area"; zero vectors give 0 <= 0.
	if (Math.abs(area) <= 1e-9 * norm(a) * norm(b)) return null;
	const text = typeof p.label === 'function' ? p.label(area) : p.label;
	return <Quad lesson={p.lesson} id={p.id} o={o} u={a} v={b} role={p.role} neg={a.length === 2 && area < 0} text={text} />;
}
