/**
 * A vector drawn as a beam of light from `from` (default: the origin) to `to`: a 2.5 px core with a soft halo and a
 * sharp head with a slightly concave base (visual-language.md). Either end is a literal or a store key; with keys
 * the arrow is live and can be dragged: 'tip' moves `to`, 'whole' translates both ends (a free vector). Default drag:
 * 'tip' when `to` is a key, else 'none'. Hover, keyboard focus or a drag light it (hot = Blender's active outline
 * around shaft and head; its own color stays). Data is y-up in 3D (three.js convention).
 */
import { useFrame } from '@react-three/fiber';
import { useMemo, useRef } from 'react';
import { BufferGeometry, ConeGeometry, DoubleSide, Float32BufferAttribute, Quaternion, Vector3, type Group, type Mesh } from 'three';
import { norm, scale, sub, to3, type V } from '../geom';
import { STYLE, usePalette, type SceneRole } from '../palette';
import { useHot } from '../store';
import { HOVER_RANK, useDraggable, useHoverable, usePos, useStage, type Draggable } from './handles';
import { SLine, Stroke, wppAt, type StrokeHandle } from './view';

export interface ArrowProps {
	lesson: string;
	id: string;
	from?: V | string;
	to: V | string;
	role: SceneRole;
	/** Name used in the keyboard button label ("Move tip of a"); defaults to `id`. */
	label?: string;
	drag?: 'tip' | 'whole' | 'none';
	/** Dashed component legs from the tip to the axes. */
	legs?: boolean;
}

const O2: V = [0, 0];
const O3: V = [0, 0, 0];
const UP = new Vector3(0, 1, 0);
const tip = new Vector3();
const dirV = new Vector3();

// Unit heads with the apex at the origin pointing +y, scaled to the head length per frame; shared by every arrow.
// 2D: a sharp triangle whose base is notched 22% (the concave base of visual-language.md).
const TRI = new BufferGeometry().setAttribute(
	'position',
	new Float32BufferAttribute([0, 0, 0, -0.36, -1, 0, 0, -0.78, 0, 0, 0, 0, 0, -0.78, 0, 0.36, -1, 0], 3),
);
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

// Rim of the outline and halo heads around the core head, in px per side, matching the shaft's rims (the outline is
// `hotPad` wider, the halo reaches half of the halo stroke beyond the core).
const PAD_OUT = STYLE.hotPad;
const PAD_HALO = (STYLE.halo - STYLE.core) / 2;

export function Arrow(p: ArrowProps): React.JSX.Element {
	const reg = useStage();
	const pal = usePalette();
	const hot = useHot(p.lesson, p.id);
	const to = usePos(p.lesson, p.to);
	const from = usePos(p.lesson, p.from ?? (to.length === 3 ? O3 : O2));
	const mode = p.drag ?? (typeof p.to === 'string' ? 'tip' : 'none');
	useDraggable(dragSpec(p, mode));
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.beam);

	const g = useMemo(() => {
		const a = to3(from);
		const b = to3(to);
		const len = norm(sub(b, a));
		const dir = len > 1e-9 ? scale(sub(b, a), 1 / len) : [0, 1, 0];
		return { a, b, len, dir, q: new Quaternion().setFromUnitVectors(UP, new Vector3(...dir)), shaft: [a, b] };
	}, [from, to]);
	const legs = useMemo(() => (p.legs ? legPoints(to) : null), [p.legs, to]);

	const head = useRef<Group>(null);
	const out = useRef<Mesh>(null);
	const glow = useRef<Mesh>(null);
	const shaft = useRef<StrokeHandle>(null);
	const last = useRef({ geom: null as unknown, end: '' });
	useFrame(({ camera }) => {
		const el = reg.el;
		const h = head.current;
		const s = shaft.current;
		if (!el || !h || !s) return;
		// Draw-in: the beam grows from its tail; the head rides on the growing tip.
		const r = reg.reveal;
		dirV.set(g.dir[0], g.dir[1], g.dir[2]);
		tip.set(g.a[0], g.a[1], g.a[2]).addScaledVector(dirV, g.len * r);
		const wpp = wppAt(camera, g.b, el.clientHeight);
		const hl = Math.min(STYLE.headPx * wpp, 0.45 * g.len * r);
		h.position.copy(tip);
		h.visible = r > 0;
		h.scale.setScalar(Math.max(hl, 1e-9));
		// Outline and halo heads: the head grown about its middle so the rim shows evenly at apex, sides and base
		// (2.4 px of growth per px of pad brings the slanted sides out by about `pad`).
		const rim = (m: Mesh | null, pad: number) => {
			if (!m || hl <= 0) return;
			const k = 1 + (2.4 * pad * wpp) / hl;
			m.scale.setScalar(k);
			m.position.set(0, 0.5 * (k - 1), 0);
		};
		rim(out.current, PAD_OUT);
		rim(glow.current, PAD_HALO);
		// The shaft stops inside the head: the line's round cap would otherwise poke out past the apex.
		const end = [tip.x - dirV.x * hl * 0.6, tip.y - dirV.y * hl * 0.6, tip.z - dirV.z * hl * 0.6];
		const key = end.join();
		if (last.current.geom !== s.geometry() || last.current.end !== key) {
			s.setPositions([...g.a, ...end]);
			last.current = { geom: s.geometry(), end: key };
		}
	});

	const color = pal[p.role];
	const geo = reg.mode === '3d' ? CONE : TRI;
	return (
		<group>
			{legs && <SLine points={legs} segments color={pal.muted} lineWidth={STYLE.edge} dashPx={STYLE.dash} transparent depthWrite={false} renderOrder={1} />}
			<group visible={g.len > 1e-9} {...hover}>
				<Stroke ref={shaft} points={g.shaft} color={color} hot={hot} order={2} />
				<group ref={head} position={g.b} quaternion={g.q}>
					<mesh ref={glow} geometry={geo} renderOrder={1.8}>
						<meshBasicMaterial color={hot ? pal.hot : color} transparent opacity={hot ? STYLE.hotHaloA : STYLE.haloA} depthWrite={false} side={DoubleSide} />
					</mesh>
					{hot && (
						<mesh ref={out} geometry={geo} renderOrder={1.9}>
							<meshBasicMaterial color={pal.hot} transparent depthWrite={false} side={DoubleSide} />
						</mesh>
					)}
					<mesh geometry={geo} renderOrder={2}>
						<meshBasicMaterial color={color} transparent depthWrite={false} side={DoubleSide} />
					</mesh>
				</group>
			</group>
		</group>
	);
}
