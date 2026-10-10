import katex, { type KatexOptions } from 'katex';

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
	trust: (ctx) => ctx.command === '\htmlData',
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
