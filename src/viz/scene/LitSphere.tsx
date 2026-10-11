/**
 * A sphere lit by a point light with an unlit custom shader that computes Lambert's term per fragment, so the
 * brightness the reader sees IS max(0, N·L) (visual-language.md "Lit sphere"; DP-4). N = the exact sphere normal at
 * the fragment (from the center, not an interpolated vertex normal), L = the direction to the light at `light` (a store
 * key, read as drawn, so an eased light glides). `lambert`: mix(dark, base, max(0, N·L)). `raw` (the "show raw N·L"
 * checkbox): negative N·L, which max() throws away, is painted mix(dark, neg, -N·L) in a cool tint.
 *
 * The mix happens in display (sRGB) values and the shader writes them unconverted: the canvas is `flat` (no tone
 * mapping) and the shader has no colorspace chunk, so a pixel's channel is exactly dark + (base - dark) * k. That
 * makes the picture a measurement: the CDP check reads k back from the pixel within 1/255. No scene lights exist; the
 * lamp glyph is a separate Point in the `light` role. Not hoverable (DP-4: "sphere: not hoverable").
 */
import { useThree } from '@react-three/fiber';
import { useEffect, useLayoutEffect, useMemo } from 'react';
import { ShaderMaterial, SphereGeometry, Vector3 } from 'three';
import { to3, type V } from '../geom';
import { usePalette, type Role } from '../palette';
import { usePos, useStage } from './handles';

export interface LitSphereProps {
	lesson: string;
	id: string;
	center: V;
	r: number;
	/** Store key of the point light position (3D). */
	light: string;
	mode: 'lambert' | 'raw';
	/** Palette roles: lit albedo, unlit (default the viewport's bottom color), negative tint for `raw` (default axisZ). */
	roles: { base: Role; dark?: Role; neg?: Role };
}

const VERT = /* glsl */ `
varying vec3 vPos;
void main() {
	vec4 w = modelMatrix * vec4(position, 1.0);
	vPos = w.xyz;
	gl_Position = projectionMatrix * viewMatrix * w;
}`;

const FRAG = /* glsl */ `
uniform vec3 uCenter;
uniform vec3 uLight;
uniform vec3 uBase;
uniform vec3 uDark;
uniform vec3 uNeg;
uniform float uRaw;
varying vec3 vPos;
void main() {
	vec3 N = normalize(vPos - uCenter);
	vec3 L = normalize(uLight - vPos);
	float k = dot(N, L);
	vec3 c = k >= 0.0 ? mix(uDark, uBase, k) : (uRaw > 0.5 ? mix(uDark, uNeg, -k) : uDark);
	gl_FragColor = vec4(c, 1.0);
}`;

/** Writes #rrggbb into `v` as 0..1 channels, deliberately without sRGB-to-linear conversion (see the module note). */
function setRgb(v: Vector3, hex: string): void {
	const m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(hex.trim());
	if (!m) throw new Error(`LitSphere: expected #rrggbb, got "${hex}"`);
	v.set(parseInt(m[1], 16) / 255, parseInt(m[2], 16) / 255, parseInt(m[3], 16) / 255);
}

export function LitSphere(p: LitSphereProps): React.JSX.Element {
	const { mode } = useStage();
	if (mode !== '3d') throw new Error(`LitSphere "${p.id}": needs a 3D stage`);
	if (!(p.r > 0) || !Number.isFinite(p.r)) throw new Error(`LitSphere "${p.id}": r must be finite and > 0, got ${p.r}`);
	if (p.center.length !== 3) throw new Error(`LitSphere "${p.id}": center must be 3D`);
	const pal = usePalette();
	const invalidate = useThree((s) => s.invalidate);
	const light = usePos(p.lesson, p.light);
	if (light.length !== 3) throw new Error(`LitSphere "${p.id}": light "${p.light}" must be a 3D vector`);
	const geo = useMemo(() => new SphereGeometry(p.r, 96, 64), [p.r]);
	const mat = useMemo(
		() =>
			new ShaderMaterial({
				vertexShader: VERT,
				fragmentShader: FRAG,
				uniforms: {
					uCenter: { value: new Vector3() },
					uLight: { value: new Vector3() },
					uBase: { value: new Vector3() },
					uDark: { value: new Vector3() },
					uNeg: { value: new Vector3() },
					uRaw: { value: 0 },
				},
			}),
		[],
	);
	useEffect(
		() => () => {
			geo.dispose();
		},
		[geo],
	);
	useEffect(() => () => mat.dispose(), [mat]);
	const base = pal[p.roles.base];
	const dark = pal[p.roles.dark ?? 'bgBottom'];
	const neg = pal[p.roles.neg ?? 'axisZ'];
	const cKey = p.center.join();
	const lKey = light.join();
	// Uniforms are written after the commit and a frame is requested, so the result never depends on whether the
	// store's invalidate or React's commit runs first. Colours change only with the theme: parsed when they change.
	useLayoutEffect(() => {
		const u = mat.uniforms;
		(u.uCenter.value as Vector3).set(...to3(p.center));
		(u.uLight.value as Vector3).set(...to3(light));
		u.uRaw.value = p.mode === 'raw' ? 1 : 0;
		invalidate();
	}, [mat, cKey, lKey, p.mode, invalidate]);
	useLayoutEffect(() => {
		const u = mat.uniforms;
		setRgb(u.uBase.value as Vector3, base);
		setRgb(u.uDark.value as Vector3, dark);
		setRgb(u.uNeg.value as Vector3, neg);
		invalidate();
	}, [mat, base, dark, neg, invalidate]);
	return <mesh geometry={geo} material={mat} position={to3(p.center)} renderOrder={0} />;
}
