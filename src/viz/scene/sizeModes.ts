/** One authority across Astro roots. Moving the host DOM never remounts its React root. */
import { useLayoutEffect, useSyncExternalStore, type RefObject } from 'react';
import { captureFormulaFocus } from '../formulaFocus';
import { reducedMotion, springProgress } from './motion';

export type SizeMode = 'normal' | 'wide' | 'full';
export interface SizeState { id: string | null; mode: SizeMode }
export type SizeAction = { type: 'toggle'; id: string; mode: 'wide' | 'full' } | { type: 'exit' };
export const NORMAL: SizeState = { id: null, mode: 'normal' };
export function reduceSize(s: SizeState, a: SizeAction): SizeState {
 if (a.type === 'exit' || (s.id === a.id && s.mode === a.mode)) return NORMAL;
 return { id: a.id, mode: a.mode };
}
let state = NORMAL;
const listeners = new Set<() => void>();
const cards = new Map<string, { el: HTMLElement; redraw: () => void }>();
let hovered: string | null = null;
let generation = 0;
let releaseFull: (() => (()=>HTMLElement | null)) | null = null;
let finishRestore = () => {};
let restoreOwner: string | null = null;
let releaseNavigation: (() => void) | null = null;
let browserOwned = false;
let requesting = false;
let stopAnimation = () => {};
let exiting = false;
function animateCard(el: HTMLElement, from: DOMRect, to: { left: number; top: number; width: number; height: number }, done = () => {}) {
 stopAnimation();
 let raf = 0;
 const clear = () => { cancelAnimationFrame(raf); for (const k of ['left','top','width','height','right','bottom']) el.style.removeProperty(k); };
 stopAnimation = clear;
 if (reducedMotion()) { done(); return; }
 const t0 = performance.now();
 const frame = () => {
  const s = springProgress(performance.now() - t0, 150);
  for (const k of ['left','top','width','height'] as const) el.style[k] = (from[k] + (to[k] - from[k]) * s) + 'px';
  el.style.right = 'auto'; el.style.bottom = 'auto';
  redraw();
  if (s < 1) raf = requestAnimationFrame(frame);
  else { clear(); done(); }
 };
 frame();
}
const get = () => state;
const subscribe = (fn: () => void) => { listeners.add(fn); return () => { listeners.delete(fn); }; };
export const useSizeState = () => useSyncExternalStore(subscribe, get, () => NORMAL);
export function viewVisible(id: string | undefined, s: SizeState): boolean { return s.mode !== 'full' || s.id === id; }
const redraw = () => cards.forEach(c => c.redraw());

function exitBrowser() {
 if (browserOwned && document.fullscreenElement === document.documentElement) {
  browserOwned = false;
  void document.exitFullscreen?.().catch(() => {});
 }
}
/** The fixed Starlight rails otherwise paint over a wide viewport's outer edges. */
function hideNavigation(card: HTMLElement) {
 const focus = captureFormulaFocus(document.activeElement as HTMLElement | null);
 const saved = [...document.querySelectorAll<HTMLElement>('.sidebar-pane, .right-sidebar')].map(el => ({
  el, visibility: el.style.getPropertyValue('visibility'), priority: el.style.getPropertyPriority('visibility'), inert: el.getAttribute('inert'),
 }));
 for (const { el } of saved) { el.style.setProperty('visibility', 'hidden', 'important'); el.inert = true; }
 if (!card.contains(document.activeElement)) card.focus({ preventScroll: true });
 return () => {
  for (const { el, visibility, priority, inert } of saved) {
   if (visibility) el.style.setProperty('visibility', visibility, priority); else el.style.removeProperty('visibility');
   if (inert === null) el.removeAttribute('inert'); else el.setAttribute('inert', inert);
  }
  focus?.()?.focus({ preventScroll: true });
 };
}
function enterFull(el: HTMLElement) {
 const body = document.body, html = document.documentElement;
 const x = window.scrollX, y = window.scrollY;
 const overflow = body.style.overflow, htmlOverflow = html.style.overflow;
 const host = document.querySelector<HTMLElement>('.lab-scene-host');
 const parent = host?.parentNode, next = host?.nextSibling;
 const focus = captureFormulaFocus(document.activeElement as HTMLElement | null);
 body.style.overflow = 'hidden';
 html.style.overflow = 'hidden';
 el.dataset.size = 'full';
 if (host) el.append(host);
 if (!el.contains(document.activeElement)) el.focus({ preventScroll: true });
 return () => {
  if (host && parent) parent.insertBefore(host, next?.parentNode === parent ? next : null);
  delete el.dataset.size;
  body.style.overflow = overflow;
  html.style.overflow = htmlOverflow;
  window.scrollTo(x, y);
  return focus;
 };
}
function change(next: SizeState, request = false, settled = false, browserExit = false) {
 const prev = state;
 if (prev.mode === 'full' && next.mode !== 'full' && !settled && !reducedMotion()) {
  const c = prev.id && cards.get(prev.id);
  const frame = c && c.el.closest('.lab-vp-frame');
  if (c && frame) {
   exiting = true;
   animateCard(c.el, c.el.getBoundingClientRect(), frame.getBoundingClientRect(), () => {
    exiting = false;
    change(next, request, true);
   });
   return;
  }
 }
 exiting = false;
 stopAnimation();
 finishRestore();
 const entryRect = next.id ? cards.get(next.id)?.el.getBoundingClientRect() : undefined;
 const token = ++generation;
 if (prev.mode === 'full') {
  const el = prev.id ? cards.get(prev.id)?.el : undefined;
  const visibility = el?.style.getPropertyValue('visibility') ?? '';
  const priority = el?.style.getPropertyPriority('visibility') ?? '';
  // Browser exit is uncued. Reattach chrome between paints instead of painting its
  // fixed-to-normal jump. The Canvas moves back to BODY and keeps rendering.
  if (browserExit && el) el.style.setProperty('visibility', 'hidden', 'important');
  const focus = releaseFull?.();
  releaseFull = null;
  let raf = 0;
  const finish = () => {
   cancelAnimationFrame(raf);
   if (browserExit && el) {
    if (visibility) el.style.setProperty('visibility', visibility, priority); else el.style.removeProperty('visibility');
   }
   focus?.()?.focus({ preventScroll: true });
   finishRestore = () => {};
   restoreOwner = null;
  };
  if (browserExit && el) {
   finishRestore = finish;
   restoreOwner = prev.id;
   raf = requestAnimationFrame(() => { raf = requestAnimationFrame(finish); });
  } else finish();
 }
 releaseNavigation?.();
 releaseNavigation = null;
 state = next;
 if (next.mode === 'wide' && next.id) {
  const card = cards.get(next.id);
  if (card) releaseNavigation = hideNavigation(card.el);
 }
 if (next.mode === 'full') {
  const card = next.id && cards.get(next.id);
  if (card) {
   releaseFull = enterFull(card.el);
   if (prev.mode !== 'full' && entryRect) animateCard(card.el, entryRect, { left: 0, top: 0, width: window.innerWidth, height: window.innerHeight });
  }
 } else exitBrowser();
 listeners.forEach(fn => fn());
 redraw();
 if (request && next.mode === 'full' && !document.fullscreenElement && !requesting) {
  // Must stay in the initiating click/keydown stack for transient user activation.
  try {
   const pending = document.documentElement.requestFullscreen?.();
   if (pending) {
    requesting = true;
    void pending.then(() => {
     requesting = false;
     browserOwned = document.fullscreenElement === document.documentElement;
     if (token !== generation && state.mode !== 'full') exitBrowser();
     redraw();
    }, () => { requesting = false; }); // Rejected API retains the usable overlay.
   }
  } catch { requesting = false; }
 }
}
export function toggleSize(id: string, mode: 'wide' | 'full') {
 change(reduceSize(state, { type: 'toggle', id, mode }), mode === 'full');
}
function key(e: KeyboardEvent) {
 if (e.defaultPrevented || e.repeat) return;
 const target = e.target as Element | null;
 if (e.key === 'Tab' && state.mode === 'full' && state.id) {
  const el = cards.get(state.id)?.el;
  const items = el ? [...el.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), [tabindex="0"], a[href]')].filter(n => n.getClientRects().length && getComputedStyle(n).visibility !== 'hidden') : [];
  const first = items[0], last = items.at(-1);
  if (first && last && ((e.shiftKey && (document.activeElement === first || document.activeElement === el)) || (!e.shiftKey && document.activeElement === last))) {
   e.preventDefault(); (e.shiftKey ? last : first).focus(); return;
  }
 }
 if (target?.closest?.('input, textarea, select, [contenteditable]:not([contenteditable="false"])')) return;
 if (e.key === 'Escape' && state.id) { e.preventDefault(); change(NORMAL); return; }
 const focused = [...cards].find(([, c]) => c.el.contains(document.activeElement))?.[0];
 const id = focused ?? hovered;
 if (!id) return;
 const space = e.code === 'Space' && e.ctrlKey && !e.metaKey;
 const full = (space && e.altKey) || (e.key.toLowerCase() === 'f' && !e.ctrlKey && !e.metaKey && !e.altKey);
 if (!space && !full) return;
 e.preventDefault();
 toggleSize(id, full ? 'full' : 'wide');
}
function fullscreenChange() {
 if (document.fullscreenElement === document.documentElement) {
  if (requesting) browserOwned = true;
 } else if (browserOwned) {
  browserOwned = false;
  if (state.mode === 'full' && !exiting) change(NORMAL, false, true, true);
 }
 redraw();
}
export function registerSizeCard(id: string, el: HTMLElement, poke: () => void) {
 if (cards.has(id)) throw new Error('Duplicate viewport size id: ' + id);
 if (!cards.size) {
  window.addEventListener('keydown', key);
  document.addEventListener('fullscreenchange', fullscreenChange);
 }
 cards.set(id, { el, redraw: poke });
 const enter = () => { hovered = id; };
 const leave = () => { if (hovered === id) hovered = null; };
 el.addEventListener('pointerenter', enter);
 el.addEventListener('pointerleave', leave);
 return () => {
  if (state.id === id) change(NORMAL, false, true);
  if (restoreOwner === id) finishRestore();
  cards.delete(id);
  leave();
  el.removeEventListener('pointerenter', enter);
  el.removeEventListener('pointerleave', leave);
  if (!cards.size) {
   finishRestore();
   window.removeEventListener('keydown', key);
   document.removeEventListener('fullscreenchange', fullscreenChange);
  }
 };
}

/** Wide changes the actual server reservation, so following prose reflows with the viewport. */
export function useCardSize(id: string, ref: RefObject<HTMLDivElement | null>, poke: () => void) {
 const shared = useSizeState();
 const mode = shared.id === id ? shared.mode : 'normal';
 useLayoutEffect(() => {
  const el = ref.current;
  if (!el) return;
  const unregister = registerSizeCard(id, el, poke);
  return () => {
   unregister();
   const fig = el.closest<HTMLElement>('.lab-vp');
   fig?.style.removeProperty('width');
   fig?.style.removeProperty('margin-left');
  };
 }, [id, ref]);
 useLayoutEffect(() => {
  const el = ref.current;
  const fig = el?.closest<HTMLElement>('.lab-vp');
  if (!el || !fig) return;
  let raf = 0;
  const apply = () => {
   cancelAnimationFrame(raf);
   const from = fig.getBoundingClientRect();
   fig.style.removeProperty('width');
   fig.style.removeProperty('margin-left');
   const normal = fig.getBoundingClientRect();
   if (mode === 'full') { poke(); return; }
   const w = mode === 'wide' ? Math.min(normal.width * 2, document.documentElement.clientWidth - 32) : normal.width;
   const left = mode === 'wide' ? (document.documentElement.clientWidth - w) / 2 - normal.left : 0;
   const start = performance.now();
   const frame = () => {
    const t = reducedMotion() ? 1 : springProgress(performance.now() - start, 150);
    fig.style.width = (from.width + (w - from.width) * t) + 'px';
    fig.style.marginLeft = ((from.left - normal.left) * (1 - t) + left * t) + 'px';
    poke();
    if (t < 1) raf = requestAnimationFrame(frame);
    else if (mode === 'normal') { fig.style.removeProperty('width'); fig.style.removeProperty('margin-left'); }
   };
   if (Math.abs(from.width - w) < .1 && Math.abs(from.left - normal.left - left) < .1) {
    if (mode === 'wide') { fig.style.width = w + 'px'; fig.style.marginLeft = left + 'px'; }
    poke();
   } else frame();
  };
  apply();
  window.addEventListener('resize', apply);
  return () => { cancelAnimationFrame(raf); window.removeEventListener('resize', apply); };
 }, [mode, ref]);
 return { mode, shared };
}
