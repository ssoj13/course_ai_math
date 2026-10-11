/**
 * A point at `at` (literal or store key), drawn like a Blender vertex: a pixel-sized disc with a dark rim, facing the
 * camera in 3D, the same size at any zoom. With a key it is draggable by default (`drag={false}` pins it). Hover,
 * keyboard focus or a drag light it (hot = an orange ring outside the rim; the disc keeps its color).
 */
import { useRef } from 'react';
import { CircleGeometry, type Group } from 'three';
import { to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useDraggable, useHoverable, usePos, useStage } from './handles';
import { usePxScale } from './view';

export interface PointProps {
	lesson: string;
	id: string;
	at: V | string;
	drag?: boolean;
	/** Name used in the keyboard button label ("Move point P"); defaults to `id`. */
	label?: string;
	role?: SceneRole;
}

// Unit disc scaled to the pixel radius per frame; shared by every point (the rim and ring are scaled copies).
const DISC = new CircleGeometry(1, 28);
const RIM = (STYLE.pointR + STYLE.rim) / STYLE.pointR;
const RING = (STYLE.pointR + STYLE.rim + STYLE.hotPad) / STYLE.pointR;

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
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.point);
	const dot = useRef<Group>(null);
	// Draw-in: points pop in over the second half of the reveal.
	usePxScale(dot, (r) => STYLE.pointR * Math.min(1, Math.max(0, r * 2 - 1)), reg.mode === '3d');
	return (
		<group ref={dot} position={to3(at)} {...hover}>
			{hot && (
				<mesh geometry={DISC} scale={RING} renderOrder={2.7}>
					<meshBasicMaterial color={pal.hot} transparent depthWrite={false} />
				</mesh>
			)}
			<mesh geometry={DISC} scale={RIM} renderOrder={2.8}>
				<meshBasicMaterial color={pal.bgBottom} transparent depthWrite={false} />
			</mesh>
			<mesh geometry={DISC} renderOrder={2.9}>
				<meshBasicMaterial color={pal[p.role ?? 'ink']} transparent depthWrite={false} />
			</mesh>
		</group>
	);
}
