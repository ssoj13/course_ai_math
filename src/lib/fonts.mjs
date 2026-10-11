// @ts-check
/**
 * Font variants for Astro's Fonts API (`fonts` in astro.config.mjs, local provider) read from the @font-face rules
 * that @fontsource ships in each package's CSS. The files stay in node_modules (no network at build or run time);
 * Astro copies them, writes the @font-face rules with the same unicode-range subsets (a page downloads only the
 * subsets it renders) and generates metric-matched fallback faces, so the swap from the fallback to the web font
 * does not move text (measured: plain @fontsource CSS shifted the sidebar on load, CLS 0.0044).
 */
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);

/**
 * @typedef {{ weight: string, style: 'normal' | 'italic', src: [string], unicodeRange: [string, ...string[]] }} Variant
 */

/**
 * Local-provider variants of every @font-face in the given CSS files of `pkg`, e.g.
 * `fontsource('@fontsource-variable/inter', 'opsz.css')`.
 * @param {string} pkg npm package name
 * @param {...string} files CSS files of the package (one per axis/style set)
 * @returns {[Variant, ...Variant[]]}
 */
export function fontsource(pkg, ...files) {
	const out = files.flatMap((css) => faces(pkg, css));
	if (!out.length) throw new Error(`${pkg}: no @font-face rules in ${files.join(', ')}`);
	return /** @type {[Variant, ...Variant[]]} */ (out);
}

/**
 * @param {string} pkg
 * @param {string} css
 * @returns {Variant[]}
 */
function faces(pkg, css) {
	const text = readFileSync(require.resolve(`${pkg}/${css}`), 'utf8');
	const rules = [...text.matchAll(/@font-face\s*{([^}]*)}/g)].map((m) => m[1]);
	if (!rules.length) throw new Error(`${pkg}/${css}: no @font-face rules`);
	return rules.map((f) => {
		const prop = (/** @type {string} */ name) => {
			const v = new RegExp(`${name}:\\s*([^;]+);`).exec(f)?.[1].trim();
			if (!v) throw new Error(`${pkg}/${css}: @font-face without ${name}`);
			return v;
		};
		const file = /url\(\.\/files\/([^)]+\.woff2)\)/.exec(prop('src'))?.[1];
		if (!file) throw new Error(`${pkg}/${css}: no woff2 source`);
		const style = prop('font-style');
		if (style !== 'normal' && style !== 'italic') throw new Error(`${pkg}/${css}: unexpected font-style ${style}`);
		const ranges = prop('unicode-range').split(',').map((r) => r.trim());
		return {
			weight: prop('font-weight'),
			style,
			src: /** @type {[string]} */ ([`${pkg}/files/${file}`]),
			unicodeRange: /** @type {[string, ...string[]]} */ (ranges),
		};
	});
}
