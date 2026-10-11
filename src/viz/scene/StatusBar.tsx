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

export type Hint = string | ((vals: Vals) => string | null);

export interface StatusSpec {
	/** By handle id (`a:tip`) or scene object id (`a`; a read-only N-panel row with the same id shows it too). */
	hints?: Readonly<Record<string, Hint>>;
	/** Shown when nothing more specific applies; defaults to the navigation of the viewport. */
	idle?: string;
	/** Evaluated first: a state of the scene that beats any hover hint, or null. */
	state?: (vals: Vals) => string | null;
}

/** Built-in hints: what any handle, field and the gizmo do, and the default navigation per mode. */
export const DEFAULT_HINTS = {
	idle2d: 'Wheel zoom · MMB pan · N panel',
	idle3d: 'LMB/MMB orbit · Shift+MMB pan · Wheel zoom · N panel',
	handle2d: 'LMB drag · arrows 0.1 · Shift 1.0',
	handle3d: 'LMB drag · arrows 0.1 · Shift 1.0 · PageUp/PageDown z',
	field: 'LMB drag scrub · Ctrl snap · Shift fine · click to type · arrows step',
	gizmo: 'LMB click an axis: align the view · again: the opposite side',
} as const;

const run = (h: Hint | undefined, vals: Vals): string | null => (h === undefined ? null : typeof h === 'string' ? h : h(vals));

/**
 * The hint for the current hover: state first, then an editable N-panel field (what dragging/typing does), the
 * handle (lesson hint or the built-in one), the gizmo, the object (lesson hint; read-only N-panel rows count as
 * their object); finally idle.
 */
export function hintFor(h: HoverState, vals: Vals, spec: StatusSpec, mode: '2d' | '3d'): string {
	const hints = spec.hints ?? {};
	return (
		spec.state?.(vals) ??
		(h.field !== null ? DEFAULT_HINTS.field : null) ??
		(h.handle !== null ? (run(hints[h.handle], vals) ?? DEFAULT_HINTS[mode === '3d' ? 'handle3d' : 'handle2d']) : null) ??
		(h.object === GIZMO_ID ? DEFAULT_HINTS.gizmo : null) ??
		(h.object !== null ? run(hints[h.object], vals) : null) ??
		spec.idle ??
		DEFAULT_HINTS[mode === '3d' ? 'idle3d' : 'idle2d']
	);
}

const KEYS = /(LMB\/MMB|LMB|MMB|RMB|Wheel|Shift\+MMB|Shift|Ctrl|PageUp\/PageDown|PageUp|PageDown|\bN(?= panel))/g;

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

export function StatusBar({ reg, lesson, spec }: { reg: StageReg; lesson: string; spec: StatusSpec }): React.JSX.Element {
	const hover = useSyncExternalStore(reg.hover.subscribe, reg.hover.get, reg.hover.get);
	// Only the hint text re-renders the bar, not every store write.
	const text = useStore(lessonStore(lesson), (s) => hintFor(hover, s.vals, spec, reg.mode));
	return (
		<div className="lab-vp-status">
			<span>
				{hintTokens(text).map((t, i) => (t.key ? <kbd key={i}>{t.text}</kbd> : <span key={i}>{t.text}</span>))}
			</span>
		</div>
	);
}
