/** A straight segment between two points (literals or store keys), solid or dashed; hover lights it (`hot`). */
import { useMemo } from 'react';
import { to3, type V } from '../geom';
import { usePalette, type Role } from '../palette';
import { hotOff, hotOn, useHot } from '../store';
import { usePos } from './handles';
import { SLine } from './view';

export interface SegmentProps {
	lesson: string;
	id: string;
	a: V | string;
	b: V | string;
	dashed?: boolean;
	role: Role;
}

const DASH: readonly [number, number] = [7, 5];

export function Segment(p: SegmentProps): React.JSX.Element {
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const a = usePos(p.lesson, p.a);
	const b = usePos(p.lesson, p.b);
	const pts = useMemo(() => [to3(a), to3(b)], [a, b]);
	// Handlers live on a group: drei's Line spreads its props onto the material too, which has no raycast.
	return (
		<group onPointerOver={() => hotOn(p.lesson, p.id)} onPointerOut={() => hotOff(p.lesson, p.id)}>
			<SLine points={pts} color={hot ? pal.hot : pal[p.role]} lineWidth={hot ? 4 : 2} dashPx={p.dashed ? DASH : undefined} renderOrder={1} />
		</group>
	);
}
