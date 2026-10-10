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
		s = createStore<LessonState>()((set) => ({
			vals: {},
			hot: new Set(),
			step: {},
			set: (key, v) => set((st) => ({ vals: { ...st.vals, [key]: v } })),
			setHot: (ids) => set({ hot: new Set(ids) }),
			setStep: (t, i) => set((st) => ({ step: { ...st.step, [t]: Math.max(0, Math.trunc(i)) } })),
		}));
		stores.set(lesson, s);
	}
	const missing = Object.entries(init).filter(([k]) => !(k in s.getState().vals));
	if (missing.length) s.setState((st) => ({ vals: { ...st.vals, ...Object.fromEntries(missing) } }));
	return s;
}

export function useVal<T extends Val>(lesson: string, key: string, fallback: T): T {
	return useStore(lessonStore(lesson), (st) => (st.vals[key] as T | undefined) ?? fallback);
}

export function useHot(lesson: string, id: string): boolean {
	return useStore(lessonStore(lesson), (st) => st.hot.has(id));
}

export function useStep(lesson: string, timeline: string): number {
	return useStore(lessonStore(lesson), (st) => st.step[timeline] ?? 0);
}
