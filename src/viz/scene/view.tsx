/**
 * r3f helpers that run inside a Stage's drei View: pointer dragging of registered handles, pixel sizing and a
 * pixel-stable line. Everything here reads sizes from the stage element, never from r3f's `size`: drei's View portal
 * captures its size once at mount (see Stage.tsx), so anything derived from it goes stale after a resize.
 */
import { Line } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type ComponentProps } from 'react';
import { BufferGeometry, Camera, Float32BufferAttribute, OrthographicCamera, PerspectiveCamera, Raycaster, Vector2, Vector3, type Object3D } from 'three';
import { sub, to3, type V } from '../geom';
import { hotOff, hotOn, lessonStore, type LessonState } from '../store';
import { hitRadius, moved, ndcToPx, rayPlane, worldPerPx } from './drag';
import { placeLabels, useStage, writeMove, type Draggable } from './handles';

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

/**
 * Keeps `obj` (unit-sized geometry) `px` CSS pixels big wherever it is: pixel-stable heads, dots, glyphs and markers.
 * Runs per frame because orbiting (3D) and resizing change the scale without a React render.
 */
export function usePxScale(obj: React.RefObject<Object3D | null>, px: number): void {
	const reg = useStage();
	useFrame(({ camera }) => {
		const o = obj.current;
		if (!o || !reg.el) return;
		o.scale.setScalar(px * wppVec(camera, o.getWorldPosition(pt), reg.el.clientHeight));
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

/** drei `Line` props plus `dashPx`: dash and gap lengths in CSS pixels (drei's are world units). */
export type SLineProps = ComponentProps<typeof Line> & { dashPx?: readonly [number, number] };

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
				active = { d, start, p0, n, h0 };
				const a = active;
				const id = e.pointerId;
				let raf = 0;
				let last: [number, number] | null = null;
				const apply = () => {
					raf = 0;
					if (!last) return;
					const h = hitPlane(last[0], last[1], a);
					last = null;
					if (h) writeMove(a.d.lesson, moved(a.start, a.d.keys, sub(h, a.h0)));
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
					if (ev.pointerType !== 'mouse') hotOff(a.d.lesson, a.d.owner);
				};
				el.addEventListener('pointerup', end, opts);
				el.addEventListener('pointercancel', end, opts);
				el.addEventListener('lostpointercapture', end, opts);
			},
			{ capture: true },
		);

		on('pointermove', (e) => {
			if (active || e.pointerType !== 'mouse') return;
			const cursor = reg.pick(...local(e.clientX, e.clientY)) ? 'grab' : '';
			if (el.style.cursor !== cursor) el.style.cursor = cursor;
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
