import { t } from '../../i18n/core';
import { localized, useLocale } from '../../i18n/react';
/**
 * Island entry for dev/engine.mdx. The server evaluates this module (see ../island.tsx), so it holds only what
 * is cheap and SSR-safe: the lesson store init, the N-panel rows (MDX passes them to SceneIsland, which reserves the
 * phone chip row from them), DOM-only views (server-rendered via client:load, so they take their space before
 * hydration: formula, readout, Ledger, Challenges) and lazy wrappers for the three/drei scenes (engine.impl.tsx).
 */
import { dot, normalize, scale, type V } from '../geom';
import { lessonStore, useVal, type LessonState } from '../store';
import { lazyIsland } from '../island';
import { Challenge } from '../Challenge';
import { Formula } from '../Formula';
import { Ledger, type LedgerItem } from '../Ledger';
import { SPHERE_R, tri, vec } from './engine.rows';

export { ROWS2, ROWS3, ROWSLAMP, ROWSLC, ROWSPOLAR, ROWSSIGN } from './engine.rows';

// a, b: 2D vectors; p: a draggable point; r: the angle arc radius (an N-panel number field); u, v: 3D vectors (y-up).
// la, lb, lth: the polar triangle (side lengths, angle in degrees); sa, sb: the sign card's vectors; nl_*: the lamp
// (light position, probe on the sphere, raw N·L checkbox as 0/1).
lessonStore('demo', {
	a: [3, 1],
	b: [1, 2],
	p: [-2, -1],
	r: 0.6,
	u: [2, 1, 1],
	v: [-1, 2, 1.5],
	la: 3,
	lb: 4,
	lth: 60,
	sa: [2, 1],
	sb: [1, 2],
	nl_light: [3, 1.5, -1.2],
	nl_pt: scale(normalize([1, 0.6, 0.8]), SPHERE_R),
	nl_raw: 0,
});

export const Demo2D = localized(lazyIsland(() => import('./engine.impl').then((m) => m.Demo2D)));
export const Demo3D = localized(lazyIsland(() => import('./engine.impl').then((m) => m.Demo3D)));
export const DemoCosines = localized(lazyIsland(() => import('./engine.impl').then((m) => m.DemoCosines)));
export const DemoPolar = localized(lazyIsland(() => import('./engine.impl').then((m) => m.DemoPolar)));
export const DemoSign = localized(lazyIsland(() => import('./engine.impl').then((m) => m.DemoSign)));
export const DemoLamp = localized(lazyIsland(() => import('./engine.impl').then((m) => m.DemoLamp)));

// Hoisted: Formula memoizes on these identities. A derived value computed from the live a and b (Task 3 `Derived`).
const DOT = { dot: (v: Record<string, unknown>) => dot(v.a as V, v.b as V) };

/** a.b as a live read-only value with linked terms: hover `a` here and the arrow and its N-panel row light up. */
function DemoDotFormulaContent(): React.JSX.Element {
 const locale = useLocale();
	return <Formula lesson="demo" tex={String.raw`\t{a}{a}\cdot\t{b}{b} = \t{dot}{\s{dot}}`} derived={DOT} block label={t(locale, 'demo.dotLabel')} />;
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

// Same ids as the polar card's N-panel rows, so a bar, its row and the scene light together.
const LEDGER: readonly LedgerItem[] = [
	{ id: 'a2', label: 'a²', role: 'vecA', value: (v) => tri(v).a2 },
	{ id: 'b2', label: 'b²', role: 'vecB', value: (v) => tri(v).b2 },
	{ id: 'corr', label: '−2ab cos θ', role: 'vecC', value: (v) => tri(v).corr },
];
const C2 = { id: 'c2', label: 'c²', value: (v: LessonState['vals']) => tri(v).c2 };

/** LC-1's ledger on the polar triangle: a² and b² stacked, the signed correction, the rule at c². */
function DemoLedgerContent(): React.JSX.Element {
 const locale = useLocale();
	return <Ledger lesson="demo" items={LEDGER} total={C2} scale={3.4} max={64} zeroLabel={t(locale, 'demo.pythagoras')} label={t(locale, 'demo.ledger')} />;
}

/** Two Challenge boxes: one on a value (LC-1), one on the sign card (DP-2). */
function DemoChallengesContent(): React.JSX.Element {
 const locale = useLocale();
	return (
		<>
			<Challenge lesson="demo" n={1} text={t(locale, 'demo.challenge1')} hint={[t(locale, 'demo.hint1')]} check={(v) => v.lth === 90} />
			<Challenge
				lesson="demo"
				n={2}
				text={t(locale, 'demo.challenge2')}
				hint={[t(locale, 'demo.hint2a'), t(locale, 'demo.hint2b')]}
				check={(v) => Math.abs(dot(vec(v, 'sa'), vec(v, 'sb'))) < 1e-9 && dot(vec(v, 'sa'), vec(v, 'sa')) > 0 && dot(vec(v, 'sb'), vec(v, 'sb')) > 0}
			/>
		</>
	);
}

export const DemoDotFormula = localized(DemoDotFormulaContent);

export const DemoLedger = localized(DemoLedgerContent);

export const DemoChallenges = localized(DemoChallengesContent);
