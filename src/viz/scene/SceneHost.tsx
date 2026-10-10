/**
 * The one WebGL context of a page. Browsers cap live contexts (~16 per page on desktop Chromium, fewer on
 * mobile; the oldest is lost), and a lesson has several scenes, so every Stage is a drei <View> rendered here.
 * frameloop="demand": a textbook page must not burn GPU while idle; stages call invalidate() on change.
 */
import { Canvas, useThree } from '@react-three/fiber';
import { View } from '@react-three/drei';
import { useEffect, useState } from 'react';
import { webglAvailable } from './webgl';

/** drei recomputes each View's scissor rect only while rendering; on-demand rendering must be poked on scroll/resize. */
function InvalidateOnScroll() {
	const invalidate = useThree((s) => s.invalidate);
	useEffect(() => {
		let raf = 0;
		const poke = () => {
			cancelAnimationFrame(raf);
			raf = requestAnimationFrame(() => invalidate());
		};
		window.addEventListener('scroll', poke, { passive: true });
		window.addEventListener('resize', poke);
		return () => {
			cancelAnimationFrame(raf);
			window.removeEventListener('scroll', poke);
			window.removeEventListener('resize', poke);
		};
	}, [invalidate]);
	return null;
}

const markNoWebgl = () => document.documentElement.classList.add('lab-no-webgl'); // CSS shows the inline notice in every stage

export default function SceneHost(): React.JSX.Element | null {
	const [ok] = useState(webglAvailable);
	useEffect(() => {
		if (!ok) markNoWebgl();
	}, [ok]);
	if (!ok) return null;
	return (
		<Canvas
			className="lab-scene-host"
			frameloop="demand"
			eventSource={document.body}
			onCreated={({ gl }) => gl.domElement.addEventListener('webglcontextlost', markNoWebgl)}
			style={{ position: 'fixed', inset: 0, pointerEvents: 'none', zIndex: 0 }}
		>
			<InvalidateOnScroll />
			<View.Port />
		</Canvas>
	);
}
