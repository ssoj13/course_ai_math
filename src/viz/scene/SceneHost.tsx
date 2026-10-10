/**
 * The one WebGL context of a page. Browsers cap live contexts (~16 per page on desktop Chromium, fewer on
 * mobile; the oldest is lost), and a lesson has several scenes, so every Stage is a drei <View> rendered here.
 * frameloop="demand": a textbook page must not burn GPU while idle; stages call invalidate() on change.
 * `flat`: no tone mapping, so palette colors render exactly as specified (r3f defaults to ACES, which lightens them).
 */
import { Canvas, useThree } from '@react-three/fiber';
import { View } from '@react-three/drei';
import { useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
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

function SceneHost(): React.JSX.Element | null {
	const [ok] = useState(webglAvailable);
	useEffect(() => {
		if (!ok) markNoWebgl();
	}, [ok]);
	if (!ok) return null;
	return (
		<Canvas
			flat
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

let mounted = false;

/**
 * Mounts the shared canvas on the first Stage of a page, as its own React root at the end of <body>. Mounting on
 * demand (instead of from the page frame) keeps pages without scenes free of three/drei and of a GL context.
 * The Views find the Port through drei's module-level tunnel, which every island shares (one ES module per page).
 */
export function ensureSceneHost(): void {
	if (mounted) return;
	mounted = true;
	const el = document.createElement('div');
	el.className = 'lab-scene-host';
	document.body.append(el);
	createRoot(el).render(<SceneHost />);
}
