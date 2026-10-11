/**
 * A linked formula: hover/focus a term and it, plus every view showing the same term id, lights up;
 * drag or arrow-key a `\s{key}` value and the store updates so every view follows.
 * Re-rendering KaTeX on each value change is ~1 ms and updates are throttled to animation frames.
 * `scrub`/`fmt`/`derived` are read through a latest-value ref, so inline object literals in JSX are fine and
 * never re-render KaTeX; the markup depends only on `tex`, `block`, the stored values and the derived results.
 */
import { useEffect, useMemo, useRef } from 'react';
import { useStore } from 'zustand';
import { hotOn, lessonStore } from './store';
import { renderLinked, showVal, texText, type Derived } from './tex';
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
	const props = useRef(p);
	props.current = p;
	// Derived values are the only prop-dependent input to the markup; a stable serialization stands in for the
	// function identities so a parent re-render with fresh inline props does not re-render KaTeX.
	const derivedKey = p.derived ? JSON.stringify(Object.values(p.derived).map((f) => f(vals))) : '';
	const html = useMemo(
		() => renderLinked(p.tex, vals, props.current.fmt, p.block, props.current.derived),
		// fmt/derived are deliberately absent from the deps: they are read via the latest-value ref.
		[p.tex, vals, p.block, derivedKey],
	);
	const root = useRef<HTMLDivElement>(null);
	// Handle key that held focus when `html` last changed: replacing innerHTML destroys it, so focus is restored
	// after the commit. Recorded during render (the old DOM is still in place) and only if focus was inside the root,
	// so a stale request can never pull focus back after the user moved elsewhere.
	const seen = useRef(html);
	const refocus = useRef<string | null>(null);
	if (seen.current !== html) {
		seen.current = html;
		const a = typeof document === 'undefined' ? null : document.activeElement;
		refocus.current = a && root.current?.contains(a) ? ((a as HTMLElement).dataset.s ?? null) : null;
	}

	// KaTeX output is plain HTML, not React: reflect `hot` onto the term spans after every render.
	useEffect(() => {
		root.current?.querySelectorAll<HTMLElement>('[data-t]').forEach((t) => t.classList.toggle('is-hot', hot.has(t.dataset.t ?? '')));
	}, [html, hot]);

	// Slider semantics for scrubbable handles, re-applied whenever the markup is replaced; then restore focus.
	useEffect(() => {
		const el = root.current;
		if (!el) return;
		const { scrub, fmt } = props.current;
		el.querySelectorAll<HTMLElement>('[data-s]').forEach((h) => {
			const k = h.dataset.s ?? '';
			const spec = scrub?.[k];
			const v = lessonStore(props.current.lesson).getState().vals[k];
			if (!spec || typeof v !== 'number') return;
			h.tabIndex = 0;
			h.setAttribute('role', 'slider');
			h.setAttribute('aria-label', k);
			h.setAttribute('aria-valuemin', String(spec.min));
			h.setAttribute('aria-valuemax', String(spec.max));
			h.setAttribute('aria-valuenow', String(v));
			h.setAttribute('aria-valuetext', texText(showVal(k, v, fmt)));
		});
		if (refocus.current) el.querySelector<HTMLElement>(`[data-s="${refocus.current}"]`)?.focus({ preventScroll: true });
		refocus.current = null;
	}, [html]);

	const term = (e: React.SyntheticEvent) => (e.target as HTMLElement).closest<HTMLElement>('[data-t]')?.dataset.t;
	const handleKey = (e: React.SyntheticEvent) => (e.target as HTMLElement).closest<HTMLElement>('[data-s]')?.dataset.s;

	const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
		// The pressed span is only read for its key: the first value change re-renders innerHTML and removes it,
		// so capture and listeners live on the root div, which survives re-renders.
		const k = handleKey(e);
		const spec = k ? props.current.scrub?.[k] : undefined;
		if (!k || !spec) return;
		const v0 = store.getState().vals[k] as number;
		startScrubDrag(e, e.currentTarget, { get: () => store.getState().vals[k] as number, set: (v) => store.getState().set(k, v) }, (dx) => scrubValue(v0, dx, spec));
	};

	const onKeyDown = (e: React.KeyboardEvent) => {
		const k = handleKey(e);
		const spec = k ? props.current.scrub?.[k] : undefined;
		if (!k || !spec) return;
		const v = keyStep(store.getState().vals[k] as number, e.key, spec);
		if (v === null) return;
		e.preventDefault();
		// An indirect change (Ruling U): the scene eases to the new value, the number jumps.
		store.getState().set(k, v, 'ease');
	};

	return (
		<div
			ref={root}
			role="group"
			className={`lab-formula not-content${p.block ? ' is-block' : ''}`}
			aria-label={p.label}
			onPointerOver={(e) => {
				const t = term(e);
				if (t) hotOn(p.lesson, t);
			}}
			onPointerOut={(e) => {
				// Moving between nested KaTeX spans of the same term fires pointerout; clear only when leaving the
				// term, and only write when something is lit (pointerout fires constantly over plain spans).
				const next = (e.relatedTarget as HTMLElement | null)?.closest?.('[data-t]');
				if (!next && store.getState().hot.size > 0) store.getState().setHot([]);
			}}
			onFocus={(e) => {
				const t = term(e);
				if (t) hotOn(p.lesson, t);
			}}
			onPointerDown={onPointerDown}
			onKeyDown={onKeyDown}
			dangerouslySetInnerHTML={{ __html: html }}
		/>
	);
}
