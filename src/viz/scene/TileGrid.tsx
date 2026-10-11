/**
 * TileGrid (docs/design/part0-1-storyboard.md §7.1): the "square is an area" primitive. A box (or a row) of unit
 * pieces clipped exactly to a real-valued size, so the union of the pieces IS the shape: a side of 2.5 gives 0.5-wide
 * pieces, never a scaled unit body. Blender-like flat look (visual-language 6d): every piece is an opaque body in a
 * muted fill of its role with a 1 px edge frame in the full role colour, so the count is readable as the grid of
 * frames. Whole pieces use `role`, clipped pieces `partialRole`.
 *
 * One InstancedMesh (64 bodies max, one draw call), opaque: three.js does not sort instances, so translucent cubes
 * would flicker or pile into a blob (§7.1 "Material and transparency"). In 3D a face is shaded by its orientation only
 * (top brightest, then the x faces, then the z faces; no lights), like Blender's solid mode without a light rig. Hover
 * dims never use alpha: non-matching pieces mix toward the viewport background in their instance colour.
 *
 * The layout comes from the pure tiles.ts (validated, counted, allocation-free); buffers are rebuilt in useFrame only
 * when the drawn (`shown`) values, the hot state or the draw-in changed. Discrete shape changes (`morph`) spring piece by
 * piece between two layouts (a born piece grows out of a stub, a dead one shrinks into one, nothing pops); counts always
 * describe the target. The figure's bounds come from a hidden proxy box, so the home view frames the union.
 */
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import {
	BackSide, BoxGeometry, BufferGeometry, Float32BufferAttribute, InstancedBufferAttribute, InstancedMesh, LineBasicMaterial, LineSegments,
	Mesh, MeshBasicMaterial, ShaderMaterial,
} from 'three';
import { useStore } from 'zustand';
import { usePalette, type Palette, type SceneRole } from '../palette';
import { lessonStore, useHot, useVals, type LessonState } from '../store';
import { HOVER_RANK, useHoverable, useLabel, useStage, type StageReg } from './handles';
import { reducedMotion } from './motion';
import {
	boxMatrix, checkTiles, layoutTiles, MAX_TILES, retarget, stepMotion, tileBounds, tileBuf, tileCount, tileMotion, tileTotal,
	type TileOpts, type TilePlane, type TileShape,
} from './tiles';
import { useWorldPx } from './view';

type Vals = LessonState['vals'];
type Of<T> = T | ((vals: Vals) => T);

export interface TileGridProps {
	lesson: string;
	/** Term id of the whole shape: hover outlines the whole (never one piece) and lights the id everywhere. */
	id: string;
	/** Box or row; a function of the drawn values follows the store (eased writes glide, drags are 1:1). */
	shape: Of<TileShape>;
	plane?: TilePlane;
	/** Row with len <= 1: width of the single body inside a unit outline. */
	fill?: Of<number>;
	origin?: readonly number[];
	/** Colour of whole pieces (default vecA). */
	role?: SceneRole;
	/** Colour of clipped pieces (default light). */
	partialRole?: SceneRole;
	/** Publishes whole / partial / pieces / total on the mesh (`userData.count`) for readers of the scene graph. */
	count?: boolean;
	/** Centre label with the total (integer when every active size is, else 2 decimals); a function adds a prefix. */
	label?: boolean | ((total: string) => string);
	negative?: 'mirror';
	/** Spring between layouts piece by piece (discrete shape changes: power morph, ladder split). */
	morph?: boolean;
	/** Term ids that light only the whole pieces / only the clipped pieces (the others dim). */
	parts?: { whole: string; partial: string };
	/** First-view draw-in: pieces rise band by band over the second half of the stage reveal. */
	rise?: boolean;
}

const resolve = <T,>(v: Of<T>, vals: Vals): T => (typeof v === 'function' ? (v as (x: Vals) => T)(vals) : v);

/** Share of the role colour in a body's fill (the rest is the viewport background): a muted, opaque 6d fill. */
const FILL = 0.38;
/** Share kept by a dimmed (non-matching) piece. */
const DIM = 0.35;

/** Flat shading by face orientation: `up` faces 1, its opposite 0.55, x faces 0.82, the rest 0.68. No lights. */
const VS = /* glsl */ `
varying vec3 vN;
varying vec3 vC;
void main() {
	vN = normal;
	vC = vec3(1.0);
#ifdef USE_INSTANCING_COLOR
	vC = instanceColor;
#endif
	mat4 m = modelMatrix;
#ifdef USE_INSTANCING
	m = m * instanceMatrix;
#endif
	gl_Position = projectionMatrix * viewMatrix * m * vec4(position, 1.0);
}`;
const FS = /* glsl */ `
uniform vec3 up;
varying vec3 vN;
varying vec3 vC;
void main() {
	float u = dot(normalize(vN), up);
	float f = u > 0.5 ? 1.0 : u < -0.5 ? 0.55 : abs(vN.x) > 0.5 ? 0.82 : 0.68;
	gl_FragColor = vec4(vC * f, 1.0);
}`;

const unitBox = new BoxGeometry(1, 1, 1);
const noRaycast = () => {};

// Frame temporaries (one grid updates at a time inside the render loop).
const TARGET = tileBuf();
const BLO = [0, 0, 0];
const BHI = [0, 0, 0];

/**
 * sRGB mix of two '#rrggbb' colours written raw into `out` at `at` (the shader outputs instance colours unconverted,
 * like the flat canvas renders palette hex exactly).
 */
function mixInto(a: string, b: string, t: number, out: Float32Array, at: number): void {
	const pa = parseInt(a.slice(1), 16);
	const pb = parseInt(b.slice(1), 16);
	for (let c = 0; c < 3; c++) {
		const s = 16 - 8 * c;
		out[at + c] = (((pa >> s) & 255) * (1 - t) + ((pb >> s) & 255) * t) / 255;
	}
}

export function TileGrid(p: TileGridProps): React.JSX.Element {
	const vals = useVals(p.lesson);
	const shape = resolve(p.shape, vals);
	const fill = p.fill === undefined ? undefined : resolve(p.fill, vals);
	const opts: TileOpts = { plane: p.plane, origin: p.origin, fill, negative: p.negative };
	// Invalid input is an authoring error, thrown before anything is drawn or published.
	checkTiles(shape, opts);
	const count = tileCount(shape, opts);
	const center = useRef<number[]>([0, 0, 0]);
	if (tileBounds(layoutTiles(shape, opts, TARGET), BLO, BHI)) for (let a = 0; a < 3; a++) center.current[a] = (BLO[a] + BHI[a]) / 2;
	else center.current = [...(p.origin ?? [0, 0]), 0].slice(0, 3);
	const text = p.label ? (typeof p.label === 'function' ? p.label(tileTotal(shape, count.total, fill)) : tileTotal(shape, count.total, fill)) : null;
	const reg = useStage();
	useLabel(text === null ? null : { lesson: p.lesson, text, hot: p.id, at: () => (reg.mode === '2d' ? center.current.slice(0, 2) : center.current) });
	const pal = usePalette();
	const invalidate = useThree((s) => s.invalidate);
	const hot = useHot(p.lesson, p.id);
	const lit = useStore(lessonStore(p.lesson), (s) => (p.parts ? (s.hot.has(p.parts.whole) ? 1 : 0) + (s.hot.has(p.parts.partial) ? 2 : 0) : 0));
	const hover = useHoverable(p.lesson, p.id, HOVER_RANK.fill);
	const pad = useWorldPx(2, [0, 0, 0]) ?? 0;
	const parts = useMemo<Parts>(() => {
		const mesh = new InstancedMesh(unitBox, new ShaderMaterial({ vertexShader: VS, fragmentShader: FS, uniforms: { up: { value: [0, 1, 0] } } }), MAX_TILES);
		mesh.instanceColor = new InstancedBufferAttribute(new Float32Array(3 * MAX_TILES), 3);
		// Instances live anywhere in the stage; the unit geometry's sphere says nothing about them.
		mesh.frustumCulled = false;
		mesh.count = 0;
		mesh.userData.aux = true;
		// Edge frames: 12 box edges per piece (3D) or 4 per piece (2D), plus a filled row's unit outline.
		const lines = new BufferGeometry().setAttribute('position', new Float32BufferAttribute(new Float32Array((MAX_TILES + 1) * 24 * 3), 3));
		lines.setAttribute('color', new Float32BufferAttribute(new Float32Array((MAX_TILES + 1) * 24 * 3), 3));
		const frame = new LineSegments(lines, new LineBasicMaterial({ vertexColors: true }));
		frame.frustumCulled = false;
		frame.raycast = noRaycast;
		frame.userData.aux = true;
		const shell = new Mesh(unitBox, new MeshBasicMaterial({ side: BackSide }));
		shell.raycast = noRaycast;
		shell.userData.aux = true;
		// Hidden box over the union: the figure bounds the home view frames (view.tsx contentBox reads geometry boxes,
		// and an instanced mesh's geometry box is only its unit body).
		const proxy = new Mesh(unitBox, new MeshBasicMaterial());
		proxy.visible = false;
		proxy.raycast = noRaycast;
		return { mesh, frame, shell, proxy, motion: tileMotion(), vals: null, reveal: NaN, hot: false, lit: 0, pad: 0, pal: null, moving: false, outline: { on: false, lo: [0, 0, 0], hi: [0, 0, 0] } };
	}, []);
	useEffect(() => () => {
		parts.mesh.dispose();
		(parts.mesh.material as ShaderMaterial).dispose();
		parts.frame.geometry.dispose();
		(parts.frame.material as LineBasicMaterial).dispose();
		(parts.shell.material as MeshBasicMaterial).dispose();
		(parts.proxy.material as MeshBasicMaterial).dispose();
	}, [parts]);
	if (p.count) parts.mesh.userData.count = count;
	const props = useRef(p);
	props.current = p;
	const flags = useRef<Look>({ hot, lit, pad, pal });
	flags.current = { hot, lit, pad, pal };
	useFrame((_, delta) => {
		if (update(parts, props.current, flags.current, reg, delta * 1000)) invalidate();
	});
	// A hot or palette change needs one frame even when nothing else moves.
	useEffect(() => invalidate(), [hot, lit, pad, pal, invalidate]);
	return (
		<group {...hover}>
			<primitive object={parts.mesh} />
			<primitive object={parts.frame} />
			<primitive object={parts.shell} />
			<primitive object={parts.proxy} />
		</group>
	);
}

interface Parts {
	mesh: InstancedMesh;
	frame: LineSegments;
	shell: Mesh;
	proxy: Mesh;
	motion: ReturnType<typeof tileMotion>;
	/** The store's `shown` record last laid out (a new record per write), and the last applied look inputs. */
	vals: Vals | null;
	reveal: number;
	hot: boolean;
	lit: number;
	pad: number;
	pal: Palette | null;
	/** True while a morph spring runs. */
	moving: boolean;
	/** Unit outline of a filled row, copied from the last layout. */
	outline: { on: boolean; lo: number[]; hi: number[] };
}

type Look = { hot: boolean; lit: number; pad: number; pal: Palette };

/**
 * Rebuilds the drawn pieces when the drawn values, a look input or a running morph changed; true while something
 * still moves. Reads the store directly (`shown`, a new record per write), so an idle frame of the shared canvas costs
 * a few comparisons and a drag frame one layout and one buffer upload, with no React render and no allocation here.
 */
function update(g: Parts, p: TileGridProps, f: Look, reg: StageReg, dtMs: number): boolean {
	const vals = lessonStore(p.lesson).getState().shown;
	const reveal = p.rise ? reg.reveal : 1;
	const looks = g.reveal !== reveal || g.hot !== f.hot || g.lit !== f.lit || g.pad !== f.pad || g.pal !== f.pal;
	if (vals === g.vals && !looks && !g.moving) return false;
	if (vals !== g.vals) {
		const shape = resolve(p.shape, vals);
		const origin = p.origin ?? [0, 0, 0];
		layoutTiles(shape, { plane: p.plane, origin, fill: p.fill === undefined ? undefined : resolve(p.fill, vals), negative: p.negative }, TARGET);
		// Morph only between two real states (not from the first frame), never under reduced motion.
		const cut = !p.morph || reducedMotion() || g.vals === null;
		retarget(g.motion, TARGET, shape.kind === 'row', origin, cut);
		g.moving = !cut;
		g.outline.on = TARGET.hasOutline;
		for (let a = 0; a < 3; a++) {
			g.outline.lo[a] = TARGET.outline.lo[a];
			g.outline.hi[a] = TARGET.outline.hi[a];
		}
		g.vals = vals;
	}
	if (g.moving && !stepMotion(g.motion, dtMs)) g.moving = false;
	g.reveal = reveal;
	g.hot = f.hot;
	g.lit = f.lit;
	g.pad = f.pad;
	g.pal = f.pal;
	write(g, p, f, reg, reveal);
	return g.moving;
}

/** Writes instance matrices/colours, the edge frames, the hot shell and the bounds proxy. */
function write(g: Parts, p: TileGridProps, f: Look, reg: StageReg, reveal: number): void {
	const { mesh, motion: m } = g;
	const pal = f.pal;
	const bg = pal.bgBottom;
	const flat = reg.mode === '2d';
	// The face looking at the 2D camera (+z) or up in 3D (+y) is the brightest.
	const up = (mesh.material as ShaderMaterial).uniforms.up.value as number[];
	up[0] = 0;
	up[1] = flat ? 0 : 1;
	up[2] = flat ? 1 : 0;
	// Draw-in: the bands rise one after another over the second half of the reveal (band 0 = nearest the origin).
	let bands = 1;
	for (let s = 0; s < MAX_TILES; s++) if (m.state[s]) bands = Math.max(bands, m.band[s] + 1);
	const q = Math.min(1, Math.max(0, (reveal - 0.5) * 2));
	const pos = (g.frame.geometry.getAttribute('position') as Float32BufferAttribute).array as Float32Array;
	const col = (g.frame.geometry.getAttribute('color') as Float32BufferAttribute).array as Float32Array;
	const ic = mesh.instanceColor!.array as Float32Array;
	let k = 0;
	let v = 0;
	let any = false;
	for (let a = 0; a < 3; a++) {
		BLO[a] = Infinity;
		BHI[a] = -Infinity;
	}
	for (let s = 0; s < MAX_TILES; s++) {
		if (!m.state[s]) continue;
		const grow = Math.min(1, Math.max(0, q * bands - m.band[s]));
		if (grow <= 0) continue;
		boxMatrix(m.cur, 6 * s, grow, mesh.instanceMatrix.array as Float32Array, 16 * k);
		const whole = m.whole[s] === 1;
		const role = pal[whole ? (p.role ?? 'vecA') : (p.partialRole ?? 'light')];
		// `whole` lit: clipped pieces dim; `partial` lit: whole pieces dim; both lit (a sum): nothing dims.
		const dim = (f.lit === 1 && !whole) || (f.lit === 2 && whole);
		mixInto(bg, role, dim ? FILL * DIM : FILL, ic, 3 * k);
		k++;
		for (let a = 0; a < 3; a++) {
			BLO[a] = Math.min(BLO[a], m.cur[6 * s + a]);
			BHI[a] = Math.max(BHI[a], m.cur[6 * s + 3 + a]);
		}
		any = true;
		if (grow < 1) continue;
		const v0 = v;
		v = flat
			? edges(pos, v, m.cur[6 * s], m.cur[6 * s + 1], m.cur[6 * s + 3], m.cur[6 * s + 4], m.cur[6 * s + 5] + 1e-3, false)
			: boxEdges(pos, v, m.cur, 6 * s);
		for (let i = v0; i < v; i++) mixInto(bg, role, dim ? DIM : 1, col, 3 * i);
	}
	mesh.count = k;
	mesh.instanceMatrix.needsUpdate = true;
	mesh.instanceColor!.needsUpdate = true;
	// Raycasting recomputes these lazily from the instances.
	mesh.boundingSphere = null;
	mesh.boundingBox = null;
	const o = g.outline;
	// The unit outline of a fraction: the frame the fill is a part of (drawn, never counted).
	if (o.on) {
		const xz = p.plane === 'xz';
		const v0 = v;
		v = edges(pos, v, o.lo[0], xz ? o.lo[2] : o.lo[1], o.hi[0], xz ? o.hi[2] : o.hi[1], (xz ? o.hi[1] : o.hi[2]) + 1e-3, xz);
		for (let i = v0; i < v; i++) mixInto(bg, pal.ink, 1, col, 3 * i);
	}
	const lines = g.frame.geometry;
	lines.setDrawRange(0, v);
	lines.getAttribute('position').needsUpdate = true;
	lines.getAttribute('color').needsUpdate = true;
	g.frame.visible = v > 0;
	// Hot: the orange outline around the whole silhouette (back faces of a box ~2 px larger), never per piece.
	g.shell.visible = any && f.hot;
	if (!any) {
		g.proxy.scale.set(1e-6, 1e-6, 1e-6);
		return;
	}
	(g.shell.material as MeshBasicMaterial).color.set(pal.hot);
	const padW = f.pad || 0.03;
	g.shell.position.set((BLO[0] + BHI[0]) / 2, (BLO[1] + BHI[1]) / 2, (BLO[2] + BHI[2]) / 2);
	g.shell.scale.set(BHI[0] - BLO[0] + 2 * padW, BHI[1] - BLO[1] + 2 * padW, BHI[2] - BLO[2] + 2 * padW);
	g.proxy.position.copy(g.shell.position);
	g.proxy.scale.set(BHI[0] - BLO[0], BHI[1] - BLO[1], BHI[2] - BLO[2]);
}

/** Corner picks of a rectangle's four edges as (u, v): 0 = low, 1 = high. */
const EDGES = [0, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 0, 1, 0, 0];

/** Four edges of [u0, u1] x [v0, v1] at depth w into `pos` from vertex `v`: in the xy plane, or (xz) on the floor. */
function edges(pos: Float32Array, v: number, u0: number, v0: number, u1: number, v1: number, w: number, xz: boolean): number {
	for (let i = 0; i < 8; i++) {
		const at = 3 * (v + i);
		const t = EDGES[2 * i + 1] ? v1 : v0;
		pos[at] = EDGES[2 * i] ? u1 : u0;
		pos[at + 1] = xz ? w : t;
		pos[at + 2] = xz ? t : w;
	}
	return v + 8;
}

/** Corner picks (x, y, z) of the 12 edges of a box, two corners per edge. */
const BOX = [
	0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 1, 0, 0, 0, 1, 1, 0, 1, 0, 1, 1, 1, 1, 1,
	0, 0, 0, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 0, 1, 0, 1, 1, 1, 0, 1, 1, 1, 1,
	0, 0, 0, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 1, 1, 0, 1, 1, 1,
];

/** The 12 edges of the box stored as lo/hi at `box[at..at+6]`. */
function boxEdges(pos: Float32Array, v: number, box: ArrayLike<number>, at: number): number {
	for (let i = 0; i < 24; i++)
		for (let a = 0; a < 3; a++) pos[3 * (v + i) + a] = BOX[3 * i + a] ? box[at + 3 + a] : box[at + a];
	return v + 24;
}
