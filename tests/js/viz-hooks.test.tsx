import { expect, test } from 'bun:test';
import { renderToString } from 'react-dom/server';
import { useHot, useStep, useVal } from '../../src/viz/store';

// Renders the hooks through react-dom/server, so the test runs under plain bun without a DOM.
// zustand's useStore reads getInitialState() as the server snapshot, so values written after store creation
// are not visible to renderToString. Islands are client:only, so only the fallback path matters on the server.
function Probe({ lesson }: { lesson: string }) {
	const missing = useVal(lesson, 'missing', [0, 0]);
	const set = useVal(lesson, 'set', 5);
	const hot = useHot(lesson, 'norm');
	const step = useStep(lesson, 'derive');
	return <p>{JSON.stringify([missing, set, hot, step])}</p>;
}

test('hooks return fallbacks and defaults for an untouched lesson', () => {
	expect(renderToString(<Probe lesson="hooks-empty" />)).toBe('<p>[[0,0],5,false,0]</p>');
});

test('the fallback literal does not change the rendered output across renders', () => {
	const first = renderToString(<Probe lesson="hooks-stable" />);
	expect(renderToString(<Probe lesson="hooks-stable" />)).toBe(first);
});
