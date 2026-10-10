/**
 * A linked formula: hover/focus a term and it, plus every view showing the same term id, lights up;
 * drag or arrow-key a `\s{key}` value and the store updates so every view follows.
 * Re-rendering KaTeX on each value change is ~1 ms and updates are throttled to animation frames.
 * `fmt`/`derived` should be module-level constants: new identities each render defeat the render memo.
 */
import { useEffect, useMemo, useRef } from 'react';
import { useStore } from 'zustand';
import { lessonStore } from './store';
import { renderLinked, type Derived } from './tex';
import { keyStep, scrubValue, type ScrubSpec } from './scrub';
import { startScrubDrag } from './useScrub';

export interface FormulaProps {
	lesson: string;
	tex: string;
	scrub?: Record<string, ScrubSpec>;
	fmt?: Record<string, (v: number) => string>;
	derived?: Derived;
	block?: boolean;
	label: string;
}

export function Formula(p: FormulaProps): React.JSX.Element {
	const store = lessonStore(p.lesson);
	const vals = useStore(store, (s) => s.vals);
	const hot = useStore(store, (s) => s.hot);
	const html = useMemo(() => renderLinked(p.tex, vals, p.fmt, p.block, p.derived), [p.tex, vals, p.fmt, p.block, p.derived]);
	const root = useRef<HTMLDivElement>(null);
	// Key of the handle being keyboard-scrubbed: innerHTML replacement destroys it, so focus is restored after render.
	const focusKey = useRef<string | null>(null);

	// KaTeX output is plain HTML, not React: reflect `hot` and the slider semantics onto it after every render.
	useEffect(() => {
		const el = root.current;
		if (!el) return;
		el.querySelectorAll<HTMLElement>('[data-t]').forEach((t) => t.classList.toggle('is-hot', hot.has(t.dataset.t ?? '')));
		el.querySelectorAll<HTMLElement>('[data-s]').forEach((h) => {
			const k = h.dataset.s ?? '';
			const spec = p.scrub?.[k];
			if (!spec) return;
			h.tabIndex = 0;
			h.setAttribute('role', 'slider');
			h.setAttribute('aria-label', k);
			h.setAttribute('aria-valuemin', String(spec.min));
			h.setAttribute('aria-valuemax', String(spec.max));
			h.setAttribute('aria-valuenow', String(vals[k]));
		});
		if (focusKey.current) el.querySelector<HTMLElement>(`[data-s="${focusKey.current}"]`)?.focus({ preventScroll: true });
	}, [html, hot, vals, p.scrub]);

	const term = (e: React.SyntheticEvent) => (e.target as HTMLElement).closest<HTMLElement>('[data-t]')?.dataset.t;
	const handleKey = (e: React.SyntheticEvent) => (e.target as HTMLElement).closest<HTMLElement>('[data-s]')?.dataset.s;
	const setHot = (id: string) => {
		const cur = store.getState().hot;
		if (!(cur.size === 1 && cur.has(id))) store.getState().setHot([id]);
	};

	const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
		// The pressed span is only read for its key: the first value change re-renders innerHTML and removes it,
		// so capture and listeners live on the root div, which survives re-renders.
		const k = handleKey(e);
		const spec = k ? p.scrub?.[k] : undefined;
		if (!k || !spec) return;
		const v0 = store.getState().vals[k] as number;
		startScrubDrag(e, e.currentTarget, (dx) => store.getState().set(k, scrubValue(v0, dx, spec)));
	};

	const onKeyDown = (e: React.KeyboardEvent) => {
		const k = handleKey(e);
		const spec = k ? p.scrub?.[k] : undefined;
		if (!k || !spec) return;
		const v = keyStep(store.getState().vals[k] as number, e.key, spec);
		if (v === null) return;
		e.preventDefault();
		focusKey.current = k;
		store.getState().set(k, v);
	};

	return (
		<div
			ref={root}
			className={`lab-formula not-content${p.block ? ' is-block' : ''}`}
			aria-label={p.label}
			onPointerOver={(e) => {
				const t = term(e);
				if (t) setHot(t);
			}}
			onPointerOut={(e) => {
				// Moving between nested KaTeX spans of the same term fires pointerout; clear only when leaving the term.
				const next = (e.relatedTarget as HTMLElement | null)?.closest?.('[data-t]');
				if (!next) store.getState().setHot([]);
			}}
			onFocus={(e) => {
				const t = term(e);
				if (t) setHot(t);
			}}
			// Focus moving to a real other element ends keyboard scrubbing; the null relatedTarget of a removed handle does not.
			onBlur={(e) => {
				if (e.relatedTarget) focusKey.current = null;
			}}
			onPointerDown={onPointerDown}
			onKeyDown={onKeyDown}
			dangerouslySetInnerHTML={{ __html: html }}
		/>
	);
}
