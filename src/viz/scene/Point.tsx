/**
 * A point at `at` (literal, store key, or a function of the lesson values for a derived point such as a triangle
 * vertex), drawn like a Blender vertex: a pixel-sized disc with a dark rim, facing the camera in 3D, the same size at
 * any zoom. With a key it is draggable by default (`drag={false}` pins it); `move` replaces the default grid move
 * with a custom mapping (a vertex written in polar terms, a point kept on a sphere; see Draggable.move), which is
 * also what makes a derived point draggable. Hover, keyboard focus or a drag light it (hot = an orange ring outside
 * the rim; the disc keeps its color).
 */
import { useContext, useRef } from 'react';
import { AdditiveBlending, CircleGeometry, type Group } from 'three';
import { to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useHot, useVals, type LessonState } from '../store';
import { HOVER_RANK, useDraggable, useHoverable, usePos, useStage, type Draggable } from './handles';
import { lightened, LookCtx, assertSubject } from './look';
import { usePxScale } from './view';

export interface PointProps {
	lesson: string;
	id: string;
	at: V | string | ((vals: LessonState['vals']) => V);
	drag?: boolean;
	/** Custom drag mapping (see Draggable.move); required to drag a derived (function) point. */
	move?: Draggable['move'];
	/** Store keys `move` writes (identity and docs; default the `at` key). */
	keys?: readonly string[];
	/** Name used in the keyboard button label ("Move point P"); defaults to `id`. */
	label?: string;
	role?: SceneRole;
	size?: number;
}

// Unit disc scaled to the pixel radius per frame; shared by every point (the rim and ring are scaled copies).
const DISC = new CircleGeometry(1, 28);
const RING = (STYLE.pointR + STYLE.rim + STYLE.hotPad) / STYLE.pointR;

export function Point(p: PointProps): React.JSX.Element {
 assertSubject(useContext(LookCtx),true);
	const reg = useStage();
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const src = p.at;
	const fn = typeof src === 'function' ? src : null;
	// The whole value record (stable until a write), not fn(...) in a selector: that would be a new array per call.
	const vals = useVals(p.lesson);
	const derived = fn ? fn(vals) : null;
	const pos = usePos(p.lesson, fn ? [0, 0] : (src as V | string));
	const at = derived ?? pos;
	const drag = p.drag ?? (typeof src === 'string' || Boolean(p.move));
	if (drag && typeof src !== 'string' && !p.move) throw new Error(`Point "${p.id}": drag needs \`at\` to be a store key, or a \`move\``);
	const keys = p.keys ?? (typeof src === 'string' ? [src] : null);
	if (drag && !keys) throw new Error(`Point "${p.id}": a derived draggable point needs \`keys\` (what its move writes)`);
	const atFn = (v: LessonState['vals']): V => (fn ? fn(v) : typeof src === 'string' ? (v[src] as V) : (src as V));
	useDraggable(drag && keys ? { id: `${p.id}:pt`, owner: p.id, label: `Move point ${p.label ?? p.id}`, lesson: p.lesson, keys, at: atFn, move: p.move } : null);
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.point);
	const dot = useRef<Group>(null);
	// Draw-in: points pop in over the second half of the reveal.
	usePxScale(dot, (r) => (p.size ? p.size / 2 : STYLE.pointR) * Math.min(1, Math.max(0, r * 2 - 1)), reg.mode === '3d');
	return (
		<group ref={dot} position={to3(at)} {...hover}>
			<mesh geometry={DISC} scale={STYLE.pointHalo / (p.size ?? 2 * STYLE.pointR)} renderOrder={2.6}><meshBasicMaterial color={pal[p.role ?? 'ink']} blending={AdditiveBlending} opacity={STYLE.pointHaloA} transparent depthWrite={false} /></mesh>
			{hot && (
				<mesh geometry={DISC} scale={RING} renderOrder={2.7}>
					<meshBasicMaterial color={pal.hot} transparent depthWrite={false} />
				</mesh>
			)}
			<mesh geometry={DISC} renderOrder={2.9}>
				<meshBasicMaterial color={lightened(pal[p.role ?? 'ink'])} transparent depthWrite={false} />
			</mesh>
		</group>
	);
}
