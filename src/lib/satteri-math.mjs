/**
 * Satteri mdast plugin: `math` / `inlineMath` nodes (enabled by `features: { math: true }`) become KaTeX HTML at
 * build time, so pages ship no math JS. Errors throw with the file name: a broken formula must fail the build.
 */
import { fileURLToPath } from 'node:url';
import { renderStaticMath } from '../viz/tex.ts';

export default function katexMath() {
	/** @param {boolean} block */
	const render = (block) => (/** @type {{ value: string }} */ node, /** @type {any} */ ctx) => {
		try {
			ctx.replaceNode(node, { type: 'html', value: renderStaticMath(node.value, block) });
		} catch (e) {
			const where = ctx.fileURL ? fileURLToPath(ctx.fileURL) : 'unknown file';
			throw new Error(`${where}: bad math "${node.value}": ${e instanceof Error ? e.message : e}`);
		}
	};
	return { name: 'katex-math', math: render(true), inlineMath: render(false) };
}
