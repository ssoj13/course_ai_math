import katex, { type KatexOptions } from 'katex';
import { trim } from './fmt';
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

/** TeX shown for scrub value `k`: `fmt[k]` or the value rounded to 2 decimals. */
export function showVal(k: string, v: number, fmt?: Record<string, (v: number) => string>): string {
	return fmt?.[k]?.(v) ?? trim(v, 2);
}

const PLAIN: Record<string, string> = { circ: '°', theta: 'θ', pi: 'π', times: '×', cdot: '·', to: '→' };

/**
 * Best-effort plain text for a short TeX value (`60^\circ` gives "60°"), used for aria-valuetext only.
 * It is not math, so Unicode is fine here; unknown commands lose their backslash, braces are dropped.
 */
export function texText(tex: string): string {
	return tex
		.replace(/\^\s*\{?\\circ\}?/g, '°')
		.replace(/\\([a-zA-Z]+)/g, (_, c: string) => PLAIN[c] ?? c)
		.replace(/\\[,;: !]/g, ' ')
		.replace(/[{}]/g, '')
		.trim();
}

/**
 * Expands the authoring macros before KaTeX sees the source: `\t{id}{` becomes `\htmlData{t=id}{` (a term to
 * hover-link) and `\s{key}` becomes `\htmlData{s=key}{<value>}` (a live, scrubbable value).
 * `\htmlData` rather than `\htmlId`: one term may appear in several formulas on a page and ids must be unique.
 *
 * Limits: this is a plain regex pass, not a TeX parser. `\t{id}{` and `\s{key}` are expanded everywhere,
 * including inside `\text{...}` and right after a `\\` line break (`\\s{k}` would become a line break plus
 * a value). Authoring rule: ids and keys match `[\w-]+`, and the two macros are written only in math, never
 * inside `\text`/`\mathrm` arguments or glued to a preceding backslash.
 */
export function expand(tex: string, vals: Record<string, Val>, fmt: Record<string, (v: number) => string> = {}): string {
	return tex
		.replace(/\\t\{([\w-]+)\}\{/g, (_, id: string) => `\\htmlData{t=${id}}{`)
		.replace(/\\s\{([\w-]+)\}/g, (_, k: string) => {
			const v = vals[k];
			if (typeof v !== 'number') throw new Error(`Formula: no numeric value for scrub key "${k}"`);
			return `\\htmlData{s=${k}}{${showVal(k, v, fmt)}}`;
		});
}

/** One line of an `aligned` block: plain TeX, or `[markId, tex]` for a line that a step can hide, dim or strike. */
export type AlignedRow = string | readonly [mark: string, tex: string];

/**
 * Builds a left-aligned multi-line formula. A marked line is wrapped in `\htmlData{m=id}` (dim/struck by `marks`);
 * a line whose mark is `hidden` is left out of the TeX entirely. KaTeX positions aligned lines absolutely inside
 * its vlist, so a line hidden by CSS would keep its height: a timeline at step 1 of 10 showed a 200-400 px hole
 * where the unrevealed derivation was reserved. Revealing a line is a reader action (timeline step), so the
 * layout change it causes carries hadRecentInput and is not counted as a layout shift.
 * Returns '' when no line is visible (Formula then renders no math at all).
 */
export function alignedRows(rows: readonly AlignedRow[], marks?: Readonly<Record<string, { hidden?: boolean }>>): string {
	const shown = rows.filter((r) => typeof r === 'string' || !marks?.[r[0]]?.hidden)
		.map((r) => (typeof r === 'string' ? r : `\\htmlData{m=${r[0]}}{${r[1]}}`));
	return shown.length ? `\\begin{aligned}${shown.map((x) => '&' + x).join('\\\\ ')}\\end{aligned}` : '';
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
