/**
 * The viewport's sky (docs/design/visual-language.md v3, reference refs/holo-style-ref.png): near-black deep space with
 * a dense fine starfield, colourful domain-warped nebulae and spiral galaxies; the clay look gets a light studio
 * backdrop instead.
 *
 * The sky is static (no time uniform), so it is painted once per stage size into a render target owned by this View
 * and then only drawn as a texture: a drag that re-renders the View every frame does not re-run ~60 noise lookups per
 * pixel. The procedural pass uses the stage's CSS px for the stars (a star is a few pixels wide at any stage size) and
 * aspect-correct coordinates in units of the stage height for nebulae and galaxies (they scale with the view, as a
 * photograph would). Each card gets its own arrangement from a seed of its id.
 */
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useMemo } from 'react';
import { Mesh, OrthographicCamera, PlaneGeometry, Scene, ShaderMaterial, Vector2, WebGLRenderTarget } from 'three';
import { usePalette } from '../palette';
import { useStage } from './handles';
import type { Look } from './look';

const VERTEX = /* glsl */ `
varying vec2 st;
void main() {
	st = uv;
	// A full-View quad: the View's viewport/scissor confines it to this stage.
	gl_Position = vec4(position.xy, 0.999, 1.0);
}`;

const SKY = /* glsl */ `
precision highp float;
varying vec2 st;
uniform vec2 resolution;
uniform vec3 top, bottom, star, nebulaA, nebulaB, vignette;
uniform float seed;
// Share of the stage width not covered by an open N-panel: galaxies are placed in the visible part.
uniform float free;

// Integer-free hash (Dave Hoskins): no sin(), so no precision banding on mobile GPUs.
float hash12(vec2 p) {
	vec3 p3 = fract(vec3(p.xyx) * 0.1031);
	p3 += dot(p3, p3.yzx + 33.33);
	return fract((p3.x + p3.y) * p3.z);
}
vec2 hash22(vec2 p) {
	vec3 p3 = fract(vec3(p.xyx) * vec3(0.1031, 0.1030, 0.0973));
	p3 += dot(p3, p3.yzx + 33.33);
	return fract((p3.xx + p3.yz) * p3.zy);
}
// Value noise with a quintic fade: C2-continuous, so no visible cell edges in smooth clouds.
float vnoise(vec2 p) {
	vec2 i = floor(p);
	vec2 f = fract(p);
	vec2 u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
	float a = hash12(i);
	float b = hash12(i + vec2(1.0, 0.0));
	float c = hash12(i + vec2(0.0, 1.0));
	float d = hash12(i + vec2(1.0, 1.0));
	return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}
// Fractal sum; each octave is rotated so the lattice of one octave never lines up with the next (no streaks).
float fbm(vec2 p) {
	float s = 0.0;
	float a = 0.5;
	mat2 m = mat2(1.6, 1.2, -1.2, 1.6);
	for (int i = 0; i < 6; i++) {
		s += a * vnoise(p);
		p = m * p + vec2(11.3, 7.1);
		a *= 0.5;
	}
	return s;
}
mat2 rot(float a) {
	float c = cos(a), s = sin(a);
	return mat2(c, -s, s, c);
}

// One layer of stars on a jittered grid of 'cell' px: at most one star per cell, with probability 'chance'.
vec3 stars(vec2 pix, float cell, float chance, float radius, float gain, float salt) {
	vec2 g = floor(pix / cell);
	if (hash12(g + salt) > chance) return vec3(0.0);
	vec2 c = (g + 0.15 + 0.7 * hash22(g + salt * 1.7)) * cell;
	vec2 d = pix - c;
	float r2 = dot(d, d) / (radius * radius);
	float core = exp(-r2 * 2.0);
	float hue = hash12(g + salt * 3.1);
	vec3 tint = mix(vec3(0.62, 0.74, 1.0), vec3(1.0, 0.88, 0.76), smoothstep(0.55, 1.0, hue));
	float level = 0.35 + 0.65 * pow(hash12(g + salt * 5.3), 2.0);
	return star * tint * core * level * gain;
}

// The brightest stars carry the four-point diffraction cross of the reference.
vec3 flares(vec2 pix, float cell, float chance, float salt) {
	vec2 g = floor(pix / cell);
	if (hash12(g + salt) > chance) return vec3(0.0);
	vec2 c = (g + 0.2 + 0.6 * hash22(g + salt)) * cell;
	vec2 d = abs(pix - c);
	float core = exp(-dot(d, d) * 0.35);
	float cross = exp(-d.x * 0.45) * exp(-d.y * d.y * 1.2) + exp(-d.y * 0.45) * exp(-d.x * d.x * 1.2);
	return star * (core * 1.6 + cross * 0.35) * (0.6 + 0.4 * hash12(g + 2.0));
}

// A spiral galaxy seen at an inclination: two logarithmic arms with clumpy star-forming knots, dust lanes on the
// inner edge of each arm, a warm bulge and a soft halo. p and c in stage-height units, r0 = disc radius.
vec3 galaxy(vec2 p, vec2 c, float r0, float tilt, float incl, vec3 tint, float s) {
	vec2 d = rot(tilt) * (p - c);
	d.y /= incl;
	d /= r0;
	float r = length(d);
	if (r > 1.7) return vec3(0.0);
	float a = atan(d.y, d.x);
	float wind = 2.0 * a - log(r + 0.03) * 5.0 + s;
	float arms = pow(0.5 + 0.5 * sin(wind), 3.0);
	float knots = smoothstep(0.45, 0.95, fbm(d * 7.0 + s));
	float lanes = smoothstep(0.55, 0.95, 0.5 + 0.5 * sin(wind - 0.9)) * smoothstep(0.35, 0.7, fbm(d * 11.0 - s));
	float disc = exp(-r * r * 3.2);
	float body = disc * (0.18 + arms * (0.75 + 1.4 * knots)) * (1.0 - 0.65 * lanes);
	float bulge = exp(-r * r * 55.0) * 1.7 + exp(-r * r * 9.0) * 0.45;
	vec3 col = tint * body + vec3(1.0, 0.86, 0.78) * bulge + vec3(0.9, 0.95, 1.0) * knots * arms * disc * 0.35;
	return col * smoothstep(1.7, 1.1, r);
}

void main() {
	vec2 pix = st * resolution;
	float aspect = resolution.x / resolution.y;
	// Stage-height units, origin at the centre, y up.
	vec2 p = (st - 0.5) * vec2(aspect, 1.0);
	vec2 off = vec2(seed * 0.137, seed * 0.071);

	// Near-black base: the palette's gradient, pulled darker so the hologram has the deep-space contrast.
	vec3 c = mix(bottom, top, st.y) * 0.62;

	// Nebulae: domain-warped clouds, patchy (thresholded), blue and violet with magenta in the dense cores.
	vec2 np = p * 1.7 + off;
	vec2 warp = vec2(fbm(np), fbm(np + vec2(5.2, 1.3)));
	float cloud = fbm(np + 2.3 * warp);
	float mask = smoothstep(0.5, 0.92, cloud);
	vec3 nebula = mix(nebulaA, nebulaB, smoothstep(0.25, 0.75, warp.x)) * 1.5 * mask;
	nebula += vec3(0.36, 0.12, 0.42) * pow(mask, 3.0) * 0.45;
	nebula += vec3(0.08, 0.18, 0.38) * pow(smoothstep(0.55, 0.92, fbm(np * 2.3 - warp)), 2.0) * 0.5;
	// Dark dust veins cut through the clouds.
	nebula *= 1.0 - 0.65 * smoothstep(0.55, 0.78, fbm(np * 3.1 + 7.7));
	c += nebula;

	// Spiral galaxies in the sky, placed toward the edges of the visible area (the figure owns the centre). The large
	// one sits upper right as in the reference; the side is chosen by the seed.
	float fa = aspect * free;
	vec2 pf = vec2((st.x / free - 0.5) * fa, st.y - 0.5);
	float side = hash12(vec2(seed, 3.0)) > 0.5 ? 1.0 : -1.0;
	vec2 g1 = vec2(side * (0.5 * fa - 0.25), 0.24 + 0.05 * hash12(vec2(seed, 5.0)));
	vec2 g2 = vec2(-side * (0.5 * fa - 0.3), 0.36);
	vec2 g3 = vec2(-side * (0.5 * fa - 0.34), -0.12);
	c += galaxy(pf, g1, 0.15, side * -0.6 + 0.3 * hash12(vec2(seed, 7.0)), 0.4, vec3(0.42, 0.5, 1.0), seed);
	c += galaxy(pf, g2, 0.045, 0.8, 0.55, vec3(0.62, 0.55, 1.0), seed + 2.0) * 0.8;
	c += galaxy(pf, g3, 0.035, -0.4, 0.38, vec3(0.75, 0.45, 0.9), seed + 4.0) * 0.7;

	// Starfield: a dense dim field (denser inside the nebulae, like a Milky Way band), medium stars, a few bright
	// ones with diffraction crosses.
	c += stars(pix, 5.0, 0.30 + 0.35 * mask, 0.75, 0.45, 1.0 + seed);
	c += stars(pix, 13.0, 0.42, 0.95, 0.85, 11.0 + seed);
	c += stars(pix, 41.0, 0.55, 1.2, 1.4, 23.0 + seed);
	c += flares(pix, 170.0, 0.55, 37.0 + seed);

	// Vignette toward the frame, then 1/255 dither so the dark gradients never band.
	float edge = smoothstep(0.35, 0.95, length((st - 0.5) * vec2(1.0, 1.15)));
	c = mix(c, vignette, edge * 0.6);
	c += (hash12(pix + seed) - 0.5) / 255.0;
	gl_FragColor = vec4(c, 1.0);
}`;

const STUDIO = /* glsl */ `
precision highp float;
varying vec2 st;
uniform vec2 resolution;
uniform vec3 top, bottom;
float hash12(vec2 p) {
	vec3 p3 = fract(vec3(p.xyx) * 0.1031);
	p3 += dot(p3, p3.yzx + 33.33);
	return fract((p3.x + p3.y) * p3.z);
}
void main() {
	// Light studio: a cool grey sweep, lighter toward the top, with a soft key light behind the subject.
	vec3 c = mix(bottom, top, st.y);
	vec2 q = (st - vec2(0.5, 0.62)) * vec2(resolution.x / resolution.y, 1.0);
	c += vec3(0.05) * exp(-dot(q, q) * 3.0);
	c += (hash12(st * resolution) - 0.5) / 255.0;
	gl_FragColor = vec4(c, 1.0);
}`;

const SHOW = /* glsl */ `
varying vec2 st;
uniform sampler2D sky;
void main() {
	gl_FragColor = texture2D(sky, st);
}`;

const rgb = (hex: string) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255);

/** Stable small number from a card id: the sky arrangement of a card never changes between visits. */
const seedOf = (id: string) => ([...id].reduce((a, ch) => (a * 31 + ch.charCodeAt(0)) % 997, 7) / 997) * 40;

/** `inset`: CSS px of the stage's right edge covered by an open N-panel (Stage prop); galaxies stay in the rest. */
export function Background({ look = 'holo', inset = 0 }: { look?: Look; inset?: number }): React.JSX.Element {
	const pal = usePalette();
	const reg = useStage();
	const invalidate = useThree((s) => s.invalidate);
	const seed = seedOf(reg.card?.id ?? 'sky');

	// The procedural pass lives in its own tiny scene, rendered into `target` only when the size or palette changes.
	const pass = useMemo(() => {
		const holo = look === 'holo';
		const material = new ShaderMaterial({
			vertexShader: VERTEX,
			fragmentShader: holo ? SKY : STUDIO,
			depthTest: false,
			depthWrite: false,
			uniforms: {
				resolution: { value: new Vector2(1, 1) },
				top: { value: rgb(holo ? pal.bgTop : pal.clayBgTop) },
				bottom: { value: rgb(holo ? pal.bgBottom : pal.clayBgBottom) },
				star: { value: rgb(pal.star) },
				nebulaA: { value: rgb(pal.nebulaA) },
				nebulaB: { value: rgb(pal.nebulaB) },
				vignette: { value: rgb(pal.vignette) },
				seed: { value: seed },
				free: { value: 1 },
			},
		});
		const scene = new Scene();
		scene.add(new Mesh(new PlaneGeometry(2, 2), material));
		return { scene, material, camera: new OrthographicCamera(), target: new WebGLRenderTarget(1, 1, { depthBuffer: false }), size: new Vector2(), inset: -1 };
	}, [look, pal, seed]);
	useEffect(
		() => () => {
			pass.target.dispose();
			pass.material.dispose();
			(pass.scene.children[0] as Mesh).geometry.dispose();
		},
		[pass],
	);

	const show = useMemo(
		() => new ShaderMaterial({ vertexShader: VERTEX, fragmentShader: SHOW, depthTest: false, depthWrite: false, uniforms: { sky: { value: pass.target.texture } } }),
		[pass],
	);
	useEffect(() => () => show.dispose(), [show]);

	// Before the View renders (priority 0 < 1): repaint the sky if the stage was resized (or the palette changed).
	useFrame(({ gl }) => {
		const el = reg.el;
		if (!el) return;
		const dpr = gl.getPixelRatio();
		const w = Math.max(1, Math.round(el.clientWidth * dpr));
		const h = Math.max(1, Math.round(el.clientHeight * dpr));
		if (pass.size.x === w && pass.size.y === h && pass.inset === inset) return;
		pass.size.set(w, h);
		pass.inset = inset;
		pass.target.setSize(w, h);
		pass.material.uniforms.resolution.value.set(el.clientWidth, el.clientHeight);
		pass.material.uniforms.free.value = Math.max(0.3, (el.clientWidth - inset) / Math.max(1, el.clientWidth));
		const prev = gl.getRenderTarget();
		gl.setRenderTarget(pass.target);
		gl.render(pass.scene, pass.camera);
		gl.setRenderTarget(prev);
		invalidate();
	});

	return (
		<mesh frustumCulled={false} renderOrder={-10} raycast={() => {}} material={show}>
			<planeGeometry args={[2, 2]} />
		</mesh>
	);
}
