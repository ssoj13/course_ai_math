/**
 * Glass face material of the holographic look (visual-language.md "Glass faces", reference refs/holo-style-ref.png):
 * a thin translucent plate in the role colour with
 * - a fine inner grid aligned with the tile's own edges: whole units (a square of side 3 shows its 9 unit cells,
 *   the area it stands for) plus quarter-unit minor lines, each level fading out before it gets denser than a few px;
 * - a gradient (denser toward the tile's base) and a faint holographic haze;
 * - an inner glow along the edges and brighter corners, so the plate reads as lit from its rim.
 * Coordinates come from the `local` attribute: tile-local position in world units along the two edge directions
 * (Quad writes it), so lines stay parallel to the edges for squares, bars and sheared parallelograms alike.
 * Lazy (its own chunk); the face is invalidated on mount so a demand canvas never keeps the loading fallback.
 */
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { Color, DoubleSide } from 'three';
import { STYLE } from '../palette';
import { useStage } from './handles';

const VERTEX = /* glsl */ `
attribute vec2 local;
varying vec2 vLocal;
varying vec2 vUv;
void main() {
	vLocal = local;
	vUv = uv;
	gl_Position = projectionMatrix * modelViewMatrix * vec4(position, 1.0);
}`;

const FRAGMENT = /* glsl */ `
varying vec2 vLocal;
varying vec2 vUv;
uniform vec3 tint;
uniform float alpha, reveal;

float hash12(vec2 p) {
	vec3 p3 = fract(vec3(p.xyx) * 0.1031);
	p3 += dot(p3, p3.yzx + 33.33);
	return fract((p3.x + p3.y) * p3.z);
}
float vnoise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * (3.0 - 2.0 * f);
	return mix(mix(hash12(i), hash12(i + vec2(1.0, 0.0)), u.x), mix(hash12(i + vec2(0.0, 1.0)), hash12(i + vec2(1.0, 1.0)), u.x), u.y);
}
// Anti-aliased lines every 'step' world units; faded as they approach 'minPx' px apart (no moire when zoomed out).
float lines(vec2 p, float step, float minPx) {
	vec2 q = p / step;
	vec2 w = fwidth(q);
	vec2 d = abs(fract(q - 0.5) - 0.5) / max(w, 1e-5);
	vec2 l = (1.0 - min(d, 1.0)) * (1.0 - smoothstep(1.0 / (minPx * 1.6), 1.0 / minPx, w));
	return max(l.x, l.y);
}

void main() {
	// Distance to the nearest edge and corner in px (uv spans the tile 0..1 in each edge direction).
	vec2 e = min(vUv, 1.0 - vUv) / max(fwidth(vUv), 1e-5);
	float edgePx = min(e.x, e.y);
	float cornerPx = length(e);

	float a = mix(alpha, alpha * 0.45, vUv.y);
	a *= 0.85 + 0.3 * vnoise(vLocal * 3.0);
	float major = lines(vLocal, 1.0, 9.0);
	float minor = lines(vLocal, 0.25, 6.0);
	float rim = exp(-edgePx / 14.0) * 0.32 + exp(-edgePx / 3.0) * 0.22;
	float corner = exp(-cornerPx / 9.0) * 0.55;

	vec3 col = tint;
	float glass = a + major * 0.24 + minor * 0.09 + rim + corner;
	// Grid lines and the rim are emissive: a little whiter than the body.
	col = mix(col, vec3(1.0), clamp(major * 0.25 + corner * 0.5 + rim * 0.3, 0.0, 0.6));
	gl_FragColor = vec4(col, clamp(glass, 0.0, 1.0) * reveal);
	#include <tonemapping_fragment>
	#include <colorspace_fragment>
}`;

/** `alpha`: body opacity at the tile's base (default STYLE.fillA; hot raises it). */
export function Glass({ color, hot, alpha = STYLE.fillA }: { color: string; hot: boolean; alpha?: number }): React.JSX.Element {
	const reg = useStage();
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => invalidate(), [invalidate]);
	const uniforms = useMemo(
		() => ({ tint: { value: new Color(color) }, alpha: { value: hot ? Math.max(alpha, 0.2) : alpha }, reveal: { value: 1 } }),
		[color, hot, alpha],
	);
	useFrame(() => {
		uniforms.reveal.value = reg.reveal;
	});
	return (
		<shaderMaterial
			// Not read by the shader: mirrors the body alpha for code that inspects a face's opacity (lesson detail views).
			opacity={alpha}
			transparent
			depthWrite={false}
			side={DoubleSide}
			polygonOffset
			polygonOffsetFactor={1}
			polygonOffsetUnits={1}
			uniforms={uniforms}
			vertexShader={VERTEX}
			fragmentShader={FRAGMENT}
		/>
	);
}
