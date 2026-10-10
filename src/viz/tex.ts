import katex, { type KatexOptions } from 'katex';
import type { Val } from './store';

/**
 * The single KaTeX configuration for the whole site (build-time prose math and, later, runtime formulas).
 * - `throwOnError`: a broken formula must fail the build / test, never render as red source text.
 * - `trust`: only `\htmlData` (used to tag scrubbable terms) is allowed; `\href`, `\includegraphics` etc. stay off.
 * - `strict`: raw Unicode (`°`, `θ`) is an error so authors write `^\circ`, `\theta` (fonts and a11y stay predictable);
 *   `htmlExtension` is the one warning we need, since `\htmlData` itself triggers it.
 */
export const OPTS: KatexOptions = {
	throwOnError: true,
	output: 'html',
	trust: (ctx) => ctx.command === '\\htmlData',
	strict: (code) => (code === 'htmlExtension' ? 'ignore' : 'error'),
};

/**
 * Renders plain (non-interactive) math to KaTeX HTML; `block` selects display mode. Throws on invalid TeX.
 * KaTeX's `strict` does not flag `°`, `θ` or `→` (they are in its symbol table), so the no-Unicode rule is enforced here.
 */
export function renderStaticMath(tex: string, block: boolean): string {
	const bad = /[^\x00-\x7f]/.exec(tex);
	if (bad) throw new Error(`non-ASCII "${bad[0]}" in math; write the TeX command (\\circ, \\theta, \\to)`);
	return katex.renderToString(tex, { ...OPTS, displayMode: block });
}

/**
 * Expands the authoring macros before KaTeX sees the source: `\t{id}{` becomes `\htmlData{t=id}{` (a term to
 * hover-link) and `\s{key}` becomes `\htmlData{s=key}{<value>}` (a live, scrubbable value).
 * `\htmlData` rather than `\htmlId`: one term may appear in several formulas on a page and ids must be unique.
 */
export function expand(tex: string, vals: Record<string, Val>, fmt: Record<string, (v: number) => string> = {}): string {
	return tex
		.replace(/\\t\{([\w-]+)\}\{/g, (_, id: string) => `\\htmlData{t=${id}}{`)
		.replace(/\\s\{([\w-]+)\}/g, (_, k: string) => {
			const v = vals[k];
			if (typeof v !== 'number') throw new Error(`Formula: no numeric value for scrub key "${k}"`);
			const shown = fmt[k]?.(v) ?? String(Math.round(v * 100) / 100);
			return `\\htmlData{s=${k}}{${shown}}`;
		});
}

/** Derived read-only values (e.g. a dot product), computed from the stored values and shown with `\s{key}`. */
export type Derived = Record<string, (vals: Record<string, Val>) => number>;

/**
 * Renders a linked formula: macros expanded, derived values added, then the same OPTS and non-ASCII guard as
 * static math (the guard runs on the expanded source, so a `fmt` that returns a raw degree sign is caught too).
 */
export function renderLinked(
	tex: string,
	vals: Record<string, Val>,
	fmt?: Record<string, (v: number) => string>,
	block = false,
	derived: Derived = {},
): string {
	const all = { ...vals, ...Object.fromEntries(Object.entries(derived).map(([k, f]) => [k, f(vals)])) };
	return renderStaticMath(expand(tex, all, fmt), block);
}
