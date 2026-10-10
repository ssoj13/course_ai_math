/**
 * Spike A demo scenes (loaded lazily via engine.tsx): three stages in three separate React roots, all on lesson
 * `demo`. If the store or drei's View tunnel were duplicated per island, the boxes would not follow the slider
 * (engine.tsx, another chunk) or would not appear in the shared canvas.
 * Task 6 demo below: every scene primitive on the shared `a`, `b`, `u`, `v` values.
 */
import { dot, norm, project, scale, signedArea2, sub, type V } from '../geom';
import { usePalette, type Role } from '../palette';
import { AngleArc } from '../scene/AngleArc';
import { AreaTile } from '../scene/AreaTile';
import { Arrow } from '../scene/Arrow';
import { Grid } from '../scene/Grid';
import { Label } from '../scene/Label';
import { Parallelogram } from '../scene/Parallelogram';
import { Point } from '../scene/Point';
import { RightAngle } from '../scene/RightAngle';
import { Segment } from '../scene/Segment';
import { Stage } from '../scene/Stage';
import { useVal } from '../store';

function DemoBox({ mode, role }: { mode: '2d' | '3d'; role: Role }): React.JSX.Element {
	const x = useVal('demo', 'x', 0);
	const pal = usePalette();
	return (
		<Stage lesson="demo" mode={mode} extent={3}>
			<mesh position={[x, 0, 0]}>
				<boxGeometry args={[1, 1, 1]} />
				<meshBasicMaterial color={pal[role]} />
			</mesh>
			{mode === '3d' && <gridHelper key={pal.grid + pal.gridMinor} args={[6, 6, pal.grid, pal.gridMinor]} />}
		</Stage>
	);
}

export const DemoBoxA = () => <DemoBox mode="2d" role="vecA" />;
export const DemoBoxB = () => <DemoBox mode="2d" role="vecB" />;
export const DemoBoxC = () => <DemoBox mode="3d" role="vecC" />;

const O2: V = [0, 0];
const O3: V = [0, 0, 0];
const UP: V = [0, 1, 0];
const TIP: readonly [number, number] = [10, -12];
// Label text is DOM, not KaTeX, so plain Unicode is right here.
const deg = (d: number) => `θ ${d.toFixed(1)}°`;
// A true minus sign (U+2212), matching the one in the bar label.
const num = (x: number) => x.toFixed(1).replace('-', '−');

const use2 = (k: string) => useVal<readonly number[]>('demo', k, O2);

/**
 * Shadow of b on the line of a (the projection), the drop from b's tip and the right-angle mark at the foot.
 * Derived points are computed here from the store and passed as literals; projecting onto a zero `a` is undefined,
 * so nothing is drawn then.
 */
function Projection(): React.JSX.Element | null {
	const a = use2('a');
	const b = use2('b');
	if (norm(a) === 0) return null;
	const foot = project(b, a);
	return (
		<>
			<Segment lesson="demo" id="proj-drop" a={b} b={foot} dashed role="muted" />
			<Segment lesson="demo" id="proj" a={O2} b={foot} role="vecC" />
			{/* The mark sits in the corner toward the origin: along -a when the shadow points along a, else along a. */}
			<RightAngle lesson="demo" id="proj-drop" at={foot} u={dot(a, b) >= 0 ? scale(a, -1) : a} v={sub(b, foot)} />
		</>
	);
}

/** The triangle O, a, b may be dragged either way round; flip keeps its tiles outside it (see AreaTile). */
const useFlip = () => signedArea2(use2('a'), use2('b')) < 0;

function TileA(): React.JSX.Element {
	return <AreaTile lesson="demo" id="sq-a" a={O2} b="a" role="vecA" flip={useFlip()} label={(s) => `|a|² ${num(s)}`} />;
}

/** Task 6: grid, draggable a and b, the angle between them, b's projection onto a and the square on a. */
export const DemoVec2D = () => (
	<Stage lesson="demo" mode="2d" extent={4}>
		<Grid extent={4} step={1} minor={0.5} />
		<TileA />
		<Segment lesson="demo" id="ab" a="a" b="b" dashed role="muted" />
		<Projection />
		<AngleArc lesson="demo" id="theta" at={O2} from="a" to="b" r={0.8} label={deg} />
		<Arrow lesson="demo" id="a" to="a" role="vecA" legs />
		<Arrow lesson="demo" id="b" to="b" role="vecB" />
		<Label lesson="demo" id="a" at="a" role="vecA" offset={TIP}>a</Label>
		<Label lesson="demo" id="b" at="b" role="vecB" offset={TIP}>b</Label>
		<Point lesson="demo" id="p" at="p" label="P" role="vecC" />
	</Stage>
);

function CosineTiles(): React.JSX.Element {
	const flip = useFlip();
	return (
		<>
			<AreaTile lesson="demo" id="sq-a" a={O2} b="a" role="vecA" flip={flip} label={'a²'} />
			<AreaTile lesson="demo" id="sq-b" a="b" b={O2} role="vecB" flip={flip} label={'b²'} />
			<AreaTile lesson="demo" id="sq-c" a="a" b="b" role="vecC" flip={flip} label={'c²'} />
		</>
	);
}

const corr = (v: Record<string, unknown>) => -2 * dot(v.a as V, v.b as V);

/**
 * Law of cosines layout: squares on the three sides of the triangle O, a, b, and the correction -2 a.b as a signed
 * bar on a fixed 10-unit base (height = area / 10): it grows down (outward) past 90 degrees, up and hatched below.
 */
export const DemoArea2D = () => (
	<Stage lesson="demo" mode="2d" extent={6}>
		<Grid extent={6} step={1} />
		<CosineTiles />
		<AreaTile lesson="demo" id="corr" a={[-5, -5]} b={[5, -5]} role="muted" signed area={corr} label={(s) => `−2a·b ${num(s)}`} />
		<Arrow lesson="demo" id="a" to="a" role="vecA" />
		<Arrow lesson="demo" id="b" to="b" role="vecB" />
	</Stage>
);

/** Signed area: counter-clockwise a -> b is a plain tint, clockwise adds the hatch; collinear draws nothing. */
export const DemoPar2D = () => (
	<Stage lesson="demo" mode="2d" extent={4}>
		<Grid extent={4} step={1} />
		<Parallelogram lesson="demo" id="par" a="a" b="b" role="vecC" label={(s) => `${num(s)}`} />
		<Arrow lesson="demo" id="a" to="a" role="vecA" />
		<Arrow lesson="demo" id="b" to="b" role="vecB" />
	</Stage>
);

/** 3D: two arrows with legs, their angle (normal = up picks the plane at 180 degrees), their parallelogram, labels. */
export const DemoVec3D = () => (
	<Stage lesson="demo" mode="3d" extent={3}>
		<Grid extent={3} step={1} />
		<Parallelogram lesson="demo" id="par3" a="u" b="v" role="vecC" />
		<AngleArc lesson="demo" id="phi" at={O3} from="u" to="v" r={0.7} normal={UP} label={deg} />
		<Arrow lesson="demo" id="u" to="u" role="vecA" legs />
		<Arrow lesson="demo" id="v" to="v" role="vecB" legs />
		<Label lesson="demo" id="u" at="u" role="vecA" offset={TIP}>u</Label>
		<Label lesson="demo" id="v" at="v" role="vecB" offset={TIP}>v</Label>
	</Stage>
);
