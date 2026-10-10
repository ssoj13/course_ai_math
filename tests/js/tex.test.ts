import { expect, test } from 'bun:test';
import { renderLinked, renderStaticMath } from '../../src/viz/tex';

// KaTeX emits one span per digit, so value assertions compare the visible text, not the markup.
const text = (html: string) => html.replace(/<[^>]*>/g, '');

test('term macro becomes a data-t span', () => {
	const html = renderLinked(String.raw`\t{dot}{a\cdot b} = \t{norm-a}{|a|}`, {});
	expect(html).toContain('data-t="dot"');
	expect(html).toContain('data-t="norm-a"');
	expect(html).not.toContain('katex-error');
});

test('scrub macro inserts the formatted current value with a data-s handle', () => {
	const html = renderLinked(String.raw`\theta = \s{theta}`, { theta: 60 }, { theta: (v) => `${v}^\\circ` });
	expect(html).toContain('data-s="theta"');
	expect(text(html)).toContain('60');
});

test('only htmlData is trusted (no href/style injection)', () => {
	// KaTeX does not throw for an untrusted command; it renders it as unsupported text. Assert no link is emitted.
	const html = renderLinked(String.raw`\href{javascript:alert(1)}{x} \htmlStyle{color:red}{y}`, {});
	expect(html).not.toContain('href=');
	expect(html).not.toContain('javascript:');
	expect(html).not.toContain('color:red');
});

test('derived values are computed before expansion and shown read-only', () => {
	const html = renderLinked(String.raw`a\cdot b = \s{dot}`, { a: [1, 2], b: [3, 4] }, undefined, false, {
		dot: (v) => (v.a as number[])[0] * (v.b as number[])[0] + (v.a as number[])[1] * (v.b as number[])[1],
	});
	expect(html).toContain('data-s="dot"');
	expect(text(html)).toContain('11');
});

test('static math', () => {
	expect(renderStaticMath(String.raw`\sum_i a_i b_i`, true)).toContain('katex-display');
});

test('linked math shares the non-ASCII guard, including text produced by fmt', () => {
	expect(() => renderLinked('θ = 1', {})).toThrow(/non-ASCII/);
	expect(() => renderLinked(String.raw`\s{t}`, { t: 1 }, { t: () => '1°' })).toThrow(/non-ASCII/);
});

test('a scrub key without a numeric value is an error, not silent text', () => {
	expect(() => renderLinked(String.raw`\s{missing}`, {})).toThrow(/missing/);
});

test('texText turns short TeX values into aria text', async () => {
	const { texText } = await import('../../src/viz/tex');
	expect(texText(String.raw`60^\circ`)).toBe('60°');
	expect(texText(String.raw`\pi/2`)).toBe('π/2');
	expect(texText('1.5')).toBe('1.5');
});
