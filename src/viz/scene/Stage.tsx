/** A scene slot in the article. 2D = orthographic camera (pixel-stable lines); 3D = perspective + orbit. */
import { OrbitControls, OrthographicCamera, PerspectiveCamera, View } from '@react-three/drei';
import { useThree } from '@react-three/fiber';
import { useEffect } from 'react';
import { lessonStore } from '../store';

function InvalidateOnStore({ lesson }: { lesson: string }) {
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => lessonStore(lesson).subscribe(() => invalidate()), [lesson, invalidate]);
	return null;
}

/**
 * drei's View sets the orthographic frustum to the element's pixel size, so zoom alone decides how many world
 * units are visible: `extent` units from the center must reach the nearer edge on every stage size.
 */
function FitOrtho({ extent }: { extent: number }) {
	const { size, camera, invalidate } = useThree();
	useEffect(() => {
		if (!('isOrthographicCamera' in camera) || !size.width || !size.height) return;
		camera.zoom = Math.min(size.width, size.height) / (2 * extent);
		camera.updateProjectionMatrix();
		invalidate();
	}, [size.width, size.height, camera, extent, invalidate]);
	return null;
}

export function Stage(p: { lesson: string; mode: '2d' | '3d'; extent: number; caption: string; children: React.ReactNode; overlay?: React.ReactNode }): React.JSX.Element {
	return (
		<div className="lab-stage not-content" role="group" aria-label={p.caption}>
			<View className="lab-stage-view">
				<InvalidateOnStore lesson={p.lesson} />
				{p.mode === '2d' ? (
					<>
						<OrthographicCamera makeDefault position={[0, 0, 10]} zoom={1} />
						<FitOrtho extent={p.extent} />
					</>
				) : (
					<>
						<PerspectiveCamera makeDefault position={[p.extent * 1.6, p.extent * 1.2, p.extent * 1.8]} fov={40} />
						<OrbitControls enableDamping={false} minDistance={p.extent} maxDistance={p.extent * 6} makeDefault />
					</>
				)}
				{p.children}
			</View>
			{/* Sibling of the View div (see interface note): DOM here never blocks scene raycasts outside its own boxes. */}
			<div className="lab-stage-overlay">{p.overlay}</div>
			<p className="lab-stage-nogl">This scene needs WebGL, which is unavailable here. The formula and table next to it show the same values.</p>
		</div>
	);
}
