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
import { lazy, Suspense, useEffect, useLayoutEffect, useRef, useState, type RefObject } from 'react';
import { Box3, MOUSE, OrthographicCamera as OrthoCam, PerspectiveCamera as PerspCam, Sphere, TOUCH, Vector3, type Group } from 'three';
import type { V } from '../geom';
import { usePalette } from '../palette';
import { lessonStore, ShownCtx } from '../store';
import { Gizmo, XYCorner } from './Gizmo';
import { createStage, DragKeys, Labels, overlayOn, StageCtx, type StageReg } from './handles';
import { axisPose, lerpPose, positionOf, poseOf, reducedMotion, springProgress, VIEW_MS, type Pose } from './motion';
import { ensureSceneHost } from './SceneHost';
import { useSizeState, viewVisible } from './sizeModes';
import { contentBox, DragLayer, DrawIn, GizmoSync, LabelLayer } from './view';
import { useStore } from 'zustand';

import { LookCtx, type Look } from './look';

import { fit2d, fit3d, MARGIN_2D, type Bounds, type FramePad } from './framing';
export type { FramePad } from './framing';

// The look's furniture (sky, floor and reflection, instrument feed) is its own lazy chunk.
const HoloLayer = lazy(() => import('./HoloLayer').then((m) => ({ default: m.HoloLayer })));

const NO_PAD: FramePad = { l: 0, r: 0, t: 0, b: 0 };

export interface StageProps {
	lesson: string;
	look?: Look;
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
	/** Space the home view keeps free around the figure (ViewportCard: instrument columns and formula plate). */
	pad?: FramePad;
}

/**
 * Whether the holographic floor is shown: always in the holo look unless the card declares a `floor` overlay that
 * the reader switched off; never in the clay studio (its light floor is the background itself).
 */
export function useFloor(reg: StageReg, look: Look): boolean {
	const card = reg.card;
	return useStore(lessonStore(card?.lesson ?? 'scene'), (s) => {
		if (look !== 'holo') return false;
		const def = card?.overlays.find((o) => o.id === 'floor');
		return !card || !def || overlayOn(s.vals, card.id, def);
	});
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
/** Horizon of the 2D floor, as a share of the figure's height above its base line. */
const HORIZON = 0.12;
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
	pad: FramePad;
	floor: boolean;
}

/** Plain bounds of a three Box3 for the pure framing math. */
const boundsOf = (b: Box3): Bounds => ({ min: [b.min.x, b.min.y, b.min.z], max: [b.max.x, b.max.y, b.max.z] });
/** Unit direction from the target to the default 3D camera. */
const HOME_DIR = home3(1 / Math.hypot(...home3(1)));

/**
 * Camera navigation of one stage: fitting (2D zoom so `extent` reaches the nearer edge of the free area, keeping the
 * reader's relative zoom across resizes; the N-panel inset re-centers the projection with a view offset), the pose
 * hand-over when the 3D camera is switched, and the smooth view of Reset view and gizmo align (`reg.nav`): the camera
 * swings around the target with the shared spring over VIEW_MS, or cuts under reduced motion. Grabbing the view
 * mid-animation cancels it (direct manipulation is never fought).
 */
function Nav({ reg, mode, extent: e, center, inset, w, h, pose, content, pad, floor }: NavProps) {
	const camera = useThree((s) => s.camera);
	const controls = useThree((s) => s.controls) as unknown as OrbitImpl | null;
	const invalidate = useThree((s) => s.invalidate);
	const fit = useRef(0);
	// Projection of the camera that last held the view (null before the first hand-over).
	const was = useRef<boolean | null>(null);
	const anim = useRef<{ from: Pose; to: Pose; t0: number } | null>(null);

	const ortho = camera instanceof OrthoCam;
	// Read by frame callbacks and late layout effects: the latest props, not those of the render that bound them.
	const padRef = useRef(pad);
	padRef.current = pad;
	const floorRef = useRef(floor);
	floorRef.current = floor;
	/** Figure bounds captured by the last home view (null: frame the extent, e.g. nothing drawn yet). */
	const homeBox = useRef<Box3 | null>(null);
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
		const box = homeBox.current;
		const base = extentZoom(width, height);
		if (!box) return base;
		if (mode === '3d') return fit3dFor(box, width, height).zoom;
		const z = fit2d(boundsOf(box), width, height, inset, padRef.current, floorRef.current).zoom;
		return Math.min(base * HOME_2D[1], Math.max(base * HOME_2D[0], z));
	};
	/** 3D fit in the view left free by the N-panel, the HUD columns and the plate. */
	const fit3dFor = (box: Box3, width: number, height: number) => {
		const p = padRef.current;
		return fit3d(boundsOf(box), HOME_DIR, width - inset - p.l - p.r, height - p.t - p.b, TAN);
	};
	/** Measures the figure (View All) without applying it; null when nothing is drawn yet. */
	const figure = (): Box3 | null => {
		const root = content.current;
		const box = root ? contentBox(root, new Box3()) : null;
		return box && !box.isEmpty() ? box : null;
	};
	/** Re-measures the figure and publishes where its floor is. */
	const measure = (b: Box3 | null = figure()) => {
		homeBox.current = b;
		if (mode === '3d') {
			const s = b ? b.getBoundingSphere(new Sphere()) : new Sphere(new Vector3(), e);
			reg.frame.set({ y: Math.min(0, b ? b.min.y : 0), horizon: 0, cx: s.center.x, cz: s.center.z, size: s.radius });
		} else {
			const size = b ? b.max.y - b.min.y : 2 * e;
			reg.frame.set({ y: b ? b.min.y : center[1] - e, horizon: HORIZON * size, cx: b ? (b.min.x + b.max.x) / 2 : center[0], cz: 0, size });
		}
	};
	/** The home view for a `width` x `height` stage (default: the last measured size). */
	const home = (width = w, height = h): Pose => {
		const box = homeBox.current;
		if (mode === '2d') {
			if (!box) return poseOf([center[0], center[1], 10], center, fitZoom(width, height));
			const f = fit2d(boundsOf(box), width, height, inset, padRef.current, floorRef.current);
			const zoom = fitZoom(width, height);
			// A clamped zoom scales the box's offset from the camera centre with it, keeping it in the free area.
			const mx = (box.min.x + box.max.x) / 2;
			const my = (box.min.y + box.max.y) / 2;
			const k = f.zoom / zoom;
			const target: [number, number, number] = [mx + (f.cx - mx) * k, my + (f.cy - my) * k, 0];
			return poseOf([target[0], target[1], 10], target, zoom);
		}
		// Same three-quarter direction as before. Without a figure: the sphere of radius FRAME_3D * extent.
		const dir = HOME_DIR;
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

	useLayoutEffect(() => {
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
		reg.nav = {
			reset: () => {
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
	const look = p.look ?? 'holo';
	const floor = useFloor(reg, look);
	const inset = p.inset ?? 0;
	const c = p.center ?? [0, 0];
	const center: [number, number, number] = [c[0], c[1], 0];
	const e = p.extent;
	const proj = p.mode === '2d' ? 'ortho' : (p.proj ?? 'persp');
	return (
		<div className="lab-stage" data-look={look} ref={ref} style={{ '--lab-vp-inset': `${inset}px` } as React.CSSProperties}>
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
					<Nav reg={reg} mode={p.mode} extent={e} center={center} inset={inset} w={w} h={h} pose={pose} content={content} pad={p.pad ?? NO_PAD} floor={floor} />
					{/* After the camera and controls, so its effects see the controls already connected. */}
					<DragLayer />
					<LabelLayer />
					{p.mode === '3d' && <GizmoSync />}
					{p.drawIn && <DrawIn />}
					{/* Scene code reads the drawn values: eased writes glide here while readouts jump (store.ts). */}
					<ShownCtx.Provider value={true}>
						<LookCtx.Provider value={look}>
							<Suspense fallback={null}>
								<HoloLayer look={look} floor={floor} content={content} inset={inset} />
							</Suspense>
							{/* The figure: framed by the home view and drawn a second time, mirrored, into the floor. */}
							<group ref={content}>{p.children}</group>
						</LookCtx.Provider>
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
