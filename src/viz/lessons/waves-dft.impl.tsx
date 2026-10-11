import { useEffect, useMemo, useRef } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import { BoxGeometry, CircleGeometry, Raycaster, Vector2, type Group } from 'three';
import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lessonStore, useVals, type LessonState } from '../store';
import { STYLE, usePalette } from '../palette';
import type { V } from '../geom';
import { snapTo } from '../scrub';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Poly } from '../scene/Poly';
import { Label } from '../scene/Label';
import { rayPlane } from '../scene/drag';
import { useOverlay, useStage, type Draggable } from '../scene/handles';
import { Stroke, usePxScale, type StrokeHandle } from '../scene/view';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor, type Scene } from './waves-dft.rows';
import {
	chain, CLOCK, coefA, coefB, derive, deriveEpi, DRAW_EXTENT, fmt2, HALF, LESSON, N, rebuildAt, resampleClosed, signalCurve,
	signalId, SPEC, xpos,
} from './waves-dft.math';

/**
 * Lesson 1.9 scenes: WD-1 signal, ruler and their products (the dot product sample by sample), WD-2 the spectrum
 * (every ruler's copies as a bar), WD-3 the rebuild (rulers added back), WD-4 epicycles (a drawing redrawn by spinning
 * sticks). Blender-like flat viewport look (Task 6d). Loaded lazily by the entry (waves-dft.tsx); shares its store.
 */

type Vals = LessonState['vals'];

const noRaycast = () => {};
const unitBox = new BoxGeometry(1, 1, 1);

/** An invisible box over everything the scene can draw: the home view frames it, so the view stays still while values change. */
function Reach({ lo, hi }: { lo: V; hi: V }) {
	return <mesh geometry={unitBox} visible={false} raycast={noRaycast}
		position={[(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, 0]} scale={[hi[0] - lo[0], hi[1] - lo[1], 0.01]} />;
}

/** A thin straight guide line (lane baseline), drawn once. */
function Baseline({ y }: { y: number }) {
	const pal = usePalette();
	const pts = useMemo(() => [[xpos(-0.6), y, 0], [xpos(N - 0.4), y, 0]] as [number, number, number][], [y]);
	return <Stroke points={pts} color={pal.muted} width={1} halo={false} order={0.5} opacity={0.6} />;
}

/** A smooth curve f(u) over the sample window, at lane y with vertical scale s (the continuous wave behind the dots). */
function Curve({ f, y, s, role, dashed }: { f: (u: number) => number; y: number; s: number; role: 'vecA' | 'vecB' | 'vecC' | 'muted'; dashed?: boolean }) {
	const pal = usePalette();
	const pts = useMemo(() => Array.from({ length: 241 }, (_, i): [number, number, number] => {
		const u = -0.4 + (i / 240) * (N - 0.2);
		return [xpos(u), y + f(u) * s, 0];
	}), [f, y, s]);
	return <Stroke points={pts} color={pal[role]} width={STYLE.edge} halo={false} order={0.8} dashPx={dashed ? STYLE.dash : undefined} />;
}

/**
 * Drag mapping of sample n on a lane with vertical scale `s`: only the height moves, snapped to 0.1 and kept in
 * [-2, 2]. A key step is in value units (arrow = 0.1, Shift = 1), a drag in world units divided by the lane scale.
 */
const sampleMove = (n: number, s: number): Draggable['move'] => (vals, raw, src) => {
	const x = [...(vals.x as number[])];
	const d = src === 'key' ? raw[1] ?? 0 : (raw[1] ?? 0) / s;
	x[n] = snapTo(x[n] + d, SPEC.x);
	return { set: { x } };
};

// ---------------------------------------------------------------------------------------------------------------
// WD-1: signal, ruler, products

/** Lane heights and vertical scales of WD-1 (values up to 2 for the signal and products, 1 for the ruler). */
const LANE = { sig: 3, rul: 0, pro: -3 } as const;
const SCALE = { sig: 0.55, rul: 1.1, pro: 0.55 } as const;
/** Lane titles sit above each lane, clear of its tallest stem. */
const TITLE_DY = 1.5;

function Lanes() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const z = derive(v);
	const curves = useOverlay('curves');
	const labels = useOverlay('labels');
	const rulerRole = z.sine ? 'vecC' : 'vecB';
	const fn = z.sine ? 'sin' : 'cos';
	const curve = z.src >= 0 ? signalCurve(signalId(z.src)) : null;
	const rulerF = useMemo(() => (u: number) => (z.sine ? Math.sin : Math.cos)((2 * Math.PI * z.k * u) / N), [z.k, z.sine]);
	return <>
		<Reach lo={[xpos(-1), -4.6]} hi={[xpos(N), 4.6]} />
		{(['sig', 'rul', 'pro'] as const).map((l) => <Baseline key={l} y={LANE[l]} />)}
		{curves && curve && <Curve f={curve} y={LANE.sig} s={SCALE.sig} role="muted" dashed />}
		{curves && <Curve f={rulerF} y={LANE.rul} s={SCALE.rul} role={rulerRole} />}
		{z.x.map((xn, n) => {
			const p = z.products[n];
			return <group key={n}>
				{xn !== 0 && <Segment lesson={LESSON} id={'x' + n} a={[xpos(n), LANE.sig]} b={[xpos(n), LANE.sig + xn * SCALE.sig]} role="vecA" />}
				{Math.abs(z.r[n]) > 1e-12 && <Segment lesson={LESSON} id={'r' + n} a={[xpos(n), LANE.rul]} b={[xpos(n), LANE.rul + z.r[n] * SCALE.rul]} role={rulerRole} />}
				{/* Sign is never color only: a negative product is a dashed stem. */}
				{Math.abs(p) > 1e-12 && <Segment lesson={LESSON} id={'p' + n} a={[xpos(n), LANE.pro]} b={[xpos(n), LANE.pro + p * SCALE.pro]} role="light" dashed={p < 0} />}
				<Point lesson={LESSON} id={'r' + n} at={[xpos(n), LANE.rul + z.r[n] * SCALE.rul]} role={rulerRole} size={5} />
				<Point lesson={LESSON} id={'x' + n} at={(w) => [xpos(n), LANE.sig + (w.x as number[])[n] * SCALE.sig]} keys={['x']}
					move={sampleMove(n, SCALE.sig)} role="vecA" label={t(locale, 'wd.handleSample', { n })} />
			</group>;
		})}
		{labels && <>
			<Label lesson={LESSON} id="signal" at={[0, LANE.sig + TITLE_DY]} role="vecA">{t(locale, 'wd.laneSignal')}</Label>
			<Label lesson={LESSON} id="ruler" at={[0, LANE.rul + TITLE_DY]} role={rulerRole}>{t(locale, 'wd.laneRuler', { fn, k: z.k })}</Label>
			<Label lesson={LESSON} id="dot" at={[0, LANE.pro + TITLE_DY]} role="light">{t(locale, 'wd.laneProduct', { sum: fmt2(z.dot) })}</Label>
		</>}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// WD-2: spectrum

/** Horizontal centre of frequency k's pair of bars, the bar half-width and the height of one copy. */
const kx = (k: number) => (k - HALF / 2) * 0.85;
const BAR = 0.13;
const BAR_H = 1.3;
const K_ROW = -3.3;

/** One bar: `side` -1 = the cosine bar (left), +1 = the sine bar (right). Negative copies hang down, hatched. */
function Bar({ k, side }: { k: number; side: -1 | 1 }) {
	const id = (side < 0 ? 'bc' : 'bs') + k;
	const value = (v: Vals) => {
		const b = derive(v).bins[k];
		return side < 0 ? coefA(b) : coefB(b);
	};
	const pick = (e: { stopPropagation(): void }) => {
		e.stopPropagation();
		lessonStore(LESSON).getState().set('k', k);
	};
	return <group onClick={pick}>
		<Poly lesson={LESSON} id={id} role={side < 0 ? 'vecB' : 'vecC'} fill={0.55} hatched={(v) => value(v) < 0}
			pts={(v) => {
				const x0 = kx(k) + side * (BAR + 0.03) - BAR;
				const h = value(v) * BAR_H;
				return [[x0, 0], [x0 + 2 * BAR, 0], [x0 + 2 * BAR, h], [x0, h]];
			}} />
	</group>;
}

function Spectrum() {
	const v = useVals(LESSON);
	const labels = useOverlay('labels');
	const sel = v.k as number;
	const ks = Array.from({ length: HALF + 1 }, (_, k) => k);
	return <>
		<Reach lo={[kx(0) - 0.6, -3.7]} hi={[kx(HALF) + 0.6, 3.2]} />
		{/* The selected frequency: a faint column behind its two bars. */}
		<Poly lesson={LESSON} id="sel" role="muted" fill={0.12} edge="none"
			pts={() => [[kx(sel) - 0.42, -2.9], [kx(sel) + 0.42, -2.9], [kx(sel) + 0.42, 2.9], [kx(sel) - 0.42, 2.9]]} />
		<Baseline y={0} />
		{ks.map((k) => <group key={k}>
			<Bar k={k} side={-1} />
			{/* The sine ruler of k = 0 and k = N/2 is all zeros: it has no bar. */}
			{k !== 0 && k !== HALF && <Bar k={k} side={1} />}
			{labels && <Label lesson={LESSON} id={'k' + k} at={[kx(k), K_ROW]} role={k === sel ? 'hot' : 'muted'}>{k}</Label>}
		</group>)}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// WD-3: rebuild

const REB_SCALE = 1;

function Rebuild() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const z = derive(v);
	const curve = useOverlay('curves');
	const misses = useOverlay('residual');
	const f = useMemo(() => (u: number) => rebuildAt(z.bins, z.K, u), [z.bins, z.K]);
	return <>
		<Reach lo={[xpos(-1), -2.4]} hi={[xpos(N), 2.4]} />
		<Baseline y={0} />
		{curve && z.K > 0 && <Curve f={f} y={0} s={REB_SCALE} role="vecC" />}
		{z.x.map((xn, n) => {
			const b = z.back[n];
			return <group key={n}>
				{misses && Math.abs(xn - b) > 1e-6 && <Segment lesson={LESSON} id={'e' + n} a={[xpos(n), b]} b={[xpos(n), xn]} role="light" dashed />}
				<Point lesson={LESSON} id={'b' + n} at={[xpos(n), b * REB_SCALE]} role="vecC" size={5} />
				<Point lesson={LESSON} id={'x' + n} at={(w) => [xpos(n), (w.x as number[])[n] * REB_SCALE]} keys={['x']}
					move={sampleMove(n, REB_SCALE)} role="vecA" label={t(locale, 'wd.handleSample', { n })} />
			</group>;
		})}
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// WD-4: epicycles

/** Segments per orbit circle and points of the full trace of one loop. */
const CIRCLE_SEG = 40;
const TRACE = 512;
/** Orbits smaller than this (world units) are not drawn: a circle of a pixel or two only adds noise. */
const MIN_ORBIT = 0.03;
const ZERO: [number, number, number][] = [[0, 0, 0], [0, 0, 0]];
const PEN = new CircleGeometry(1, 24);

/** Flat xyz list of 2D points. */
const flat = (pts: readonly (readonly number[])[]): number[] => pts.flatMap((p) => [p[0], p[1], 0]);

/**
 * The chain of sticks, their orbit circles, the pen and the trace it leaves. Nothing here re-renders per frame: the
 * time lives in the CLOCK store and a frame callback rewrites the line positions (the canvas renders on demand, so a
 * clock change asks for a frame).
 */
function Chain() {
	const pal = usePalette();
	const v = useVals(LESSON);
	const d = useMemo(() => deriveEpi(v), [v.path, v.S]);
	const showCircles = useOverlay('circles');
	const showTrace = useOverlay('trace');
	const sticks = useRef<StrokeHandle>(null);
	const circles = useRef<StrokeHandle>(null);
	const trace = useRef<StrokeHandle>(null);
	const pen = useRef<Group>(null);
	const invalidate = useThree((s) => s.invalidate);
	usePxScale(pen, 4.5);
	// The whole trace of one loop for these sticks, computed once; a frame shows the part up to t.
	const full = useMemo(() => Array.from({ length: TRACE + 1 }, (_, i) => {
		const j = chain(d.e, d.S, i / TRACE);
		return j[j.length - 1];
	}), [d]);
	useEffect(() => lessonStore(CLOCK).subscribe(() => invalidate()), [invalidate]);
	useEffect(() => invalidate(), [d, showCircles, showTrace, invalidate]);
	useFrame(() => {
		const time = lessonStore(CLOCK).getState().vals.t as number;
		const joints = chain(d.e, d.S, time);
		sticks.current?.setPositions(flat(joints));
		const ring: number[] = [];
		d.e.list.slice(0, d.S).forEach((st, j) => {
			if (st.r < MIN_ORBIT) return;
			const [cx, cy] = joints[j];
			for (let i = 0; i < CIRCLE_SEG; i++) {
				const a0 = (2 * Math.PI * i) / CIRCLE_SEG;
				const a1 = (2 * Math.PI * (i + 1)) / CIRCLE_SEG;
				ring.push(cx + st.r * Math.cos(a0), cy + st.r * Math.sin(a0), 0, cx + st.r * Math.cos(a1), cy + st.r * Math.sin(a1), 0);
			}
		});
		circles.current?.setPositions(ring.length ? ring : flat(ZERO));
		const end = joints[joints.length - 1];
		const upto = full.slice(0, Math.floor(time * TRACE) + 1);
		trace.current?.setPositions(flat([...upto, end]));
		pen.current?.position.set(end[0], end[1], 0);
	});
	return <>
		{showCircles && <Stroke ref={circles} points={ZERO} segments color={pal.muted} width={1} halo={false} order={0.7} opacity={0.7} />}
		{showTrace && <Stroke ref={trace} points={ZERO} color={pal.vecC} width={STYLE.core} order={1.5} />}
		<Stroke ref={sticks} points={ZERO} color={pal.vecA} width={2} halo={false} order={2} />
		<group ref={pen}>
			<mesh geometry={PEN} renderOrder={2.9}>
				<meshBasicMaterial color={pal.light} transparent depthWrite={false} />
			</mesh>
		</group>
	</>;
}

/** The drawing the sticks aim at (dashed, closed), from the drawn (`shown`) path. */
function Target() {
	const pal = usePalette();
	const path = useVals(LESSON).path as number[];
	const pts = useMemo(() => {
		const out: [number, number, number][] = [];
		for (let m = 0; m <= path.length / 2; m++) {
			const i = (m % (path.length / 2)) * 2;
			out.push([path[i], path[i + 1], 0]);
		}
		return out;
	}, [path]);
	return <Stroke points={pts} color={pal.ink} width={STYLE.edge} halo={false} order={0.9} opacity={0.55} dashPx={STYLE.dash} />;
}

/**
 * Freehand input while draw mode is on: one stroke of the pointer (mouse, pen or finger) is collected on the z = 0
 * plane, shown live, and on release resampled to the drawing's M points. The listeners sit on the stage element in the
 * capture phase like DragLayer's; while drawing, a finger must not scroll the page, so touch-action is set to none for
 * the whole mode (touch-action is latched when the finger lands) and touchstart is cancelled.
 */
function DrawPad() {
	const reg = useStage();
	const pal = usePalette();
	const camera = useThree((s) => s.camera);
	const invalidate = useThree((s) => s.invalidate);
	const on = useStore(lessonStore(LESSON), (s) => s.vals.draw === 1);
	const live = useRef<StrokeHandle>(null);
	useEffect(() => {
		const el = reg.el;
		if (!el || !on) return;
		const rc = new Raycaster();
		const ndc = new Vector2();
		const world = (cx: number, cy: number): [number, number] | null => {
			const r = el.getBoundingClientRect();
			ndc.set(((cx - r.left) / el.clientWidth) * 2 - 1, 1 - ((cy - r.top) / el.clientHeight) * 2);
			rc.setFromCamera(ndc, camera);
			const h = rayPlane(rc.ray.origin.toArray(), rc.ray.direction.toArray(), [0, 0, 0], [0, 0, 1]);
			if (!h) return null;
			const c = (u: number) => Math.max(-DRAW_EXTENT, Math.min(DRAW_EXTENT, u));
			return [c(h[0]), c(h[1])];
		};
		const ac = new AbortController();
		const opts = { signal: ac.signal, capture: true };
		let pts: [number, number][] = [];
		let id: number | null = null;
		el.style.touchAction = 'none';
		el.style.cursor = 'crosshair';
		el.addEventListener('touchstart', (e) => e.preventDefault(), { signal: ac.signal, passive: false });
		el.addEventListener('pointerdown', (e) => {
			if (id !== null || (e.pointerType === 'mouse' && e.button !== 0)) return;
			const p = world(e.clientX, e.clientY);
			if (!p) return;
			e.preventDefault();
			e.stopPropagation();
			el.setPointerCapture(e.pointerId);
			id = e.pointerId;
			pts = [p];
		}, opts);
		el.addEventListener('pointermove', (e) => {
			if (e.pointerId !== id) return;
			const p = world(e.clientX, e.clientY);
			const last = pts[pts.length - 1];
			// Keep points at least a twentieth of a unit apart: a still finger adds nothing.
			if (!p || Math.hypot(p[0] - last[0], p[1] - last[1]) < 0.05) return;
			pts.push(p);
			live.current?.setPositions(flat(pts.length > 1 ? pts : [pts[0], pts[0]]));
			invalidate();
		}, opts);
		const end = (e: PointerEvent) => {
			if (e.pointerId !== id) return;
			id = null;
			live.current?.setPositions(flat(ZERO));
			invalidate();
			const path = resampleClosed(pts);
			pts = [];
			// A tap or a scribble shorter than half a unit is not a drawing: stay in draw mode and wait for one.
			if (!path) return;
			lessonStore(LESSON).getState().setMany({ path, draw: 0 });
			lessonStore(CLOCK).getState().setMany({ t: 0, play: 1 });
		};
		el.addEventListener('pointerup', end, opts);
		el.addEventListener('pointercancel', end, opts);
		return () => {
			ac.abort();
			el.style.touchAction = '';
			el.style.cursor = '';
			live.current?.setPositions(flat(ZERO));
		};
	}, [reg, camera, invalidate, on]);
	return <Stroke ref={live} points={ZERO} color={pal.hot} width={STYLE.core} halo={false} order={3} />;
}

function Epicycles() {
	const drawing = useOverlay('target');
	const grid = useOverlay('grid');
	return <>
		<Reach lo={[-DRAW_EXTENT - 0.2, -DRAW_EXTENT - 0.2]} hi={[DRAW_EXTENT + 0.2, DRAW_EXTENT + 0.2]} />
		<Grid lesson={LESSON} extent={DRAW_EXTENT + 0.4} step={1} axes={false} lines={grid} ticks={false} />
		{drawing && <Target />}
		<Chain />
		<DrawPad />
	</>;
}

// ---------------------------------------------------------------------------------------------------------------
// Cards

/** Status bar hints per scene; values are read live. */
function statusFor(scene: Scene, locale: Locale): StatusSpec {
	if (scene === 1 || scene === 3) {
		const hints: StatusSpec['hints'] = Object.fromEntries(Array.from({ length: N }, (_, n) => [
			['x' + n, { mouse: t(locale, 'wd.hintSampleMouse', { n }), touch: t(locale, 'wd.hintSampleTouch', { n }) }],
			['r' + n, (v: Vals) => t(locale, 'wd.hintRuler', { n, v: fmt2(derive(v).r[n]) })],
			['p' + n, (v: Vals) => {
				const z = derive(v);
				return t(locale, 'wd.hintProduct', { n, x: fmt2(z.x[n]), r: fmt2(z.r[n]), p: fmt2(z.products[n]) });
			}],
			['e' + n, (v: Vals) => {
				const z = derive(v);
				return t(locale, 'wd.hintMiss', { n, d: fmt2(z.x[n] - z.back[n]) });
			}],
		]).flat());
		return scene === 1
			? { hints, idle: { mouse: t(locale, 'wd.idleDotMouse'), touch: t(locale, 'wd.idleDotTouch') } }
			: { hints, idle: { mouse: t(locale, 'wd.idleRebuildMouse'), touch: t(locale, 'wd.idleRebuildTouch') } };
	}
	if (scene === 2) {
		const hints: StatusSpec['hints'] = Object.fromEntries(Array.from({ length: HALF + 1 }, (_, k) => [
			['bc' + k, (v: Vals) => t(locale, 'wd.hintBar', { k, a: fmt2(coefA(derive(v).bins[k])), b: fmt2(coefB(derive(v).bins[k])) })],
			['bs' + k, (v: Vals) => t(locale, 'wd.hintBar', { k, a: fmt2(coefA(derive(v).bins[k])), b: fmt2(coefB(derive(v).bins[k])) })],
		]).flat());
		return { hints, idle: t(locale, 'wd.idleSpectrum') };
	}
	return {
		state: (v: Vals) => (v.draw === 1 ? t(locale, 'wd.stateDraw') : null),
		idle: { mouse: t(locale, 'wd.idleEpiMouse'), touch: t(locale, 'wd.idleEpiTouch') },
	};
}

const CARD = {
	1: { id: 'WD-1', name: 'wd.nameDot' }, 2: { id: 'WD-2', name: 'wd.nameSpectrum' },
	3: { id: 'WD-3', name: 'wd.nameRebuild' }, 4: { id: 'WD-4', name: 'wd.nameEpi' },
} as const;

export function WavesScene({ scene }: { scene: Scene }) {
	const locale = useLocale();
	const ov = (id: string, key: Parameters<typeof t>[1]) => ({ id, label: t(locale, key), on: true });
	const overlays = scene === 1 ? [ov('curves', 'wd.ovCurves'), ov('labels', 'wd.ovLabels')]
		: scene === 2 ? [ov('labels', 'wd.ovLabels')]
		: scene === 3 ? [ov('curves', 'wd.ovCurves'), ov('residual', 'wd.ovResidual')]
		: [ov('grid', 'wd.ovGrid'), ov('target', 'wd.ovTarget'), ov('circles', 'wd.ovCircles'), ov('trace', 'wd.ovTrace')];
	const extent = scene === 1 ? 4.8 : 4;
	return <ViewportCard lesson={LESSON} id={CARD[scene].id} name={CARD[scene].id + ' · ' + t(locale, CARD[scene].name)} mode="2d"
		extent={extent} center={[0, 0]} rows={rowsFor(scene, locale)} overlays={overlays} status={statusFor(scene, locale)} drawIn={scene === 1}>
		{scene === 1 ? <Lanes /> : scene === 2 ? <Spectrum /> : scene === 3 ? <Rebuild /> : <Epicycles />}
	</ViewportCard>;
}
