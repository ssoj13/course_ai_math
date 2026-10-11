/**
 * Island entry for dev/engine.mdx. The server evaluates this module (see ../island.tsx), so it holds only what
 * is cheap and SSR-safe: the lesson store init, DOM-only views (server-rendered via client:load, so they take
 * their space before hydration) and lazy wrappers for the three/drei scenes (three ViewportCards, engine.impl.tsx).
 */
import { dot, type V } from '../geom';
import { lessonStore, useVal } from '../store';
import { lazyIsland } from '../island';
import { Formula } from '../Formula';

// a, b: 2D vectors; p: a draggable point; r: the angle arc radius (an N-panel number field); u, v: 3D vectors (y-up).
lessonStore('demo', { a: [3, 1], b: [1, 2], p: [-2, -1], r: 0.6, u: [2, 1, 1], v: [-1, 2, 1.5] });

export const Demo2D = lazyIsland(() => import('./engine.impl').then((m) => m.Demo2D));
export const Demo3D = lazyIsland(() => import('./engine.impl').then((m) => m.Demo3D));
export const DemoCosines = lazyIsland(() => import('./engine.impl').then((m) => m.DemoCosines));

// Hoisted: Formula memoizes on these identities. A derived value computed from the live a and b (Task 3 `Derived`).
const DOT = { dot: (v: Record<string, unknown>) => dot(v.a as V, v.b as V) };

/** a.b as a live read-only value with linked terms: hover `a` here and the arrow and its N-panel row light up. */
export function DemoDotFormula(): React.JSX.Element {
	return <Formula lesson="demo" tex={String.raw`\t{a}{a}\cdot\t{b}{b} = \t{dot}{\s{dot}}`} derived={DOT} block label="Dot product of a and b" />;
}

const fmtV = (v: readonly number[]) => `(${v.map((x) => x.toFixed(1)).join(', ')})`;

/** Live readout of the dragged vectors: shows that scene drags, N-panel fields and keyboard moves write the shared store. */
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
