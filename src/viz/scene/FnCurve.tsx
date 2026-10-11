/**
 * FnCurve (storyboard §7.13): the graph y = f(t) of a function of the lesson values over [from, to] in a 2D stage,
 * drawn with the scene's one stroke recipe (`Stroke`: core + halo, hot outline), so it is pixel-stable at every zoom.
 * Samples are uniform in t; `samples` must be dense enough for the steepest curve the lesson allows (0.2-5's t^32
 * bends within the last tenth of [0, 1]). The function must be finite on the whole interval: a NaN or an infinity
 * is an authoring error (a gap in a drawn graph would hide exactly the domain problem the reader should see).
 */
import { useMemo } from 'react';
import type { V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useHot, useVals, type LessonState } from '../store';
import { HOVER_RANK, useHoverable } from './handles';
import { Stroke } from './view';

type Vals = LessonState['vals'];

export interface FnCurveProps {
	lesson: string;
	/** Term id: hover lights it everywhere, a hot id draws the hot outline. */
	id: string;
	f: (t: number, vals: Vals) => number;
	from: number;
	to: number;
	/** Number of samples, ends included (at least 2). */
	samples: number;
	role: SceneRole;
	/** Core width in px (default the beam core); a secondary curve is thinner and has no halo. */
	width?: number;
	halo?: boolean;
	dashPx?: readonly [number, number];
}

/** Points of y = f(t) at `samples` uniform t in [from, to]; throws on a non-finite value. */
export function sampleFn(f: (t: number) => number, from: number, to: number, samples: number): V[] {
	if (!Number.isInteger(samples) || samples < 2) throw new Error('FnCurve: samples must be an integer >= 2');
	if (!Number.isFinite(from) || !Number.isFinite(to) || !(to > from)) throw new Error('FnCurve: needs a finite interval from < to');
	const out: V[] = [];
	for (let i = 0; i < samples; i++) {
		// The last sample is exactly `to` (no accumulated rounding at the end point every curve passes through).
		const t = i === samples - 1 ? to : from + ((to - from) * i) / (samples - 1);
		const y = f(t);
		if (!Number.isFinite(y)) throw new Error(`FnCurve: f(${t}) is not finite`);
		out.push([t, y]);
	}
	return out;
}

export function FnCurve(p: FnCurveProps): React.JSX.Element {
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	// Inside a Stage this is the drawn (`shown`) record: an eased write glides the curve with everything else.
	const vals = useVals(p.lesson);
	const { f, from, to, samples } = p;
	const pts = useMemo(() => sampleFn((t) => f(t, vals), from, to, samples).map(([x, y]) => [x, y, 0] as [number, number, number]), [f, vals, from, to, samples]);
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.beam);
	return (
		<group {...hover}>
			<Stroke points={pts} color={pal[p.role]} hot={hot} width={p.width ?? STYLE.core} halo={p.halo ?? true} dashPx={p.dashPx} />
		</group>
	);
}
