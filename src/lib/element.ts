/**
 * Base class for every lab widget (custom element).
 *
 * Why it exists: `connectedCallback` runs again whenever an element is moved in the DOM, so a widget
 * that registers listeners there (or keeps "ready" flags) duplicates handlers, double-fires actions
 * or keeps long-lived resources that no longer match the DOM. Here the lifecycle is explicit:
 *
 *  - `init()`   once, on the first connect: build one-time DOM and default state.
 *  - `mount()`  on every connect: attach listeners/subscriptions (only through `on()` / `watch()`),
 *               refresh the view from state.
 *  - `unmount()` on every disconnect: stop timers, abort in-flight requests, release workers, flush
 *               pending saves. All listeners registered through `on()` / `watch()` are already gone
 *               (the shared AbortController is aborted first), so no manual removal is needed.
 *
 * Widget state that should survive a move lives in fields; resources that must not (workers) are
 * released in `unmount()`.
 */
import { watch as watchStore } from './store';

export abstract class LabElement extends HTMLElement {
	#ctl: AbortController | null = null;
	#inited = false;
	readonly #refs = new Map<string, Element>();

	/**
	 * Aborts when the element disconnects. Capture it before an `await` and check `.aborted` after,
	 * to drop results that arrive for an element that is gone. While disconnected it is already aborted.
	 */
	protected get signal(): AbortSignal {
		return this.#ctl?.signal ?? AbortSignal.abort();
	}

	/** Browser callback; widgets implement `init` / `mount` / `unmount` instead of overriding this. */
	connectedCallback(): void {
		if (this.#ctl) return;
		this.#ctl = new AbortController();
		if (!this.#inited) {
			this.#inited = true;
			this.init();
		}
		this.mount(this.#ctl.signal);
	}

	/** Browser callback; see `connectedCallback`. */
	disconnectedCallback(): void {
		const ctl = this.#ctl;
		if (!ctl) return;
		this.#ctl = null;
		ctl.abort();
		this.unmount();
	}

	/** First connect only: one-time DOM/state setup. */
	protected init(): void {}

	/** Every connect: register listeners via `on()` / `watch()` and render. Must be idempotent. */
	protected abstract mount(signal: AbortSignal): void;

	/** Every disconnect: release timers, workers and in-flight requests. */
	protected unmount(): void {}

	/** `addEventListener` that is removed automatically on disconnect. */
	protected on<K extends keyof HTMLElementEventMap>(
		target: HTMLElement,
		type: K,
		handler: (e: HTMLElementEventMap[K]) => void,
	): void;
	protected on<K extends keyof WindowEventMap>(target: Window, type: K, handler: (e: WindowEventMap[K]) => void): void;
	protected on(target: EventTarget, type: string, handler: (e: Event) => void): void;
	protected on(target: EventTarget, type: string, handler: (e: never) => void): void {
		target.addEventListener(type, handler as EventListener, { signal: this.signal });
	}

	/** Subscribes to a `store.ts` key until the element disconnects. */
	protected watch(key: string, fn: () => void): void {
		watchStore(key, fn, this.signal);
	}

	/** Typed lookup of `[data-ref="name"]` inside this widget; throws on a markup/script mismatch. */
	protected ref<T extends Element = HTMLElement>(name: string): T {
		let e = this.#refs.get(name);
		if (!e) {
			e = this.querySelector(`[data-ref="${name}"]`) ?? undefined;
			if (!e) throw new Error(`${this.localName}: missing [data-ref="${name}"]`);
			this.#refs.set(name, e);
		}
		return e as T;
	}
}

/** Registers a widget once (the script may be evaluated twice, e.g. by dev-server HMR). */
export function define(tag: string, cls: CustomElementConstructor): void {
	if (!customElements.get(tag)) customElements.define(tag, cls);
}
