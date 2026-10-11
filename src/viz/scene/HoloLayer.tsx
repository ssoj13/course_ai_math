/**
 * The look's furniture of one Stage, in its own lazy chunk (pages without scenes never load it): the instrument feed,
 * the sky (or the clay studio backdrop) and, in the holo look, the reflective floor with the figure's reflection.
 */
import type { RefObject } from 'react';
import type { Group } from 'three';
import { Background } from './Background';
import { Floor } from './Floor';
import { InstrumentSync } from './InstrumentSync';
import type { Look } from './look';

export function HoloLayer({ look, floor, content }: { look: Look; floor: boolean; content: RefObject<Group | null> }): React.JSX.Element {
	return (
		<>
			<InstrumentSync />
			<Background look={look} />
			{floor && <Floor content={content} />}
		</>
	);
}
