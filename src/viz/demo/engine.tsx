/**
 * Spike A demo islands (dev/engine.mdx): three stages in three separate React roots plus a slider in a fourth,
 * all on lesson `demo`. If the store or drei's View tunnel were duplicated per island, the boxes would not
 * follow the slider or would not appear in the shared canvas.
 */
import { Stage } from '../scene/Stage';
import { lessonStore, useVal } from '../store';

const S = lessonStore('demo', { x: 0 });

const BOX_A = '#4f7cff'; // palette: Task 5
const BOX_B = '#e0794a'; // palette: Task 5
const BOX_C = '#3fae7a'; // palette: Task 5

function DemoBox({ mode, color, caption }: { mode: '2d' | '3d'; color: string; caption: string }): React.JSX.Element {
	const x = useVal('demo', 'x', 0);
	return (
		<Stage lesson="demo" mode={mode} extent={3} caption={caption}>
			<mesh position={[x, 0, 0]}>
				<boxGeometry args={[1, 1, 1]} />
				<meshBasicMaterial color={color} />
			</mesh>
			{mode === '3d' && <gridHelper args={[6, 6]} />}
		</Stage>
	);
}

export const DemoBoxA = () => <DemoBox mode="2d" color={BOX_A} caption="Box A (2D) at x" />;
export const DemoBoxB = () => <DemoBox mode="2d" color={BOX_B} caption="Box B (2D) at x" />;
export const DemoBoxC = () => <DemoBox mode="3d" color={BOX_C} caption="Box C (3D, orbit) at x" />;

export function DemoSlider(): React.JSX.Element {
	const x = useVal('demo', 'x', 0);
	return (
		<label className="not-content lab-demo-slider">
			x = <output>{x.toFixed(2)}</output>{' '}
			<input type="range" min={-2} max={2} step={0.05} value={x} onChange={(e) => S.getState().set('x', Number(e.currentTarget.value))} />
		</label>
	);
}
