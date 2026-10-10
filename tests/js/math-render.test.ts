import { expect, test } from 'bun:test';
import { renderStaticMath } from '../../src/viz/tex';

test('inline and block', () => {
	expect(renderStaticMath(String.raw`\sum_i a_i b_i`, false)).toContain('class="katex"');
	expect(renderStaticMath(String.raw`\|a\| < b`, true)).toContain('katex-display');
});

test('invalid TeX throws so the build fails loudly', () => {
	expect(() => renderStaticMath(String.raw`\frac{1}{`, false)).toThrow();
});

test('raw Unicode in math is rejected (write \\circ, \\theta)', () => {
	expect(() => renderStaticMath('60°', false)).toThrow();
});
