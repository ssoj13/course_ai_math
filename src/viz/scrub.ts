/**
 * Pure scrub math shared by every number a reader can drag, type or arrow-key (Formula, Num, N-panel fields): values
 * snap to `step` multiples from `min` and clamp to [min, max]. One rule set, so a value reached by dragging, by the
 * keyboard or by typing lands on the same grid.
 */
import { fixed } from './fmt';

export interface ScrubSpec {
	min: number;
	max: number;
	step: number;
}

/** Decimal places of a step (0.1 -> 1, 0.25 -> 2, 5 -> 0). */
function decimals(step: number): number {
	const s = String(step);
	const e = /e-(\d+)$/.exec(s);
	if (e) return Number(e[1]);
	const dot = s.indexOf('.');
	return dot < 0 ? 0 : s.length - dot - 1;
}

/** `v` on the step grid from `min`, clamped; re-rounded to the step's decimals so 0.1 steps never print 0.30000000000000004. */
export function snapTo(v: number, s: ScrubSpec): number {
	const n = Math.round((v - s.min) / s.step);
	const out = Number((s.min + n * s.step).toFixed(decimals(s.step)));
	return Math.min(s.max, Math.max(s.min, out)) + 0;
}

/** Value after dragging `dxPx` pixels from `start`; `pxPerStep` pixels move one step. */
export function scrubValue(start: number, dxPx: number, spec: ScrubSpec, pxPerStep = 4): number {
	return snapTo(start + Math.round(dxPx / pxPerStep) * spec.step, spec);
}

const KEY_DELTA: Record<string, number> = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 10, PageDown: -10 };

/** Next value for a slider key (arrows +-step, PageUp/Down x10), or null when the key is not a scrub key. */
export function keyStep(v: number, key: string, spec: ScrubSpec): number | null {
	const d = KEY_DELTA[key];
	return d === undefined ? null : snapTo(v + d * spec.step, spec);
}

// A decimal number: optional sign, digits with an optional fraction (or a bare fraction), optional exponent.
const NUM = /^[+-]?(\d+(\.\d*)?|\.\d+)(e[+-]?\d+)?$/i;

/**
 * A typed field value: accepts a leading `+`, the true minus sign (U+2212, which the fields print), a decimal comma
 * and a trailing degree sign; anything else that is not a finite decimal number is rejected with a reason. Valid
 * values are snapped to the step and clamped (out-of-range input clamps, it is not an error).
 */
export function parseField(text: string, spec: ScrubSpec): { ok: true; v: number } | { ok: false; err: string } {
	const t = text.trim().replace(/−/g, '-').replace(',', '.').replace(/°$/, '').trim();
	if (!NUM.test(t)) return { ok: false, err: t ? `"${text.trim()}" is not a number` : 'enter a number' };
	const v = Number(t);
	if (!Number.isFinite(v)) return { ok: false, err: `"${text.trim()}" is too large` };
	return { ok: true, v: snapTo(v, spec) };
}

/**
 * Ctrl-snap of a dragged value: the nearest of `snap` targets (e.g. the magnet angles 0, 30, 45, ...), or the nearest
 * multiple of an increment counted from `min` (e.g. lengths to 0.5); clamped to the spec's range.
 */
export function ctrlSnap(v: number, snap: number | readonly number[], spec: ScrubSpec): number {
	let out: number;
	if (typeof snap === 'number') out = snapTo(v, { ...spec, step: snap });
	else {
		if (!snap.length) throw new Error('ctrlSnap needs at least one target');
		out = snap.reduce((best, t) => (Math.abs(t - v) < Math.abs(best - v) ? t : best));
	}
	return Math.min(spec.max, Math.max(spec.min, out));
}

/**
 * Field text for `v` on a `step` grid: the step's decimals, at least 2 for fractional steps (so 3 reads "3.00" like a
 * DCC field), one more decimal for a value between grid steps (a Shift-fine drag), a true minus sign, never "-0.00".
 */
export function fmtField(v: number, step: number): string {
	const off = Math.abs(v / step - Math.round(v / step)) > 1e-6;
	const d = Math.max(decimals(off ? step / 10 : step), step < 1 ? 2 : 0);
	return fixed(v, d).replace('-', '−');
}
