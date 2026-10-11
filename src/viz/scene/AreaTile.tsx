/**
 * An area drawn on segment a->b, for "squares on the sides" (Pythagoras, law of cosines, |v|^2 as a sum of tiles).
 *
 * Outward side: the tile lies to the RIGHT of a->b (2D: b - a turned clockwise; 3D: (b - a) x `normal`, the same rule
 * seen from `normal`). Walk a polygon counter-clockwise and the right of every edge is its outside, so tiles on the
 * edges O->A, A->B, B->O of a CCW triangle all face outward; for a clockwise triangle pass `flip` (e.g.
 * `flip={signedArea2(a, b) < 0}` for the triangle O, a, b when the reader can drag it either way).
 *
 * Default: the square |b - a|^2. `signed`: a bar on the same base whose height is `area / |b - a|`, so its length
 * shows a signed area such as the law of cosines' correction -2ab*cos(theta): positive grows outward, negative grows
 * inward and is hatched (sign readable without color), zero draws nothing. A zero-length base also draws nothing (a
 * reader can drag a onto b); mixing `signed` and `area` wrong, or a 3D tile without `normal`, is an authoring error.
 */
import { useMemo } from 'react';
import { useStore } from 'zustand';
import { norm, rightOf, scale, sub, type V } from '../geom';
import type { SceneRole } from '../palette';
import { lessonStore, type LessonState } from '../store';
import { usePos } from './handles';
import { Quad } from './Parallelogram';

export interface AreaTileProps {
	lesson: string;
	id: string;
	a: V | string;
	b: V | string;
	role: SceneRole;
	/** Draw a bar of signed area `area` instead of the square. */
	signed?: boolean;
	/** Signed area for `signed` tiles: a number, or computed from the lesson's values (e.g. -2 a.b). */
	area?: number | ((vals: LessonState['vals']) => number);
	/** Put the tile on the left of a->b instead (clockwise polygons). */
	flip?: boolean;
	/** 3D only: the side is (b - a) x normal. */
	normal?: V;
	/** Center text: fixed, or computed from the area (|b - a|^2 for squares, the signed area for bars). */
	label?: string | ((area: number) => string);
}

export function AreaTile(p: AreaTileProps): React.JSX.Element | null {
	if (p.signed && p.area === undefined) throw new Error(`AreaTile "${p.id}": a signed tile needs \`area\``);
	if (!p.signed && p.area !== undefined) throw new Error(`AreaTile "${p.id}": \`area\` is only for signed tiles (a square's area is |b - a|^2)`);
	const a = usePos(p.lesson, p.a);
	const b = usePos(p.lesson, p.b);
	const src = p.area;
	const signedArea = useStore(lessonStore(p.lesson), (s) => (typeof src === 'function' ? src(s.vals) : (src ?? 0)));
	if (!Number.isFinite(signedArea)) throw new Error(`AreaTile "${p.id}": area is ${signedArea}`);
	const nKey = p.normal?.join();
	// u and v are memoized by value: Quad rebuilds its geometry and hatch when their identities change, and an unrelated
	// store write (another key) re-renders this tile with equal numbers.
	const g = useMemo(() => {
		const side = rightOf(a, b, p.normal);
		const u = sub(b, a);
		const len = norm(u);
		const h = p.signed ? signedArea / len : len;
		// Nothing to draw: zero base (side undefined) or a zero signed area (e.g. the correction term at 90 degrees).
		if (!side || !(Math.abs(h) > 1e-9 * len)) return null;
		return { o: a, u, v: scale(side, p.flip ? -h : h), len };
		// By value: a literal `a`/`b` prop is a new array on every render.
	}, [a.join(), b.join(), nKey, p.signed, p.flip, signedArea]);
	if (!g) return null;
	const area = p.signed ? signedArea : g.len * g.len;
	const text = typeof p.label === 'function' ? p.label(area) : p.label;
	return (
		<Quad
			lesson={p.lesson}
			id={p.id}
			o={g.o}
			u={g.u}
			v={g.v}
			role={p.role}
			neg={Boolean(p.signed) && signedArea < 0}
			text={text}
		/>
	);
}
