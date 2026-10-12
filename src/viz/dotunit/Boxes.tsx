/**
 * Instanced renderer of a DotUnit `Frame` (choreo.ts): every leg, tile, stack segment, bar and operand cell of a scene
 * is one box instance of ONE InstancedMesh (one draw call, spec §1.5). Per-instance attributes: colour (role),
 * `aInfo` = (alpha = fade-in progress, hatch = 0..1 wipe of the negative sign, solid, flash/hot). The fragment shader
 * draws the 6d Blender look without line geometry: flat tiles mix the role colour into the viewport background at the
 * area-fill share with a 1.5 px edge in the full role colour, solid bodies are shaded by face orientation (like
 * TileGrid), negatives get 1 px hatch stripes every 6 CSS px at 45 degrees (screen space, so pixel-stable at any zoom),
 * and a hot or flashing instance gets the orange edge. Opaque: three.js does not sort instances.
 *
 * Per frame: `update(dtMs)` (the scene's choreography) rewrites the frame; this component uploads matrices and
 * attributes only while something moves or the hot set / palette changed. No allocation per frame. Hover and click
 * resolve the instance id to the frame's term id (hot is shared with the formula, the table and the N-panel).
 */
import { useFrame, useThree, type ThreeEvent } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import { BoxGeometry, Color, InstancedBufferAttribute, InstancedMesh, Mesh, MeshBasicMaterial, ShaderMaterial } from 'three';
import { STYLE, usePalette, type Palette } from '../palette';
import { hotOff, hotOn, lessonStore } from '../store';
import { useStage } from '../scene/handles';
import { ROLES, type Frame } from './choreo';

const VS = /* glsl */ `
attribute vec4 aInfo;
varying vec3 vC;
varying vec4 vInfo;
varying vec3 vLocal;
varying vec3 vN;
void main() {
	vC = instanceColor;
	vInfo = aInfo;
	vLocal = position;
	vN = normal;
	gl_Position = projectionMatrix * viewMatrix * modelMatrix * instanceMatrix * vec4(position, 1.0);
}`;

const FS = /* glsl */ `
uniform vec3 bg;
uniform vec3 hot;
uniform float fill;
uniform float dpr;
varying vec3 vC;
varying vec4 vInfo;
varying vec3 vLocal;
varying vec3 vN;
void main() {
	float alpha = vInfo.x, hatch = vInfo.y, solid = vInfo.z, flash = vInfo.w;
	if (alpha < 0.01) discard;
	vec3 n = abs(vN);
	// In-face coordinates of this face (the two local axes other than its normal), in [-0.5, 0.5].
	vec2 uv = n.x > 0.5 ? vLocal.yz : n.y > 0.5 ? vLocal.xz : vLocal.xy;
	vec2 px = (0.5 - abs(uv)) / max(fwidth(uv), vec2(1e-6));
	float edgePx = min(px.x, px.y) / dpr;
	vec3 ink = flash > 0.5 ? hot : vC;
	float edgeW = flash > 0.5 ? ${(STYLE.edge + STYLE.hotPad).toFixed(1)} : ${STYLE.edge.toFixed(1)};
	vec3 c;
	if (solid > 0.5) {
		float shade = n.y > 0.5 ? 1.0 : n.x > 0.5 ? 0.82 : 0.9;
		c = vC * shade;
		if (flash > 0.5 && edgePx < edgeW) c = hot;
	} else {
		c = mix(bg, vC, fill);
		// Hatch wipe: stripes appear left to right over the face as hatch goes 0 -> 1.
		float stripe = mod((gl_FragCoord.x + gl_FragCoord.y) / dpr, ${STYLE.hatchPx.toFixed(1)});
		if (hatch > 0.0 && uv.x + 0.5 <= hatch && stripe < ${STYLE.hatchW.toFixed(1)}) c = ink;
		if (edgePx < edgeW) c = ink;
	}
	gl_FragColor = vec4(mix(bg, c, alpha), 1.0);
}`;

const unitBox = new BoxGeometry(1, 1, 1);
const noRaycast = () => {};
const col = new Color();

export interface BoxesProps {
	lesson: string;
	/** The frame to draw (owned by the scene; its capacity fixes the instance buffers). */
	frame: Frame;
	/**
	 * Advances the scene's choreography by `dtMs` and rewrites `frame`; returns 'still' when nothing changed since the
	 * last call, 'moved' when the frame changed and is now at rest, 'moving' while it keeps changing.
	 */
	update(dtMs: number): 'still' | 'moved' | 'moving';
	/** A click on an instance (dive into a cell). */
	onPick?(id: string, index: number): void;
	/** World margin added around the drawn boxes for the home view (their DOM number labels hang outside). */
	pad?: number;
}

export function Boxes({ lesson, frame, update, onPick, pad = 0.6 }: BoxesProps): React.JSX.Element {
	const pal = usePalette();
	const reg = useStage();
	const invalidate = useThree((s) => s.invalidate);
	const parts = useMemo(() => {
		const cap = Math.max(1, frame.cap);
		const mat = new ShaderMaterial({
			vertexShader: VS,
			fragmentShader: FS,
			uniforms: { bg: { value: new Color() }, hot: { value: new Color() }, fill: { value: STYLE.fillA }, dpr: { value: 1 } },
		});
		const mesh = new InstancedMesh(unitBox, mat, cap);
		mesh.instanceColor = new InstancedBufferAttribute(new Float32Array(3 * cap), 3);
		mesh.geometry = unitBox.clone();
		mesh.geometry.setAttribute('aInfo', new InstancedBufferAttribute(new Float32Array(4 * cap), 4));
		mesh.frustumCulled = false;
		mesh.count = 0;
		mesh.userData.aux = true;
		// Hidden box over the drawn instances: the figure bounds the home view frames (contentBox skips `aux`).
		const proxy = new Mesh(unitBox, new MeshBasicMaterial());
		proxy.visible = false;
		proxy.raycast = noRaycast;
		return { mesh, proxy, hot: null as ReadonlySet<string> | null, pal: null as Palette | null };
	}, [frame]);
	useEffect(() => () => {
		parts.mesh.geometry.dispose();
		(parts.mesh.material as ShaderMaterial).dispose();
		parts.mesh.dispose();
		(parts.proxy.material as MeshBasicMaterial).dispose();
	}, [parts]);
	const props = useRef({ update, lesson });
	props.current = { update, lesson };

	useFrame((state, delta) => {
		const r = props.current.update(delta * 1000);
		const hot = lessonStore(props.current.lesson).getState().hot;
		if (r === 'still' && hot === parts.hot && pal === parts.pal) return;
		parts.hot = hot;
		parts.pal = pal;
		write(parts.mesh, parts.proxy, frame, pal, hot, state.gl.getPixelRatio(), pad);
		if (r === 'moving') invalidate();
	});
	// The palette or a new frame needs one frame even when nothing moves.
	useEffect(() => invalidate(), [pal, parts, invalidate]);
	useEffect(() => lessonStore(lesson).subscribe((s, prev) => s.hot !== prev.hot && invalidate()), [lesson, invalidate]);

	const idAt = (e: ThreeEvent<PointerEvent | MouseEvent>) => (e.instanceId !== undefined && e.instanceId < frame.n && frame.alpha[e.instanceId] > 0.01 ? frame.id[e.instanceId] : null);
	const last = useRef<string | null>(null);
	const leave = () => {
		if (last.current) {
			hotOff(lesson, last.current);
			reg.hover.clear('object', last.current);
		}
		last.current = null;
	};
	return (
		<group>
			<primitive
				object={parts.mesh}
				onPointerMove={(e: ThreeEvent<PointerEvent>) => {
					const id = idAt(e);
					if (!id) return leave();
					e.stopPropagation();
					if (id === last.current) return;
					leave();
					last.current = id;
					hotOn(lesson, id);
					reg.hover.set({ object: id });
				}}
				onPointerOut={leave}
				onClick={(e: ThreeEvent<MouseEvent>) => {
					const id = idAt(e);
					if (id && onPick) {
						e.stopPropagation();
						onPick(id, e.instanceId!);
					}
				}}
			/>
			<primitive object={parts.proxy} />
		</group>
	);
}

/** Uploads the frame: matrices (scale + translation), colours, info; recomputes the bounds proxy. */
function write(mesh: InstancedMesh, proxy: Mesh, f: Frame, pal: Palette, hot: ReadonlySet<string>, dpr: number, pad: number): void {
	const m = mesh.instanceMatrix.array as Float32Array;
	const ic = mesh.instanceColor!.array as Float32Array;
	const info = (mesh.geometry.getAttribute('aInfo') as InstancedBufferAttribute).array as Float32Array;
	let lx = Infinity, ly = Infinity, lz = Infinity, hx = -Infinity, hy = -Infinity, hz = -Infinity;
	for (let i = 0; i < f.n; i++) {
		const a = f.alpha[i];
		// Hidden instances get a zero scale: nothing to draw, nothing to hit.
		const k = a > 0.01 ? 1 : 0;
		const sx = f.s[3 * i] * k, sy = f.s[3 * i + 1] * k, sz = f.s[3 * i + 2] * k;
		const x = f.c[3 * i], y = f.c[3 * i + 1], z = f.c[3 * i + 2];
		const o = 16 * i;
		m[o] = sx; m[o + 1] = 0; m[o + 2] = 0; m[o + 3] = 0;
		m[o + 4] = 0; m[o + 5] = sy; m[o + 6] = 0; m[o + 7] = 0;
		m[o + 8] = 0; m[o + 9] = 0; m[o + 10] = sz; m[o + 11] = 0;
		m[o + 12] = x; m[o + 13] = y; m[o + 14] = z; m[o + 15] = 1;
		col.set(pal[ROLES[f.role[i]]]);
		ic[3 * i] = col.r; ic[3 * i + 1] = col.g; ic[3 * i + 2] = col.b;
		info[4 * i] = a;
		info[4 * i + 1] = f.hatch[i];
		info[4 * i + 2] = f.solid[i];
		info[4 * i + 3] = f.flash[i] || hot.has(f.id[i]) ? 1 : 0;
		if (k) {
			lx = Math.min(lx, x - sx / 2); hx = Math.max(hx, x + sx / 2);
			ly = Math.min(ly, y - sy / 2); hy = Math.max(hy, y + sy / 2);
			lz = Math.min(lz, z - sz / 2); hz = Math.max(hz, z + sz / 2);
		}
	}
	mesh.count = f.n;
	mesh.instanceMatrix.needsUpdate = true;
	mesh.instanceColor!.needsUpdate = true;
	(mesh.geometry.getAttribute('aInfo') as InstancedBufferAttribute).needsUpdate = true;
	// Raycasting recomputes these lazily from the instances.
	mesh.boundingSphere = null;
	mesh.boundingBox = null;
	const u = (mesh.material as ShaderMaterial).uniforms;
	(u.bg.value as Color).set(pal.bgBottom);
	(u.hot.value as Color).set(pal.hot);
	u.dpr.value = dpr;
	if (lx > hx) proxy.scale.set(1e-6, 1e-6, 1e-6);
	else {
		proxy.position.set((lx + hx) / 2, (ly + hy) / 2, (lz + hz) / 2);
		proxy.scale.set(hx - lx + 2 * pad, hy - ly + 2 * pad, hz - lz + 2 * pad);
	}
}
