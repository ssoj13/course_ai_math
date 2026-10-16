import { expect, test } from 'bun:test';
import { classify } from '../../scripts/size-report.ts';

test('chunks are classified by content, not by the name Vite gave them', () => {
	expect(classify('ViewportCard.abc.js', Buffer.from('class WebGLRenderer{}'))).toBe('renderer');
	expect(classify('elk.worker-xyz.js', Buffer.from('layout()'))).toBe('lazy-map');
	expect(classify('TermMap.impl.js', Buffer.from('className="react-flow__node"'))).toBe('lazy-map');
	expect(classify('client.js', Buffer.from('hydrate()'))).toBe('other');
	// A content hash that happens to contain "Elk" must not make a chunk part of the map.
	expect(classify('Point.BEuP7Elk.js', Buffer.from('export const p=1'))).toBe('other');
});
