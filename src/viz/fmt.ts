/**
 * The one decimal formatter behind every printed number (readouts, N-panel fields, formulas, tables, hints).
 *
 * Why: lesson pages are server-rendered by Bun (JavaScriptCore) and hydrated in the reader's browser (often V8), and
 * the engines' Math.cos/sin/pow/exp may differ in the last bit. A value that sits on a display boundary, such as
 * 0.425 computed as 0.42500000000000004 on one side and 0.42499999999999993 on the other, then prints "0.43" against
 * "0.42" and React rejects the hydration (#418). So a value is first settled on an absolute grid six decimals finer
 * than the display (far coarser than last-bit noise, far finer than anything shown); both noisy variants land on the
 * same double, and `toFixed`/`Math.round`, which are exact in every engine, then print the same text. Negative zero
 * (and anything that rounds to zero) prints unsigned. The minus is ASCII: callers that want U+2212 replace it.
 */

/** Extra decimals of the settling grid beyond the displayed ones. */
const GUARD = 6;

/** `x` rounded to `d + GUARD` decimals, -0 made +0; non-finite and huge values (already coarser) pass through. */
export function settle(x: number, d: number): number {
	if (!Number.isFinite(x)) return x;
	const k = 10 ** (d + GUARD);
	const y = x * k;
	if (Math.abs(y) >= Number.MAX_SAFE_INTEGER) return x + 0;
	return Math.round(y) / k + 0;
}

/** Exactly `d` decimals ("0.50", "-1.25"), never "-0.00". */
export function fixed(x: number, d: number): string {
	const s = settle(x, d).toFixed(d);
	return Number(s) === 0 ? (0).toFixed(d) : s;
}

/** Up to `d` decimals with trailing zeros dropped ("2.5", "4", "0.667"). */
export function trim(x: number, d: number): string {
	return String(Number(fixed(x, d)));
}

/** Nearest integer ("8", never "-0"). */
export const int = (x: number): string => trim(x, 0);
