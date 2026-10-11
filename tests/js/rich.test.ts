import { expect, test } from 'bun:test';
import { isValidElement } from 'react';
import { plain, rich } from '../../src/viz/rich';

test('rich renders **bold** and *em* as elements and leaves the rest as text', () => {
	const nodes = rich('the dot product **a**·**b** gets *added*; |**a** − **b**|²');
	const shape = nodes.map((n) => (isValidElement<{ children: string }>(n) ? `${n.type}:${n.props.children}` : n));
	expect(shape).toEqual(['the dot product ', 'strong:a', '·', 'strong:b', ' gets ', 'em:added', '; |', 'strong:a', ' − ', 'strong:b', '|²']);
});

test('plain strips the markers; text without markup passes through untouched', () => {
	expect(plain('|**a** − **b**|² and *added*')).toBe('|a − b|² and added');
	expect(rich('a × (b cos θ) = 6.00')).toEqual(['a × (b cos θ) = 6.00']);
	expect(rich('')).toEqual([]);
});
