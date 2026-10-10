/**
 * A vector drawn as an arrow from `from` (default: the origin) to `to`. Either end is a literal or a store key; with
 * keys the arrow is live and can be dragged: 'tip' moves `to`, 'whole' translates both ends (a free vector).
 * Default drag: 'tip' when `to` is a key, else 'none'. Hover, keyboard focus or a drag light it (`hot`).
 * Data is y-up in 3D (three.js convention), so a 3D lesson's [x, y, z] is drawn as given.
 */
import { useFrame } from '@react-three/fiber';
import { useMemo, useRef } from 'react';
import { BufferGeometry, ConeGeometry, DoubleSide, Float32BufferAttribute, Quaternion, Vector3, type Group } from 'three';
import { norm, scale, sub, to3, type V } from '../geom';
import { usePalette, type Role } from '../palette';
import { hotOff, hotOn, useHot } from '../store';
import { useDraggable, usePos, useStage, type Draggable } from './handles';
import { SLine, wppAt } from './view';

export interface ArrowProps {
	lesson: string;
	id: string;
	from?: V | string;
	to: V | string;
	role: Role;
	/** Name used in the keyboard button label ("Move tip of a"); defaults to `id`. */
	label?: string;
	drag?: 'tip' | 'whole' | 'none';
	/** Dashed component legs from the tip to the axes. */
	legs?: boolean;
}

const HEAD_PX = 14;
const W = 3;
const W_HOT = 5;
const DASH: readonly [number, number] = [5, 4];
const O2: V = [0, 0];
const O3: V = [0, 0, 0];
const UP = new Vector3(0, 1, 0);

// Unit heads with the apex at the origin pointing +y; scaled to HEAD_PX per frame. Shared by every arrow.
const TRI = new BufferGeometry().setAttribute('position', new Float32BufferAttribute([0, 0, 0, -0.36, -1, 0, 0.36, -1, 0], 3));
const CONE = new ConeGeometry(0.32, 1, 20).translate(0, -0.5, 0);

/** Drag spec for the arrow, or null; asking to drag a literal end is an authoring error. */
function dragSpec(p: ArrowProps, mode: 'tip' | 'whole' | 'none'): Draggable | null {
	if (mode === 'none') return null;
	const name = p.label ?? p.id;
	const { to, from } = p;
	if (typeof to !== 'string') throw new Error(`Arrow "${p.id}": drag="${mode}" needs \`to\` to be a store key`);
	if (mode === 'tip') return { id: `${p.id}:tip`, owner: p.id, label: `Move tip of ${name}`, lesson: p.lesson, keys: [to], at: (v) => v[to] as V };
	if (typeof from !== 'string') throw new Error(`Arrow "${p.id}": drag="whole" needs \`from\` to be a store key`);
	// The handle sits mid-shaft so it never competes with another arrow's tip; `from` is the anchor that snaps.
	return {
		id: `${p.id}:whole`,
		owner: p.id,
		label: `Move ${name}`,
		lesson: p.lesson,
		keys: [from, to],
		at: (v) => (v[from] as V).map((x, i) => (x + (v[to] as V)[i]) / 2),
	};
}

/** Legs from the tip to the axes: 2D drops to both axes; 3D drops to the floor (y = 0), then to the x and z axes. */
function legPoints(t: readonly number[]): [number, number, number][] {
	if (t.length === 2) {
		const [x, y] = t;
		return [[x, y, 0], [x, 0, 0], [x, y, 0], [0, y, 0]];
	}
	const [x, y, z] = t;
	return [[x, y, z], [x, 0, z], [x, 0, z], [x, 0, 0], [x, 0, z], [0, 0, z]];
}

export function Arrow(p: ArrowProps): React.JSX.Element {
	const reg = useStage();
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const to = usePos(p.lesson, p.to);
	const from = usePos(p.lesson, p.from ?? (to.length === 3 ? O3 : O2));
	const mode = p.drag ?? (typeof p.to === 'string' ? 'tip' : 'none');
	useDraggable(dragSpec(p, mode));

	const g = useMemo(() => {
		const a = to3(from);
		const b = to3(to);
		const len = norm(sub(b, a));
		const dir = len > 1e-9 ? scale(sub(b, a), 1 / len) : [0, 1, 0];
		return { a, b, len, dir, q: new Quaternion().setFromUnitVectors(UP, new Vector3(...dir)), shaft: [a, b] };
	}, [from, to]);
	const legs = useMemo(() => (p.legs ? legPoints(to) : null), [p.legs, to]);

	const head = useRef<Group>(null);
	const shaft = useRef<React.ComponentRef<typeof SLine>>(null);
	const last = useRef({ geom: null as unknown, hl: -1 });
	useFrame(({ camera }) => {
		const el = reg.el;
		const h = head.current;
		const s = shaft.current;
		if (!el || !h || !s) return;
		const hl = Math.min((hot ? HEAD_PX * 1.2 : HEAD_PX) * wppAt(camera, g.b, el.clientHeight), 0.45 * g.len);
		h.scale.setScalar(hl);
		// The shaft stops inside the head: the line's round cap would otherwise poke out past the apex.
		if (last.current.geom !== s.geometry || last.current.hl !== hl) {
			const end = sub(g.b, scale(g.dir, hl * 0.6));
			s.geometry.setPositions([...g.a, ...end]);
			last.current = { geom: s.geometry, hl };
		}
	});

	const color = hot ? pal.hot : pal[p.role];
	return (
		<group>
			{legs && <SLine points={legs} segments color={pal.muted} lineWidth={1.5} dashPx={DASH} renderOrder={1} />}
			<group
				visible={g.len > 1e-9}
				onPointerOver={() => hotOn(p.lesson, p.id)}
				onPointerOut={() => hotOff(p.lesson, p.id)}
			>
				<SLine ref={shaft} points={g.shaft} color={color} lineWidth={hot ? W_HOT : W} renderOrder={2} />
				<group ref={head} position={g.b} quaternion={g.q}>
					<mesh geometry={reg.mode === '3d' ? CONE : TRI} renderOrder={2}>
						<meshBasicMaterial color={color} side={DoubleSide} />
					</mesh>
				</group>
			</group>
		</group>
	);
}
