/**
 * Per-lesson shared state for linked views. Islands are separate React roots, so the link between a scene,
 * a formula and a table is this module-level registry: every island that imports it from the same Vite chunk
 * gets the same store for the same lesson id (verified by Spike A in Task 4).
 * `hot` holds term ids (the ids used in `\htmlData{t=…}`, NumTable cells and scene objects) so hovering any
 * view highlights the same thing everywhere.
 * Call `lessonStore(id, init)` with `init` only at module level of a lesson's .tsx file: merging init during a
 * React render would update a store other mounted islands subscribe to ("Cannot update a component while
 * rendering a different component"). Components use the hooks, which never initialize.
 *
 * Two views of the values (motion rules, Ruling U): `vals` is the truth that every number, check and formula reads;
 * `shown` is what scenes draw. A write is a cut (both change at once) unless it is marked `ease` (keyboard nudges,
 * typed values, timeline steps): then `vals` jumps to the new value and `shown` follows it with the shared critically
 * damped spring (~180 ms), so a glyph glides while every readout already shows the exact value and no challenge ever
 * sees an in-between number. Direct manipulation (drags, scrubs) is a cut, i.e. 1:1. Components rendered inside a
 * Stage read `shown` through the same hooks (`ShownCtx`); DOM views read `vals`.
 */
import { createContext, useContext } from 'react';
import { useStore } from 'zustand';
import { createStore, type StoreApi } from 'zustand/vanilla';
import { omegaFor, reducedMotion, SPRING_MS, springStep } from './scene/motion';

export type Val = number | readonly number[];

/** How a write reaches the scene: `cut` = at once (direct manipulation), `ease` = the glyph springs to it. */
export type WriteHow = 'cut' | 'ease';

export interface LessonState {
	vals: Readonly<Record<string, Val>>;
	/** What scenes draw: equal to `vals` except while an eased write is in flight. */
	shown: Readonly<Record<string, Val>>;
	hot: ReadonlySet<string>;
	step: Readonly<Record<string, number>>;
	set(key: string, v: Val, how?: WriteHow): void;
	setHot(ids: Iterable<string>): void;
	setStep(timeline: string, i: number): void;
}

/** True inside a Stage: `useVal` and `useVals` then read the drawn (`shown`) values. Provided by Stage.tsx. */
export const ShownCtx = createContext(false);

/** One eased key: per-component spring position/velocity toward `to` (a scalar is a 1-vector). */
interface Ease {
	x: number[];
	v: number[];
	to: number[];
	scalar: boolean;
}

const OMEGA = omegaFor(SPRING_MS);
// Arrival: far below a pixel for anything the course draws (world units, degrees), and at rest.
const EPS_X = 1e-4;
const EPS_V = 1e-3;

const asVec = (v: Val): number[] => (typeof v === 'number' ? [v] : [...v]);
const sameShape = (a: Val | undefined, b: Val): a is Val =>
	a !== undefined && (typeof a === 'number' ? typeof b === 'number' : typeof b !== 'number' && a.length === b.length);

const stores = new Map<string, StoreApi<LessonState>>();

/**
 * Advances every eased key by `dtMs` and writes the new `shown` values in one update; true while any key still
 * moves. Exact spring solution, so the motion does not depend on the frame rate.
 */
function advance(s: StoreApi<LessonState>, eases: Map<string, Ease>, dtMs: number): boolean {
	if (!eases.size) return false;
	const dt = Math.max(0, dtMs) / 1000;
	const next: Record<string, Val> = {};
	for (const [k, e] of eases) {
		let done = true;
		for (let i = 0; i < e.x.length; i++) {
			const st = springStep({ x: e.x[i], v: e.v[i] }, e.to[i], OMEGA, dt);
			e.x[i] = st.x;
			e.v[i] = st.v;
			if (Math.abs(st.x - e.to[i]) > EPS_X || Math.abs(st.v) > EPS_V) done = false;
		}
		// Arrival lands on the exact target (no 2.9999999 left on screen).
		const x = done ? e.to : e.x;
		next[k] = e.scalar ? x[0] : [...x];
		if (done) eases.delete(k);
	}
	s.setState((st) => ({ shown: { ...st.shown, ...next } }));
	return eases.size > 0;
}

/**
 * Calls `cb` with the current time on the next frame. Time comes from performance.now(), not the rAF argument (whose
 * origin can differ), and a requestAnimationFrame that calls back synchronously (test stubs) is detected and replaced
 * by a timer, so the ease loop can never recurse.
 */
function nextFrame(cb: (t: number) => void): void {
	let sync = true;
	let fired = false;
	const run = () => {
		if (fired) return;
		fired = true;
		cb(performance.now());
	};
	if (typeof requestAnimationFrame === 'function')
		requestAnimationFrame(() => {
			if (sync) setTimeout(run, 16);
			else run();
		});
	else setTimeout(run, 16);
	sync = false;
}

export function lessonStore(lesson: string, init: Record<string, Val> = {}): StoreApi<LessonState> {
	let s = stores.get(lesson);
	if (!s) {
		const eases = new Map<string, Ease>();
		let running = false;
		const loop = (last: number): void =>
			nextFrame((now) => {
				if (advance(store, eases, now - last)) loop(now);
				else running = false;
			});
		// Seeding at creation (not only via the merge below) makes init part of the initial state, so
		// getInitialState() and SSR/server snapshots already carry the first caller's defaults.
		const store = createStore<LessonState>()((set, get) => ({
			vals: { ...init },
			shown: { ...init },
			hot: new Set(),
			step: {},
			set: (key, v, how = 'cut') => {
				const from = get().shown[key];
				if (how === 'cut' || reducedMotion() || !sameShape(from, v)) {
					eases.delete(key);
					set((st) => ({ vals: { ...st.vals, [key]: v }, shown: { ...st.shown, [key]: v } }));
					return;
				}
				// A retarget keeps the glyph's current position and velocity: a held arrow key glides, never restarts.
				const cur = eases.get(key);
				eases.set(key, { x: cur ? cur.x : asVec(from), v: cur ? cur.v : asVec(from).map(() => 0), to: asVec(v), scalar: typeof v === 'number' });
				set((st) => ({ vals: { ...st.vals, [key]: v } }));
				if (!running) {
					running = true;
					loop(performance.now());
				}
			},
			setHot: (ids) => set({ hot: new Set(ids) }),
			// NaN/Infinity would poison the step index (and any `step[t] + 1` UI arithmetic), so they reset to 0.
			setStep: (t, i) => set((st) => ({ step: { ...st.step, [t]: Number.isFinite(i) ? Math.max(0, Math.trunc(i)) : 0 } })),
		}));
		s = store;
		stores.set(lesson, s);
	}
	const missing = Object.entries(init).filter(([k]) => !Object.hasOwn(s.getState().vals, k));
	if (missing.length) {
		const add = Object.fromEntries(missing);
		s.setState((st) => ({ vals: { ...st.vals, ...add }, shown: { ...st.shown, ...add } }));
	}
	return s;
}

/** `vals` for DOM views, `shown` inside a Stage (see ShownCtx). */
const view = (st: LessonState, shown: boolean) => (shown ? st.shown : st.vals);

export function useVal<T extends Val>(lesson: string, key: string, fallback: T): T {
	const shown = useContext(ShownCtx);
	// The selector returns only the stored reference or undefined, both stable across renders. A caller's
	// fallback literal (e.g. `[0, 0]`) is a new object each render, and returning it from the selector would
	// make useSyncExternalStore see a new snapshot every time.
	const v = useStore(lessonStore(lesson), (st) => view(st, shown)[key]);
	return (v as T | undefined) ?? fallback;
}

/** All values of a lesson (`shown` inside a Stage): for derived geometry that reads several keys. */
export function useVals(lesson: string): LessonState['vals'] {
	const shown = useContext(ShownCtx);
	return useStore(lessonStore(lesson), (st) => view(st, shown));
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
