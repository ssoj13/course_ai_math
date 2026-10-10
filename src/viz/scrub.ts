/** Pure scrub math shared by Formula, Num and scene handles: values snap to `step` multiples from `min`. */
export interface ScrubSpec {
	min: number;
	max: number;
	step: number;
}

const snap = (v: number, s: ScrubSpec): number => {
	const n = Math.round((v - s.min) / s.step);
	const out = s.min + n * s.step;
	// Re-round to the step's decimals so 0.1 steps never print 0.30000000000000004.
	const dec = Math.max(0, -Math.floor(Math.log10(s.step)));
	return Math.min(s.max, Math.max(s.min, Number(out.toFixed(dec))));
};

/** Value after dragging `dxPx` pixels from `start`; `pxPerStep` pixels move one step. */
export function scrubValue(start: number, dxPx: number, spec: ScrubSpec, pxPerStep = 4): number {
	return snap(start + Math.round(dxPx / pxPerStep) * spec.step, spec);
}

const KEY_DELTA: Record<string, number> = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 10, PageDown: -10 };

/** Next value for a slider key (arrows +-step, PageUp/Down x10), or null when the key is not a scrub key. */
export function keyStep(v: number, key: string, spec: ScrubSpec): number | null {
	const d = KEY_DELTA[key];
	return d === undefined ? null : snap(v + d * spec.step, spec);
}
