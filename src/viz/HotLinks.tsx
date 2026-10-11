/**
 * Directional hover links of a lesson (the storyboard "hover maps"): a hot id lights its linked ids, transitively
 * (a Set grows while it is iterated, so additions are visited too). The expansion keeps the originating source
 * (guide or interaction), and a new hover starts a new set, so expansions never accumulate. Mount once per lesson,
 * next to the lead scene's DOM controls; renders nothing.
 */
import { useEffect } from 'react';
import { lessonStore } from './store';

export type HotLinkMap = Readonly<Record<string, readonly string[]>>;

/** The ids lit by `ids` under `links` (pure; the component and tests share it). */
export function expandHot(ids: Iterable<string>, links: HotLinkMap): Set<string> {
	const out = new Set(ids);
	for (const id of out) for (const linked of links[id] ?? []) out.add(linked);
	return out;
}

export function HotLinks({ lesson, links }: { lesson: string; links: HotLinkMap }): null {
	useEffect(() => {
		const store = lessonStore(lesson);
		const sync = () => {
			const st = store.getState();
			const ids = expandHot(st.hot, links);
			if (ids.size !== st.hot.size) st.setHot([...ids], st.hotSource ?? 'interaction');
		};
		sync();
		return store.subscribe(sync);
	}, [lesson, links]);
	return null;
}
