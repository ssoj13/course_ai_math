/**
 * Shared three-side piece of the tensor lessons 3.2 and 3.3: a set of tensor cells drawn as ONE instanced draw (the
 * DotUnit's `Boxes` renderer over a `Frame`), repainted when its data change and re-framed from a fixed direction when
 * its layout changes. 3.1's road has its own animated variant (the extrusion); these cards are static per state.
 */
import { useThree } from '@react-three/fiber';
import { useEffect, useMemo, useRef } from 'react';
import { useStage } from '../scene/handles';
import { Boxes } from '../dotunit/Boxes';
import { Frame } from '../dotunit/choreo';

export interface CellsProps {
	lesson: string;
	/** Most boxes the card ever draws (fixes the instance buffers). */
	cap: number;
	/** Writes every box (centre, size, look, id) and `f.n`; called after `paintKey` changes. */
	paint(f: Frame): void;
	/** Changes whenever the drawn cells change (values, selection, eased progress). */
	paintKey: unknown;
	/** Changes whenever the figure's extent changes: the home view is re-framed from `home`. */
	frameKey: unknown;
	/** Direction from the figure toward the camera for the home view. */
	home: readonly [number, number, number];
	/** World margin around the figure for the home view (labels hang outside the boxes). */
	pad?: number;
	onPick?(id: string, index: number): void;
}

export function Cells({ lesson, cap, paint, paintKey, frameKey, home, pad, onPick }: CellsProps): React.JSX.Element {
	const reg = useStage();
	const invalidate = useThree((st) => st.invalidate);
	const frame = useMemo(() => new Frame(cap), [cap]);
	const st = useRef({ dirty: true, refit: true });
	const props = useRef({ paint, home });
	props.current = { paint, home };
	useEffect(() => {
		st.current.dirty = true;
		invalidate();
	}, [paintKey, invalidate]);
	useEffect(() => {
		st.current.refit = true;
		invalidate();
	}, [frameKey, invalidate]);

	const update = (): 'still' | 'moved' | 'moving' => {
		const s = st.current;
		if (s.dirty) {
			s.dirty = false;
			props.current.paint(frame);
			if (frame.n > frame.cap) throw new Error(`tensor-cells: ${frame.n} boxes exceed the capacity ${frame.cap}`);
			// The bounds proxy is updated by this upload; the re-frame waits for it (next frame).
			return s.refit ? 'moving' : 'moved';
		}
		if (s.refit) {
			// The Stage binds its navigation a few frames after mount: keep the request until it can be served.
			if (!reg.nav) return 'moving';
			s.refit = false;
			reg.nav.frame(props.current.home);
			return 'moved';
		}
		return 'still';
	};
	return <Boxes lesson={lesson} frame={frame} update={update} onPick={onPick} pad={pad} />;
}
