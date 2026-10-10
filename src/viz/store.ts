/**
 * Per-lesson shared state for linked views. Islands are separate React roots, so the link between a scene,
 * a formula and a table is this module-level registry: every island that imports it from the same Vite chunk
 * gets the same store for the same lesson id (verified by Spike A in Task 4).
 * `hot` holds term ids (the ids used in `\htmlData{t=…}`, NumTable cells and scene objects) so hovering any
 * view highlights the same thing everywhere.
 * Call `lessonStore(id, init)` with `init` only at module level of a lesson's .tsx file: merging init during a
 * React render would update a store other mounted islands subscribe to ("Cannot update a component while
 * rendering a different component"). Components use the hooks, which never initialize.
 */
import { useStore } from 'zustand';
import { createStore, type StoreApi } from 'zustand/vanilla';

export type Val = number | readonly number[];

export interface LessonState {
	vals: Readonly<Record<string, Val>>;
	hot: ReadonlySet<string>;
	step: Readonly<Record<string, number>>;
	set(key: string, v: Val): void;
	setHot(ids: Iterable<string>): void;
	setStep(timeline: string, i: number): void;
}

const stores = new Map<string, StoreApi<LessonState>>();

export function lessonStore(lesson: string, init: Record<string, Val> = {}): StoreApi<LessonState> {
	let s = stores.get(lesson);
	if (!s) {
		// Seeding at creation (not only via the merge below) makes init part of the initial state, so
		// getInitialState() and SSR/server snapshots already carry the first caller's defaults.
		s = createStore<LessonState>()((set) => ({
			vals: { ...init },
			hot: new Set(),
			step: {},
			set: (key, v) => set((st) => ({ vals: { ...st.vals, [key]: v } })),
			setHot: (ids) => set({ hot: new Set(ids) }),
			// NaN/Infinity would poison the step index (and any `step[t] + 1` UI arithmetic), so they reset to 0.
			setStep: (t, i) => set((st) => ({ step: { ...st.step, [t]: Number.isFinite(i) ? Math.max(0, Math.trunc(i)) : 0 } })),
		}));
		stores.set(lesson, s);
	}
	const missing = Object.entries(init).filter(([k]) => !Object.hasOwn(s.getState().vals, k));
	if (missing.length) s.setState((st) => ({ vals: { ...st.vals, ...Object.fromEntries(missing) } }));
	return s;
}

export function useVal<T extends Val>(lesson: string, key: string, fallback: T): T {
	// The selector returns only the stored reference or undefined, both stable across renders. A caller's
	// fallback literal (e.g. `[0, 0]`) is a new object each render, and returning it from the selector would
	// make useSyncExternalStore see a new snapshot every time.
	const v = useStore(lessonStore(lesson), (st) => st.vals[key]);
	return (v as T | undefined) ?? fallback;
}

export function useHot(lesson: string, id: string): boolean {
	return useStore(lessonStore(lesson), (st) => st.hot.has(id));
}

export function useStep(lesson: string, timeline: string): number {
	return useStore(lessonStore(lesson), (st) => st.step[timeline] ?? 0);
}

/**
 * Lights exactly `id` (hover/focus/drag of one object). Writes only on a change: pointer and frame handlers call
 * this constantly, and every write re-renders all subscribers of the lesson.
 */
export function hotOn(lesson: string, id: string): void {
	const s = lessonStore(lesson).getState();
	if (!(s.hot.size === 1 && s.hot.has(id))) s.setHot([id]);
}

/** Clears the highlight if `id` is lit, so leaving one object never wipes a highlight another view set meanwhile. */
export function hotOff(lesson: string, id: string): void {
	const s = lessonStore(lesson).getState();
	if (s.hot.has(id)) s.setHot([]);
}
