import { t } from '../../i18n/core';
import { useLocale, LocaleContext } from '../../i18n/react';
/**
 * A scene slot: one drei <View> of the shared canvas plus its DOM overlay (labels, keyboard handles, navigation
 * gizmo). 2D = orthographic camera (pixel-stable lines), wheel zoom and MMB/RMB pan; 3D = orbit (LMB or MMB, Shift
 * pans like Blender), perspective or orthographic. Usually rendered by ViewportCard, which owns the chrome; the
 * accessible name comes from the server-rendered <figcaption> of SceneIsland, so the stage carries none.
 */
import { OrbitControls, OrthographicCamera, PerspectiveCamera, View } from '@react-three/drei';
import { useFrame, useThree } from '@react-three/fiber';
import { useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { Box3, MOUSE, OrthographicCamera as OrthoCam, PerspectiveCamera as PerspCam, TOUCH, Vector3, type Group, type Object3D } from 'three';
import type { V } from '../geom';
import { usePalette } from '../palette';
import { lessonStore, ShownCtx } from '../store';
import { Gizmo, XYCorner } from './Gizmo';
import { createStage, DragKeys, Labels, StageCtx, type StageReg } from './handles';
import { axisPose, lerpPose, positionOf, poseOf, reducedMotion, springProgress, SPRING_MS, VIEW_MS, type Pose } from './motion';
import { ensureSceneHost } from './SceneHost';
import { useSizeState, viewVisible } from './sizeModes';
import { contentBox, DragLayer, DrawIn, GizmoSync, LabelLayer } from './view';
import { fit2d, fit3d, MARGIN_2D, outside, roomFrame, stepRefitDelay, union, type Bounds, type Part } from './framing';

export interface StageProps {
	lesson: string;
	mode: '2d' | '3d';
	extent: number;
	children: React.ReactNode;
	overlay?: React.ReactNode;
	/** The card's registry (card info, hover, nav); a bare Stage makes its own. */
	reg?: StageReg;
	/** 3D projection (default perspective); switching keeps the view (same direction, same apparent size). */
	proj?: 'persp' | 'ortho';
	/** 2D view center, the home of Reset view (default the origin). */
	center?: V;
	/** CSS px of the stage's right edge covered by an overlaid N-panel: the scene is centered in the rest. */
	inset?: number;
	/** The lesson's one first-view draw-in (visual-language.md "Motion"). */
	drawIn?: boolean;
}

function InvalidateOnStore({ lesson }: { lesson: string }) {
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => lessonStore(lesson).subscribe(() => invalidate()), [lesson, invalidate]);
	return null;
}

/**
 * A theme switch yields a new palette object. Scene materials are updated by React, but the canvas runs on demand
 * (`frameloop="demand"`), so something must also ask for a frame: this runs inside the View portal, and `invalidate`
 * there schedules a render of the shared root.
 */
function InvalidateOnTheme() {
	const palette = usePalette();
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => invalidate(), [palette, invalidate]);
	return null;
}

/**
 * Content-box size of the stage element. drei's View portal captures its `size` once at mount and never refreshes
 * it (measured: after a resize from 1280 to 900 px the box kept its old 66 px side), so scaling reads the element.
 */
function useBoxSize(ref: RefObject<HTMLElement | null>): { w: number; h: number } {
	const [size, setSize] = useState({ w: 0, h: 0 });
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
		ro.observe(el);
		return () => ro.disconnect();
	}, [ref]);
	return size;
}

// drei's controls instance type, taken from drei so we do not import its transitive three-stdlib.
type OrbitImpl = React.ComponentRef<typeof OrbitControls>;

const FOV = 40;
const TAN = Math.tan((FOV * Math.PI) / 360);
// 2D zoom range relative to the fitted zoom; 3D orbit distance range in units of `extent`.
const ZOOM_2D: readonly [number, number] = [0.5, 8];
const DIST_3D: readonly [number, number] = [0.4, 6];

/** Default 3D camera position: an elevated three-quarter view of the floor. */
const home3 = (e: number): [number, number, number] => [e * 1.6, e * 1.2, e * 1.8];

/** Radius, in units of `extent`, of the sphere the default 3D view must show whole: the axes reach +-extent, and the
 * margin keeps the axis letters, tip labels and the gizmo corner clear of the frame. */
const FRAME_3D = 1.3;
/** 2D home zoom range relative to the fit of the whole extent: a tiny figure is not blown up without limit. */
const HOME_2D: readonly [number, number] = [0.25, 6];
/**
 * From the first time the stage is on screen, the home view follows the figure while it is still arriving (lazy
 * chunks, primitives that size themselves on their first frame, the draw-in) for this long, unless the reader
 * touches the view first.
 */
const SETTLE_MS = 2000;

/** Whether two boxes differ by more than 1 % of the larger one's size (re-framing threshold while settling). */
function moved(a: Box3 | null, b: Box3 | null): boolean {
	if (!a || !b) return a !== b;
	const tol = 0.01 * Math.max(a.max.distanceTo(a.min), b.max.distanceTo(b.min), 1e-9);
	return a.min.distanceTo(b.min) > tol || a.max.distanceTo(b.max) > tol;
}

/**
 * Camera distance that fits a sphere of radius `r` in a perspective view `w` x `h` px (vertical fov FOV): the
 * narrower of the vertical and horizontal half-angles decides (a tall phone viewport is limited horizontally).
 */
export function fitDistance(r: number, w: number, h: number): number {
	const half = Math.min(Math.atan(TAN), Math.atan(TAN * (w / h)));
	return r / Math.sin(half);
}

/** Orthographic zoom (px per world unit) that shows a `dist`-away perspective view's apparent size in `hPx`. */
const orthoZoom = (dist: number, hPx: number) => hPx / (2 * dist * TAN);

interface NavProps {
	reg: StageReg;
	mode: '2d' | '3d';
	extent: number;
	center: readonly [number, number, number];
	inset: number;
	w: number;
	h: number;
	/** Pose of the view, shared across camera switches (Persp/Ortho) and kept current while orbiting. */
	pose: React.RefObject<Pose | null>;
	/** The figure (the Stage's content group): the home view frames its bounds, like Blender's View All. */
	content: RefObject<Group | null>;
}

/**
 * The 3D gizmo's corner square in px (its top offset + size + a 4 px gap), read from the rendered element so the fit
 * follows lab.css (84 px, 66 px on phones). 0 before the overlay is laid out.
 */
const gizmoCorner = (stage: HTMLElement | null): number => {
	const g = stage?.querySelector<HTMLElement>('.lab-gizmo');
	return g && g.offsetHeight ? g.offsetTop + g.offsetHeight + 4 : 0;
};
/** The objects inside a measured figure box (per-object box + bounding sphere), for the 3D fit. */
const partsOf = new WeakMap<Box3, Part[]>();
/** Plain bounds of a three Box3 for the pure framing math. */
const boundsOf = (b: Box3): Bounds => ({ min: [b.min.x, b.min.y, b.min.z], max: [b.max.x, b.max.y, b.max.z] });
/** World bounds of every `Room` under `root` (their union), or null when the scene declares none. */
function roomOf(root: Object3D | null): Bounds | null {
	if (!root) return null;
	root.updateWorldMatrix(true, true);
	const out = new Box3(), one = new Box3();
	root.traverse((o) => {
		const r = o.userData.room as Bounds | undefined;
		if (r) out.union(one.set(new Vector3(...r.min), new Vector3(...r.max)).applyMatrix4(o.matrixWorld));
	});
	return out.isEmpty() ? null : boundsOf(out);
}
/** Quiet time after the last value change before the home view checks whether the figure left it (`outside`). */
const GROW_MS = 250;

/**
 * Room the reader may drag the figure into, beyond what is drawn (Blender's View All frames objects, not a canvas):
 * the home view frames the drawn figure plus as much of this box as keeps the figure at least MIN_FILL_2D of its own
 * fit (framing.ts `roomFrame`; 3D: the whole box). Not part of the figure (`aux`), never drawn, never picked. Use it
 * instead of an invisible mesh over the drag range, which made the figure tiny on phones. A drag past the framed area
 * grows the home view after the drag.
 */
export function Room({ lo, hi }: { lo: V; hi: V }): React.JSX.Element {
	const room: Bounds = { min: [lo[0], lo[1], lo[2] ?? 0], max: [hi[0], hi[1], hi[2] ?? 0] };
	return <group userData={{ aux: true, room }} />;
}

/** Unit direction from the target to the default 3D camera. */
const HOME_DIR = home3(1 / Math.hypot(...home3(1)));

/**
 * Camera navigation of one stage: fitting (2D zoom so `extent` reaches the nearer edge of the free area, keeping the
 * reader's relative zoom across resizes; the N-panel inset re-centers the projection with a view offset), the pose
 * hand-over when the 3D camera is switched, and the smooth view of Reset view and gizmo align (`reg.nav`): the camera
 * swings around the target with the shared spring over VIEW_MS, or cuts under reduced motion. Grabbing the view
 * mid-animation cancels it (direct manipulation is never fought).
 */
function Nav({ reg, mode, extent: e, center, inset, w, h, pose, content }: NavProps) {
	const camera = useThree((s) => s.camera);
	const controls = useThree((s) => s.controls) as unknown as OrbitImpl | null;
	const invalidate = useThree((s) => s.invalidate);
	const fit = useRef(0);
	// Projection of the camera that last held the view (null before the first hand-over).
	const was = useRef<boolean | null>(null);
	const anim = useRef<{ from: Pose; to: Pose; t0: number } | null>(null);

	const ortho = camera instanceof OrthoCam;
	/** Figure bounds captured by the last home view (null: frame the extent, e.g. nothing drawn yet). */
	const homeBox = useRef<Box3 | null>(null);
	/** The scene's `Room` bounds captured with `homeBox` (null: none declared). */
	const homeRoom = useRef<Bounds | null>(null);
	/** Direction (target -> camera) of the 3D home view: the three-quarter view, or the one a scene asked for (`nav.frame`). */
	const homeDir = useRef<typeof HOME_DIR>(HOME_DIR);
	/** End of the settling period (performance.now()), 0 before the first frame; -1 once the reader took over. */
	const settle = useRef(0);
	/**
	 * The view is still the home view (not navigated since): a resize (wide/full mode, a rotated phone) re-frames the
	 * figure for the new size, like a DCC's View All, instead of scaling the old framing.
	 */
	const atHome = useRef(false);
	const extentZoom = (width: number, height: number) => {
		const near = Math.min(Math.max(1, width - inset), height);
		return mode === '2d' ? (near - 2 * MARGIN_2D) / (2 * e) : near / (2 * FRAME_3D * e);
	};
	/** Zoom of the home view at this size: the extent until a figure was measured, then the figure. */
	const fitZoom = (width = w, height = h) => {
		const box = framed(width, height);
		const base = extentZoom(width, height);
		if (!box) return base;
		if (mode === '3d') return fit3dFor(box, width, height).zoom;
		const z = fit2d(boundsOf(box), width, height, inset).zoom;
		return Math.min(base * HOME_2D[1], Math.max(base * HOME_2D[0], z));
	};
	/** 3D fit in the view left free by the N-panel. */
	const fit3dFor = (box: Box3, width: number, height: number) => fit3d(boundsOf(box), homeDir.current, width - inset, height, TAN, partsOf.get(box), gizmoCorner(reg.el));
	/** Measures the figure (View All) without applying it; null when nothing is drawn yet. */
	const figure = (): Box3 | null => {
		const root = content.current, parts: Part[] = [];
		const box = root ? contentBox(root, new Box3(), parts) : null;
		if (!box || box.isEmpty()) return null;
		partsOf.set(box, parts);
		return box;
	};
	/** Re-measures the figure and the scene's room (the home view frames them). */
	const measure = (b: Box3 | null = figure()) => {
		homeBox.current = b;
		homeRoom.current = roomOf(content.current);
	};
	/**
	 * The box the home view fits at this size: the measured figure plus the room it can afford (2D `roomFrame`), or
	 * figure and room together in 3D; null before a figure was measured.
	 */
	const framed = (width: number, height: number): Box3 | null => {
		const box = homeBox.current, room = homeRoom.current;
		if (!box || !room) return box;
		const b = mode === '2d' ? roomFrame(boundsOf(box), room, width, height, inset) : union(boundsOf(box), room);
		const out = new Box3(new Vector3(...b.min), new Vector3(...b.max));
		partsOf.set(out, [...(partsOf.get(box) ?? [{ box: boundsOf(box) }]), { box: room }]);
		return out;
	};
	/** The home view for a `width` x `height` stage (default: the last measured size). */
	const home = (width = w, height = h): Pose => {
		const box = framed(width, height);
		if (mode === '2d') {
			if (!box) return poseOf([center[0], center[1], 10], center, fitZoom(width, height));
			const f = fit2d(boundsOf(box), width, height, inset);
			const target: [number, number, number] = [f.cx, f.cy, 0];
			return poseOf([f.cx, f.cy, 10], target, fitZoom(width, height));
		}
		// Same three-quarter direction as before. Without a figure: the sphere of radius FRAME_3D * extent.
		const dir = homeDir.current;
		const c: readonly number[] = box ? fit3dFor(box, width, height).target : [0, 0, 0];
		const fitD = box ? fit3dFor(box, width, height).dist : fitDistance(FRAME_3D * e, Math.max(1, width - inset), height);
		const d = Math.min(e * DIST_3D[1], Math.max(e * DIST_3D[0], fitD));
		const pos: [number, number, number] = [c[0] + dir[0] * d, c[1] + dir[1] * d, c[2] + dir[2] * d];
		return poseOf(pos, [c[0], c[1], c[2]], ortho ? fitZoom(width, height) : 1);
	};

	const apply = (p: Pose) => {
		if (!controls) return;
		camera.position.set(...positionOf(p));
		controls.target.set(...p.target);
		if (camera instanceof OrthoCam) {
			camera.zoom = p.zoom;
			camera.updateProjectionMatrix();
		}
		controls.update();
	};
	const current = (): Pose => poseOf(camera.position.toArray(), controls ? (controls.target.toArray() as [number, number, number]) : center, camera.zoom);

	// A new camera (mount, or a Persp/Ortho switch) takes over the view: same direction and target, same apparent
	// size (an orthographic zoom from the perspective distance and back).
	useLayoutEffect(() => {
		if (!controls || !h) return;
		const p = pose.current;
		if (!p) {
			pose.current = home();
			apply(pose.current);
		} else if (mode === '3d') {
			const wasOrtho = was.current === true;
			if (ortho && !wasOrtho) apply({ ...p, zoom: orthoZoom(p.dist, h) });
			else if (!ortho && wasOrtho) apply({ ...p, dist: Math.min(e * DIST_3D[1], Math.max(e * DIST_3D[0], h / (2 * p.zoom * TAN))), zoom: 1 });
			else apply(p);
		} else apply(p);
		was.current = ortho;
		const sync = () => (pose.current = current());
		const cancel = () => {
			anim.current = null;
			settle.current = -1;
			atHome.current = false;
		};
		controls.addEventListener('change', sync);
		controls.addEventListener('start', cancel);
		invalidate();
		return () => {
			controls.removeEventListener('change', sync);
			controls.removeEventListener('start', cancel);
		};
		// Only a new camera/controls pair (or the first measured size) hands the pose over.
	}, [camera, controls, h > 0]);

	// A mode redraw can precede ResizeObserver's React commit. Read its actual box
	// synchronously so the first normal-size Canvas frame already has the normal fit.
	const refit = () => {
		const box = reg.el?.getBoundingClientRect();
		const width = box?.width ?? w, height = box?.height ?? h;
		if (!width || !height || !controls) return;
		const f = fitZoom(width, height);
		if (mode === '2d' && camera instanceof OrthoCam) {
			const rel = fit.current ? camera.zoom / fit.current : 1;
			camera.zoom = f * rel;
			controls.minZoom = f * ZOOM_2D[0];
			controls.maxZoom = f * ZOOM_2D[1];
		} else if (camera instanceof OrthoCam) {
			const rel = fit.current ? camera.zoom / fit.current : 1;
			camera.zoom = f * rel;
			controls.minZoom = orthoZoom(e * DIST_3D[1], height);
			controls.maxZoom = orthoZoom(e * DIST_3D[0], height);
		}
		fit.current = f;
		if (inset > 0) camera.setViewOffset(width, height, inset / 2, 0, width, height);
		else camera.clearViewOffset();
		if (atHome.current && homeBox.current && !anim.current) apply(home(width, height));
		if (camera instanceof OrthoCam || camera instanceof PerspCam) camera.updateProjectionMatrix();
		// A DOM box may exist before the first measured-size home handoff. Do not
		// turn OrbitControls' default origin target into an established reader pose.
		if (pose.current) pose.current = current();
		invalidate();
	};
	useLayoutEffect(refit, [w, h, inset, camera, controls, e]);

	/** Smooth view to `to` (Blender's smooth view with the shared spring), a cut under reduced motion. */
	const go = (to: Pose) => {
		if (!controls) return;
		const from = current();
		if (reducedMotion()) {
			anim.current = null;
			apply(to);
			pose.current = current();
			invalidate();
			return;
		}
		anim.current = { from, to, t0: performance.now() };
		invalidate();
	};
	useLayoutEffect(() => {
		reg.nav = {
			reset: () => {
				homeDir.current = HOME_DIR;
				measure();
				go(home());
				atHome.current = true;
			},
			frame: (dir) => {
				const l = Math.hypot(dir[0], dir[1], dir[2]);
				if (!(l > 0)) throw new Error('nav.frame needs a nonzero direction');
				homeDir.current = [dir[0] / l, dir[1] / l, dir[2] / l];
				measure();
				go(home());
				atHome.current = true;
			},
			align: (axis, sign) => {
				atHome.current = false;
				go(axisPose(axis, sign, current()));
			},
			viewDir: () => {
				const t = controls ? controls.target : new Vector3();
				return camera.position.clone().sub(t).normalize().toArray();
			},
			redraw: () => { refit(); invalidate(); },
		};
		return () => {
			reg.nav = null;
		};
	});

	// The figure's geometry exists once its layout effects ran, i.e. by the first frame: frame it then, at priority -1,
	// before this frame's render, so the first picture is already the home view.
	const seen = useRef(false);
	useEffect(() => {
		const el = reg.el;
		if (!el || typeof IntersectionObserver !== 'function') {
			seen.current = true;
			return;
		}
		const io = new IntersectionObserver(([en]) => {
			if (!en?.isIntersecting) return;
			io.disconnect();
			seen.current = true;
			// Restart the settling period now that the figure is actually drawn and seen.
			if (settle.current > 0) settle.current = performance.now() + SETTLE_MS;
			invalidate();
		});
		io.observe(el);
		return () => io.disconnect();
	}, [reg, invalidate]);
	useFrame(({ invalidate: again }) => {
		if (settle.current < 0 || !controls || !h) return;
		const now = performance.now();
		const first = settle.current === 0;
		if (first) settle.current = now + SETTLE_MS;
		// Off screen (never seen yet): frame once, then wait for the IntersectionObserver instead of measuring the
		// figure on every frame the shared canvas renders for other cards.
		else if (!seen.current) return;
		// During the draw-in the figure is still unfolding (its groups are scaled by the reveal): the first frame
		// measured it whole, so wait for the draw-in to finish instead of zooming along with it.
		else if (reg.reveal < 1) return;
		else if (now > settle.current) {
			settle.current = -1;
			return;
		}
		const box = figure();
		if (!first && !moved(box, homeBox.current)) return;
		measure(box);
		fit.current = 0;
		apply(home());
		atHome.current = true;
		pose.current = current();
		refit();
		again();
	}, -1);
	// Step-revealed geometry (LC-0/LC-2 tiles, DP-1's squares and rectangle) arrives after the settling window: while
	// the view is still home (the reader has not zoomed, panned or orbited), a timeline step re-measures the figure once
	// its step motion has played and glides to the new View All. A reader's navigation clears `atHome`, so their view
	// is never taken away; a handle drag does not navigate, so the home view keeps following the figure on the next step.
	const stepRefit = useRef(0);
	useEffect(() => {
		const lesson = reg.card?.lesson;
		if (!lesson) return;
		return lessonStore(lesson).subscribe((s, prev) => {
			if (s.step === prev.step) return;
			const delay = stepRefitDelay(prev.step, s.step, SPRING_MS, reducedMotion());
			if (delay === null) return;
			stepRefit.current = performance.now() + delay;
			invalidate();
		});
	}, [reg, invalidate]);
	useFrame(({ invalidate: again }) => {
		const due = stepRefit.current;
		if (!due || !controls || !h) return;
		// The settling period measures on its own; a step during it needs nothing extra.
		if (settle.current > 0 || !atHome.current) {
			stepRefit.current = 0;
			return;
		}
		if (performance.now() < due) {
			again();
			return;
		}
		stepRefit.current = 0;
		const box = figure();
		if (!moved(box, homeBox.current)) return;
		measure(box);
		const to = home();
		// The zoom limits follow the new home zoom (as a resize would set them).
		const f = fitZoom();
		if (camera instanceof OrthoCam && mode === '2d') {
			controls.minZoom = f * ZOOM_2D[0];
			controls.maxZoom = f * ZOOM_2D[1];
		}
		fit.current = f;
		go(to);
		atHome.current = true;
	});
	// The home view frames the drawn figure (plus what it can afford of the scene's Room), so a handle can be dragged
	// past its edge. While the view is still home, once the values have been quiet for GROW_MS and no pointer is down
	// (anywhere: a scrubbed chip under the stage moves the figure too), a figure that reaches outside the framed box grows the home view to hold it (the old figure and the
	// new one), with the smooth view. It never shrinks on a drag (the view would breathe with every move) and never
	// moves while the finger or mouse is still dragging.
	const grow = useRef(0);
	const pressed = useRef(false);
	useEffect(() => {
		const lesson = reg.card?.lesson;
		if (!lesson) return;
		const down = () => (pressed.current = true);
		const up = () => {
			if (!pressed.current) return;
			pressed.current = false;
			if (grow.current) grow.current = performance.now() + GROW_MS;
			invalidate();
		};
		window.addEventListener('pointerdown', down, true);
		window.addEventListener('pointerup', up, true);
		window.addEventListener('pointercancel', up, true);
		const off = lessonStore(lesson).subscribe((s, prev) => {
			if (s.vals === prev.vals) return;
			grow.current = performance.now() + GROW_MS;
			invalidate();
		});
		return () => {
			off();
			window.removeEventListener('pointerdown', down, true);
			window.removeEventListener('pointerup', up, true);
			window.removeEventListener('pointercancel', up, true);
		};
	}, [reg, invalidate]);
	useFrame(({ invalidate: again }) => {
		const due = grow.current;
		if (!due || !controls || !h) return;
		if (settle.current > 0 || !atHome.current || anim.current) {
			grow.current = 0;
			return;
		}
		if (pressed.current) return;
		if (performance.now() < due) {
			again();
			return;
		}
		grow.current = 0;
		const fig = figure(), was = homeBox.current, frame = framed(w, h);
		if (!fig || !was || !frame || !outside(boundsOf(frame), boundsOf(fig))) return;
		const parts = [...(partsOf.get(was) ?? []), ...(partsOf.get(fig) ?? [])];
		const both = was.clone().union(fig);
		partsOf.set(both, parts);
		homeBox.current = both;
		const f = fitZoom();
		if (camera instanceof OrthoCam && mode === '2d') {
			controls.minZoom = f * ZOOM_2D[0];
			controls.maxZoom = f * ZOOM_2D[1];
		}
		fit.current = f;
		go(home());
		atHome.current = true;
	});
	// A pointer press on the stage (a handle drag, a pan) ends the settling: the view is the reader's now.
	useEffect(() => {
		const el = reg.el;
		if (!el) return;
		const stop = () => (settle.current = -1);
		el.addEventListener('pointerdown', stop, true);
		return () => el.removeEventListener('pointerdown', stop, true);
	}, [reg]);

	useFrame(() => {
		const a = anim.current;
		if (!a) return;
		const s = springProgress(performance.now() - a.t0, VIEW_MS);
		apply(lerpPose(a.from, a.to, s));
		if (s >= 1) {
			anim.current = null;
			pose.current = current();
		} else invalidate();
	});

	return null;
}

export function Stage(p: StageProps): React.JSX.Element {
	const locale = useLocale();
	const sizeState = useSizeState();
	useEffect(ensureSceneHost, []);
	const ref = useRef<HTMLDivElement>(null);
	const { w, h } = useBoxSize(ref);
	// One registry per stage, created once (mode and card of a mounted stage never change).
	const [reg] = useState(() => p.reg ?? createStage(p.mode));
	if (reg.mode !== p.mode) throw new Error(`Stage: registry mode ${reg.mode} does not match mode ${p.mode}`);
	// Same value on every render; primitives that cover the stage (HalfPlane) read it while rendering.
	reg.extent = p.extent;
	useLayoutEffect(() => {
		reg.el = ref.current;
		return () => {
			reg.el = null;
		};
	}, [reg]);
	const visible = viewVisible(reg.card ? reg.card.lesson + ':' + reg.card.id : undefined, sizeState);
	useLayoutEffect(() => { reg.nav?.redraw(); }, [visible, reg]);
	const pose = useRef<Pose | null>(null);
	const content = useRef<Group>(null);
	const inset = p.inset ?? 0;
	const c = p.center ?? [0, 0];
	const center: [number, number, number] = [c[0], c[1], 0];
	const e = p.extent;
	const proj = p.mode === '2d' ? 'ortho' : (p.proj ?? 'persp');
	return (
		<div className="lab-stage" ref={ref} style={{ '--lab-vp-inset': `${inset}px` } as React.CSSProperties}>
			<View className="lab-stage-view" visible={visible}>
				{/* The provider sits inside the View: context does not cross drei's tunnel into the shared Canvas. */}
				<LocaleContext.Provider value={locale}><StageCtx.Provider value={reg}>
					<InvalidateOnStore lesson={p.lesson} />
					<InvalidateOnTheme />
					{p.mode === '2d' ? (
						<OrthographicCamera makeDefault position={[center[0], center[1], 10]} zoom={1} />
					) : proj === 'persp' ? (
						<PerspectiveCamera key="persp" makeDefault position={home3(e)} fov={FOV} />
					) : (
						<OrthographicCamera key="ortho" makeDefault position={home3(e)} near={-100 * e} far={100 * e} />
					)}
					{p.mode === '2d' ? (
						<OrbitControls
							makeDefault
							enableRotate={false}
							enableDamping={false}
							screenSpacePanning
							// No LEFT / ONE entry: LMB on empty 2D space and one finger do nothing to the view (handles take
							// LMB; a finger scrolls the page), so a near-miss on a handle never pans the drawing away.
							mouseButtons={{ MIDDLE: MOUSE.PAN, RIGHT: MOUSE.PAN }}
							touches={{ TWO: TOUCH.DOLLY_PAN }}
						/>
					) : (
						<OrbitControls
							makeDefault
							enableDamping={false}
							minDistance={e * DIST_3D[0]}
							maxDistance={e * DIST_3D[1]}
							mouseButtons={{ LEFT: MOUSE.ROTATE, MIDDLE: MOUSE.ROTATE, RIGHT: MOUSE.PAN }}
						/>
					)}
					<Nav reg={reg} mode={p.mode} extent={e} center={center} inset={inset} w={w} h={h} pose={pose} content={content} />
					{/* After the camera and controls, so its effects see the controls already connected. */}
					<DragLayer />
					<LabelLayer />
					{p.mode === '3d' && <GizmoSync />}
					{p.drawIn && <DrawIn />}
					{/* Scene code reads the drawn values: eased writes glide here while readouts jump (store.ts). */}
					<ShownCtx.Provider value={true}>
						{/* The figure: the home view frames its bounds (View All). */}
						<group ref={content}>{p.children}</group>
					</ShownCtx.Provider>
				</StageCtx.Provider></LocaleContext.Provider>
			</View>
			{/* Sibling of the View div (see interface note): DOM here never blocks scene raycasts outside its own boxes. */}
			<div className="lab-stage-overlay">
				<Labels reg={reg} />
				<DragKeys reg={reg} />
				{p.mode === '3d' ? <Gizmo reg={reg} /> : <XYCorner />}
				{p.overlay}
			</div>
			<p className="lab-stage-nogl">{t(locale, 'viewport.nogl')}</p>
		</div>
	);
}
