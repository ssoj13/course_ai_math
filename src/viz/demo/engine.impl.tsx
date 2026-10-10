/**
 * Spike A demo scenes (loaded lazily via engine.tsx): three stages in three separate React roots, all on lesson
 * `demo`. If the store or drei's View tunnel were duplicated per island, the boxes would not follow the slider
 * (engine.tsx, another chunk) or would not appear in the shared canvas.
 */
import { usePalette, type Role } from '../palette';
import { Arrow } from '../scene/Arrow';
import { Grid } from '../scene/Grid';
import { Point } from '../scene/Point';
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

/** Task 6b: drag the tips of a and b (or Tab to their hidden buttons and use the arrow keys); the dashed segment follows. */
export const DemoVec2D = () => (
	<Stage lesson="demo" mode="2d" extent={4}>
		<Grid extent={4} step={1} minor={0.5} />
		<Segment lesson="demo" id="ab" a="a" b="b" dashed role="muted" />
		<Arrow lesson="demo" id="a" to="a" role="vecA" legs />
		<Arrow lesson="demo" id="b" to="b" role="vecB" />
		<Point lesson="demo" id="p" at="p" label="P" role="vecC" />
	</Stage>
);

/** 3D: two arrows with legs; dragging a tip must not orbit, dragging empty space must. */
export const DemoVec3D = () => (
	<Stage lesson="demo" mode="3d" extent={3}>
		<Grid extent={3} step={1} />
		<Arrow lesson="demo" id="u" to="u" role="vecA" legs />
		<Arrow lesson="demo" id="v" to="v" role="vecB" legs />
	</Stage>
);
