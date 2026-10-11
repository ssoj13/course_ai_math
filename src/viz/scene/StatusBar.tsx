/**
 * Status bar (Blender's): one line saying what the pointer does where it is. The lesson gives hints per handle id,
 * object id or N-panel row id (a string, or a function of the store values returning text or null), an `idle` text,
 * and an optional `state` function evaluated first (e.g. "Target out of reach" while a value is clamped). Mouse and
 * key words (LMB, MMB, Wheel, Shift, Ctrl, ...) are drawn as small key caps. Not a live region: hover hints change
 * constantly and would flood a screen reader; the same information is in the labels and the caption.
 * Three-free (DOM only); `hintFor` is pure.
 */
import { useSyncExternalStore } from 'react';
import { useStore } from 'zustand';
import { lessonStore, type LessonState } from '../store';
import { GIZMO_ID } from './Gizmo';
import type { HoverState, StageReg } from './handles';

type Vals = LessonState['vals'];

/** Hint text: fixed, or computed from the store values (null = no hint here, fall through). */
export type HintText = string | ((vals: Vals) => string | null);

/**
 * A lesson hint. Plain text is device-neutral (a value, a description) and is shown on every device. An instruction
 * that names mouse buttons or keys is written as `{ mouse, touch }`: the touch wording replaces it on touch screens
 * (`touch: null` = nothing specific on touch, the built-in touch hint is shown). Explicit, never guessed from words.
 */
export type Hint = HintText | { mouse: HintText; touch: HintText | null };

export interface StatusSpec {
	/** By handle id (`a:tip`) or scene object id (`a`; a read-only N-panel row with the same id shows it too). */
	hints?: Readonly<Record<string, Hint>>;
	/** Shown when nothing more specific applies; defaults to the navigation of the viewport. */
	idle?: Hint;
	/** Evaluated first: a state of the scene that beats any hover hint, or null. */
	state?: Hint;
}

/** Built-in mouse hints: what any handle, field and the gizmo do, and the default navigation per mode. */
export const DEFAULT_HINTS = {
	idle2d: 'Ctrl+Wheel zoom · MMB pan · N panel',
	idle3d: 'LMB/MMB orbit · Shift+MMB pan · Ctrl+Wheel zoom · N panel',
	handle2d: 'LMB drag · arrows 0.1 · Shift 1.0',
	handle3d: 'LMB drag · arrows 0.1 · Shift 1.0 · PageUp/PageDown z',
	field: 'LMB drag scrub · Ctrl snap · Shift fine · click to type · arrows step',
	gizmo: 'LMB click an axis: align the view · again: the opposite side',
} as const;

/** Built-in touch hints (visual-language.md: touch devices never see mouse buttons, wheel or keys). */
export const TOUCH_HINTS = {
	idle2d: 'Drag a tip · pinch to zoom',
	idle3d: 'Drag a tip · swipe sideways to orbit · pinch to zoom',
	handle2d: 'Drag to move',
	handle3d: 'Drag to move',
	field: 'Swipe sideways to scrub · tap to type',
	gizmo: 'Tap an axis to align the view',
} as const;

const KEYS = /(LMB\/MMB|LMB|MMB|RMB|Ctrl\+Wheel|Wheel|Shift\+MMB|Shift|Ctrl|PageUp\/PageDown|PageUp|PageDown|\bN(?= panel))/g;

const text = (h: HintText | null, vals: Vals): string | null => (h === null ? null : typeof h === 'string' ? h : h(vals));

const run = (h: Hint | undefined, vals: Vals, touch: boolean): string | null => {
	if (h === undefined) return null;
	if (typeof h === 'object') return text(touch ? h.touch : h.mouse, vals);
	return text(h, vals);
};

/**
 * The hint for the current hover: state first, then an editable N-panel field (what dragging/typing does), the
 * handle (lesson hint or the built-in one), the gizmo, the object (lesson hint; read-only N-panel rows count as
 * their object); finally idle. `touch` picks the touch wording of the built-ins and of `{ mouse, touch }` hints.
 */
export function hintFor(h: HoverState, vals: Vals, spec: StatusSpec, mode: '2d' | '3d', touch = false): string {
	const hints = spec.hints ?? {};
	const D = touch ? TOUCH_HINTS : DEFAULT_HINTS;
	return (
		run(spec.state, vals, touch) ??
		(h.field !== null ? D.field : null) ??
		(h.handle !== null ? (run(hints[h.handle], vals, touch) ?? D[mode === '3d' ? 'handle3d' : 'handle2d']) : null) ??
		(h.object === GIZMO_ID ? D.gizmo : null) ??
		(h.object !== null ? run(hints[h.object], vals, touch) : null) ??
		run(spec.idle, vals, touch) ??
		D[mode === '3d' ? 'idle3d' : 'idle2d']
	);
}

/** Splits a hint into text and key-cap tokens. */
export function hintTokens(s: string): { text: string; key: boolean }[] {
	const out: { text: string; key: boolean }[] = [];
	let i = 0;
	for (const m of s.matchAll(KEYS)) {
		if (m.index > i) out.push({ text: s.slice(i, m.index), key: false });
		out.push({ text: m[0], key: true });
		i = m.index + m[0].length;
	}
	if (i < s.length) out.push({ text: s.slice(i), key: false });
	return out;
}

// Whether the reader is on touch: a coarse primary pointer, then whatever pointer was last pressed (hybrid devices).
// One page-wide listener shared by every status bar.
let touchNow = false;
const touchSubs = new Set<() => void>();
let touchInit = false;
function touchSubscribe(cb: () => void): () => void {
	if (!touchInit) {
		touchInit = true;
		touchNow = typeof matchMedia === 'function' && matchMedia('(pointer: coarse)').matches;
		window.addEventListener(
			'pointerdown',
			(e) => {
				const t = e.pointerType === 'touch' || e.pointerType === 'pen';
				if (t === touchNow) return;
				touchNow = t;
				for (const f of touchSubs) f();
			},
			{ capture: true, passive: true },
		);
	}
	touchSubs.add(cb);
	return () => void touchSubs.delete(cb);
}
const touchSnap = () => touchNow;

export function StatusBar({ reg, lesson, spec }: { reg: StageReg; lesson: string; spec: StatusSpec }): React.JSX.Element {
	const hover = useSyncExternalStore(reg.hover.subscribe, reg.hover.get, reg.hover.get);
	const touch = useSyncExternalStore(touchSubscribe, touchSnap, () => false);
	// Only the hint text re-renders the bar, not every store write.
	const text = useStore(lessonStore(lesson), (s) => hintFor(hover, s.vals, spec, reg.mode, touch));
	return (
		<div className="lab-vp-status">
			<span>
				{hintTokens(text).map((t, i) => (t.key ? <kbd key={i}>{t.text}</kbd> : <span key={i}>{t.text}</span>))}
			</span>
		</div>
	);
}
