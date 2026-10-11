/**
 * The dark glossy floor of the holographic look and the figure's reflection in it (visual-language.md v3, reference
 * refs/holo-style-ref.png: the figure stands on a reflective floor that recedes to a horizon of lights).
 *
 * Where the floor is (`reg.frame`, set by the home view): 3D = the plane y = min(0, lowest point of the figure); 2D =
 * the figure is a sheet standing on the line y = frame.y, and the floor is drawn as the ground plane of a perspective
 * view of that sheet: it recedes from the base line up to a horizon `frame.horizon` above it, with grid lines that
 * converge to a vanishing point over the figure's centre. The 2D floor is pure shading of a world-space quad, so it
 * pans and zooms with the drawing and leaves the stage's orthographic camera (hit tests, labels) exact.
 *
 * Reflection = a planar reflection pass: before the View renders, the figure group is rendered once more into this
 * View's own half-resolution render target with a mirror matrix (about the floor plane) on the group. Three flips the
 * face winding for a negative determinant, so solids stay solid; billboards and screen-facing line quads are drawn
 * double-sided for that pass. The floor samples the target at its own screen position, blurred and faded with the
 * distance below the base line. Nothing is rendered twice in React, so handles, labels and hover never see a copy.
 * Scene furniture (`userData.aux`: grids, half-planes) is hidden during the pass.
 */
import { useFrame } from '@react-three/fiber';
import { useEffect, useMemo, useSyncExternalStore, type RefObject } from 'react';
import { Color, DoubleSide, HalfFloatType, Matrix4, ShaderMaterial, Vector2, WebGLRenderTarget, type Group, type Material, type Mesh, type Object3D, type Side } from 'three';
import { usePalette } from '../palette';
import { useStage, type FrameInfo } from './handles';

/** Reflection target resolution relative to the stage's drawing buffer: it is blurred anyway. */
const REFLECT_SCALE = 0.5;
/** Peak strength of the reflection right under the base line. */
const REFLECT_A = 0.42;
/** 2D ground depth of one grid cell, in world units at the base line (foreshortening of the floor). */
const FORE = 0.32;

const VERTEX = /* glsl */ `
varying vec3 wpos;
varying vec4 clip;
void main() {
	vec4 world = modelMatrix * vec4(position, 1.0);
	wpos = world.xyz;
	clip = projectionMatrix * viewMatrix * world;
	gl_Position = clip;
}`;

const FRAGMENT = /* glsl */ `
precision highp float;
varying vec3 wpos;
varying vec4 clip;
uniform sampler2D reflection;
uniform vec2 texel;
uniform float flat2d;
uniform float floorY, horizon, cx, cz, size;
uniform vec3 base, line, glow;

float hash12(vec2 p) {
	vec3 p3 = fract(vec3(p.xyx) * 0.1031);
	p3 += dot(p3, p3.yzx + 33.33);
	return fract((p3.x + p3.y) * p3.z);
}

// Anti-aliased lines at whole ground units, faded where they get denser than ~3 px (no moire toward the horizon).
float gridLines(vec2 g) {
	vec2 w = fwidth(g);
	vec2 d = abs(fract(g - 0.5) - 0.5) / max(w, 1e-5);
	vec2 l = (1.0 - min(d, 1.0)) * (1.0 - smoothstep(0.12, 0.35, w));
	return max(l.x, l.y);
}

// Small lights at some grid crossings, like the reference's horizon of lights.
float sparkles(vec2 g) {
	vec2 cell = floor(g + 0.5);
	float pick = step(0.86, hash12(cell * 1.31 + 7.0));
	vec2 d = (g - cell) / max(fwidth(g), 1e-5);
	return pick * exp(-dot(d, d) * 0.35) * (0.4 + 0.6 * hash12(cell + 3.3));
}

void main() {
	vec2 uv = clip.xy / clip.w * 0.5 + 0.5;
	vec3 c = base;
	float alpha;
	float below;
	if (flat2d > 0.5) {
		// Perspective ground of the 2D sheet: r = distance below the horizon (world units), zg = ground depth
		// (1 at the base line, growing to infinity at the horizon).
		float r = floorY + horizon - wpos.y;
		if (r <= 0.0) discard;
		float zg = horizon / r;
		vec2 g = vec2((wpos.x - cx) * zg, (zg - 1.0) * horizon / ${FORE.toFixed(3)});
		float far = clamp(1.0 / zg, 0.0, 1.0);
		alpha = smoothstep(0.0, 0.7 * horizon, r) * 0.95;
		c += line * gridLines(g) * (0.10 + 0.12 * (1.0 - far));
		// Lights sit on the far part of the floor, toward the horizon, as in the reference.
		c += glow * sparkles(g) * 0.75 * smoothstep(0.25, 0.85, far);
		// A band of light along the horizon, where the floor meets the sky.
		c += glow * exp(-r / (0.05 * horizon)) * 0.18;
		alpha = max(alpha, exp(-r / (0.05 * horizon)) * 0.45);
		below = floorY - wpos.y;
	} else {
		float d = length(wpos.xz - vec2(cx, cz));
		alpha = (1.0 - smoothstep(size * 1.6, size * 5.0, d)) * 0.92;
		// Sparse lights on the floor around the figure, fewer near it.
		c += glow * sparkles(wpos.xz) * 0.45 * smoothstep(size * 0.8, size * 2.2, d) * (1.0 - smoothstep(size * 2.5, size * 5.0, d));
		below = d * 0.35;
	}
	// Glossy sheen under the figure.
	c += line * 0.06 * exp(-abs(wpos.x - cx) / max(size, 1e-3));

	if (below > 0.0) {
		// Blur grows with the distance from the base line (a glossy, not a perfect, mirror).
		float k = 1.0 + 5.0 * clamp(below / max(size, 1e-3), 0.0, 1.0);
		vec2 s = texel * k;
		vec3 refl = texture2D(reflection, uv).rgb * 0.24;
		refl += texture2D(reflection, uv + vec2(s.x, 0.0)).rgb * 0.13;
		refl += texture2D(reflection, uv - vec2(s.x, 0.0)).rgb * 0.13;
		refl += texture2D(reflection, uv + vec2(0.0, s.y)).rgb * 0.13;
		refl += texture2D(reflection, uv - vec2(0.0, s.y)).rgb * 0.13;
		refl += texture2D(reflection, uv + s * 0.7).rgb * 0.06;
		refl += texture2D(reflection, uv - s * 0.7).rgb * 0.06;
		refl += texture2D(reflection, uv + vec2(s.x, -s.y) * 0.7).rgb * 0.06;
		refl += texture2D(reflection, uv + vec2(-s.x, s.y) * 0.7).rgb * 0.06;
		float fade = 1.0 - smoothstep(0.0, 0.6 * size, below);
		fade *= fade;
		c += refl * fade * ${REFLECT_A.toFixed(2)};
	}
	gl_FragColor = vec4(c, alpha);
	#include <colorspace_fragment>
}`;

const clearColor = new Color();

/** Per-pass bookkeeping, reused every frame: furniture hidden and materials made double-sided for the pass. */
interface PassState {
	hidden: Object3D[];
	sided: { m: Material; side: Side }[];
}

/** Hides furniture and makes billboards/line quads double-sided under `root`; returns them for `restore`. */
function prepare(root: Object3D, st: PassState): void {
	st.hidden.length = 0;
	st.sided.length = 0;
	root.traverse((o) => {
		if (o.userData.aux && o.visible) {
			o.visible = false;
			st.hidden.push(o);
		}
		const mesh = o as Mesh;
		const mats = mesh.material;
		if (!mats) return;
		const billboard = mesh.geometry?.type === 'CircleGeometry';
		for (const m of Array.isArray(mats) ? mats : [mats]) {
			if (m.side === DoubleSide || !(billboard || m.type === 'LineMaterial')) continue;
			st.sided.push({ m, side: m.side });
			m.side = DoubleSide;
		}
	});
}

function restore(st: PassState): void {
	for (const o of st.hidden) o.visible = true;
	for (const { m, side } of st.sided) m.side = side;
}

export function Floor({ content }: { content: RefObject<Group | null> }): React.JSX.Element | null {
	const reg = useStage();
	const pal = usePalette();
	const frame: FrameInfo | null = useSyncExternalStore(reg.frame.subscribe, reg.frame.get, reg.frame.get);
	const flat2d = reg.mode === '2d';

	const target = useMemo(() => new WebGLRenderTarget(1, 1, { type: HalfFloatType }), []);
	useEffect(() => () => target.dispose(), [target]);
	const material = useMemo(
		() =>
			new ShaderMaterial({
				vertexShader: VERTEX,
				fragmentShader: FRAGMENT,
				transparent: true,
				depthWrite: false,
				side: DoubleSide,
				uniforms: {
					reflection: { value: target.texture },
					texel: { value: new Vector2(1, 1) },
					flat2d: { value: flat2d ? 1 : 0 },
					floorY: { value: 0 },
					horizon: { value: 1 },
					cx: { value: 0 },
					cz: { value: 0 },
					size: { value: 1 },
					base: { value: new Color(pal.floor) },
					line: { value: new Color(pal.grid).lerp(new Color(pal.vecA), 0.35) },
					glow: { value: new Color(pal.vecA) },
				},
			}),
		[target, flat2d, pal],
	);
	useEffect(() => () => material.dispose(), [material]);
	useEffect(() => {
		if (!frame) return;
		const u = material.uniforms;
		u.floorY.value = frame.y;
		u.horizon.value = Math.max(frame.horizon, 1e-3);
		u.cx.value = frame.cx;
		u.cz.value = frame.cz;
		u.size.value = Math.max(frame.size, 1e-3);
	}, [material, frame]);

	const mirror = useMemo(() => new Matrix4(), []);
	const pass = useMemo<PassState>(() => ({ hidden: [], sided: [] }), []);

	// Priority 0.5: after the figure's own per-frame updates (pixel sizes, trimmed beams) at 0, before the View's
	// render at 1, so the floor of this frame samples this frame's reflection.
	useFrame(({ gl, camera }) => {
		const root = content.current;
		const el = reg.el;
		if (!root || !el || !frame) return;
		const dpr = gl.getPixelRatio();
		const w = Math.max(1, Math.round(el.clientWidth * dpr * REFLECT_SCALE));
		const h = Math.max(1, Math.round(el.clientHeight * dpr * REFLECT_SCALE));
		if (target.width !== w || target.height !== h) target.setSize(w, h);
		(material.uniforms.texel.value as Vector2).set(1 / w, 1 / h);

		// Reflection about the horizontal plane (3D) or line (2D) y = frame.y: y -> 2 y0 - y.
		mirror.makeScale(1, -1, 1).setPosition(0, 2 * frame.y, 0);
		prepare(root, pass);
		root.matrixAutoUpdate = false;
		root.matrix.copy(mirror);
		root.updateMatrixWorld(true);
		const prev = gl.getRenderTarget();
		const autoClear = gl.autoClear;
		gl.getClearColor(clearColor);
		const clearAlpha = gl.getClearAlpha();
		gl.setRenderTarget(target);
		gl.setClearColor(0x000000, 0);
		gl.clear(true, true, false);
		gl.autoClear = false;
		gl.render(root, camera);
		gl.autoClear = autoClear;
		gl.setClearColor(clearColor, clearAlpha);
		gl.setRenderTarget(prev);
		root.matrix.identity();
		root.matrixAutoUpdate = true;
		root.updateMatrixWorld(true);
		restore(pass);
	}, 0.5);

	if (!frame) return null;
	const far = reg.extent * 200;
	if (flat2d) {
		// World-space quad below the horizon, behind the figure (z < 0) and over the 2D grid (render order).
		const top = frame.y + frame.horizon;
		return (
			<mesh position={[frame.cx, top - far / 2, -0.5]} renderOrder={-2} raycast={() => {}} material={material} frustumCulled={false} userData={{ aux: true }}>
				<planeGeometry args={[far, far]} />
			</mesh>
		);
	}
	return (
		<mesh rotation={[-Math.PI / 2, 0, 0]} position={[frame.cx, frame.y - 0.002, frame.cz]} renderOrder={-4} raycast={() => {}} material={material} userData={{ aux: true }}>
			<planeGeometry args={[reg.extent * 16, reg.extent * 16]} />
		</mesh>
	);
}
