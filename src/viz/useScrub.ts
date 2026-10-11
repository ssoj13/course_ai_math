/**
 * The one pointer-drag loop for scrubbable numbers (Formula, Num and the N-panel fields). Capture and listeners go on
 * an element that outlives value re-renders (Formula's root div, Num's own span, the field), because Formula re-sets
 * innerHTML on every value change and removes the span that was pressed. Updates are throttled to animation frames;
 * `onValue` also gets the latest pointer event, so callers can read modifier keys (Ctrl snap, Shift fine), and
 * `onEnd` reports the final travel at release (a field that barely moved treats the press as a click).
 */
export function startScrubDrag(
	e: React.PointerEvent,
	captureEl: HTMLElement,
	onValue: (dxPx: number, ev: PointerEvent) => void,
	onEnd?: (dxPx: number) => void,
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
	const end = () => {
		cancelAnimationFrame(raf);
		// A frame still pending at release must not swallow the final position.
		if (pending) onValue(pending.clientX - x0, pending);
		pending = null;
		ctl.abort();
		onEnd?.(dx);
	};
	captureEl.addEventListener('pointerup', end, { signal: ctl.signal });
	captureEl.addEventListener('pointercancel', end, { signal: ctl.signal });
}
