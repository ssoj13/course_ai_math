/**
 * Lesson 3.1 scene (docs/design/backlog-tensor-road-scene.md): the tensor road, rank 0..6 on one slider, in the 6d
 * Blender look. Every cell of the tensor is one box of ONE instanced draw (the DotUnit's `Boxes` renderer and `Frame`
 * buffers): numbered cells for the abstract road, grey or colour-channel bytes for images, Hinton squares (size =
 * magnitude, hatched = negative) for signed numbers. A rank change extrudes: the new axis's copies slide out of the
 * old tensor (reduced motion: a cut). Loaded lazily by the entry (tensor-road.tsx); shares its store and model.
 */
import { useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import { t, type Locale } from '../../i18n/core';
import { useLocale } from '../../i18n/react';
import type { V } from '../geom';
import { lessonStore, useVals } from '../store';
import { ViewportCard } from '../scene/ViewportCard';
import { Segment } from '../scene/Segment';
import { Label } from '../scene/Label';
import { useOverlay, useStage } from '../scene/handles';
import { reducedMotion } from '../scene/motion';
import type { Hint, StatusSpec } from '../scene/StatusBar';
import { Boxes } from '../dotunit/Boxes';
import { Frame, type BoxRole } from '../dotunit/choreo';
import { Tag } from '../dotunit/scene';
import { roadRows } from './tensor-road.rows';
import {
	fmtInt, fmtShape, fmtVal, grow, indexOf, layout, LESSON, model, roadDirs, roadShape, selectPatch, SPEC, type Layout, type Model,
} from './tensor-road.math';

/** Largest tensor the road draws: six levels of size 4. */
const CAP = SPEC.size.max ** 6;
/** One rank step of the extrusion (ms). */
const STEP_MS = 450;
/** Edge of a cell box (pitch 1): the gap between neighbours keeps every cell readable. */
const CELL = 0.8;
/** Value labels on every cell of a row or sheet up to this many cells. */
const LABEL_ALL = 24;
/**
 * The selected cell's `x[i, j] = v` tag is drawn on tensors up to this size; on images and big roads it would cover the
 * cells, so there the orange outline, the readout under the card and the status line carry it.
 */
const SEL_TAG = 64;
/** Home view direction: front, from the right and above, so all three faces of a block show. */
const HOME: readonly [number, number, number] = [0.55, 0.45, 1.5];
/** Images are read from almost straight in front, like a picture on the wall. */
const HOME_IMAGE: readonly [number, number, number] = [0.2, 0.25, 1.6];
const HOME_DEEP: readonly [number, number, number] = [0.7, 1.1, 1.2];
/**
 * World margin around the cells for the home view: the axis rulers and their labels hang outside the figure. Labels
 * keep their pixel size while a big figure is drawn smaller, so the margin grows with the figure.
 */
const padOf = (m: Model) => {
	const [mx, my, mz] = layoutOf(m.shape, m).max;
	return 0.9 + 0.1 * Math.max(mx, my, mz);
};

const cellId = (i: number) => 'c' + i;
const CHANNEL: readonly BoxRole[] = ['axisX', 'axisY', 'axisZ'];

/** Layouts of the shapes the road passes through (a rank step draws the larger of the two shapes). */
const layoutCache = new Map<string, Layout>();
function layoutOf(shape: readonly number[], m: Model): Layout {
	const key = JSON.stringify([shape, m.ex?.id ?? null]);
	let L = layoutCache.get(key);
	if (!L) {
		L = layout(shape, m.ex ? m.ex.dirs : roadDirs(shape.length));
		layoutCache.set(key, L);
	}
	return L;
}

/** Writes the cells of `m` drawn at rank `s` (fractional while extruding) into the frame. */
function writeCells(f: Frame, m: Model, s: number, sel: boolean): void {
	const D = Math.ceil(s - 1e-9);
	const u = s - (D - 1);
	const shape = m.ex ? m.shape : roadShape(D, m.sizes);
	const L = layoutOf(shape, m);
	const n = L.pos.length / 3;
	const maxAbs = m.ex?.look === 'signed' ? Math.max(1e-9, ...Array.from(m.ex.data, Math.abs)) : 1;
	const c = [0, 0, 0];
	for (let i = 0; i < n; i++) {
		const a = grow(L, shape, i, Math.min(1, u), c);
		const look = m.ex?.look ?? 'cells';
		if (look === 'signed') {
			const v = m.data[i];
			const e = 0.15 + 0.7 * Math.abs(v) / maxAbs;
			f.box(i, c[0] - e / 2, c[0] + e / 2, c[1] - e / 2, c[1] + e / 2, c[2] - e / 2, c[2] + e / 2);
			f.look(i, v < 0 ? 'vecB' : 'vecC', false, a, v < 0 ? 1 : 0, false);
		} else {
			const h = CELL / 2;
			f.box(i, c[0] - h, c[0] + h, c[1] - h, c[1] + h, c[2] - h, c[2] + h);
			// Neutral cells: the colours belong to the axes (rulers, shape readout).
			if (look === 'cells') f.look(i, 'ink', false, a, 0, false);
			else {
				// A byte as brightness over the dark viewport: 0 = background, 255 = the full role colour.
				const k = m.ex!.channel;
				const role: BoxRole = look === 'gray' ? 'ink' : CHANNEL[indexOf(m, i)[k!]];
				f.look(i, role, true, a * (0.06 + 0.94 * m.data[i] / 255), 0, false);
			}
		}
	}
	if (sel && m.offset < n) f.flash[m.offset] = 1;
	f.n = n;
}

/** The cells, their extrusion clock and the re-framing of the home view. */
function Cells({ m, locale }: { m: Model; locale: Locale }) {
	const reg = useStage();
	const invalidate = useThree((st) => st.invalidate);
	const frame = useMemo(() => {
		const f = new Frame(CAP);
		for (let i = 0; i < CAP; i++) f.id[i] = cellId(i);
		return f;
	}, []);
	// What the frame shows: the drawn rank (fractional while extruding) and the key of the data it was drawn from.
	const anim = useRef({ s: m.rank, key: '', dirty: true, refit: true });
	// Same key = the same road at another rank (that change extrudes); any other change is a cut.
	const shapeKey = m.ex ? 'ex:' + m.ex.id : 'road:' + m.sizes.join();
	const cur = useRef(m);
	cur.current = m;
	useEffect(() => {
		anim.current.dirty = true;
		invalidate();
	}, [m, invalidate]);

	const update = (dt: number): 'still' | 'moved' | 'moving' => {
		const st = anim.current, mm = cur.current, R = mm.rank;
		if (st.key !== shapeKey) {
			st.s = R;
			st.key = shapeKey;
			st.dirty = true;
			st.refit = true;
		}
		if (mm.ex) st.s = R;
		let moving = false;
		if (st.s !== R) {
			if (reducedMotion()) st.s = R;
			else {
				const dir = Math.sign(R - st.s);
				// A jump of several ranks plays only the last step (the earlier axes appear at once).
				if (Math.abs(R - st.s) > 1) st.s = R - dir;
				st.s += dir * Math.min(Math.abs(R - st.s), dt / STEP_MS);
				if (Math.abs(R - st.s) < 1e-6) st.s = R;
			}
			moving = st.s !== R;
			if (!moving) st.refit = true;
			st.dirty = true;
		}
		if (st.dirty) {
			st.dirty = false;
			writeCells(frame, mm, st.s, st.s === R);
			return moving || st.refit ? 'moving' : 'moved';
		}
		if (st.refit) {
			// The Stage binds its navigation a few frames after mount: keep the request until it can be served.
			if (!reg.nav) return 'moving';
			st.refit = false;
			// A flat picture (no axis into the screen) is framed from the front; anything with depth from three-quarter.
			// Sheets one behind another (the video's clips) are seen from higher up, so the back sheet shows above the front.
			const image = mm.ex?.look === 'gray' || mm.ex?.look === 'rgb';
			const deep = mm.dirs.some((d) => d.axis === 2);
			reg.nav.frame(image ? (deep ? HOME_DEEP : HOME_IMAGE) : HOME);
			return 'moved';
		}
		return 'still';
	};
	const pick = (_: string, i: number) => {
		const s = lessonStore(LESSON).getState();
		const now = model(s.vals);
		if (i < now.size) s.setMany(selectPatch(now, i));
	};
	return <>
		<Boxes lesson={LESSON} frame={frame} update={update} onPick={pick} pad={padOf(m)} />
		<CellTags m={m} frame={frame} locale={locale} />
	</>;
}

/** Numbers on the cells (small tensors) and the selected cell's `x[i, j, k] = v` tag. */
function CellTags({ m, frame, locale }: { m: Model; frame: Frame; locale: Locale }) {
	const labels = useOverlay('labels');
	if (!labels) return null;
	const at = (i: number) => (): number[] | null => (i < frame.n && frame.alpha[i] > 0.5 ? [frame.c[3 * i], frame.c[3 * i + 1], frame.c[3 * i + 2]] : null);
	// Every value is printed on a row or a sheet; inside a block the back layers' numbers would show through the gaps.
	const all = m.size <= LABEL_ALL && m.size > 1 && m.rank <= 2;
	// The selected tag sits on the top face of its cell, clear of the cell's own number.
	const top = (i: number) => (): number[] | null => (i < frame.n && frame.alpha[i] > 0.5 ? [frame.c[3 * i], frame.c[3 * i + 1] + frame.s[3 * i + 1] / 2, frame.c[3 * i + 2]] : null);
	const sel = 'x' + (m.rank ? '[' + m.idx.join(', ') + ']' : '') + ' = ' + fmtVal(m, m.value);
	return <>
		{all && Array.from({ length: m.size }, (_, i) => i !== m.offset && (
			<Tag key={i} lesson={LESSON} hot={cellId(i)} small dense role="ink" text={fmtVal(m, m.data[i])} at={at(i)} />
		))}
		{m.size <= SEL_TAG && <Tag key="sel" lesson={LESSON} hot={cellId(m.offset)} role="hot" text={sel} offset={[0, -18]} at={top(m.offset)} />}
		{m.rank === 0 && <Tag key="scalar" lesson={LESSON} role="muted" small text={t(locale, 'tr.exLoss')} offset={[0, 22]} at={at(0)} />}
	</>;
}

/** One ruler per axis: a line along its direction outside the figure, `axis a · n` (or `H = 8`) in the axis colour. */
function Rulers({ m, locale }: { m: Model; locale: Locale }) {
	const show = useOverlay('rulers');
	if (!show || m.rank === 0) return null;
	const L = layoutOf(m.shape, m);
	const [mx, my] = L.max;
	return <>
		{m.shape.map((n, a) => {
			const d = m.dirs[a];
			// Span of the axis: cell centres 0..(n-1)·pitch, plus the width of the last block for a block axis.
			const span = (n - 1) * L.pitch[a] + (d.level === 1 ? L.pitch[a] - 2 : 0);
			const off = 0.75;
			const lo = -0.4, hi = span + 0.4;
			// Cell axes are measured on the bottom-left-front edges (x below, y on the left, z from the front-left-bottom
			// corner backwards: a small tripod), block axes on the opposite sides (x above, y on the right), so the two
			// rulers of one world axis and their labels never sit on top of each other.
			const top = d.level === 1;
			let p: V, q: V;
			if (d.axis === 0) {
				const y = top ? 0.5 + off : -my - 0.5 - off;
				p = [lo, y, 0.5];
				q = [hi, y, 0.5];
			} else if (d.axis === 1) {
				const x = top ? mx + 0.5 + off : -0.5 - off;
				p = [x, -lo, 0.5];
				q = [x, -hi, 0.5];
			} else {
				const x = top ? -0.5 - 2 * off : -0.5 - off;
				p = [x, -my - 0.5 - off, -lo];
				q = [x, -my - 0.5 - off, -hi];
			}
			const id = 'ax' + a;
			const text = m.names ? t(locale, 'tr.axisNamed', { name: m.names[a], n }) : t(locale, 'tr.axisRoad', { a, n });
			// Labels sit where nothing else is: off the middle of an x ruler (below / above), above the top end of a y ruler
			// (a label beside a tall figure would leave a phone-width stage), left of the front end of a z ruler (its far end
			// projects onto the figure).
			const at: V = d.axis === 0 ? [(p[0] + q[0]) / 2, p[1], p[2]] : p;
			// `away` is a point just on the figure's side of the anchor: the label's near edge stops `gap` px beyond it.
			const toward: V = d.axis === 0 ? [at[0], at[1] + (top ? -0.05 : 0.05), at[2]] : d.axis === 1 ? [at[0], at[1] - 0.05, at[2]] : [at[0] + 0.05, at[1], at[2]];
			return <group key={a}>
				<Segment lesson={LESSON} id={id} a={p} b={q} role={m.role[a]} />
				<Label lesson={LESSON} id={id} at={at} role={m.role[a]} away={toward} gap={6}>{text}</Label>
			</group>;
		})}
	</>;
}

function Road() {
	const locale = useLocale();
	const v = useVals(LESSON);
	const m = useMemo(() => model(v), [v]);
	return <>
		<Cells m={m} locale={locale} />
		<Rulers m={m} locale={locale} />
	</>;
}

/** Status line: the hovered cell (index, value, address) or, idle, the rank and shape. */
function statusFor(locale: Locale): StatusSpec {
	const cell = (id: string): Hint | undefined => {
		const off = Number(id.slice(1));
		return (vals) => {
			const m = model(vals);
			if (!(off < m.size)) return null;
			return t(locale, 'tr.hintCell', { name: 'x', idx: indexOf(m, off).join(', '), v: fmtVal(m, m.data[off]), off });
		};
	};
	const idle = (key: 'tr.idleMouse' | 'tr.idleTouch') => (vals: Parameters<typeof model>[0]) => {
		const m = model(vals);
		return t(locale, key, { rank: m.rank, shape: fmtShape(m.shape), n: fmtInt(m.size) });
	};
	return {
		hints: new Proxy({} as Record<string, Hint>, { get: (_, id) => (typeof id === 'string' && /^c\d+$/.test(id) ? cell(id) : undefined) }),
		idle: { mouse: idle('tr.idleMouse'), touch: idle('tr.idleTouch') },
		state: (vals) => (model(vals).noExample ? t(locale, 'tr.noExample') : null),
	};
}

export function RoadScene() {
	const locale = useLocale();
	const status = useMemo(() => statusFor(locale), [locale]);
	const rows = useMemo(() => roadRows(locale), [locale]);
	const m = model(useVals(LESSON));
	const [mx, my, mz] = layout(m.shape, m.dirs).max;
	// The camera's distance range scales with the extent: a 4096-cell road needs a far camera, one cell a near one.
	const extent = Math.max(3, 0.6 * Math.hypot(mx + 3, my + 3, mz + 3));
	return <ViewportCard lesson={LESSON} id="3.1-1" name={'3.1-1 · ' + t(locale, 'tr.name')} mode="3d" extent={extent}
		rows={rows} status={status}
		overlays={[{ id: 'labels', label: t(locale, 'tr.ovLabels'), on: true }, { id: 'rulers', label: t(locale, 'tr.ovRulers'), on: true }]}>
		<Road />
	</ViewportCard>;
}
