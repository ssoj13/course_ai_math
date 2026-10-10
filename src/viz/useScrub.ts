/**
 * The one pointer-drag loop for scrubbable numbers (Formula and Num). Capture and listeners go on an element that
 * outlives value re-renders (Formula's root div, Num's own span), because Formula re-sets innerHTML on every value
 * change and removes the span that was pressed. Updates are throttled to animation frames.
 */
export function startScrubDrag(e: React.PointerEvent, captureEl: HTMLElement, onValue: (dxPx: number) => void): void {
	e.preventDefault();
	captureEl.setPointerCapture(e.pointerId);
	const x0 = e.clientX;
	let raf = 0;
	let pending: number | null = null;
	const ctl = new AbortController();
	captureEl.addEventListener(
		'pointermove',
		(ev) => {
			pending = ev.clientX - x0;
			cancelAnimationFrame(raf);
			raf = requestAnimationFrame(() => {
				const dx = pending;
				pending = null;
				if (dx !== null) onValue(dx);
			});
		},
		{ signal: ctl.signal },
	);
	const end = () => {
		cancelAnimationFrame(raf);
		// A frame still pending at release must not swallow the final position.
		if (pending !== null) onValue(pending);
		ctl.abort();
	};
	captureEl.addEventListener('pointerup', end, { signal: ctl.signal });
	captureEl.addEventListener('pointercancel', end, { signal: ctl.signal });
}
