/**
 * Per-stage registry of draggable handles, shared by the three halves of a Stage that live in different React trees:
 * the scene objects (inside the drei View, i.e. the shared Canvas root) register handles, DragLayer (also inside the
 * View, it owns the camera) binds `project` and turns pointers into drags, and DragKeys (the Stage's DOM overlay)
 * renders one keyboard button per handle. React context does not cross the View tunnel, so Stage creates the
 * registry once and provides it inside the View itself, and hands the same object to the overlay.
 * Deliberately free of three/drei so the keyboard path is testable in happy-dom.
 */
import { createContext, useContext, useEffect, useRef, useState, useSyncExternalStore } from 'react';
import { useStore } from 'zustand';
import type { V } from '../geom';
import { hotOff, hotOn, lessonStore, type LessonState } from '../store';
import { hitHandle, keyDelta, moved } from './drag';

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

export interface StageReg {
	readonly mode: '2d' | '3d';
	list(): readonly Draggable[];
	subscribe(cb: () => void): () => void;
	/** Registers a handle; returns its removal. */
	add(d: Draggable): () => void;
	/** Data point -> stage-relative CSS px. Bound by DragLayer once the View camera exists; null before. */
	project: ((p: readonly number[]) => [number, number] | null) | null;
	/** The `.lab-stage` element (pointer target, size reference for pixel-stable sizes). */
	el: HTMLElement | null;
	/** Handle under a stage-relative pixel position (14 px radius), or null so the page keeps scrolling. */
	pick(x: number, y: number): Draggable | null;
	/** Applies a key press to `d`; false when the key does not move it (caller keeps the browser default). */
	nudge(d: Draggable, key: string, shift: boolean): boolean;
}

/** Writes every key of a move; values that did not change are skipped so a still pointer causes no re-render. */
export function writeMove(lesson: string, next: Record<string, number[]>): void {
	const st = lessonStore(lesson).getState();
	for (const [k, v] of Object.entries(next)) {
		const cur = st.vals[k];
		if (typeof cur === 'number' || !cur || cur.length !== v.length || cur.some((x, i) => x !== v[i])) st.set(k, v);
	}
}

export function createStage(mode: '2d' | '3d'): StageReg {
	const handles = new Map<string, Draggable>();
	const subs = new Set<() => void>();
	// useSyncExternalStore needs the same array until something changes.
	let snapshot: readonly Draggable[] = [];
	const changed = () => {
		snapshot = [...handles.values()];
		for (const cb of subs) cb();
	};
	const reg: StageReg = {
		mode,
		list: () => snapshot,
		subscribe(cb) {
			subs.add(cb);
			return () => void subs.delete(cb);
		},
		add(d) {
			if (handles.has(d.id)) throw new Error(`duplicate drag handle id "${d.id}" in one stage`);
			handles.set(d.id, d);
			changed();
			return () => {
				if (handles.get(d.id) === d) {
					handles.delete(d.id);
					changed();
				}
			};
		},
		project: null,
		el: null,
		pick(x, y) {
			const project = reg.project;
			if (!project) return null;
			const px = snapshot.flatMap((d) => {
				const at = project(d.at(lessonStore(d.lesson).getState().vals));
				return at ? [{ id: d.id, at }] : [];
			});
			const id = hitHandle([x, y], px);
			return id === null ? null : (handles.get(id) ?? null);
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
				}}
				onBlur={() => {
					setFocused(false);
					hotOff(d.lesson, d.owner);
				}}
				onKeyDown={(e) => {
					if (reg.nudge(d, e.key, e.shiftKey)) e.preventDefault();
				}}
			/>
			{px && <span className="lab-handle-ring" aria-hidden="true" style={{ left: `${px[0]}px`, top: `${px[1]}px` }} />}
		</>
	);
}
