/**
 * Per-stage registry of draggable handles, shared by the three halves of a Stage that live in different React trees:
 * the scene objects (inside the drei View, i.e. the shared Canvas root) register handles, DragLayer (also inside the
 * View, it owns the camera) binds `project` and turns pointers into drags, and DragKeys (the Stage's DOM overlay)
 * renders one keyboard button per handle. React context does not cross the View tunnel, so Stage creates the
 * registry once and provides it inside the View itself, and hands the same object to the overlay.
 * Text labels take the same route: scene objects register them, the overlay (`Labels`) renders them as DOM, and
 * LabelLayer (inside the View) calls `placeLabels` once per rendered frame with the camera of that frame.
 * The registry also carries the per-stage state the ViewportCard chrome shares with the scene: the card's overlay
 * flags (`useOverlay`, `Show`), what the pointer is over (`hover`, read by the status bar), the camera commands bound
 * by the View (`nav`: Reset view, gizmo align), the gizmo's per-frame update and the draw-in progress (`reveal`).
 * Deliberately free of three/drei so the keyboard, label, overlay and hover paths are testable in happy-dom.
 */
import { createContext, useCallback, useContext, useEffect, useId, useMemo, useRef, useState, useSyncExternalStore, type ReactNode } from 'react';
import { useStore } from 'zustand';
import type { V } from '../geom';
import { roleVar, type SceneRole } from '../palette';
import { hotOff, hotOn, lessonStore, type LessonState } from '../store';
import { hitHandle, keyDelta, moved, pushAway } from './drag';

type Vals = LessonState['vals'];

export interface Draggable {
	/** Unique within the stage, e.g. `a:tip`. */
	id: string;
	/** Scene object id: lit (hot) while its handle is focused or dragged. */
	owner: string;
	/** Accessible name of the keyboard button, e.g. "Move tip of a". */
	label: string;
	lesson: string;
	/** Store keys moved together; keys[0] is the anchor that snaps to the grid (see `moved`). */
	keys: readonly string[];
	/** Handle position in data coordinates for the given store values. */
	at(vals: Vals): readonly number[];
}

/**
 * A text label drawn in the stage overlay. Plain text only: it is rendered by another React root (the overlay), and
 * KaTeX per frame would be heavy (formulas belong in Formula).
 */
export interface LabelSpec {
	/** Unique within the stage (components use `useId`). */
	id: string;
	lesson: string;
	text: string;
	/** Text color role (default `ink`). */
	role?: SceneRole;
	/** Small text (axis ticks). */
	small?: boolean;
	/** Scene object/term id: the label switches to the hot color while that id is hot. */
	hot?: string;
	/** Anchor in data coordinates for the given store values; null hides the label (degenerate geometry). */
	at(vals: Vals): readonly number[] | null;
	/** Optional second point: the label moves away from its projection until its near edge is `gap` px from the anchor (e.g. off an arc, away from its center). */
	away?(vals: Vals): readonly number[] | null;
	gap?: number;
	/** Fixed screen offset in CSS px, y down (e.g. up and to the right of an arrow tip). */
	offset?: readonly [number, number];
}

/** Keyed list with a stable snapshot for useSyncExternalStore; `owner` tokens make a second registrant of an id loud. */
interface Keyed<T extends { id: string }> {
	list(): readonly T[];
	subscribe(cb: () => void): () => void;
	/** Inserts or replaces `v` for `owner`; another owner holding the same id throws. */
	put(v: T, owner: object): void;
	/** Removes `id` if `owner` still holds it (a late cleanup must not remove a successor's entry). */
	drop(id: string, owner: object): void;
}

function keyed<T extends { id: string }>(what: string): Keyed<T> {
	const items = new Map<string, { v: T; owner: object }>();
	const subs = new Set<() => void>();
	// useSyncExternalStore needs the same array until something changes.
	let snapshot: readonly T[] = [];
	const changed = () => {
		snapshot = [...items.values()].map((e) => e.v);
		for (const cb of subs) cb();
	};
	return {
		list: () => snapshot,
		subscribe(cb) {
			subs.add(cb);
			return () => void subs.delete(cb);
		},
		put(v, owner) {
			const cur = items.get(v.id);
			if (cur && cur.owner !== owner) throw new Error(`duplicate ${what} id "${v.id}" in one stage`);
			items.set(v.id, { v, owner });
			changed();
		},
		drop(id, owner) {
			if (items.get(id)?.owner !== owner) return;
			items.delete(id);
			changed();
		},
	};
}

export interface StageReg {
	readonly mode: '2d' | '3d';
	list(): readonly Draggable[];
	subscribe(cb: () => void): () => void;
	/** Registers a handle; returns its removal. */
	add(d: Draggable): () => void;
	/** Text labels shown in the overlay (see `useLabel`, `Labels`, `placeLabels`). */
	readonly labels: Keyed<LabelSpec>;
	/** Overlay element of each label id, set by `Labels`; written by `placeLabels` without React renders. */
	readonly labelEls: Map<string, HTMLElement>;
	/** Data point -> stage-relative CSS px, null when off-stage or behind the camera. Bound by DragLayer once the View camera exists; null before. */
	project: ((p: readonly number[]) => [number, number] | null) | null;
	/** The `.lab-stage` element (pointer target, size reference for pixel-stable sizes). */
	el: HTMLElement | null;
	/** Handle within `radius` px (default 14, see `hitRadius`) of a stage-relative position, or null so the page keeps scrolling. */
	pick(x: number, y: number, radius?: number): Draggable | null;
	/** Applies a key press to `d`; false when the key does not move it (caller keeps the browser default). */
	nudge(d: Draggable, key: string, shift: boolean): boolean;
	/** The ViewportCard this stage belongs to (lesson, card id, declared overlays), or null for a bare Stage. */
	readonly card: CardInfo | null;
	/** What the pointer (or keyboard focus) is over, for the status bar. */
	readonly hover: Hover;
	/** Camera commands, bound by the View's navigation once its camera and controls exist; null before. */
	nav: Nav | null;
	/** Per-frame gizmo update with the camera quaternion [x, y, z, w], bound by the DOM gizmo; null without one. */
	gizmo: ((q: readonly number[]) => void) | null;
	/** Draw-in progress 0..1 read per frame by the primitives (1 = fully drawn; see DrawIn in view.tsx). */
	reveal: number;
}

/** An overlay flag a card declares (Blender's Overlays popover): `on` is the default. */
export interface OverlayDef {
	id: string;
	label: string;
	on: boolean;
}

export interface CardInfo {
	lesson: string;
	id: string;
	overlays: readonly OverlayDef[];
}

/** Store key of overlay `ov` of card `card`; flags live in the lesson store as 0/1 so every view can read them. */
export const overlayKey = (card: string, ov: string): string => `ov:${card}:${ov}`;

/** Whether overlay `def` of `card` is on in `vals` (absent = its default). */
export function overlayOn(vals: Vals, card: string, def: OverlayDef): boolean {
	const v = vals[overlayKey(card, def.id)];
	return v === undefined ? def.on : v !== 0;
}

export interface HoverState {
	/** Drag handle under the pointer (or focused by keyboard), e.g. `a:tip`. */
	handle: string | null;
	/** Scene object under the pointer, e.g. `a`. */
	object: string | null;
	/** N-panel field under the pointer (its row id). */
	field: string | null;
}

export interface Hover {
	get(): HoverState;
	/** Merges `p`; notifies only on a real change (pointer handlers call this constantly). */
	set(p: Partial<HoverState>): void;
	/** Clears `kind` only if it still holds `id` (a late leave must not wipe a newer hover). */
	clear(kind: keyof HoverState, id: string): void;
	subscribe(cb: () => void): () => void;
}

export interface Nav {
	reset(): void;
	align(axis: 0 | 1 | 2, sign: 1 | -1): void;
	/** Unit vector from the target toward the camera, for "click the facing bubble again = opposite view". */
	viewDir(): readonly number[];
	/** Requests a frame of the shared canvas (DOM chrome that needs the scene redrawn, e.g. a gizmo just mounted). */
	redraw(): void;
}

function hoverStore(): Hover {
	let st: HoverState = { handle: null, object: null, field: null };
	const subs = new Set<() => void>();
	const set = (p: Partial<HoverState>) => {
		if (!(Object.keys(p) as (keyof HoverState)[]).some((k) => p[k] !== st[k])) return;
		st = { ...st, ...p };
		for (const cb of subs) cb();
	};
	return {
		get: () => st,
		set,
		clear(kind, id) {
			if (st[kind] === id) set({ [kind]: null });
		},
		subscribe(cb) {
			subs.add(cb);
			return () => void subs.delete(cb);
		},
	};
}

/** Writes every key of a move; values that did not change are skipped so a still pointer causes no re-render. */
export function writeMove(lesson: string, next: Record<string, number[]>): void {
	const st = lessonStore(lesson).getState();
	for (const [k, v] of Object.entries(next)) {
		const cur = st.vals[k];
		if (typeof cur === 'number' || !cur || cur.length !== v.length || cur.some((x, i) => x !== v[i])) st.set(k, v);
	}
}

export function createStage(mode: '2d' | '3d', card: CardInfo | null = null): StageReg {
	const handles = keyed<Draggable>('drag handle');
	const reg: StageReg = {
		mode,
		card,
		hover: hoverStore(),
		nav: null,
		gizmo: null,
		reveal: 1,
		list: handles.list,
		subscribe: handles.subscribe,
		add(d) {
			const owner = {};
			handles.put(d, owner);
			return () => handles.drop(d.id, owner);
		},
		labels: keyed<LabelSpec>('label'),
		labelEls: new Map(),
		project: null,
		el: null,
		pick(x, y, radius) {
			const project = reg.project;
			if (!project) return null;
			const list = handles.list();
			const px = list.flatMap((d) => {
				const at = project(d.at(lessonStore(d.lesson).getState().vals));
				return at ? [{ id: d.id, at }] : [];
			});
			const id = hitHandle([x, y], px, radius);
			return list.find((d) => d.id === id) ?? null;
		},
		nudge(d, key, shift) {
			const vals = lessonStore(d.lesson).getState().vals;
			const anchor = vals[d.keys[0]];
			if (anchor === undefined || typeof anchor === 'number') throw new Error(`store key "${d.keys[0]}" is not a vector`);
			const delta = keyDelta(key, shift, anchor.length);
			if (!delta) return false;
			writeMove(d.lesson, moved(vals, d.keys, delta));
			return true;
		},
	};
	return reg;
}

export const StageCtx = createContext<StageReg | null>(null);

/** The enclosing Stage's registry; scene primitives outside a Stage are an authoring error. */
export function useStage(): StageReg {
	const reg = useContext(StageCtx);
	if (!reg) throw new Error('scene primitives must be rendered inside <Stage>');
	return reg;
}

/**
 * A position that is either a literal or a store key. A key that is missing or holds a number throws: drawing a
 * fallback would hide a typo in a lesson.
 */
export function usePos(lesson: string, v: V | string): V {
	const stored = useStore(lessonStore(lesson), (s) => (typeof v === 'string' ? s.vals[v] : undefined));
	if (typeof v !== 'string') return v;
	if (stored === undefined || typeof stored === 'number') throw new Error(`lesson "${lesson}": store key "${v}" is not a vector`);
	return stored;
}

/**
 * Registers `d` with the enclosing stage while mounted (null = not draggable). `at` is read through a ref, so an
 * inline function does not re-register the handle on every render.
 */
export function useDraggable(d: Draggable | null): void {
	const reg = useStage();
	const at = useRef(d?.at);
	at.current = d?.at;
	const sig = d && JSON.stringify([d.id, d.owner, d.label, d.lesson, d.keys]);
	useEffect(() => {
		if (!d) return;
		return reg.add({ ...d, at: (v) => at.current?.(v) ?? [] });
		// `d` is represented by `sig`; its function member is read via the ref.
	}, [reg, sig]);
}

/** One visually hidden button per handle. Its focus ring is drawn at the handle, where the reader is looking. */
export function DragKeys({ reg }: { reg: StageReg }): React.JSX.Element {
	const list = useSyncExternalStore(reg.subscribe, reg.list, reg.list);
	return (
		<div className="lab-drag-keys">
			{list.map((d) => (
				<KeyHandle key={d.id} reg={reg} d={d} />
			))}
		</div>
	);
}

function KeyHandle({ reg, d }: { reg: StageReg; d: Draggable }): React.JSX.Element {
	const vals = useStore(lessonStore(d.lesson), (s) => s.vals);
	const [focused, setFocused] = useState(false);
	const px = focused ? (reg.project?.(d.at(vals)) ?? null) : null;
	return (
		<>
			<button
				type="button"
				className="lab-handle-key"
				aria-label={d.label}
				onFocus={() => {
					setFocused(true);
					hotOn(d.lesson, d.owner);
					reg.hover.set({ handle: d.id });
				}}
				onBlur={() => {
					setFocused(false);
					hotOff(d.lesson, d.owner);
					reg.hover.clear('handle', d.id);
				}}
				onKeyDown={(e) => {
					if (reg.nudge(d, e.key, e.shiftKey)) e.preventDefault();
				}}
			/>
			{px && <span className="lab-handle-ring" aria-hidden="true" style={{ left: `${px[0]}px`, top: `${px[1]}px` }} />}
		</>
	);
}

/**
 * Registers a text label with the enclosing stage while mounted (null = none). `at`/`away` are read through a ref,
 * so inline functions are fine; the overlay re-renders only when text, color, hot id or offsets change.
 */
export function useLabel(spec: Omit<LabelSpec, 'id'> | null): void {
	const reg = useStage();
	const id = useId();
	const [owner] = useState(() => ({}));
	const fns = useRef(spec);
	fns.current = spec;
	const sig = spec && JSON.stringify([spec.lesson, spec.text, spec.role, spec.small, spec.hot, spec.gap, spec.offset, Boolean(spec.away)]);
	useEffect(() => {
		if (sig === null) return;
		const s = fns.current!;
		reg.labels.put(
			{
				...s,
				id,
				at: (v) => fns.current?.at(v) ?? null,
				away: s.away && ((v) => fns.current?.away?.(v) ?? null),
			},
			owner,
		);
		// `spec` is represented by `sig`; its functions are read via the ref.
	}, [reg, id, owner, sig]);
	// Removal only on unmount or when the label is switched off, so text updates keep the same overlay element.
	useEffect(() => {
		if (sig === null) return;
		return () => reg.labels.drop(id, owner);
	}, [reg, id, owner, sig === null]);
}

/** Overlay half of the labels: one positioned span per registered label, decorative (the formula carries the values). */
export function Labels({ reg }: { reg: StageReg }): React.JSX.Element {
	const list = useSyncExternalStore(reg.labels.subscribe, reg.labels.list, reg.labels.list);
	return (
		<div className="lab-labels" aria-hidden="true">
			{list.map((s) => (
				<LabelEl key={s.id} reg={reg} s={s} />
			))}
		</div>
	);
}

function LabelEl({ reg, s }: { reg: StageReg; s: LabelSpec }): React.JSX.Element {
	const hot = useStore(lessonStore(s.lesson), (st) => s.hot !== undefined && st.hot.has(s.hot));
	const ref = useCallback(
		(el: HTMLSpanElement) => {
			reg.labelEls.set(s.id, el);
			return () => void reg.labelEls.delete(s.id);
		},
		[reg, s.id],
	);
	return (
		<span
			ref={ref}
			className={`lab-label${s.small ? ' is-sm' : ''}${hot ? ' is-hot' : ''}`}
			style={{ '--lab-label-c': roleVar(s.role ?? 'ink') } as React.CSSProperties}
		>
			{s.text}
		</span>
	);
}

/**
 * Moves every label of the stage to its projected anchor. Called by LabelLayer once per rendered frame (after the
 * View has set this frame's camera), so labels follow drags, orbiting and resizes exactly in step with the scene.
 * Writes styles directly: a React render per frame per label would be wasted work.
 */
export function placeLabels(reg: StageReg): void {
	const project = reg.project;
	for (const s of reg.labels.list()) {
		const el = reg.labelEls.get(s.id);
		if (!el) continue;
		const p = project && labelPx(s, lessonStore(s.lesson).getState().vals, project, el);
		if (!p) {
			el.style.visibility = 'hidden';
			continue;
		}
		const t = `translate(${+p[0].toFixed(1)}px, ${+p[1].toFixed(1)}px)`;
		if (el.style.transform !== t) el.style.transform = t;
		el.style.visibility = 'visible';
		// Draw-in: labels fade in over the second half of the reveal, after the beams they name.
		const o = reg.reveal >= 1 ? '' : String(Math.max(0, reg.reveal * 2 - 1));
		if (el.style.opacity !== o) el.style.opacity = o;
	}
}

function labelPx(s: LabelSpec, vals: Vals, project: NonNullable<StageReg['project']>, el: HTMLElement): [number, number] | null {
	const at = s.at(vals);
	let p = at && project(at);
	if (!p) return null;
	const away = s.away?.(vals);
	const q = away && project(away);
	// The label's own size (layout box; transforms do not change it) keeps its near edge `gap` px from the anchor.
	if (q) p = pushAway(p, q, s.gap ?? 0, [el.offsetWidth, el.offsetHeight]);
	const o = s.offset;
	return o ? [p[0] + o[0], p[1] + o[1]] : p;
}

/** Overlay flag `id` of the enclosing card; an undeclared id or a scene outside a ViewportCard is an authoring error. */
export function useOverlay(id: string): boolean {
	const reg = useStage();
	const card = reg.card;
	if (!card) throw new Error(`overlay "${id}": the scene is not inside a ViewportCard`);
	const def = card.overlays.find((o) => o.id === id);
	if (!def) throw new Error(`overlay "${id}" is not declared by card "${card.id}" (declared: ${card.overlays.map((o) => o.id).join(', ') || 'none'})`);
	return useStore(lessonStore(card.lesson), (s) => overlayOn(s.vals, card.id, def));
}

/** Renders `children` only while overlay `ov` of the enclosing card is on (hides objects; never touches values). */
export function Show({ ov, children }: { ov: string; children: ReactNode }): ReactNode {
	return useOverlay(ov) ? children : null;
}

/**
 * Hover priority of scene objects. In 2D everything lies in one plane, so the ray hits a beam, the shadow band under
 * it and the fill it borders at the same distance and r3f's order is just scene order; the rank decides instead:
 * the object a reader is most likely pointing at wins (a point over a beam over a guide line over an arc over a fill).
 */
export const HOVER_RANK = { fill: 0, arc: 1, guide: 2, beam: 3, point: 4 } as const;

/** Minimal shape of an r3f pointer event (handles.tsx stays three-free). */
interface HoverObj {
	userData: Record<string, unknown>;
	parent: HoverObj | null;
}
interface HoverEvent {
	stopPropagation(): void;
	intersections: readonly { object: HoverObj }[];
}

/** Rank of the hoverable group an intersected object belongs to (-1 when it is not part of one). */
function rankOf(o: HoverObj | null): number {
	for (let n = o; n; n = n.parent) if (typeof n.userData.hoverRank === 'number') return n.userData.hoverRank;
	return -1;
}

/** r3f pointer handlers (and the rank tag) of a hoverable scene object; spread onto its group. */
export interface HoverProps {
	onPointerOver?: (e: HoverEvent) => void;
	onPointerMove?: (e: HoverEvent) => void;
	onPointerOut?: () => void;
	userData?: { hoverRank: number };
}

/**
 * Pointer handlers for scene object `id` (r3f events): hover lights it (hot, shared with formula and N-panel) and
 * tells the status bar what is under the pointer. Among the objects under the pointer only the highest `rank` claims
 * the hover (see HOVER_RANK); it stops propagation so nothing behind it competes. Re-claimed on move, so leaving a
 * beam that lies over a band hands the hover back to the band.
 */
export function useHoverable(lesson: string, id: string | undefined, rank: number): HoverProps {
	const reg = useStage();
	return useMemo(() => {
		if (id === undefined) return {};
		const claim = (e: HoverEvent) => {
			const top = Math.max(...e.intersections.map((i) => rankOf(i.object)));
			if (rank < top) return;
			e.stopPropagation();
			if (reg.hover.get().object === id && lessonStore(lesson).getState().hot.has(id)) return;
			hotOn(lesson, id);
			reg.hover.set({ object: id });
		};
		return {
			onPointerOver: claim,
			onPointerMove: claim,
			onPointerOut: () => {
				hotOff(lesson, id);
				reg.hover.clear('object', id);
			},
			userData: { hoverRank: rank },
		};
	}, [reg, lesson, id, rank]);
}
