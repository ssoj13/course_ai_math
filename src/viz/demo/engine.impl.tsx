/**
 * Spike A demo scenes (loaded lazily via engine.tsx): three stages in three separate React roots, all on lesson
 * `demo`. If the store or drei's View tunnel were duplicated per island, the boxes would not follow the slider
 * (engine.tsx, another chunk) or would not appear in the shared canvas.
 */
import { Stage } from '../scene/Stage';
import { useVal } from '../store';

const BOX_A = '#4f7cff'; // palette: Task 5
const BOX_B = '#e0794a'; // palette: Task 5
const BOX_C = '#3fae7a'; // palette: Task 5

function DemoBox({ mode, color }: { mode: '2d' | '3d'; color: string }): React.JSX.Element {
	const x = useVal('demo', 'x', 0);
	return (
		<Stage lesson="demo" mode={mode} extent={3}>
			<mesh position={[x, 0, 0]}>
				<boxGeometry args={[1, 1, 1]} />
				<meshBasicMaterial color={color} />
			</mesh>
			{mode === '3d' && <gridHelper args={[6, 6]} />}
		</Stage>
	);
}

export const DemoBoxA = () => <DemoBox mode="2d" color={BOX_A} />;
export const DemoBoxB = () => <DemoBox mode="2d" color={BOX_B} />;
export const DemoBoxC = () => <DemoBox mode="3d" color={BOX_C} />;
