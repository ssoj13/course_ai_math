/**
 * Island entry for dev/engine.mdx. The server evaluates this module (see ../island.tsx), so it holds only what
 * is cheap and SSR-safe: the lesson store init, DOM-only controls (server-rendered via client:load, so they take
 * their space before hydration) and lazy wrappers for the three/drei scenes.
 */
import { dot, type V } from '../geom';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';
import { lessonStore, useVal } from '../store';

// a, b: 2D vectors of the drag demo; u, v: 3D vectors (y-up); p: a draggable point.
const S = lessonStore('demo', { x: 0, theta: 60, a: [3, 1], b: [1, 2], p: [-2, -1], u: [2, 1, 1], v: [-1, 2, 1.5] });

// Hoisted: Formula memoizes on these identities.
const THETA = { theta: { min: 0, max: 180, step: 1 } };
const FMT = { theta: (v: number) => `${v}^\\circ` };

export const DemoBoxA = lazyIsland(() => import('./engine.impl').then((m) => m.DemoBoxA));
export const DemoBoxB = lazyIsland(() => import('./engine.impl').then((m) => m.DemoBoxB));
export const DemoBoxC = lazyIsland(() => import('./engine.impl').then((m) => m.DemoBoxC));
export const DemoVec2D = lazyIsland(() => import('./engine.impl').then((m) => m.DemoVec2D));
export const DemoVec3D = lazyIsland(() => import('./engine.impl').then((m) => m.DemoVec3D));
export const DemoArea2D = lazyIsland(() => import('./engine.impl').then((m) => m.DemoArea2D));
export const DemoPar2D = lazyIsland(() => import('./engine.impl').then((m) => m.DemoPar2D));

export function DemoSlider(): React.JSX.Element {
	const x = useVal('demo', 'x', 0);
	return (
		<label className="not-content lab-demo-slider">
			x = <output>{x.toFixed(2)}</output>{' '}
			<input type="range" min={-2} max={2} step={0.05} value={x} onChange={(e) => S.getState().set('x', Number(e.currentTarget.value))} />
		</label>
	);
}

/** DOM-only linked formula (no three/drei), so it lives in the light entry and is server-rendered. */
export function DemoFormula(): React.JSX.Element {
	return <Formula lesson="demo" tex={String.raw`\t{ang}{\theta} = \s{theta}`} scrub={THETA} fmt={FMT} block label="Angle theta in degrees; drag or use arrow keys" />;
}

// Hoisted like THETA: a derived value computed from the live a and b (Task 3 `Derived`).
const DOT = { dot: (v: Record<string, unknown>) => dot(v.a as V, v.b as V) };

/** a.b as a live read-only value: dragging a or b in any stage updates it. */
export function DemoDotFormula(): React.JSX.Element {
	return <Formula lesson="demo" tex={String.raw`\t{dot}{a\cdot b} = \s{dot}`} derived={DOT} block label="Dot product of a and b" />;
}

const fmtV = (v: readonly number[]) => `(${v.map((x) => x.toFixed(1)).join(', ')})`;

/** Live readout of the dragged vectors: shows that scene drags and keyboard moves write the shared store. */
export function DemoVecReadout(): React.JSX.Element {
	const a = useVal<readonly number[]>('demo', 'a', [0, 0]);
	const b = useVal<readonly number[]>('demo', 'b', [0, 0]);
	const u = useVal<readonly number[]>('demo', 'u', [0, 0, 0]);
	const v = useVal<readonly number[]>('demo', 'v', [0, 0, 0]);
	const pt = useVal<readonly number[]>('demo', 'p', [0, 0]);
	return (
		<p className="not-content lab-demo-readout">
			a = <output data-k="a">{fmtV(a)}</output>, b = <output data-k="b">{fmtV(b)}</output>, u = <output data-k="u">{fmtV(u)}</output>, v ={' '}
			<output data-k="v">{fmtV(v)}</output>, P = <output data-k="p">{fmtV(pt)}</output>
		</p>
	);
}
