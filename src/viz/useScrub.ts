/**
 * The one pointer-drag loop for scrubbable numbers (Formula, Num and the N-panel fields). Capture and listeners go on
 * an element that outlives value re-renders (Formula's root div, Num's own span, the field), because Formula re-sets
 * innerHTML on every value change and removes the span that was pressed. Updates are throttled to animation frames;
 * `onValue` also gets the latest pointer event, so callers can read modifier keys (Ctrl snap, Shift fine), and
 * `onEnd` reports the final travel and whether the gesture was cancelled: a `pointercancel` (e.g. the browser took a
 * vertical touch swipe as a page scroll, `touch-action: pan-y`) is never a release, so a field must not treat it as a
 * click, and should restore the value it started from.
 */
export function startScrubDrag(
	e: React.PointerEvent,
	captureEl: HTMLElement,
	onValue: (dxPx: number, ev: PointerEvent) => void,
	onEnd?: (dxPx: number, cancelled: boolean) => void,
): void {
	e.preventDefault();
	captureEl.setPointerCapture(e.pointerId);
	const x0 = e.clientX;
	let raf = 0;
	let pending: PointerEvent | null = null;
	let dx = 0;
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
				if (p) onValue(p.clientX - x0, p);
			});
		},
		{ signal: ctl.signal },
	);
	const end = (cancelled: boolean) => {
		cancelAnimationFrame(raf);
		// A frame still pending at release must not swallow the final position (a cancelled gesture applies nothing).
		if (pending && !cancelled) onValue(pending.clientX - x0, pending);
		pending = null;
		ctl.abort();
		onEnd?.(dx, cancelled);
	};
	captureEl.addEventListener('pointerup', () => end(false), { signal: ctl.signal });
	captureEl.addEventListener('pointercancel', () => end(true), { signal: ctl.signal });
}
