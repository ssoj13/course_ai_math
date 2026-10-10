/**
 * A scene slot in the article. 2D = orthographic camera (pixel-stable lines); 3D = perspective + orbit.
 * The accessible name comes from the server-rendered <figcaption> of SceneIsland, so the stage carries none.
 */
import { OrbitControls, OrthographicCamera, PerspectiveCamera, View } from '@react-three/drei';
import { useThree } from '@react-three/fiber';
import { useEffect, useRef, useState, type RefObject } from 'react';
import { OrthographicCamera as OrthoCam } from 'three';
import { usePalette } from '../palette';
import { lessonStore } from '../store';
import { ensureSceneHost } from './SceneHost';

function InvalidateOnStore({ lesson }: { lesson: string }) {
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => lessonStore(lesson).subscribe(() => invalidate()), [lesson, invalidate]);
	return null;
}

/**
 * A theme switch yields a new palette object. Scene materials are updated by React, but the canvas runs on demand
 * (`frameloop="demand"`), so something must also ask for a frame: this runs inside the View portal, and `invalidate`
 * there schedules a render of the shared root.
 */
function InvalidateOnTheme() {
	const palette = usePalette();
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => invalidate(), [palette, invalidate]);
	return null;
}

/**
 * Content-box size of the stage element. drei's View portal captures its `size` once at mount and never refreshes
 * it (measured: after a resize from 1280 to 900 px the box kept its old 66 px side), so scaling reads the element.
 */
function useBoxSize(ref: RefObject<HTMLElement | null>): { w: number; h: number } {
	const [size, setSize] = useState({ w: 0, h: 0 });
	useEffect(() => {
		const el = ref.current;
		if (!el) return;
		const ro = new ResizeObserver(([e]) => setSize({ w: e.contentRect.width, h: e.contentRect.height }));
		ro.observe(el);
		return () => ro.disconnect();
	}, [ref]);
	return size;
}

/**
 * Refits the scene when the stage box changes. 2D: drei's View sets the orthographic frustum to the element's pixel
 * size, so zoom alone decides how many world units are visible: `extent` units from the center must reach the nearer
 * edge on every stage size. Both modes redraw, because a View's rect and aspect only update inside a rendered frame
 * and a stage can resize without a window resize (sidebar, font load).
 */
function FitView({ extent, w, h }: { extent: number; w: number; h: number }) {
	const { camera, invalidate } = useThree();
	useEffect(() => {
		if (!w || !h) return;
		if (camera instanceof OrthoCam) {
			camera.zoom = Math.min(w, h) / (2 * extent);
			camera.updateProjectionMatrix();
		}
		invalidate();
	}, [w, h, camera, extent, invalidate]);
	return null;
}

export function Stage(p: { lesson: string; mode: '2d' | '3d'; extent: number; children: React.ReactNode; overlay?: React.ReactNode }): React.JSX.Element {
	useEffect(ensureSceneHost, []);
	const ref = useRef<HTMLDivElement>(null);
	const { w, h } = useBoxSize(ref);
	return (
		<div className="lab-stage" ref={ref}>
			<View className="lab-stage-view">
				<InvalidateOnStore lesson={p.lesson} />
				<InvalidateOnTheme />
				<FitView extent={p.extent} w={w} h={h} />
				{p.mode === '2d' ? (
					<OrthographicCamera makeDefault position={[0, 0, 10]} zoom={1} />
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
