/**
 * A dot at `at` (literal or store key), pixel-sized so it reads the same at any zoom. With a key it is draggable by
 * default (`drag={false}` pins it). Hover, keyboard focus or a drag light it (`hot`).
 */
import { useRef } from 'react';
import { CircleGeometry, SphereGeometry, type Group } from 'three';
import { to3, type V } from '../geom';
import { usePalette, type Role } from '../palette';
import { hotOff, hotOn, useHot } from '../store';
import { useDraggable, usePos, useStage } from './handles';
import { usePxScale } from './view';

export interface PointProps {
	lesson: string;
	id: string;
	at: V | string;
	drag?: boolean;
	/** Name used in the keyboard button label ("Move point P"); defaults to `id`. */
	label?: string;
	role?: Role;
}

// Unit shapes scaled to the pixel radius per frame; shared by every point.
const DISC = new CircleGeometry(1, 24);
const BALL = new SphereGeometry(1, 16, 12);

export function Point(p: PointProps): React.JSX.Element {
	const reg = useStage();
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const at = usePos(p.lesson, p.at);
	const key = p.at;
	const drag = p.drag ?? typeof key === 'string';
	if (drag && typeof key !== 'string') throw new Error(`Point "${p.id}": drag needs \`at\` to be a store key`);
	useDraggable(
		drag && typeof key === 'string'
			? { id: `${p.id}:pt`, owner: p.id, label: `Move point ${p.label ?? p.id}`, lesson: p.lesson, keys: [key], at: (v) => v[key] as V }
			: null,
	);
	const dot = useRef<Group>(null);
	usePxScale(dot, hot ? 7 : 5);
	return (
		<group ref={dot} position={to3(at)} onPointerOver={() => hotOn(p.lesson, p.id)} onPointerOut={() => hotOff(p.lesson, p.id)}>
			<mesh geometry={reg.mode === '3d' ? BALL : DISC} renderOrder={3}>
				<meshBasicMaterial color={hot ? pal.hot : pal[p.role ?? 'ink']} />
			</mesh>
		</group>
	);
}
