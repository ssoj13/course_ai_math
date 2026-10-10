/**
 * Lazy island entry. MDX keeps the `import` of a `client:only` component in the server-rendered page module
 * (only the .astro compiler drops it), so the server would evaluate three/drei just to never render them.
 * Lesson entry modules therefore export `lazyIsland(() => import('./x.impl').then((m) => m.X))`: the server
 * evaluates only this wrapper; the browser fetches the implementation chunk on mount. The fallback is null
 * because SceneIsland already reserves the box and prints the caption.
 */
import { lazy, Suspense, type ComponentType } from 'react';

export function lazyIsland<P extends object>(load: () => Promise<ComponentType<P>>): ComponentType<P> {
	const Impl = lazy(async () => ({ default: await load() }));
	return function LazyIsland(p: P) {
		return (
			<Suspense fallback={null}>
				<Impl {...p} />
			</Suspense>
		);
	};
}
