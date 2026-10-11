/**
 * Per-viewer persistence for course widgets (progress, per-lesson visualization state).
 *
 * Browser storage can be missing or throw (private windows, blocked site data), so every
 * access is guarded and widgets must still render with defaults when it is unavailable.
 * When a write fails the value is kept in memory for the lifetime of the page, so toggles and
 * settings still work (and read back consistently) until the reader leaves; `save()` still returns
 * false so callers can warn that nothing was persisted.
 * All keys are namespaced to avoid clashing with other sites on the shared
 * `ssoj13.github.io` origin.
 */

const PREFIX = 'math-course:';

/**
 * One storage area (localStorage = survives the browser, sessionStorage = this tab only) with its own
 * page-lifetime fallback: serialized values of keys whose last write failed.
 */
interface Area {
	get(): Storage;
	memory: Map<string, string>;
}

const local: Area = { get: () => localStorage, memory: new Map() };
const session: Area = { get: () => sessionStorage, memory: new Map() };

function read<T>(area: Area, key: string, fallback: T): T {
	try {
		const raw = area.memory.get(key) ?? area.get().getItem(PREFIX + key);
		return raw === null ? fallback : (JSON.parse(raw) as T);
	} catch {
		return fallback;
	}
}

function write<T>(area: Area, key: string, value: T): boolean {
	let raw: string;
	try {
		raw = JSON.stringify(value);
	} catch {
		return false;
	}
	let persisted = true;
	try {
		area.get().setItem(PREFIX + key, raw);
		area.memory.delete(key);
	} catch {
		area.memory.set(key, raw);
		persisted = false;
	}
	window.dispatchEvent(new CustomEvent('math-course:store', { detail: { key } }));
	return persisted;
}

/** Reads a JSON value; returns `fallback` when absent, unreadable or storage is blocked. */
export function load<T>(key: string, fallback: T): T {
	return read(local, key, fallback);
}

/**
 * Writes a JSON value and notifies watchers (also when only the in-memory fallback took it).
 * Returns false when storage is unavailable so callers can say the value will not survive a reload.
 */
export function save<T>(key: string, value: T): boolean {
	return write(local, key, value);
}

/**
 * `load` for sessionStorage: for secrets that must not outlive the tab. It is still per origin, so a page of
 * another repo on the shared `ssoj13.github.io` can read it only while it is open in this very tab
 * (localStorage would expose it to every tab, and for good).
 */
export function loadSession<T>(key: string, fallback: T): T {
	return read(session, key, fallback);
}

/** `save` for sessionStorage, with the same in-memory fallback and change notification. */
export function saveSession<T>(key: string, value: T): boolean {
	return write(session, key, value);
}

/**
 * Subscribes to changes of one key made by any widget on the page (or another tab).
 * Returns an unsubscribe function; passing `signal` also unsubscribes when it aborts.
 */
export function watch(key: string, fn: () => void, signal?: AbortSignal): () => void {
	const ctl = new AbortController();
	if (signal?.aborted) ctl.abort();
	else signal?.addEventListener('abort', () => ctl.abort(), { once: true });
	window.addEventListener(
		'math-course:store',
		(e) => {
			if ((e as CustomEvent<{ key: string }>).detail?.key === key) fn();
		},
		{ signal: ctl.signal },
	);
	window.addEventListener(
		'storage',
		(e) => {
			if (e.key === PREFIX + key) fn();
		},
		{ signal: ctl.signal },
	);
	return () => ctl.abort();
}

/** Storage keys shared by more than one widget. */
export const KEYS = {
	/** Record<lesson slug (course.json), boolean> */
	progress: 'progress',
	/** Key prefix of per-lesson persisted viz state: `lesson:<slug>` (see the lesson store in src/viz/store.ts) */
	lessonState: 'lesson',
	/** Record<`challenge:<lesson>:<n>`, true>: solved Challenge boxes (src/lib/challenges.ts) */
	challenges: 'challenges',
} as const;
