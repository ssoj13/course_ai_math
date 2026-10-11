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
import { boundIndex, finite, type LessonLinks } from './links';
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
	/** Keys whose shared radial resolver replaces ordinary per-component box bounds. */
	radialKeys: ReadonlySet<string>;
	step: Readonly<Record<string, number>>;
	set(key: string, v: Val, how?: WriteHow): void;
	setMany(patch: Record<string, Val>, how?: WriteHow, ease?: readonly string[]): void;
	/** Restore only this transaction's keys, mirrors and in-flight motion; unrelated edits survive. */
	checkpoint(keys: readonly string[]): () => void;
	boundTimeline(id: string, count: number, phases?: readonly number[]): void;
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
function advance(s: StoreApi<LessonState>, eases: Map<string, Ease>, dtMs: number, links: LessonLinks): boolean {
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
	s.setState((st) => {
		const shown = { ...st.shown, ...next };
		for (const g of links.groups ?? []) Object.assign(shown, g.shown(shown));
		return { shown };
	});
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

export function lessonStore(lesson: string, init: Record<string, Val> = {}, links: LessonLinks = {}): StoreApi<LessonState> {
 let s = stores.get(lesson);
 if (!s) {
  const eases = new Map<string, Ease>();
  const bounds = new Map<string, number>();
  const phases = new Map<string, readonly number[]>();
  const adapters = links.timelines ?? {};
  const groups = links.groups ?? [];
  const initial = {...init};
  for (const g of groups) {
   if (g.keys.some(k => Object.hasOwn(initial,k))) {
    const input = Object.fromEntries(g.keys.filter(k=>Object.hasOwn(initial,k)).map(k=>[k,initial[k]]));
    Object.assign(initial,g.resolve(input,initial));
   }
  }
  const initialStep: Record<string,number> = {};
  for (const [id,a] of Object.entries(adapters)) {
   const v=initial[a.key];
   if(typeof v!=='number')throw new Error('Timeline adapter needs initial '+a.key);
   finite(v);
   initial[a.key]=Math.max(a.min,Math.min(a.max,Math.trunc(v)));
   initialStep[id]=a.index(initial[a.key] as number);
   initialStep[id+':max']=initialStep[id];
   bounds.set(id,a.max-a.min);
  }
  let running = false;
  const loop = (last: number): void => nextFrame(now => {
   if(advance(store,eases,now-last,links))loop(now); else running=false;
  });
  const start = () => {if(eases.size&&!running){running=true;loop(performance.now())}};
  const store=createStore<LessonState>()((set,get)=>({
   vals:initial, shown:initial, hot:new Set(), step:initialStep,
   radialKeys:new Set(groups.filter(g=>g.rangePolicy==='radial').flatMap(g=>g.keys)),
   set:(key,v,how='cut')=>get().setMany({[key]:v},how),
   setMany:(input,how='cut',ease=[])=>{
    for(const v of Object.values(input))finite(v);
    const patch={...input};
    const modes: Record<string,WriteHow> = Object.fromEntries(Object.keys(patch).map(k=>[k,ease.includes(k)?'ease':how]));
    for(const g of groups) {
     const keys=g.keys.filter(k=>Object.hasOwn(input,k));
     if(!keys.length)continue;
     Object.assign(patch,g.resolve(Object.fromEntries(keys.map(k=>[k,input[k]])),get().vals));
     modes[g.canonical]=keys.some(k=>modes[k]==='ease')?'ease':'cut';
     for(const k of g.keys)if(k!==g.canonical)modes[k]='cut';
    }
    const step={...get().step};
    for(const [id,a] of Object.entries(adapters))if(Object.hasOwn(patch,a.key)){
     const v=patch[a.key];if(typeof v!=='number')throw new Error('Timeline value must be scalar');
     const max=bounds.get(id)??a.max-a.min;
     const index=boundIndex(a.index(Math.max(a.min,Math.min(a.max,Math.trunc(v)))),max);
     patch[a.key]=a.value(index); modes[a.key]='cut';
     step[id]=index;step[id+':max']=Math.max(step[id+':max']??0,index);step[id+':phases']=phases.get(id)?.[index]??1;
    }
    const vals={...get().vals},shown={...get().shown};
    let changed=false;
    for(const [key,v] of Object.entries(patch)){
     const old=vals[key];
     const equal=typeof v==='number'?old===v:Array.isArray(old)&&old.length===v.length&&v.every((n,i)=>old[i]===n);
     const from=shown[key];
     const cut=modes[key]!=='ease'||reducedMotion()||!sameShape(from,v);
     const drawnEqual=typeof v==='number'?from===v:Array.isArray(from)&&from.length===v.length&&v.every((n,i)=>from[i]===n);
     if(equal&&(!cut||(drawnEqual&&!eases.has(key))))continue;
     changed=true;vals[key]=v;
     if(cut){eases.delete(key);shown[key]=v}
     else {
      const cur=eases.get(key);
      eases.set(key,{x:cur?cur.x:asVec(from),v:cur?cur.v:asVec(from).map(()=>0),to:asVec(v),scalar:typeof v==='number'});
     }
    }
    for(const g of groups)Object.assign(shown,g.shown(shown));
    if(changed||Object.keys(step).some(k=>step[k]!==get().step[k]))set({vals,shown,step});
    start();
   },
   checkpoint:(keys)=>{
    const relevant=new Set(keys);
    for(const g of groups)if(g.keys.some(k=>relevant.has(k)))g.keys.forEach(k=>relevant.add(k));
    const st=get(),saved=new Map<string,Ease>();
    for(const k of relevant){const e=eases.get(k);if(e)saved.set(k,{...e,x:[...e.x],v:[...e.v],to:[...e.to]})}
    return ()=>{
     const vals={...get().vals},shown={...get().shown},step={...get().step};
     for(const k of relevant){
      if(Object.hasOwn(st.vals,k))vals[k]=st.vals[k];else delete vals[k];
      if(Object.hasOwn(st.shown,k))shown[k]=st.shown[k];else delete shown[k];
      eases.delete(k);const e=saved.get(k);if(e)eases.set(k,{...e,x:[...e.x],v:[...e.v],to:[...e.to]});
     }
     for(const [id,a] of Object.entries(adapters))if(relevant.has(a.key)){step[id]=st.step[id];step[id+':max']=st.step[id+':max'];step[id+':phases']=st.step[id+':phases']}
     set({vals,shown,step});start();
    };
   },
   boundTimeline:(id,count,counts)=>{
    if(!Number.isInteger(count)||count<1)throw new Error('Timeline needs at least one step');
    bounds.set(id,count-1);
    if(counts)phases.set(id,counts);
    const i=boundIndex(get().step[id]??0,count-1);
    get().setStep(id,i);
   },
   setHot:ids=>set({hot:new Set(ids)}),
   setStep:(id,i)=>{
    const index=boundIndex(i,bounds.get(id)??Number.MAX_SAFE_INTEGER);
    const a=adapters[id];
    if(a){get().set(a.key,a.value(index),'ease');return}
    const st=get(),max=Math.max(st.step[id+':max']??0,index);
    const n=phases.get(id)?.[index]??1;
    if(st.step[id]!==index||st.step[id+':max']!==max||st.step[id+':phases']!==n)set({step:{...st.step,[id]:index,[id+':max']:max,[id+':phases']:n}});
   },
  }));
  s=store;stores.set(lesson,s);
 } else if(links.groups||links.timelines)throw new Error('Register lesson links only on first initialization: '+lesson);
 const missing=Object.fromEntries(Object.entries(init).filter(([k])=>!Object.hasOwn(s.getState().vals,k)));
 if(Object.keys(missing).length)s.getState().setMany(missing);
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
