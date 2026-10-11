import { useEffect, useMemo, useRef, useState } from 'react';
import { useFrame, useThree } from '@react-three/fiber';
import { DoubleSide, MeshBasicMaterial, Matrix4, Vector3, type Group, type Mesh, type Material, type BufferGeometry } from 'three';
import { useStore } from 'zustand';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import { lessonStore, useVals, useStep, useHot } from '../store';
import { add, sub, scale, to3, type V } from '../geom';
import { STYLE, usePalette, roleVar, type SceneRole } from '../palette';
import { ViewportCard } from '../scene/ViewportCard';
import { Grid } from '../scene/Grid';
import { Point } from '../scene/Point';
import { Segment } from '../scene/Segment';
import { Poly, type PolyProps } from '../scene/Poly';
import { AngleArc } from '../scene/AngleArc';
import { RightAngle } from '../scene/RightAngle';
import { Circle } from '../scene/Circle';
import { Label } from '../scene/Label';
import { Stroke } from '../scene/view';
import { Hatch } from '../scene/hatchMaterial';
import { reducedMotion } from '../scene/motion';
import { Show, useOverlay, useHoverable, useLabel, useStage, HOVER_RANK, type StageReg } from '../scene/handles';
import type { StatusSpec } from '../scene/StatusBar';
import { rowsFor } from './law-of-cosines.rows';
import { LESSON, derive, solveIK, square, strip, pieceAt, moveA, moveB, MAGNETS, fmt, fmtAngle, CUT_PIECES, PROOF_PIECES, shownPiece, type DetailPiece } from './law-of-cosines.math';

/**
 * Lesson 0.5 scenes (three): LC-0 missing rectangles, LC-1 ledger triangle, LC-2 proof tiles, LC-3 two-bone IK,
 * plus the two detail views that re-draw one selected piece of LC-0 / LC-2 at a readable scale. Loaded lazily by
 * the entry (law-of-cosines.tsx); shares its store, `derive()` and hot ids.
 */

type Scene = 0 | 1 | 2 | 3;
type Vals = Parameters<typeof derive>[0];
type Lane = readonly [0 | 1 | 2, number];

/** Stage extent per scene (controller rulings: LC-0/LC-2 at 11 so every reachable square fits). */
const EXTENTS = [11, 5.5, 11, 6.5];

const midpoint = (a: V, b: V) => scale(add(a, b), .5);
const centroid = (pts: V[]) => pts.reduce<V>((acc, p) => add(acc, scale(p, 1 / pts.length)), [0, 0]);

/**
 * Overview polygons publish their drawn mesh (group) by id so a detail view can sample the very same geometry and
 * world matrix, including intermediate rigid rotations: no second placement model, no second clock.
 */
const sources = new Map<string, Group>();
function sourceMesh(id: string): Mesh | undefined {
	let mesh: Mesh | undefined;
	sources.get(id)?.traverse((o) => {
		const m = o as Mesh;
		if (!mesh && m.isMesh && m.geometry.getAttribute('position')?.count === 4) mesh = m;
	});
	return mesh;
}
function meshPoints(mesh: Mesh): V[] {
	const a = mesh.geometry.getAttribute('position');
	mesh.updateWorldMatrix(true, false);
	return Array.from({ length: a.count }, (_, i) => {
		const v = new Vector3(a.getX(i), a.getY(i), a.getZ(i)).applyMatrix4(mesh.matrixWorld);
		return [v.x, v.y] as V;
	});
}


/**
 * Screen position of a label lane (controller ruling: LC-0/LC-2 labels sit in lanes with leaders, because at
 * 320 px the overview geometry is too dense for centred labels). Lane 0 = left column, 1 = right column (rows
 * from the top), 2 = bottom row (rows from the bottom). Width excludes the desktop N-panel inset.
 */
function laneXY(reg: StageReg, lane: Lane): { x: number; y: number; width: number } {
	const el = reg.el!;
	const width = el.clientWidth - parseFloat(el.style.getPropertyValue('--lab-vp-inset') || '0');
	const x = width * (lane[0] === 2 ? .5 : lane[0] === 0 ? .23 : .77);
	// Bottom lane 0 sits 20 px up, so a label's lower edge clears the stage edge (at 14 px it touched the status bar).
	const y = lane[0] === 2 ? el.clientHeight - 20 - lane[1] * 22 : 18 + lane[1] * 27;
	return { x, y, width };
}

/** Leader line SVG over the stage; one per laned label. Returns the path element ref. */
function useLeader(reg: StageReg, visible: boolean, id?: string) {
	const line = useRef<SVGPathElement | null>(null);
	useEffect(() => {
		if (!visible || !reg.el) return;
		const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
		svg.setAttribute('aria-hidden', 'true');
		Object.assign(svg.style, { position: 'absolute', inset: '0', width: '100%', height: '100%', pointerEvents: 'none', zIndex: '2' });
		const path = document.createElementNS(svg.namespaceURI, 'path') as SVGPathElement;
		path.setAttribute('fill', 'none');
		path.setAttribute('stroke-width', '1');
		if (id) path.dataset.lcLeader = id;
		svg.append(path);
		reg.el.append(svg);
		line.current = path;
		return () => { line.current = null; svg.remove(); };
	}, [reg, visible, id]);
	return line;
}

/** A text label placed in a lane (with a leader to its anchor) or next to its anchor. */
function LaneText({ id, at, text, role, lane }: { id: string; at: V; text: string; role: SceneRole; lane: Lane }) {
	const reg = useStage(), offset = useRef<[number, number]>([0, 0]), hot = useHot(LESSON, id);
	useLabel({ lesson: LESSON, text, role, hot: id, at: () => at, offset: offset.current });
	const line = useLeader(reg, true);
	useFrame(() => {
		const point = reg.project?.(at);
		if (!reg.el || !point) { line.current?.setAttribute('d', ''); return; }
		const { x: lx, y, width } = laneXY(reg, lane);
		// Keep the whole label inside the viewport: clamp its centre by its own measured half width.
		const spec = reg.labels.list().find((s) => s.hot === id), el = spec && reg.labelEls.get(spec.id), half = (el?.offsetWidth ?? 100) / 2;
		const x = Math.max(half + 4, Math.min(width - half - 4, lx));
		offset.current[0] = x - point[0];
		offset.current[1] = y - point[1];
		line.current?.setAttribute('d', `M ${point[0]} ${point[1]} L ${x} ${y}`);
		line.current?.setAttribute('stroke', `var(${roleVar(hot ? 'hot' : role)})`);
	}, 1.5);
	return null;
}
function Text({ id, at, text, offset = [0, -9], role = 'ink', lane }: { id: string; at: V; text: string; offset?: readonly [number, number]; role?: SceneRole; lane?: Lane }) {
	if (lane) return <Show ov="labels"><LaneText id={id} at={at} text={text} role={role} lane={lane} /></Show>;
	return <Show ov="labels"><Label lesson={LESSON} id={id} at={at} offset={offset} role={role}>{text}</Label></Show>;
}

/**
 * Screen offset (CSS px, y down) that puts a bone's length label beside the bone a-b, on the side away from `away`
 * (the other joint), so it never sits on the bone, the joint glyphs or the target point at the bone's end.
 */
function besideBone(a: V, b: V, away: V): [number, number] {
	const d = sub(b, a);
	const n = Math.hypot(d[0], d[1]) || 1;
	let nx = -d[1] / n;
	let ny = d[0] / n;
	const m = midpoint(a, b);
	if (nx * (away[0] - m[0]) + ny * (away[1] - m[1]) > 0) {
		nx = -nx;
		ny = -ny;
	}
	// Clear the label's half box along the normal: ~32 px half width sideways, ~8 px half height up/down.
	const px = 10 + 32 * Math.abs(nx) + 8 * Math.abs(ny);
	return [nx * px, -ny * px];
}

/** The shared triangle: sides, vertices (A polar drag, B along x), angle arc with magnet ticks, right angle at 90°. */
function Triangle() {
	const z = derive(useVals(LESSON)), labels = useOverlay('labels');
	return <>
		<Segment lesson={LESSON} id="a" a={z.C} b={z.B} role="vecA" />
		<Segment lesson={LESSON} id="b" a={z.C} b={z.A} role="vecB" />
		{!z.degenerate && <Segment lesson={LESSON} id="c" a={z.B} b={z.A} role="vecC" />}
		<Point lesson={LESSON} id="C" at={z.C} role="ink" />
		<Point lesson={LESSON} id="B" at={(v) => derive(v).B} keys={['a']} move={moveB} role="vecA" label="B" />
		<Point lesson={LESSON} id="A" at={(v) => derive(v).A} keys={['b', 'th']} move={moveA} role="vecB" label="A" />
		<Text id="C" at={z.C} text="C" offset={[-10, 10]} /><Text id="A" at={z.A} text="A" offset={[-7, -13]} /><Text id="B" at={z.B} text="B" offset={[10, 10]} />
		<Text id="a" at={midpoint(z.C, z.B)} text="a" offset={[0, 15]} role="vecA" />
		<Text id="b" at={midpoint(z.C, z.A)} text="b" offset={[-32, 0]} role="vecB" />
		<Text id="c" at={midpoint(z.B, z.A)} text="c" offset={[24, 0]} role="vecC" />
		<Show ov="angle"><AngleArc lesson={LESSON} id="th" at={z.C} from={z.B} to={z.A} r={.6} ticks={MAGNETS} label={labels ? 'θ = ' + fmtAngle(z.th) + '°' : undefined} /></Show>
		<Show ov="right"><RightAngle lesson={LESSON} id="rt" at={z.C} u={z.B} v={z.A} show={z.th === 90} /></Show>
	</>;
}

/** Shadow of b on a (side 1) or of a on b (side 2): band, drop line, foot and its right angle, value label. */
function Shadow({ side = 1, solid = false, names = true, lanes = false, lane }: { side?: 1 | 2; solid?: boolean; names?: boolean; lanes?: boolean; lane?: Lane }) {
	const z = derive(useVals(LESSON));
	const onA = side === 1, id = onA ? 'p' : 'q', source = onA ? z.A : z.B, foot = onA ? z.D : z.E;
	return <>
		<Segment lesson={LESSON} id={id} a={z.C} b={foot} band role="muted" />
		<Segment lesson={LESSON} id={onA ? 'drop_a' : 'drop_b'} a={source} b={foot} dashed={!solid} role="muted" />
		<Point lesson={LESSON} id={onA ? 'D' : 'E'} at={foot} role="muted" size={4} />
		<RightAngle lesson={LESSON} id={onA ? 'rtD' : 'rtE'} at={foot} u={sub(source, foot)} v={onA ? [1, 0] : z.u} role="muted" />
		<Text id={onA ? 'D' : 'E'} at={foot} text={onA ? 'D' : 'E'} offset={[8, 12]} />
		{names && <Text lane={lane ?? (lanes ? [0, 1] : undefined)} id={id} at={midpoint(z.C, foot)} text={(onA ? 'b cos θ = ' : 'a cos θ = ') + fmt(onA ? z.p : z.q)} offset={[0, -16]} role="muted" />}
	</>;
}

type LanePolyProps = PolyProps & {
	/** Screen lane of the label (with a leader), or null: the label sits on its polygon (the a², b², c² squares). */
	lane: Lane | null;
	/** Draw-in: the square unfolds outward from this side of the triangle (LC-0 first view). */
	unfold?: 'a' | 'b' | 'c';
	/** Label anchor other than the polygon centre (spec step 2: the square's label moves to its remainder). */
	anchor?: (vals: Vals) => V;
};

const tmp = new Vector3();

/**
 * A Poly whose label lives in a screen lane with a leader to the drawn polygon, following intermediate rigid
 * rotations (the anchor is read from the rendered mesh, not recomputed). Publishes its group for the detail view.
 */
function LanePoly({ lane, unfold, anchor, ...p }: LanePolyProps) {
	const invalidate = useThree((s) => s.invalidate), lastReveal = useRef(-1);
	const reg = useStage(), group = useRef<Group>(null), center = useRef<V>([0, 0]), offset = useRef<[number, number]>([0, 0]);
	useEffect(() => {
		if (group.current) sources.set(p.id, group.current);
		const own = group.current;
		return () => { if (sources.get(p.id) === own) sources.delete(p.id); };
	}, [p.id]);
	const vals = useVals(LESSON), step = useStep(LESSON, p.timeline ?? 'cut');
	const hot = useHot(LESSON, p.id), visible = !!p.label;
	center.current = centroid(p.pts(vals, p.step ?? step));
	useLabel(visible ? { lesson: LESSON, text: p.label!(vals), role: p.role, hot: p.id, at: () => center.current, offset: offset.current } : null);
	const line = useLeader(reg, visible && lane !== null, p.id);
	useFrame(() => {
		if (unfold && group.current) {
			// Unfold = a shear from the side's line: scale across the side grows from 0 to 1 with the stage reveal.
			const z = derive(vals), [a, b] = unfold === 'a' ? [z.C, z.B] : unfold === 'b' ? [z.A, z.C] : [z.B, z.A], d = sub(b, a), n = Math.hypot(...d);
			const x = n ? d[0] / n : 1, y = n ? d[1] / n : 0, r = reg.reveal;
			const xx = x * x + r * y * y, xy = x * y * (1 - r), yy = y * y + r * x * x;
			group.current.matrixAutoUpdate = false;
			group.current.matrix.set(xx, xy, 0, a[0] - xx * a[0] - xy * a[1], xy, yy, 0, a[1] - xy * a[0] - yy * a[1], 0, 0, 1, 0, 0, 0, 0, 1);
			group.current.updateWorldMatrix(true, true);
			if (lastReveal.current !== r) { lastReveal.current = r; invalidate(); }
		}
		if (!visible || !reg.el) return;
		if (anchor && group.current) {
			const [ax, ay] = anchor(vals);
			tmp.set(ax, ay, 0).applyMatrix4(group.current.matrixWorld);
			center.current = [tmp.x, tmp.y];
		} else {
			let found = false;
			group.current?.traverse((o) => {
				const m = o as Mesh;
				if (found || !m.isMesh) return;
				const a = m.geometry.getAttribute('position');
				if (!a || a.count !== 4) return;
				found = true;
				center.current = centroid(meshPoints(m));
			});
		}
		if (lane === null) {
			offset.current[0] = 0;
			offset.current[1] = 0;
			return;
		}
		const point = reg.project?.(center.current);
		const { x, y } = laneXY(reg, lane);
		if (point) {
			offset.current[0] = x - point[0];
			offset.current[1] = y - point[1];
			const end = x + (lane[0] === 0 ? 45 : -45);
			line.current?.setAttribute('d', `M ${point[0]} ${point[1]} L ${end} ${y}`);
		} else line.current?.setAttribute('d', '');
		line.current?.setAttribute('stroke', `var(${roleVar(hot ? 'hot' : p.role)})`);
	}, 1.5);
	return <group ref={group}><Poly {...p} label={undefined} /></group>;
}

/**
 * Multiplies the opacity of every material in the subtree (fill, strokes, hatch), its label and leader by `alpha`.
 * Used for the proof tiles fading into b² (spec step 8) and for the LC-0 squares receding at step 4. At zero the
 * subtree unmounts, removing hit targets and labels; values and formulas stay.
 */
function Fade({ alpha, ...p }: LanePolyProps & { alpha: number }) {
	const group = useRef<Group>(null), reg = useStage(), bases = useRef(new WeakMap<Material, { base: number; last: number }>());
	useFrame(() => {
		group.current?.traverse((o) => {
			const value = (o as Mesh).material;
			if (!value) return;
			for (const material of Array.isArray(value) ? value : [value]) {
				// A material whose opacity is still the one this fade wrote keeps its remembered base; any other
				// value is a fresh base (a prop change, a hot outline mounting).
				const saved = bases.current.get(material), base = saved && material.opacity === saved.last ? saved.base : material.opacity;
				material.opacity = base * alpha;
				bases.current.set(material, { base, last: material.opacity });
			}
		});
	});
	useFrame(() => {
		for (const s of reg.labels.list()) if (s.hot === p.id) { const el = reg.labelEls.get(s.id); if (el) el.style.opacity = String(alpha); }
		reg.el?.querySelectorAll<SVGPathElement>('[data-lc-leader="' + p.id + '"]').forEach((el) => el.style.opacity = String(alpha));
	}, 3);
	return alpha > 0 ? <group ref={group}><LanePoly {...p} /></group> : null;
}

/** A strip cut off a square; fills in from C toward its foot (fractional `cut_position` 1..2 or 2..3). */
function Strip({ piece, id, lane, label }: { piece: 1 | 2; id: string; lane: Lane; label?: PolyProps['label'] }) {
	const v = useVals(LESSON), z = derive(v), hot = useHot(LESSON, id);
	const phase = Math.min(1, Math.max(0, (v.cut_position as number) - (piece === 1 ? 1 : 2)));
	const points = (vals: Vals, fill: number) => strip(vals, piece).map((pt, i) => {
		const base = strip(vals, piece)[i === 1 ? 0 : 3];
		return i === 1 || i === 2 ? add(base, scale(sub(pt, base), fill)) : pt;
	});
	return <LanePoly lesson={LESSON} id={id} lane={lane} position={phase}
		pts={points} path={(vals, a, b, f) => points(vals, a + (b - a) * f)} role={piece === 1 ? 'vecA' : 'vecB'}
		fill={hot ? .40 : .28} hatched={(vals) => derive(vals).strip_a < 0}
		edge={(piece === 1 ? z.p > z.a : z.q > z.b) ? 'dashed' : 'solid'} label={label} />;
}

/** LC-0: squares, shadows, strips, the moving leftovers and their targets inside the square on c. */
function Missing() {
	const locale = useLocale(), v = useVals(LESSON), z = derive(v), step = useStep(LESSON, 'cut'), labels = useOverlay('labels');
	const reg = useStage();
	// Spec step 3: the two strip rows pulse once together (hot outline 300 ms; nothing under reduced motion).
	useEffect(() => {
		if (step !== 3 || reducedMotion()) return;
		const animations: Animation[] = [];
		const frame = requestAnimationFrame(() => {
			reg.el?.closest('.lab-vp-card')?.querySelectorAll<HTMLElement>('.lab-np-row[data-id="strip_a"],.lab-np-row[data-id="strip_b"]').forEach((el) => {
				animations.push(el.animate([{ outline: '2px solid var(--viz-hot)' }, { outline: '2px solid var(--viz-hot)' }, { outline: '2px solid transparent' }], { duration: 300 }));
			});
		});
		return () => { cancelAnimationFrame(frame); animations.forEach((a) => a.cancel()); };
	}, [step, reg]);
	// Spec step 4: the squares on a and b recede (fill to 8 %, outline dimmed) so the leftovers stand out.
	const squareAlpha = step >= 4 ? .08 / STYLE.fillA : 1;
	const stripLabel = (key: 'strip_a' | 'strip_b', name: string) => (labels ? (vals: Vals) => (step === 11 ? 'a·b = ' : name) + fmt(derive(vals)[key]) : undefined);
	return <>
		<Triangle />
		<Show ov="squares">
			<Fade alpha={squareAlpha} lesson={LESSON} unfold="a" lane={null} id="sq_a" pts={(vals) => { const d = derive(vals); return square(d.C, d.B); }} role="vecA"
				anchor={step >= 2 ? (vals) => centroid(pieceAt(vals, 1, 0)) : undefined}
				label={labels && step < 4 ? (vals) => 'a² = ' + fmt(derive(vals).a2) : undefined} />
			<Fade alpha={squareAlpha} lesson={LESSON} unfold="b" lane={null} id="sq_b" pts={(vals) => { const d = derive(vals); return square(d.A, d.C); }} role="vecB"
				anchor={step >= 3 ? (vals) => centroid(pieceAt(vals, 2, 0)) : undefined}
				label={labels && step < 4 ? (vals) => 'b² = ' + fmt(derive(vals).b2) : undefined} />
			{!z.degenerate && <LanePoly lesson={LESSON} unfold="c" lane={null} id="sq_c" pts={(vals) => { const d = derive(vals); return square(d.B, d.A); }} role="vecC" fill={STYLE.fillA}
				label={labels && step < 5 ? (vals) => 'c² = ' + fmt(derive(vals).c2) : undefined} />}
		</Show>
		<Show ov="shadows">{step >= 1 && <Shadow lane={[1, 2]} />}{step >= 3 && <Shadow side={2} lane={[1, 3]} />}</Show>
		<Show ov="strips">
			{step >= 2 && <Strip piece={1} id="strip_a" lane={[0, 4]} label={stripLabel('strip_a', 'a·p = ')} />}
			{step >= 3 && <Strip piece={2} id="strip_b" lane={[0, 1]} label={stripLabel('strip_b', 'b·q = ')} />}
		</Show>
		<Show ov="pieces">
			{step >= 4 && <>
				<LanePoly lesson={LESSON} lane={[0, 3]} id="R1" timeline="cut" position={v.cut_position as number} pts={(vals, s) => pieceAt(vals, 1, s)} path={(vals, a, b, f) => pieceAt(vals, 1, a + (b - a) * f)} role="vecA"
					hatched={(vals) => derive(vals).R1 < 0} label={labels ? (vals) => 'a·DB = ' + fmt(derive(vals).R1) : undefined} />
				<LanePoly lesson={LESSON} lane={[0, 0]} id="R2" timeline="cut" position={v.cut_position as number} pts={(vals, s) => pieceAt(vals, 2, s)} path={(vals, a, b, f) => pieceAt(vals, 2, a + (b - a) * f)} role="vecB"
					hatched={(vals) => derive(vals).R2 < 0} label={labels ? (vals) => 'b·EA = ' + fmt(derive(vals).R2) : undefined} />
			</>}
			{step >= 5 && !z.degenerate && <>
				<LanePoly lesson={LESSON} lane={[1, 0]} id="R1c" pts={(vals) => pieceAt(vals, 1, 8)} role="vecC" fill={0} edge="dashed" hatched={(vals) => derive(vals).R1c < 0} label={labels && step < 8 ? (vals) => 'c·FB = ' + fmt(derive(vals).R1c) : undefined} />
				<LanePoly lesson={LESSON} lane={[1, 1]} id="R2c" pts={(vals) => pieceAt(vals, 2, 9)} role="vecC" fill={0} edge="dashed" hatched={(vals) => derive(vals).R2c < 0} label={labels && step < 9 ? (vals) => 'c·AF = ' + fmt(derive(vals).R2c) : undefined} />
			</>}
		</Show>
		<Show ov="altitude">{step >= 5 && !z.degenerate && <>
			<Segment lesson={LESSON} id="drop_c" a={z.C} b={z.F} dashed role="muted" />
			<Segment lesson={LESSON} id="drop_c" a={z.F} b={add(z.F, scale(z.m, z.c))} dashed role="muted" />
			<Point lesson={LESSON} id="F" at={z.F} role="muted" size={4} /><Text id="F" at={z.F} text="F" />
			<RightAngle lesson={LESSON} id="rtF" at={z.F} u={sub(z.C, z.F)} v={z.w} role="muted" />
		</>}</Show>
		{/* Spec step 10: at a square corner the strips are gone and the square on c reads "= Pythagoras". */}
		{step >= 10 && z.th === 90 && !z.degenerate && <Text id="rt" at={centroid(square(z.B, z.A))} text={t(locale, 'lc.pythagoras')} offset={[0, 0]} role="light" />}
	</>;
}

/** Hot outline on the row that the spec singles out: `corr` while θ = 90° (LC-1), `cosraw` while |raw| > 1 (LC-3). */
function RowEmphasis({ scene }: { scene: Scene }) {
	const reg = useStage(), v = useVals(LESSON), active = scene === 1 ? v.th === 90 : scene === 3 && Math.abs(solveIK(v).cosraw) > 1;
	useEffect(() => {
		const row = reg.el?.closest('.lab-vp-card')?.querySelector<HTMLElement>('.lab-np-row[data-id="' + (scene === 1 ? 'corr' : 'cosraw') + '"]');
		if (!row) return;
		row.style.outline = active ? '2px solid var(--viz-hot)' : '';
		return () => { row.style.outline = ''; };
	}, [reg, scene, active]);
	return null;
}

/** LC-1: the triangle only; the shadow appears with its overlay or while the correction / shadow is hot. */
function LedgerScene() {
	const shadow = useOverlay('shadows'), p = useHot(LESSON, 'p'), corr = useHot(LESSON, 'corr');
	return <><Triangle />{(shadow || p || corr) && <Shadow />}</>;
}

/**
 * LC-2's construction on the triangle, revealed by step: the foot D with the shadow CD and height AD (step 2), their
 * names (3) and the rest DB of side a (4). The overview puts the labels in lanes; the detail view (readable scale)
 * next to their segments. Overlay flags of the overview only exist on its card.
 */
function ProofLegs({ lanes = false }: { lanes?: boolean }) {
	const z = derive(useVals(LESSON)), step = useStep(LESSON, 'proof');
	const legs = <>
		{step >= 1 && <Shadow solid names={step >= 2} lanes={lanes} />}
		{step >= 2 && <Text lane={lanes ? [1, 1] : undefined} id="rise" at={midpoint(z.D, z.A)} text={'b sin θ = ' + fmt(z.h)} offset={[-48, 0]} />}
	</>;
	// From step 6 the leg tile's label names a − b cos θ itself, so the length label leaves the crowded bottom lanes.
	const rest = step >= 3 && <>
		<Segment lesson={LESSON} id="rest" a={add(z.D, [0, -.25])} b={add(z.B, [0, -.25])} role="vecA" />
		{step < 5 && <Text lane={lanes ? [2, 3] : undefined} id="rest" at={add(midpoint(z.D, z.B), [0, -.25])} text={'a − b cos θ = ' + fmt(z.DB)} offset={[0, 14]} />}
	</>;
	return lanes ? <><Show ov="shadows">{legs}</Show><Show ov="rest">{rest}</Show></> : <>{legs}{rest}</>;
}

/** LC-2: shadow and height, the remainder DB, and the tiles of the proof revealed by step. */
function Proof() {
	const vals = useVals(LESSON), fade = vals.proof_fade_position as number;
	const z = derive(vals), step = useStep(LESSON, 'proof'), labels = useOverlay('labels');
	const sq = (a: 'C' | 'A' | 'B' | 'D', b: 'C' | 'A' | 'B' | 'D') => (v: Vals) => { const d = derive(v); return square(d[a], d[b]); };
	// Spec steps 4 / 5 / 7: the leg tile is named DB², then (b sin θ)², then b² sin² θ when it joins the b² pieces.
	const riseName = step >= 7 ? 'b² sin² θ = ' : step === 4 ? 'AD² = ' : '(b sin θ)² = ';
	return <><Triangle /><ProofLegs lanes />
		<Show ov="tiles">{step >= 4 && <>
			{step < 8 && <LanePoly lane={[2, 1]} lesson={LESSON} id="rest2" timeline="proof" pts={sq('D', 'B')} role="vecA" fill={step >= 6 ? 0 : .16}
				label={labels ? (v) => (step === 4 ? 'DB² = ' : '(a − b cos θ)² = ') + fmt(derive(v).rest2) : undefined} />}
			<Fade lane={[2, 0]} alpha={step >= 7 ? 1 - fade : 1} lesson={LESSON} id={step >= 7 ? 'bsin2' : 'rise2'} timeline="proof" pts={sq('A', 'D')} role="vecB" fill={.16}
				label={labels ? (v) => riseName + fmt(derive(v).rise2) : undefined} />
			{!z.degenerate && <LanePoly lane={null} lesson={LESSON} id="c2" pts={sq('B', 'A')} role="vecC" fill={.16} label={labels ? (v) => 'c² = ' + fmt(derive(v).c2) : undefined} />}
			{step >= 6 && <LanePoly lane={null} lesson={LESSON} id="a2" pts={sq('C', 'B')} role="vecA" label={labels ? (v) => 'a² = ' + fmt(derive(v).a2) : undefined} />}
			{step >= 7 && <Fade lane={[2, 2]} alpha={1 - fade} lesson={LESSON} id="bcos2" pts={sq('C', 'D')} role="vecB" label={labels ? (v) => 'b² cos² θ = ' + fmt(derive(v).bcos2) : undefined} />}
			{step >= 7 && <Fade lane={null} alpha={fade} lesson={LESSON} id="b2" pts={sq('A', 'C')} role="vecB" label={labels ? (v) => 'b² = ' + fmt(derive(v).b2) : undefined} />}
		</>}</Show>
	</>;
}

/** An IK bone: a 6 px beam, hot on its own id or while the cosine fraction (three lengths in) is hot. */
function Bone({ id, a, b, role }: { id: string; a: V; b: V; role: SceneRole }) {
	const pal = usePalette(), own = useHot(LESSON, id), fraction = useHot(LESSON, 'cosphi'), hover = useHoverable(LESSON, id, HOVER_RANK.beam);
	return <group {...hover}><Stroke points={[to3(a), to3(b)]} color={pal[role]} width={6} hot={own || fraction} /></group>;
}

/** LC-3: shoulder, two bones, elbow, wrist, target, reach rings, the gap when unreachable, d on hover. */
function IK() {
	const z = solveIK(useVals(LESSON)), locale = useLocale(), hot = useStore(lessonStore(LESSON), (s) => s.hot), distance = useOverlay('distance'), labels = useOverlay('labels');
	// Some objects answer to several hot ids (clamp lights the gap and the max ring); the id follows the hot state.
	return <>
		<Show ov="reach">
			<Circle lesson={LESSON} id={hot.has('clamp') ? 'clamp' : hot.has('reach') ? 'reach' : 'reach_max'} center={z.S} r={z.reachMax} role="muted" dashPx={[7, 5]} />
			<Circle lesson={LESSON} id={hot.has('reach') ? 'reach' : 'reach_min'} center={z.S} r={z.reachMin} role="muted" dashPx={[7, 5]} />
			{/* Inside the ring: the home view frames the drawn rings, so a label outside the bottom of the outer one fell
			    under the status bar (live check). */}
			<Text id="reach_max" at={[0, -z.reachMax]} text={t(locale, 'lc.maxReach')} offset={[0, -12]} />
			{z.reachMin > 1e-9 && <Text id="reach_min" at={[0, -z.reachMin]} text={t(locale, 'lc.minReach')} offset={[0, 14]} />}
		</Show>
		<Bone id="l1" a={z.S} b={z.E} role="vecA" /><Bone id="l2" a={z.E} b={z.W} role="vecB" />
		<Point lesson={LESSON} id="S" at={z.S} role="ink" size={10} />
		<Point lesson={LESSON} id="E" at={z.E} role="ink" size={12} />
		<Point lesson={LESSON} id="W" at={z.W} role="vecB" size={8} />
		<Point lesson={LESSON} id="hand" at="ik_hand" role="vecC" size={12} label={t(locale, 'lc.targetLabel')} />
		<Text id="S" at={z.S} text={t(locale, 'lc.shoulder')} offset={[-20, 16]} />
		<Text id="E" at={z.E} text={t(locale, 'lc.elbow')} offset={[12, -15]} />
		<Text id="hand" at={z.T} text={t(locale, 'lc.targetLabel')} offset={[20, 16]} />
		<Text id="l1" at={midpoint(z.S, z.E)} text={'L₁ = ' + fmt(z.l1)} offset={besideBone(z.S, z.E, z.W)} role="vecA" />
		<Text id="l2" at={midpoint(z.E, z.W)} text={'L₂ = ' + fmt(z.l2)} offset={besideBone(z.E, z.W, z.S)} role="vecB" />
		{(z.outside || z.inside) && <>
			<Segment lesson={LESSON} id={hot.has('clamp') ? 'clamp' : 'gap'} a={z.W} b={z.T} dashed role="axisX" />
			<Text id="gap" at={midpoint(z.W, z.T)} text={t(locale, 'lc.unreachable')} role="axisX" />
		</>}
		{(distance || hot.has('d') || hot.has('cosphi')) && <>
			<Segment lesson={LESSON} id={hot.has('cosphi') ? 'cosphi' : 'd'} a={z.S} b={z.T} dashed role="vecC" />
			<Text id="d" at={midpoint(z.S, z.T)} text={'d = ' + fmt(z.d)} offset={[0, 20]} />
		</>}
		<Show ov="angle"><AngleArc lesson={LESSON} id="phi" at={z.E} from={sub(z.S, z.E)} to={sub(z.W, z.E)} r={.5} label={labels ? 'φ = ' + fmtAngle(z.phi) + '°' : undefined} /></Show>
		<Show ov="right"><RightAngle lesson={LESSON} id="rt" at={z.E} u={sub(z.S, z.E)} v={sub(z.W, z.E)} show={Math.abs(z.cosc) < 1e-12} /></Show>
	</>;
}

/** Status bar hints per scene (spec status lines); values are read live from the store. */
function statusFor(scene: Scene, locale: Locale): StatusSpec {
	const vertex = {
		A: { mouse: t(locale, 'lc.hintAMouse'), touch: t(locale, 'lc.hintATouch') },
		B: t(locale, 'lc.hintB'),
	};
	const sq = (id: string, label: string, key: 'a2' | 'b2' | 'c2') => ({ [id]: (v: Vals) => t(locale, 'lc.hintSquare', { label, value: fmt(derive(v)[key]) }) });
	if (scene === 0) {
		return {
			hints: {
				...vertex,
				...sq('sq_a', 'a²', 'a2'), ...sq('sq_b', 'b²', 'b2'), ...sq('sq_c', 'c²', 'c2'),
				strip_a: (v) => t(locale, 'lc.hintStrip', { label: 'a × (b cos θ)', value: fmt(derive(v).strip_a) }),
				strip_b: (v) => t(locale, 'lc.hintStrip', { label: 'b × (a cos θ)', value: fmt(derive(v).strip_b) }),
				R1: (v) => t(locale, 'lc.hintPiece', { label: 'a·DB', value: fmt(derive(v).R1) }),
				R2: (v) => t(locale, 'lc.hintPiece', { label: 'b·EA', value: fmt(derive(v).R2) }),
				p: t(locale, 'lc.hintShadow'), drop_a: t(locale, 'lc.hintShadow'),
			},
			// While a piece moves, the bar names the step and reminds that A and B stay draggable.
			state: (v) => (v.cut_play ? t(locale, 'lc.hintMotion', { step: (v.cut_to as number) + 1 }) : null),
		};
	}
	if (scene === 1) return { hints: { ...vertex, p: t(locale, 'lc.hintShadow'), drop_a: t(locale, 'lc.hintShadow'), corr: t(locale, 'lc.hintCorr') } };
	if (scene === 2) {
		const tiles = Object.fromEntries(PROOF_PIECES.filter((part) => part.id !== 'tri').map((part) => [part.id, (v: Vals) => t(locale, 'lc.hintTile', { label: part.label, value: fmt(derive(v)[part.key]) })]));
		return { hints: { ...vertex, ...tiles, rest: t(locale, 'lc.hintRest'), p: t(locale, 'lc.hintShadow'), drop_a: t(locale, 'lc.hintShadow') } };
	}
	const ring = (v: Vals) => { const ik = solveIK(v); return t(locale, 'lc.hintRing', { min: fmt(ik.reachMin), max: fmt(ik.reachMax) }); };
	return {
		hints: {
			hand: { mouse: t(locale, 'lc.hintHandMouse'), touch: t(locale, 'lc.hintHandTouch') },
			E: t(locale, 'lc.hintElbow'), reach_max: ring, reach_min: ring, reach: ring, clamp: ring,
		},
		state: (v) => {
			const ik = solveIK(v);
			return ik.zero ? t(locale, 'lc.stateZero') : ik.outside ? t(locale, 'lc.stateOutside') : ik.inside ? t(locale, 'lc.stateInside') : null;
		},
	};
}

export function LawScene({ scene }: { scene: Scene }) {
	const locale = useLocale(), cut = useStep(LESSON, 'cut');
	const names = [t(locale, 'lc.nameMissing'), t(locale, 'lc.nameLedger'), t(locale, 'lc.nameProof'), t(locale, 'lc.nameIk')];
	const ov = (id: string, key: Parameters<typeof t>[1], on = true) => ({ id, label: t(locale, key), on });
	const overlays = [
		ov('grid', 'lc.ovGrid'), ov('labels', 'lc.ovLabels'), ov('angle', 'lc.ovAngle'),
		...(scene !== 3 ? [ov('right', 'lc.ovRight')] : []),
		...(scene === 0 ? [ov('shadows', 'lc.ovShadows'), ov('squares', 'lc.ovSquares'), ov('strips', 'lc.ovStrips'), ov('pieces', 'lc.ovPieces'), ov('altitude', 'lc.ovAltitude')]
			: scene === 1 ? [ov('shadows', 'lc.ovShadow', false)]
			: scene === 2 ? [ov('shadows', 'lc.ovShadowHeight'), ov('tiles', 'lc.ovTiles'), ov('rest', 'lc.ovRest')]
			: [ov('reach', 'lc.ovReach'), ov('distance', 'lc.ovDistance', false), ov('right', 'lc.ovRight')]),
	];
	return <ViewportCard lesson={LESSON} id={'LC-' + scene} name={'LC-' + scene + ' · ' + names[scene]} mode="2d" extent={EXTENTS[scene]}
		center={scene === 3 ? [0, 0] : [1.5, scene === 0 || scene === 2 ? 0 : 1]} rows={rowsFor(scene, locale, cut)} overlays={overlays} status={statusFor(scene, locale)}
		drawIn={scene === 0}>
		<RowEmphasis scene={scene} />
		<Show ov="grid"><Grid lesson={LESSON} extent={EXTENTS[scene]} step={1} axes={false} ticks={false} /></Show>
		{scene === 0 ? <Missing /> : scene === 1 ? <LedgerScene /> : scene === 2 ? <Proof /> : <IK />}
	</ViewportCard>;
}

/**
 * The detail view's copy of one overview piece: it samples the overview's public position buffer and world matrix
 * each frame (no second placement animation) and draws it with its own Stroke, so pixel widths are right for this
 * View's resolution. Shadows (p, q) are redrawn from the geometry instead.
 */
function DetailGeometry({ part, proof = false }: { part: DetailPiece; proof?: boolean }) {
	if (part.id === 'tri') return <><Triangle /><ProofLegs /></>;
	return <DetailPieceView part={part} proof={proof} />;
}

function DetailPieceView({ part, proof }: { part: DetailPiece; proof: boolean }) {
	const vals = useVals(LESSON), z = derive(vals), palette = usePalette(), hot = useHot(LESSON, part.id);
	const [sample, setSample] = useState<{ geometry: BufferGeometry; points: V[]; matrix: Matrix4; opacity: number } | null>(null);
	const invalidate = useThree((s) => s.invalidate), labels = useOverlay('labels'), signature = useRef('');
	const material = useMemo(() => new MeshBasicMaterial({ color: palette[part.role], side: DoubleSide, transparent: true, opacity: STYLE.fillA, depthWrite: false, polygonOffset: true, polygonOffsetFactor: 1 }), [palette, part.role]);
	useEffect(() => () => material.dispose(), [material]);
	useFrame(() => {
		const mesh = sourceMesh(part.id);
		if (!mesh) {
			if (signature.current !== 'none') { signature.current = 'none'; setSample(null); }
			return;
		}
		const points = meshPoints(mesh), opacity = (mesh.material as Material).opacity, next = JSON.stringify([points, opacity]);
		if (next !== signature.current || sample?.geometry !== mesh.geometry) {
			signature.current = next;
			setSample({ geometry: mesh.geometry, points, matrix: mesh.matrixWorld.clone(), opacity });
			invalidate();
		}
	});
	const hover = useHoverable(LESSON, part.id, HOVER_RANK.fill), edgeGroup = useRef<Group>(null), reg = useStage();
	// The proof fade applies here too: b² appears as the two pieces disappear.
	const fade = vals.proof_fade_position as number;
	const alpha = proof ? (part.id === 'b2' ? fade : ['bcos2', 'bsin2'].includes(part.id) ? 1 - fade : 1) : 1;
	const edgeBase = useRef(new WeakMap<Material, { base: number; last: number }>());
	useFrame(() => {
		edgeGroup.current?.traverse((o) => {
			const m = (o as Mesh).material;
			if (!m) return;
			for (const mat of Array.isArray(m) ? m : [m]) {
				const old = edgeBase.current.get(mat), base = old && mat.opacity === old.last ? old.base : mat.opacity;
				mat.opacity = base * alpha;
				edgeBase.current.set(mat, { base, last: mat.opacity });
			}
		});
	});
	useFrame(() => { for (const s of reg.labels.list()) if (s.hot === part.id) { const el = reg.labelEls.get(s.id); if (el) el.style.opacity = String(alpha); } }, 3);
	if (part.id === 'p' || part.id === 'q') return <Shadow side={part.id === 'p' ? 1 : 2} />;
	if (!sample || alpha <= 0) return null;
	material.opacity = sample.opacity;
	const center = centroid(sample.points);
	return <group {...hover}>
		<mesh geometry={sample.geometry} material={material} matrix={sample.matrix} matrixAutoUpdate={false} dispose={null} />
		{z[part.key] < 0 && <group matrix={sample.matrix} matrixAutoUpdate={false} dispose={null}><Hatch geometry={sample.geometry} role={part.role} /></group>}
		<group ref={edgeGroup}><Stroke points={[...sample.points, sample.points[0]].map(to3)} color={palette[part.role]} hot={hot} width={STYLE.edge} /></group>
		{labels && <Text id={part.id} at={center} text={part.label + ' = ' + fmt(z[part.key])} role={part.role} offset={[0, 0]} />}
	</group>;
}

/** The triangle piece's frame: its vertices and D (behind C when θ > 90°). */
const triangle = (z: ReturnType<typeof derive>): V[] => [z.C, z.A, z.B, z.D];

/** Bounds of a point set as a detail frame: centre and an extent with margin. */
function frameOf(pts: V[], serial: number) {
	const xs = pts.map((p) => p[0]), ys = pts.map((p) => p[1]);
	const w = Math.max(...xs) - Math.min(...xs), h = Math.max(...ys) - Math.min(...ys);
	return { center: [(Math.min(...xs) + Math.max(...xs)) / 2, (Math.min(...ys) + Math.max(...ys)) / 2] as V, extent: Math.max(.6, Math.max(w, h) * .7 + .25), serial };
}

/** The detail ViewportCard: one selected piece, framed on selection and on explicit Fit (never under a drag). */
export function LawDetail({ proof = false }: { proof?: boolean }) {
	const locale = useLocale(), v = useVals(LESSON), step = useStep(LESSON, proof ? 'proof' : 'cut'), key = proof ? 'proof_detail' : 'detail';
	// The piece the view draws: the selection while the overview draws it, otherwise piece 0 (shared with the selector).
	const part = (proof ? PROOF_PIECES : CUT_PIECES)[shownPiece(proof, (v[key + '_part'] as number) ?? 0, step, v.proof_fade_position as number)];
	const fit = (v[key + '_fit'] as number) ?? 0;
	// Selection and explicit Fit capture framing. Ordinary value writes and animation frames cannot move the camera
	// under a reader's pointer; the ViewportCard is keyed by the frame serial so a new frame is a fresh home.
	const [frame, setFrame] = useState(() => { const z = derive(v); return frameOf(proof ? triangle(z) : square(z.B, z.A), 0); });
	useEffect(() => {
		const request = requestAnimationFrame(() => {
			const mesh = sourceMesh(part.id), z = derive(v);
			const pts = part.id === 'tri' ? triangle(z) : part.id === 'p' ? [z.C, z.A, z.D] : part.id === 'q' ? [z.C, z.B, z.E] : mesh ? meshPoints(mesh) : null;
			if (!pts) return;
			setFrame((old) => frameOf(pts, old.serial + 1));
		});
		return () => cancelAnimationFrame(request);
	}, [part.id, fit]);
	return <ViewportCard key={frame.serial} lesson={LESSON} id={proof ? 'LC-2-detail' : 'LC-0-detail'} name={t(locale, 'lc.detailName', { scene: proof ? 'LC-2' : 'LC-0', part: part.label })}
		mode="2d" extent={frame.extent} center={frame.center}
		overlays={[{ id: 'labels', label: t(locale, 'lc.ovLabels'), on: true }, ...(proof ? [{ id: 'angle', label: t(locale, 'lc.ovAngle'), on: true }, { id: 'right', label: t(locale, 'lc.ovRight'), on: true }] : [])]}
		status={{ idle: t(locale, 'lc.detailIdle') }}>
		<DetailGeometry part={part} proof={proof} />
	</ViewportCard>;
}
