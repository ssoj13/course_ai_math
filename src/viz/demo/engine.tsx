/**
 * Island entry for dev/engine.mdx. The server evaluates this module (see ../island.tsx), so it holds only what
 * is cheap and SSR-safe: the lesson store init, DOM-only controls (server-rendered via client:load, so they take
 * their space before hydration) and lazy wrappers for the three/drei scenes.
 */
import { lazyIsland } from '../island';
import { lessonStore, useVal } from '../store';

const S = lessonStore('demo', { x: 0 });

export const DemoBoxA = lazyIsland(() => import('./engine.impl').then((m) => m.DemoBoxA));
export const DemoBoxB = lazyIsland(() => import('./engine.impl').then((m) => m.DemoBoxB));
export const DemoBoxC = lazyIsland(() => import('./engine.impl').then((m) => m.DemoBoxC));

export function DemoSlider(): React.JSX.Element {
	const x = useVal('demo', 'x', 0);
	return (
		<label className="not-content lab-demo-slider">
			x = <output>{x.toFixed(2)}</output>{' '}
			<input type="range" min={-2} max={2} step={0.05} value={x} onChange={(e) => S.getState().set('x', Number(e.currentTarget.value))} />
		</label>
	);
}
