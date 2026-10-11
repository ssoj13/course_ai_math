/**
 * A straight segment between two points (literals or store keys); hover lights it (hot outline). Three looks
 * (visual-language.md): a beam (solid, core + halo, the default), a dashed drop line (thin, no halo) and a
 * projection shadow band (a soft 6 px translucent strip on the target line). Beams and bands draw in from `a`.
 */
import { useFrame } from '@react-three/fiber';
import { useContext, useMemo, useRef } from 'react';
import { to3, type V } from '../geom';
import { LookCtx, assertSubject } from './look';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useHoverable, usePos, useStage } from './handles';
import { Stroke, type StrokeHandle } from './view';

export interface SegmentProps {
	lesson: string;
	id: string;
	a: V | string;
	b: V | string;
	/** Dashed drop line (thin, no halo). */
	dashed?: boolean;
	/** Projection shadow band (wide, soft, no halo). */
	band?: boolean;
	role: SceneRole;
}

const DASH: readonly [number, number] = [7, 5];

export function Segment(p: SegmentProps): React.JSX.Element {
 assertSubject(useContext(LookCtx), !p.dashed && !p.band);
	if (p.dashed && p.band) throw new Error(`Segment "${p.id}": a segment is either dashed or a band`);
	const reg = useStage();
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const a = usePos(p.lesson, p.a);
	const b = usePos(p.lesson, p.b);
	const pts = useMemo(() => [to3(a), to3(b)], [a, b]);
	const hover = useHoverable(p.lesson, p.id, p.band ? HOVER_RANK.fill : p.dashed ? HOVER_RANK.guide : HOVER_RANK.beam);
	const line = useRef<StrokeHandle>(null);
	const drawn = useRef<{ geom: unknown; r: number }>({ geom: null, r: 1 });
	// Draw-in: the segment grows from `a`; only while revealing (or right after drei rebuilt the geometry mid-reveal).
	useFrame(() => {
		const s = line.current;
		if (!s) return;
		const r = reg.reveal;
		if (drawn.current.r === r && drawn.current.geom === s.geometry()) return;
		const [p0, p1] = pts;
		s.setPositions([...p0, ...p0.map((x, i) => x + (p1[i] - x) * r)]);
		drawn.current = { geom: s.geometry(), r };
	});
	const look = p.band
		? { width: STYLE.bandPx, halo: false, opacity: STYLE.bandA }
		: p.dashed
			? { width: STYLE.edge, halo: false, dashPx: DASH }
			: { width: STYLE.core, halo: true };
	// Handlers live on a group: drei's Line spreads its props onto the material too, which has no raycast.
	return (
		<group {...hover}>
			<Stroke ref={line} points={pts} color={pal[p.role]} hot={hot} order={p.band ? 0.5 : 1} {...look} />
		</group>
	);
}
