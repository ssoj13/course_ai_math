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

test('raw Unicode symbols are rejected', () => {
	expect(() => renderStaticMath('θ', false)).toThrow();
	expect(() => renderStaticMath('x → y', false)).toThrow();
});

test('only \htmlData is trusted', () => {
	expect(renderStaticMath(String.raw`\htmlData{t=a}{x}`, false)).toContain('data-t="a"');
	for (const tex of [String.raw`\htmlStyle{color:red}{y}`, String.raw`\href{javascript:alert(1)}{x}`]) {
		let out = '';
		try {
			out = renderStaticMath(tex, false);
		} catch {
			// Rejecting outright is also safe.
		}
		expect(out).not.toContain('color:red');
		expect(out).not.toContain('href=');
		expect(out).not.toContain('javascript:');
	}
});
