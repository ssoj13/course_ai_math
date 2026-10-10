/**
 * Pointer hit-test for draggable handles, in screen pixels. Pure: only a hit may start a drag
 * (and lock touch scrolling); a miss returns null so the browser keeps scrolling the page.
 */
export interface Handle {
	id: string;
	at: [number, number];
}

/** Id of the nearest handle within `radiusPx` (inclusive) of the pointer; the first wins an exact tie. */
export function hitHandle(pointerPx: [number, number], handlesPx: readonly Handle[], radiusPx = 14): string | null {
	let best: string | null = null;
	let bestD = Infinity;
	for (const h of handlesPx) {
		const d = Math.hypot(h.at[0] - pointerPx[0], h.at[1] - pointerPx[1]);
		if (d <= radiusPx && d < bestD) {
			best = h.id;
			bestD = d;
		}
	}
	return best;
}
