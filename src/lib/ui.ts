/**
 * DOM helpers shared by widgets. Everything sets `textContent`, never `innerHTML`, so values that
 * come from a server or from the reader cannot inject markup.
 */

/** Creates an element with an optional class name and text. */
export function h<K extends keyof HTMLElementTagNameMap>(tag: K, cls = '', text?: string): HTMLElementTagNameMap[K] {
	const e = document.createElement(tag);
	if (cls) e.className = cls;
	if (text !== undefined) e.textContent = text;
	return e;
}

export type MsgKind = 'ok' | 'warn' | 'err';

/** Fills a `.lab-msg` element: hidden when `text` is empty, `.is-<kind>` otherwise; other classes are kept. */
export function showMsg(box: HTMLElement, text: string, kind: MsgKind | null = null): void {
	box.hidden = text === '';
	box.textContent = text;
	box.classList.remove('is-ok', 'is-warn', 'is-err');
	if (kind) box.classList.add(`is-${kind}`);
}

/** `showMsg` in the error style; an empty string hides it. */
export function showErr(box: HTMLElement, text: string): void {
	showMsg(box, text, 'err');
}

/** One `.lab-stat` tile: big value over a small label. */
export function tile(value: string, label: string): HTMLDivElement {
	const d = h('div', 'lab-stat');
	d.append(h('b', '', value), h('span', '', label));
	return d;
}

/** Replaces the tiles of a `.lab-stats` box with `[value, label]` pairs; an empty list hides the box. */
export function setStats(box: HTMLElement, items: readonly (readonly [value: string, label: string])[]): void {
	box.replaceChildren(...items.map(([v, l]) => tile(v, l)));
	box.hidden = items.length === 0;
}

/**
 * Starts a browser download of `text`. The object URL is revoked later, not right after `click()`:
 * some browsers resolve it asynchronously and a synchronous revoke can cancel the download.
 */
export function download(name: string, text: string, type: string): void {
	const url = URL.createObjectURL(new Blob([text], { type }));
	const a = h('a');
	a.href = url;
	a.download = name;
	a.click();
	setTimeout(() => URL.revokeObjectURL(url), 10_000);
}
