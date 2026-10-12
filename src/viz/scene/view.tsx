/**
 * r3f helpers that run inside a Stage's drei View: pointer dragging of registered handles, pixel sizing and a
 * pixel-stable line. Everything here reads sizes from the stage element, never from r3f's `size`: drei's View portal
 * captures its size once at mount (see Stage.tsx), so anything derived from it goes stale after a resize.
 */
import { Line } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type ComponentProps } from 'react';
import { Box3, BufferGeometry, Camera, Float32BufferAttribute, OrthographicCamera, PerspectiveCamera, Raycaster, Sphere, Vector2, Vector3, type Mesh, type Object3D } from 'three';
import { sub, to3, type V } from '../geom';
import { STYLE, usePalette } from '../palette';
import { hotOff, hotOn, lessonStore, type LessonState } from '../store';
import { hitRadius, ndcToPx, rayPlane, worldPerPx } from './drag';
import { gateWheel, placeLabels, useStage, type Draggable } from './handles';
import { DRAW_MS, reducedMotion, springProgress } from './motion';
import { writeLine } from './fatLine';
import type { Part } from './framing';

// Module-level temporaries for the per-frame helpers below (frames run one at a time, so sharing is safe): a frame
// with dozens of pixel-sized objects must not allocate vectors and arrays for each of them.
const tmp = new Vector3();
const pt = new Vector3();
const dir = new Vector3();

/** Sets `v` from a 2D (z = 0) or 3D data point without allocating. */
function setV(v: Vector3, p: V): Vector3 {
	if (p.length === 2) return v.set(p[0], p[1], 0);
	if (p.length === 3) return v.set(p[0], p[1], p[2]);
	throw new Error(`expected a 2D or 3D point, got ${p.length} components`);
}

function wppVec(camera: Camera, p: Vector3, hPx: number): number {
	if (camera instanceof OrthographicCamera) return worldPerPx({ ortho: true, zoom: camera.zoom }, hPx);
	if (camera instanceof PerspectiveCamera) {
		const depth = tmp.copy(p).sub(camera.position).dot(camera.getWorldDirection(dir));
		return worldPerPx({ ortho: false, fovDeg: camera.fov, depth: Math.max(depth, camera.near) }, hPx);
	}
	throw new Error('unsupported camera type');
}

/** World units per CSS pixel at world point `p` as seen by `camera` in a viewport `hPx` tall. */
export function wppAt(camera: Camera, p: V, hPx: number): number {
	return wppVec(camera, setV(pt, p), hPx);
}

const part = new Box3();
const sphere = new Sphere();

/**
 * World bounds of the figure under `root` (the Stage's content group) into `out`; empty when nothing is drawn yet.
 * Scene furniture marked `userData.aux` (grids, half-planes) is skipped, and glyphs sized in screen px
 * (`usePxScale`: points, arrow heads) count as their anchor point only. Bounding boxes are recomputed: shapes rewrite their buffers in place
 * (`writeGeo`), so a cached box can be stale. Called only when the home view is computed, not per frame.
 */
export function contentBox(root: Object3D, out: Box3, parts?: Part[]): Box3 {
	out.makeEmpty();
	const vec = (v: Vector3): [number, number, number] => [v.x, v.y, v.z];
	root.updateWorldMatrix(true, true);
	const visit = (o: Object3D) => {
		if (o.userData.pxSized) {
			out.expandByPoint(o.getWorldPosition(pt));
			parts?.push({ box: { min: vec(pt), max: vec(pt) } });
			return;
		}
		if (o.userData.aux) return;
		const geo = (o as Mesh).geometry;
		if (geo) {
			geo.computeBoundingBox();
			const b = geo.boundingBox;
			if (b && !b.isEmpty() && Number.isFinite(b.min.x) && Number.isFinite(b.max.x)) {
				out.union(part.copy(b).applyMatrix4(o.matrixWorld));
				if (parts) {
					geo.computeBoundingSphere();
					const s = geo.boundingSphere && Number.isFinite(geo.boundingSphere.radius) ? sphere.copy(geo.boundingSphere).applyMatrix4(o.matrixWorld) : null;
					parts.push({ box: { min: vec(part.min), max: vec(part.max) }, sphere: s ? { c: vec(s.center), r: s.radius } : undefined });
				}
			}
		}
		for (const c of o.children) visit(c);
	};
	visit(root);
	return out;
}

/**
 * Keeps `obj` (unit-sized geometry) `px` CSS pixels big wherever it is: pixel-stable heads, dots and markers.
 * Runs per frame because orbiting (3D) and resizing change the scale without a React render. `billboard` also turns
 * it to face the camera (flat discs that read as Blender vertices from any orbit). `px` may be a function of the
 * draw-in progress (`reg.reveal`).
 */
export function usePxScale(obj: React.RefObject<Object3D | null>, px: number | ((reveal: number) => number), billboard = false): void {
	const reg = useStage();
	// A glyph sized in screen px has no world extent: the figure's bounds (contentBox) count only its position.
	useLayoutEffect(() => {
		if (obj.current) obj.current.userData.pxSized = true;
	});
	useFrame(({ camera }) => {
		const o = obj.current;
		if (!o || !reg.el) return;
		const size = typeof px === 'number' ? px : px(reg.reveal);
		o.scale.setScalar(Math.max(size, 1e-6) * wppVec(camera, o.getWorldPosition(pt), reg.el.clientHeight));
		if (billboard) o.quaternion.copy(camera.quaternion);
	});
}

/**
 * World length of `px` CSS pixels at `at`, as React state for geometry that must be rebuilt (hatch spacing), unlike
 * a scale that a frame can set. Quantized to eighths of an octave (~9%) so orbiting or zooming re-renders only
 * occasionally; null until the first frame has measured it.
 */
export function useWorldPx(px: number, at: V): number | null {
	const reg = useStage();
	const invalidate = useThree((s) => s.invalidate);
	const [w, setW] = useState<number | null>(null);
	useFrame(({ camera }) => {
		if (!reg.el?.clientHeight) return;
		const raw = px * wppAt(camera, at, reg.el.clientHeight);
		const q = 2 ** (Math.round(Math.log2(raw) * 8) / 8);
		if (q !== w) setW(q);
	});
	// The rebuilt geometry is committed after the frame that measured it; the canvas is on demand, so ask for one more.
	useEffect(() => invalidate(), [w, invalidate]);
	return w;
}

/**
 * A BufferGeometry owned by one component, with room for `verts` vertices and the given triangle index, freed on
 * unmount. Shapes whose vertex count is fixed (quads, arc sectors) update it in place with `writeGeo`, so a drag
 * re-uploads a few floats instead of creating GPU buffers.
 */
export function useDynGeo(verts: number, index: readonly number[]): BufferGeometry {
	const geo = useMemo(
		() => new BufferGeometry().setAttribute('position', new Float32BufferAttribute(new Float32Array(verts * 3), 3)).setIndex([...index]),
		// The index is static per shape kind; `verts` identifies it.
		[verts],
	);
	useEffect(() => () => geo.dispose(), [geo]);
	return geo;
}

/** Writes 2D/3D points into `geo` (count must match) and refreshes the bounding sphere used by hover raycasts. */
export function writeGeo(geo: BufferGeometry, pts: readonly V[]): void {
	const a = geo.getAttribute('position') as Float32BufferAttribute;
	if (a.count !== pts.length) throw new Error(`geometry has ${a.count} vertices, got ${pts.length} points`);
	pts.forEach((p, i) => a.setXYZ(i, p[0], p[1], p[2] ?? 0));
	a.needsUpdate = true;
	geo.computeBoundingSphere();
}

// drei's Line ref type (Line2 | LineSegments2); taken from drei so we do not import its transitive three-stdlib.
type LineObj = React.ComponentRef<typeof Line>;

const NO_POINTS: [number, number, number][] = [[0, 0, 0], [0, 0, 0]];
const NO_FLAT: number[] = [];

/** drei `Line` props plus `dashPx`: dash and gap lengths in CSS pixels (drei's are world units). */
export type SLineProps = ComponentProps<typeof Line> & { dashPx?: readonly [number, number] };

/** Imperative handle of a Stroke: all of its lines share the same points. */
export interface StrokeHandle {
	/**
	 * Replaces the points of every line of the stroke (flat xyz list), e.g. per frame for a growing beam or trace. The
	 * point count may change at any time, down to none (`writeLine`: in place, reallocating only on growth).
	 */
	setPositions(flat: number[]): void;
	/** Geometry identity of the core line; drei rebuilds it when `points` change, which resets the positions. */
	geometry(): object | null;
}

export interface StrokeProps {
	points: SLineProps['points'];
	color: string;
	/** Linked/hovered: Blender's active outline (hot stroke `hotPad` px wider each side, behind) and a hot halo. */
	hot?: boolean;
	/** Core width in px (default the beam core, `STYLE.core`). */
	width?: number;
	/** Soft halo of the stroke's own color (beams); off for secondary lines. */
	halo?: boolean;
	dashPx?: readonly [number, number];
	segments?: boolean;
	/** Render order of the core; halo and outline go just below it. */
	order?: number;
	/** Core opacity (a shadow band is a translucent wide stroke). */
	opacity?: number;
}

/**
 * The scene's one stroke recipe (visual-language.md): halo (9 px at 14% of its own color), core, and when hot the
 * Blender active outline behind the core plus a 30% hot halo; the object's own color never changes. Every line is
 * transparent so the three layers sort by render order (three draws all opaque objects before transparent ones, so
 * an opaque core would be painted over by its own halo); depth writes are off for the same reason.
 */
export const Stroke = forwardRef<StrokeHandle, StrokeProps>(function Stroke(
	{ points, color, hot = false, width = STYLE.core, halo = true, dashPx, segments, order = 2, opacity = 1 },
	fwd,
) {
	const pal = usePalette();
	const core = useRef<LineObj>(null);
	const out = useRef<LineObj>(null);
	const glow = useRef<LineObj>(null);
	// Last per-frame override (e.g. a beam trimmed inside its head). The hot outline and halo mount on hover with drei's
	// untrimmed `points`, so they get the override as soon as they exist, not on the next geometry change.
	const last = useRef<number[] | null>(null);
	useImperativeHandle(fwd, () => ({
		setPositions(flat) {
			last.current = flat;
			for (const l of [core.current, out.current, glow.current]) if (l) writeLine(l.geometry, flat, Boolean(segments));
		},
		geometry: () => core.current?.geometry ?? null,
	}));
	// Runs after the children's layout effects (drei sets `points` there), so the override wins on mount.
	useLayoutEffect(() => {
		const f = last.current ?? (points.length ? null : NO_FLAT);
		if (f) for (const l of [out.current, glow.current]) if (l) writeLine(l.geometry, f, Boolean(segments));
	}, [hot, halo]);
	// New `points` reset every line; the owner re-applies its override on the next frame. An empty stroke draws no
	// segment at all (the placeholder point drei needs would otherwise paint a round-capped dot).
	useLayoutEffect(() => {
		last.current = null;
		if (!points.length) for (const l of [core.current, out.current, glow.current]) if (l) writeLine(l.geometry, NO_FLAT, Boolean(segments));
	}, [points, segments]);
	const outW = width + 2 * STYLE.hotPad;
	// drei's LineGeometry throws on an empty list, and a segment list needs whole pairs (one point gives LineSegmentsGeometry
	// half an instance and a NaN bound): a stroke that starts empty mounts on a placeholder pair, emptied above.
	const common = { points: points.length ? points : NO_POINTS, segments, dashPx, transparent: true, depthWrite: false };
	return (
		<>
			{(halo || hot) && (
				<SLine
					ref={glow}
					{...common}
					color={hot ? pal.hot : color}
					lineWidth={hot ? Math.max(STYLE.halo, outW + 6) : STYLE.halo}
					opacity={hot ? STYLE.hotHaloA : STYLE.haloA}
					renderOrder={order - 0.2}
				/>
			)}
			{hot && <SLine ref={out} {...common} color={pal.hot} lineWidth={outW} renderOrder={order - 0.1} />}
			<SLine ref={core} {...common} color={color} lineWidth={width} opacity={opacity} renderOrder={order} />
		</>
	);
});

/**
 * drei `Line` that stays pixel-true after a stage resize: LineMaterial converts `lineWidth` pixels with its
 * `resolution`, which drei sets from the stale View size, so it is refreshed from the stage element every frame.
 * Dashes are re-sized per frame too (measured at the origin), so a dashed leg looks the same at any zoom.
 */
export const SLine = forwardRef<LineObj, SLineProps>(function SLine({ dashPx, ...p }, fwd) {
	const reg = useStage();
	const ref = useRef<LineObj>(null);
	useImperativeHandle(fwd, () => ref.current as LineObj);
	useFrame(({ camera }) => {
		const el = reg.el;
		const m = ref.current?.material;
		if (!el || !m) return;
		m.resolution.set(el.clientWidth, el.clientHeight);
		if (dashPx) {
			const wpp = wppAt(camera, [0, 0, 0], el.clientHeight);
			m.dashSize = dashPx[0] * wpp;
			m.gapSize = dashPx[1] * wpp;
		}
	});
	return <Line ref={ref} dashed={Boolean(dashPx)} {...p} />;
});

/** One drag in progress: values at press time plus the drag plane (point, normal) and the pointer's first hit on it. */
interface Drag {
	d: Draggable;
	start: LessonState['vals'];
	p0: number[];
	n: number[];
	h0: number[];
	/** Dimension of the handle position (the raw move passed to `move`). */
	dims: number;
}

/**
 * Binds the View camera to the stage registry (projection for pick, keyboard focus ring) and turns pointer input
 * on handles into store writes. Listeners sit on the `.lab-stage` element in the capture phase, so a handle press is
 * claimed before OrbitControls (listening on the View's own div, the event target) can start an orbit: the controls
 * are disabled for the drag and check `enabled` in their pointerdown.
 * Touch: `.lab-stage` is `touch-action: pan-y`, and touch-action is latched when the finger lands, so changing it
 * after a hit would not stop this gesture from scrolling. A non-passive `touchstart` that calls preventDefault() on a
 * handle hit is what keeps the page still; a miss is left alone and the page scrolls (Review Focus 2).
 */
export function DragLayer(): null {
	const reg = useStage();
	const camera = useThree((s) => s.camera);
	const controls = useThree((s) => s.controls) as unknown as { enabled: boolean } | null;

	useLayoutEffect(() => {
		reg.project = (p) => {
			const el = reg.el;
			if (!el) return null;
			const v = setV(pt, p).project(camera);
			return ndcToPx(v.x, v.y, v.z, el.clientWidth, el.clientHeight);
		};
		return () => {
			reg.project = null;
		};
	}, [reg, camera]);

	// OrbitControls sets an inline touch-action:none on the View div when it connects (and again on reconnect), which
	// would trap the page on phones; `.lab-stage-view { touch-action: pan-y !important }` in lab.css overrides it.

	useEffect(() => {
		const el = reg.el;
		if (!el) return;
		const rc = new Raycaster();
		const ndc = new Vector2();
		const local = (cx: number, cy: number): [number, number] => {
			const r = el.getBoundingClientRect();
			return [cx - r.left, cy - r.top];
		};
		const hitPlane = (cx: number, cy: number, a: Pick<Drag, 'p0' | 'n'>): number[] | null => {
			const [x, y] = local(cx, cy);
			ndc.set((x / el.clientWidth) * 2 - 1, 1 - (y / el.clientHeight) * 2);
			rc.setFromCamera(ndc, camera);
			return rayPlane(rc.ray.origin.toArray(), rc.ray.direction.toArray(), a.p0, a.n);
		};
		let active: Drag | null = null;
		let stopDrag: (() => void) | null = null;
		const ac = new AbortController();
		const on = <K extends keyof HTMLElementEventMap>(t: K, f: (e: HTMLElementEventMap[K]) => void, o: AddEventListenerOptions = {}) =>
			el.addEventListener(t, f, { ...o, signal: ac.signal });

		on(
			'pointerdown',
			(e) => {
				if (active || (e.pointerType === 'mouse' && e.button !== 0)) return;
				const d = reg.pick(...local(e.clientX, e.clientY), hitRadius(e.pointerType));
				if (!d) return;
				// A glyph still gliding after a key step is grabbed where it is drawn but measured from the stored value:
				// finish its ease now (a cut to the value it was heading for), so the drag starts from what is on screen.
				const st = lessonStore(d.lesson).getState();
				for (const k of d.keys) if (st.shown[k] !== st.vals[k] && st.vals[k] !== undefined) st.set(k, st.vals[k]);
				const start = lessonStore(d.lesson).getState().vals;
				const p0 = to3(d.at(start));
				// 2D: the data plane z = 0. 3D: the plane through the handle facing the camera, so the handle follows the
				// pointer exactly under any orbit; depth changes come from orbiting or PageUp/PageDown.
				const n = reg.mode === '2d' ? [0, 0, 1] : camera.getWorldDirection(new Vector3()).toArray();
				const h0 = hitPlane(e.clientX, e.clientY, { p0, n });
				if (!h0) return;
				e.preventDefault(); // no text selection or synthetic mouse events for a drag
				el.setPointerCapture(e.pointerId);
				const orbit = controls?.enabled ?? false;
				if (controls) controls.enabled = false;
				// Too late for this finger (see above), but keeps a second finger from panning or pinching mid-drag.
				el.style.touchAction = 'none';
				el.style.cursor = 'grabbing';
				hotOn(d.lesson, d.owner);
				reg.hover.set({ handle: d.id });
				active = { d, start, p0, n, h0, dims: d.at(start).length };
				const a = active;
				const id = e.pointerId;
				let raf = 0;
				let last: [number, number] | null = null;
				const apply = () => {
					raf = 0;
					if (!last) return;
					const h = hitPlane(last[0], last[1], a);
					last = null;
					if (!h) return;
					// The raw move in the handle's own dimension (a 2D handle moves on z = 0), through the one write path.
					reg.apply(a.d, a.start, sub(h, a.h0).slice(0, a.dims), 'drag');
				};
				const drag = new AbortController();
				const opts = { signal: drag.signal };
				// Also run on unmount mid-drag, so the controls and the stage never stay locked.
				stopDrag = () => {
					drag.abort();
					stopDrag = null;
					active = null;
					if (controls) controls.enabled = orbit;
					el.style.touchAction = '';
					el.style.cursor = '';
				};
				const release = stopDrag;
				el.addEventListener(
					'pointermove',
					(ev) => {
						if (ev.pointerId !== id) return;
						last = [ev.clientX, ev.clientY];
						raf ||= requestAnimationFrame(apply);
					},
					opts,
				);
				const end = (ev: PointerEvent) => {
					if (ev.pointerId !== id) return;
					cancelAnimationFrame(raf);
					apply(); // a frame still pending at release must not swallow the final position
					release();
					// A mouse is still hovering the object (r3f pointerout clears it later); a finger has left.
					if (ev.pointerType !== 'mouse') {
						hotOff(a.d.lesson, a.d.owner);
						reg.hover.clear('handle', a.d.id);
					}
				};
				el.addEventListener('pointerup', end, opts);
				el.addEventListener('pointercancel', end, opts);
				el.addEventListener('lostpointercapture', end, opts);
			},
			{ capture: true },
		);

		on('pointermove', (e) => {
			if (active || e.pointerType !== 'mouse') return;
			const d = reg.pick(...local(e.clientX, e.clientY));
			const cursor = d ? 'grab' : '';
			if (el.style.cursor !== cursor) el.style.cursor = cursor;
			reg.hover.set({ handle: d?.id ?? null });
		});
		// MMB orbits (3D) and pans (2D) like Blender; without this Chrome on Windows also starts its middle-click
		// autoscroll and the page drifts under the drag (measured: 17 px after one MMB pan).
		on('mousedown', (e) => {
			if (e.button === 1) e.preventDefault();
		});
		gateWheel(el, ac.signal);
		// Leaving the stage clears what the status bar describes (r3f's pointerout covers objects, not handles).
		on('pointerleave', () => {
			if (!active) reg.hover.set({ handle: null });
		});

		const lockTouch = (e: TouchEvent) => {
			const t = e.touches[0];
			if (active || (t && reg.pick(...local(t.clientX, t.clientY), hitRadius('touch')))) e.preventDefault();
		};
		on('touchstart', lockTouch, { passive: false });
		on('touchmove', (e) => active && e.preventDefault(), { passive: false });

		return () => {
			ac.abort();
			stopDrag?.();
		};
	}, [reg, camera, controls]);

	return null;
}

/**
 * Places the stage's overlay labels once per rendered frame. Priority 2 runs after drei's View (priority 1) has set
 * this frame's viewport-derived camera state (orthographic frustum, perspective aspect) and rendered, which also
 * refreshes the camera matrices; at the default priority a label would lag one frame behind an orbit or a resize,
 * and with an on-demand canvas that stale frame can be the last one. A new or changed label asks for a frame.
 */
export function LabelLayer(): null {
	const reg = useStage();
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => reg.labels.subscribe(() => invalidate()), [reg, invalidate]);
	useFrame(() => placeLabels(reg), 2);
	return null;
}

/**
 * Feeds the DOM navigation gizmo (Gizmo.tsx) the camera orientation of every rendered frame. Priority 2: after the
 * View's render, like the labels, so gizmo and scene show the same frame.
 */
export function GizmoSync(): null {
	const reg = useStage();
	useFrame(({ camera }) => {
		const q = camera.quaternion;
		reg.gizmo?.([q.x, q.y, q.z, q.w]);
	}, 2);
	return null;
}

/**
 * The one orchestrated moment of a lesson (visual-language.md "Motion"): the stage draws itself once, the first time
 * it is substantially on screen. Drives `reg.reveal` 0 -> 1 with the shared spring over DRAW_MS; primitives read it
 * per frame (beams grow from their tails, fills, points and labels fade in). Reduced motion: drawn at once.
 * Runs at priority -1, before the primitives' frame callbacks read the value.
 */
export function DrawIn(): null {
	const reg = useStage();
	const invalidate = useThree((s) => s.invalidate);
	const t0 = useRef<number | null>(null);
	useLayoutEffect(() => {
		const el = reg.el;
		if (!el || reducedMotion() || typeof IntersectionObserver !== 'function') return;
		reg.reveal = 0;
		const io = new IntersectionObserver(
			([e]) => {
				if (!e?.isIntersecting) return;
				io.disconnect();
				t0.current = performance.now();
				invalidate();
			},
			{ threshold: 0.4 },
		);
		io.observe(el);
		return () => {
			io.disconnect();
			reg.reveal = 1;
		};
	}, [reg, invalidate]);
	useFrame(() => {
		if (t0.current === null || reg.reveal >= 1) return;
		reg.reveal = springProgress(performance.now() - t0.current, DRAW_MS);
		invalidate();
	}, -1);
	return null;
}

/**
 * Calls `apply(reveal)` on frames where the draw-in progress changed (and once at 1 when it ends), for objects whose
 * draw-in is a material or transform change rather than geometry.
 */
export function useReveal(apply: (reveal: number) => void): void {
	const reg = useStage();
	const fn = useRef(apply);
	fn.current = apply;
	const last = useRef(1);
	useFrame(() => {
		const r = reg.reveal;
		if (r === last.current) return;
		last.current = r;
		fn.current(r);
	});
}
