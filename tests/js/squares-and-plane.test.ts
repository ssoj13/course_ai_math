import { expect, test } from 'bun:test';
import '../../src/i18n/ui';
import { lessonStore } from '../../src/viz/store';
import { writeMove, clampMove } from '../../src/viz/scene/handles';
import { CHECKS, derive, INITIAL, squareLink, SPEC, fmt1 } from '../../src/viz/lessons/squares-and-plane.math';
import { rowsFor } from '../../src/viz/lessons/squares-and-plane.rows';
import { rowRanges } from '../../src/viz/scene/NPanel';
import { chipLines } from '../../src/viz/scene/chips';

let n = 0;
const fresh = () => {
	const id = 'sq-test-' + n++;
	return { id, store: lessonStore(id, { ...INITIAL }, { groups: [squareLink()] }) };
};

test('x and A publish together: every synchronous subscriber sees A = x·x (x writes) or x = √A (A writes)', () => {
	const { store } = fresh();
	const seen: [number, number][] = [];
	const off = store.subscribe((s) => seen.push([s.vals.x as number, s.vals.A as number]));
	store.getState().set('x', -2.5);
	store.getState().set('A', 2);
	store.getState().setMany({ x: 0.1 });
	off();
	expect(seen).toEqual([[-2.5, 6.25], [Math.SQRT2, 2], [0.1, 0.1 * 0.1]]);
	// A keeps the exact typed value (not √2·√2 = 2.0000000000000004).
	store.getState().set('A', 2);
	expect(store.getState().vals.A).toBe(2);
});

test('a later A write replaces a negative x by the principal root, even when A itself does not change', () => {
	const { store } = fresh();
	store.getState().set('x', -3);
	expect(store.getState().vals).toMatchObject({ x: -3, A: 9 });
	// Same A: the resolver runs before same-value pruning, so x still flips to +3.
	store.getState().set('A', 9);
	expect(store.getState().vals).toMatchObject({ x: 3, A: 9 });
});

test('inputs are validated and clamped before publication; a conflicting pair is rejected', () => {
	const { store } = fresh();
	store.getState().set('x', 7);
	expect(store.getState().vals).toMatchObject({ x: 4, A: 16 });
	store.getState().set('A', -5);
	expect(store.getState().vals).toMatchObject({ x: 0, A: 0 });
	store.getState().set('A', 20);
	expect(store.getState().vals).toMatchObject({ x: 4, A: 16 });
	expect(() => store.getState().set('x', NaN)).toThrow();
	expect(() => store.getState().set('A', Infinity)).toThrow();
	expect(() => store.getState().setMany({ x: 2, A: 5 })).toThrow(/Conflicting/);
	expect(store.getState().vals).toMatchObject({ x: 4, A: 16 });
	store.getState().setMany({ x: -2, A: 4 });
	expect(store.getState().vals).toMatchObject({ x: -2, A: 4 });
});

test('eased writes spring only x; the drawn A is x·x at every frame (side and area coherent)', async () => {
	const { store } = fresh();
	store.getState().set('A', 1, 'ease');
	expect(store.getState().vals).toMatchObject({ x: 1, A: 1 });
	const checks: number[] = [];
	for (let i = 0; i < 40; i++) {
		const sh = store.getState().shown;
		checks.push(Math.abs((sh.A as number) - (sh.x as number) ** 2));
		await new Promise((r) => setTimeout(r, 12));
	}
	expect(Math.max(...checks)).toBeLessThan(1e-12);
	// Arrival depends on the frame source (a DOM test file may have stubbed requestAnimationFrame); coherence does not.
});

test('a scrub checkpoint restores the whole linked pair', () => {
	const { store } = fresh();
	store.getState().set('x', -3);
	const restore = store.getState().checkpoint(['A']);
	store.getState().set('A', 5);
	expect(store.getState().vals.x).toBe(Math.sqrt(5));
	restore();
	expect(store.getState().vals).toMatchObject({ x: -3, A: 9 });
	const back = store.getState().checkpoint(['P']);
	store.getState().set('P', [-1, 1]);
	back();
	expect(store.getState().vals.P).toEqual([1, 2]);
});

test('handles write x through the same resolver and the card clamp', () => {
	const { id, store } = fresh();
	const ranges = rowRanges(rowsFor(1, 'en'));
	writeMove(id, clampMove({ x: -4.6 }, ranges));
	expect(store.getState().vals).toMatchObject({ x: -4, A: 16 });
	const p = rowRanges(rowsFor(2, 'en'));
	writeMove(id, clampMove({ P: [9, -9] }, p));
	expect(store.getState().vals.P).toEqual([4.5, -4.5]);
});

test('derived numbers of the spec edge cases (0.G-1, 0.G-2, 0.G-3)', () => {
	const v = (o: Record<string, unknown>) => derive({ ...INITIAL, ...o } as never);
	expect(v({ x: 0 })).toMatchObject({ x2: 0, whole: 0, partial: 0, partialPieces: 0 });
	expect(v({ x: -3, A: 9 })).toMatchObject({ x2: 9, whole: 9, partial: 0 });
	expect(v({ x: 2.5, A: 6.25 })).toMatchObject({ x2: 6.25, whole: 4, partial: 2.25, partialPieces: 5 });
	expect(v({ A: 0, x: 0 })).toMatchObject({ sqrtA: 0, whole3: 0, partialPieces3: 0 });
	const two = v({ A: 2, x: Math.SQRT2 });
	expect(two.sqrtA.toFixed(4)).toBe('1.4142');
	expect(two.whole3).toBe(1);
	expect(two.partialPieces3).toBe(3);
	expect(two.partial3.toFixed(2)).toBe('1.00');
	expect(two.check.toFixed(2)).toBe('2.00');
	for (const [P, sum] of [[[-2, 3], 13], [[3, 2], 13], [[0, 0], 0], [[0, -4], 16], [[4.5, 0], 20.25], [[-1, -1], 2], [[1, -1], 2]] as const) {
		const d = v({ P });
		expect(d.sum).toBe(sum);
		expect(Number.isFinite(d.px2) && Number.isFinite(d.py2)).toBe(true);
	}
	expect(fmt1(-0)).toBe('0.0');
});

test('challenges: store reads only; the default point does not solve "put P at (-2, 3)"', () => {
	const base = { ...INITIAL } as never as Parameters<typeof CHECKS.sixteen>[0];
	const w = (o: Record<string, unknown>) => ({ ...base, ...o }) as typeof base;
	expect(CHECKS.sixteen(w({ x: 4 }))).toBe(true);
	expect(CHECKS.sixteen(w({ x: -4 }))).toBe(true);
	expect(CHECKS.sixteen(w({ x: 3.9 }))).toBe(false);
	expect(CHECKS.negative(w({ x: -0.1 }))).toBe(true);
	expect(CHECKS.negative(base)).toBe(false);
	expect(CHECKS.pointTyped(base)).toBe(false);
	expect(CHECKS.pointTyped(w({ P: [-2, 3] }))).toBe(true);
	expect(CHECKS.pointTyped(w({ P: [-2, 3.1] }))).toBe(false);
	expect(CHECKS.sumOther(base)).toBe(false);
	expect(CHECKS.sumOther(w({ P: [3, 2] }))).toBe(true);
	expect(CHECKS.sumOther(w({ P: [-3, -2] }))).toBe(true);
	expect(CHECKS.wholeSide(base)).toBe(false);
	expect(CHECKS.wholeSide(w({ A: 4 }))).toBe(true);
	expect(CHECKS.wholeSide(w({ A: 0 }))).toBe(false);
	expect(CHECKS.wholeSide(w({ A: 2.25 }))).toBe(false);
	expect(CHECKS.sideHalf(w({ A: 2.25 }))).toBe(true);
	// The default point is not the worked example, so challenge 3 starts unsolved and typing (-2, 3) solves it.
	const { store } = fresh();
	expect(CHECKS.pointTyped(store.getState().vals)).toBe(false);
	store.getState().set('P', [-2, 3]);
	expect(CHECKS.pointTyped(store.getState().vals)).toBe(true);
});

test('rows: semantic ids, ranges and math labels identical in EN and RU; chips reserve lines', () => {
	const ids = [['x', 'x2', 'whole', 'partial'], ['P', 'px2', 'py2', 'sum'], ['A', 'sqrtA', 'check']];
	([1, 2, 3] as const).forEach((scene, i) => {
		const en = rowsFor(scene, 'en'), ru = rowsFor(scene, 'ru');
		expect(en.map((r) => r.id)).toEqual(ids[i]);
		expect(ru.map((r) => r.id)).toEqual(ids[i]);
		const contract = (rows: typeof en) => rows.map(({ label, tip, group, value, fmt, visible, ...row }) => ({ ...row, value: value?.toString(), fmt: fmt?.toString(), visible: visible?.toString() }));
		expect(contract(ru)).toEqual(contract(en));
		expect(chipLines(en)).toBeGreaterThan(0);
	});
	expect(rowRanges(rowsFor(1, 'en'))).toEqual({ x: { min: SPEC.x.min, max: SPEC.x.max } });
	expect(rowRanges(rowsFor(3, 'ru'))).toEqual({ A: { min: 0, max: 16 } });
	const partial = rowsFor(1, 'en')[3];
	expect(partial.visible!({ ...INITIAL, x: 3 }, {})).toBe(false);
	expect(partial.visible!({ ...INITIAL, x: 2.5 }, {})).toBe(true);
});
