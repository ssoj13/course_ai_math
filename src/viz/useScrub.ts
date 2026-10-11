/** The value a scrub reads and writes (a store key in practice). */
export interface ScrubTarget {
	get(): number;
	/** Capture transaction values/provenance before any frame is applied. */
	checkpoint?(): () => void;
	set(v: number): void;
}

/**
 * The one pointer-drag loop for scrubbable numbers (Formula, Num and the N-panel fields). Capture and listeners go on
 * an element that outlives value re-renders (Formula's root div, Num's own span, the field), because Formula re-sets
 * innerHTML on every value change and removes the span that was pressed. Updates are throttled to animation frames.
 *
 * `next(dxPx, ev)` maps the pointer travel to a value (null = no change yet, e.g. inside a touch slop); it gets the
 * latest pointer event, so callers can read modifier keys (Ctrl snap, Shift fine). The loop writes the value, and
 * owns the cancel rule for every caller: a `pointercancel` (e.g. the browser took a vertical touch swipe as a page
 * scroll) is not a release, so no pending frame is applied and the value is restored to what it was at the press.
 * `onEnd` reports the final travel and whether the gesture was cancelled (a cancel is never a click).
 */
export function startScrubDrag(
	e: React.PointerEvent,
	captureEl: HTMLElement,
	target: ScrubTarget,
	next: (dxPx: number, ev: PointerEvent) => number | null,
	onEnd?: (dxPx: number, cancelled: boolean) => void,
): void {
	e.preventDefault();
	captureEl.setPointerCapture(e.pointerId);
	const x0 = e.clientX;
	const v0 = target.get();
	const restore = target.checkpoint?.();
	let raf = 0;
	let pending: PointerEvent | null = null;
	let dx = 0;
	const apply = (p: PointerEvent) => {
		const v = next(p.clientX - x0, p);
		if (v !== null && v !== target.get()) target.set(v);
	};
	const ctl = new AbortController();
	captureEl.addEventListener(
		'pointermove',
		(ev) => {
			pending = ev;
			dx = ev.clientX - x0;
			cancelAnimationFrame(raf);
			raf = requestAnimationFrame(() => {
				const p = pending;
				pending = null;
				if (p) apply(p);
			});
		},
		{ signal: ctl.signal },
	);
	const end = (cancelled: boolean) => {
		cancelAnimationFrame(raf);
		// A frame still pending at release must not swallow the final position.
		if (pending && !cancelled) apply(pending);
		pending = null;
		ctl.abort();
		if (cancelled) {
			if (restore) restore();
			else if (target.get() !== v0) target.set(v0);
		}
		onEnd?.(dx, cancelled);
	};
	captureEl.addEventListener('pointerup', () => end(false), { signal: ctl.signal });
	captureEl.addEventListener('pointercancel', () => end(true), { signal: ctl.signal });
}
