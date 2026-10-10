/**
 * r3f helpers that run inside a Stage's drei View: pointer dragging of registered handles, pixel sizing and a
 * pixel-stable line. Everything here reads sizes from the stage element, never from r3f's `size`: drei's View portal
 * captures its size once at mount (see Stage.tsx), so anything derived from it goes stale after a resize.
 */
import { Line } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef, type ComponentProps } from 'react';
import { Camera, OrthographicCamera, PerspectiveCamera, Raycaster, Vector2, Vector3, type Object3D } from 'three';
import { sub, to3, type V } from '../geom';
import { hotOff, hotOn, lessonStore, type LessonState } from '../store';
import { moved, rayPlane, worldPerPx } from './drag';
import { useStage, writeMove, type Draggable } from './handles';

const tmp = new Vector3();

/** World units per CSS pixel at world point `p` as seen by `camera` in a viewport `hPx` tall. */
export function wppAt(camera: Camera, p: V, hPx: number): number {
	if (camera instanceof OrthographicCamera) return worldPerPx({ ortho: true, zoom: camera.zoom }, hPx);
	if (camera instanceof PerspectiveCamera) {
		const dir = camera.getWorldDirection(new Vector3());
		const depth = tmp.set(...to3(p)).sub(camera.position).dot(dir);
		return worldPerPx({ ortho: false, fovDeg: camera.fov, depth: Math.max(depth, camera.near) }, hPx);
	}
	throw new Error('unsupported camera type');
}

/**
 * Keeps `obj` (unit-sized geometry) `px` CSS pixels big wherever it is: pixel-stable heads, dots and glyphs.
 * Runs per frame because orbiting (3D) and resizing change the scale without a React render.
 */
export function usePxScale(obj: React.RefObject<Object3D | null>, px: number): void {
	const reg = useStage();
	useFrame(({ camera }) => {
		const o = obj.current;
		if (!o || !reg.el) return;
		o.scale.setScalar(px * wppAt(camera, o.getWorldPosition(tmp).toArray(), reg.el.clientHeight));
	});
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
	const controls = useThree((s) => s.controls) as unknown as { enabled: boolean; domElement?: HTMLElement } | null;

	useLayoutEffect(() => {
		reg.project = (p) => {
			const el = reg.el;
			if (!el) return null;
			const v = tmp.set(...to3(p)).project(camera);
			return [((v.x + 1) / 2) * el.clientWidth, ((1 - v.y) / 2) * el.clientHeight];
		};
		return () => {
			reg.project = null;
		};
	}, [reg, camera]);

	// OrbitControls sets touch-action:none on the View div when it connects, which would trap the page on phones.
	// Clearing it lets `.lab-stage`'s pan-y apply: a vertical swipe scrolls, a horizontal one still orbits.
	useEffect(() => {
		if (controls?.domElement) controls.domElement.style.touchAction = '';
	}, [controls]);

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
				const d = reg.pick(...local(e.clientX, e.clientY));
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
			if (active || (t && reg.pick(...local(t.clientX, t.clientY)))) e.preventDefault();
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
