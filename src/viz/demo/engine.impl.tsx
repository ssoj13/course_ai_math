/**
 * Spike A demo scenes (loaded lazily via engine.tsx): three stages in three separate React roots, all on lesson
 * `demo`. If the store or drei's View tunnel were duplicated per island, the boxes would not follow the slider
 * (engine.tsx, another chunk) or would not appear in the shared canvas.
 */
import { usePalette, type Role } from '../palette';
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
