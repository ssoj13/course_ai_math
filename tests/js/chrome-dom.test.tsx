import './dom';
import { afterEach, expect, test } from 'bun:test';
import { act, type ReactElement } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { createStage, HOVER_RANK, overlayKey, StageCtx, Show, useHoverable, useOverlay, type HoverProps, type OverlayDef } from '../../src/viz/scene/handles';
import { StatusBar } from '../../src/viz/scene/StatusBar';
import { Gizmo } from '../../src/viz/scene/Gizmo';
import { Overlays } from '../../src/viz/scene/Header';
import { lessonStore } from '../../src/viz/store';

const roots: Root[] = [];

function mount(el: ReactElement): HTMLElement {
	const host = document.createElement('div');
	document.body.appendChild(host);
	const root = createRoot(host);
	roots.push(root);
	act(() => root.render(el));
	return host;
}

afterEach(() => {
	for (const r of roots.splice(0)) act(() => r.unmount());
	document.body.innerHTML = '';
});

const OV: readonly OverlayDef[] = [
	{ id: 'grid', label: 'Grid', on: true },
	{ id: 'par', label: 'Parallelogram', on: false },
];

test('Overlays popover: closed by default; lists the declared flags with their defaults; toggles write 0/1 to the lesson store', () => {
	lessonStore('ov1', {});
	const host = mount(<Overlays lesson="ov1" card="c1" overlays={OV} />);
	const btn = host.querySelector<HTMLButtonElement>('button')!;
	expect(btn.getAttribute('aria-expanded')).toBe('false');
	expect(host.querySelector('.lab-vp-pop')).toBeNull();
	act(() => btn.click());
	expect(btn.getAttribute('aria-expanded')).toBe('true');
	const boxes = [...host.querySelectorAll<HTMLInputElement>('input[type="checkbox"]')];
	expect(boxes.map((b) => [b.parentElement!.textContent, b.checked])).toEqual([
		['Grid', true],
		['Parallelogram', false],
	]);
	act(() => boxes[0].click());
	expect(lessonStore('ov1').getState().vals[overlayKey('c1', 'grid')]).toBe(0);
	act(() => boxes[1].click());
	expect(lessonStore('ov1').getState().vals[overlayKey('c1', 'par')]).toBe(1);
	expect(host.querySelectorAll<HTMLInputElement>('input')[0].checked).toBe(false);
});

test('Overlays popover: Escape closes and returns focus to the button; a press outside closes', () => {
	lessonStore('ov2', {});
	const host = mount(<Overlays lesson="ov2" card="c" overlays={OV} />);
	const btn = host.querySelector<HTMLButtonElement>('button')!;
	act(() => btn.click());
	const box = host.querySelector<HTMLInputElement>('input')!;
	box.focus();
	act(() => void box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })));
	expect(host.querySelector('.lab-vp-pop')).toBeNull();
	expect(document.activeElement).toBe(btn);
	act(() => btn.click());
	act(() => void document.body.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })));
	expect(host.querySelector('.lab-vp-pop')).toBeNull();
	// Tabbing out of the popover closes it.
	const after = document.createElement('button');
	document.body.appendChild(after);
	act(() => btn.click());
	const box2 = host.querySelector<HTMLInputElement>('input')!;
	box2.focus();
	act(() => void box2.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: after })));
	expect(host.querySelector('.lab-vp-pop')).toBeNull();
	// A mouse press on label text blurs the button with relatedTarget = null (Chrome focus fixup): stays open.
	act(() => btn.click());
	act(() => void btn.dispatchEvent(new FocusEvent('focusout', { bubbles: true, relatedTarget: null })));
	expect(host.querySelector('.lab-vp-pop')).not.toBeNull();
	const label = host.querySelector<HTMLLabelElement>('.lab-vp-check')!;
	act(() => void label.dispatchEvent(new PointerEvent('pointerdown', { bubbles: true })));
	expect(host.querySelector('.lab-vp-pop')).not.toBeNull();
});

test('a bare Stage registry has no card: useOverlay says so', () => {
	const reg = createStage('2d');
	function Bad() {
		useOverlay('grid');
		return null;
	}
	expect(() =>
		mount(
			<StageCtx.Provider value={reg}>
				<Bad />
			</StageCtx.Provider>,
		),
	).toThrow(/not inside a ViewportCard/);
});

test('gizmo: keyboard buttons keep focus while the bubbles re-stack by depth; Enter aligns', () => {
	const reg = createStage('3d', { lesson: 'gz1', id: 'c', overlays: [] });
	const calls: string[] = [];
	reg.nav = { reset() {}, align: (a, s) => void calls.push(`${a},${s}`), viewDir: () => [0, 0, 1], redraw() {} };
	const host = mount(<Gizmo reg={reg} />);
	const keys = [...host.querySelectorAll<HTMLButtonElement>('.lab-gizmo-keys button')];
	expect(keys.map((k) => k.getAttribute('aria-label'))).toEqual(['Right view (+X)', 'Top view (+Y)', 'Front view (+Z)', 'Left view (−X)', 'Bottom view (−Y)', 'Back view (−Z)']);
	const top = keys[1];
	act(() => top.focus());
	act(() => top.click()); // Enter/Space on a <button> fire click
	expect(calls).toEqual(['1,1']);
	const order = () => [...host.querySelectorAll('.lab-gizmo-head')].map((g) => g.getAttribute('data-k')).join(';');
	act(() => reg.gizmo!([0, 0, 0, 1]));
	const o1 = order();
	const s = Math.SQRT1_2;
	act(() => reg.gizmo!([s, 0, 0, s])); // looking down: the depth order of the bubbles changes
	expect(order()).not.toBe(o1);
	expect(document.activeElement).toBe(top);
	expect([...host.querySelectorAll<HTMLButtonElement>('.lab-gizmo-keys button')]).toEqual(keys);
	expect(host.querySelector('.lab-gizmo-head[data-k="1,1"]')!.classList.contains('is-focus')).toBe(true);
});

test('useOverlay/Show read the flag (default when absent) and re-render on a toggle; undeclared ids throw', () => {
	const S = lessonStore('ov3', {});
	const reg = createStage('2d', { lesson: 'ov3', id: 'c', overlays: OV });
	const host = mount(
		<StageCtx.Provider value={reg}>
			<Show ov="grid">
				<i>grid</i>
			</Show>
			<Show ov="par">
				<i>par</i>
			</Show>
		</StageCtx.Provider>,
	);
	expect(host.textContent).toBe('grid');
	act(() => S.getState().set(overlayKey('c', 'par'), 1));
	expect(host.textContent).toBe('gridpar');
	function Bad() {
		useOverlay('nope');
		return null;
	}
	expect(() =>
		mount(
			<StageCtx.Provider value={reg}>
				<Bad />
			</StageCtx.Provider>,
		),
	).toThrow(/not declared/);
});

test('StatusBar shows the hint of what the pointer is over, with key caps', () => {
	lessonStore('sb1', { a: 2 });
	const reg = createStage('3d', { lesson: 'sb1', id: 'c', overlays: [] });
	const host = mount(<StatusBar reg={reg} lesson="sb1" spec={{ hints: { a: (v) => `a is ${v.a}` } }} />);
	const bar = host.querySelector('.lab-vp-status')!;
	expect(bar.textContent).toContain('orbit');
	expect([...bar.querySelectorAll('kbd')].map((k) => k.textContent)).toContain('Ctrl+Wheel');
	act(() => reg.hover.set({ object: 'a' }));
	expect(bar.textContent).toBe('a is 2');
	act(() => lessonStore('sb1').getState().set('a', 5));
	expect(bar.textContent).toBe('a is 5');
	act(() => reg.hover.clear('object', 'b'));
	expect(bar.textContent).toBe('a is 5');
	act(() => reg.hover.clear('object', 'a'));
	expect(bar.textContent).toContain('orbit');
});

test('useHoverable: among the objects under the pointer only the highest rank claims hot and the status hover', () => {
	lessonStore('hv1', {});
	const reg = createStage('2d', { lesson: 'hv1', id: 'c', overlays: [] });
	const got: Record<string, HoverProps> = {};
	function Probe({ id, rank }: { id: string; rank: number }) {
		got[id] = useHoverable('hv1', id, rank);
		return null;
	}
	mount(
		<StageCtx.Provider value={reg}>
			<Probe id="band" rank={HOVER_RANK.fill} />
			<Probe id="a" rank={HOVER_RANK.beam} />
		</StageCtx.Provider>,
	);
	const obj = (rank: number) => ({ userData: {}, parent: { userData: { hoverRank: rank }, parent: null } });
	const hits = [{ object: obj(HOVER_RANK.fill) }, { object: obj(HOVER_RANK.beam) }];
	let stopped = 0;
	const ev = { stopPropagation: () => void stopped++, intersections: hits };
	// r3f delivers to the band first (same distance, scene order): it must not claim.
	act(() => got.band.onPointerOver!(ev));
	expect(lessonStore('hv1').getState().hot.size).toBe(0);
	expect(stopped).toBe(0);
	act(() => got.a.onPointerOver!(ev));
	expect([...lessonStore('hv1').getState().hot]).toEqual(['a']);
	expect(reg.hover.get().object).toBe('a');
	expect(stopped).toBe(1);
	// Leaving the beam while still over the band: the band re-claims on move.
	act(() => got.a.onPointerOut!());
	act(() => got.band.onPointerMove!({ stopPropagation: () => {}, intersections: [{ object: obj(HOVER_RANK.fill) }] }));
	expect([...lessonStore('hv1').getState().hot]).toEqual(['band']);
	expect(reg.hover.get().object).toBe('band');
});
