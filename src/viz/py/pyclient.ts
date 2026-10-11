/**
 * Page side of the in-browser Python: one Pyodide worker per page, spawned on the first `run()` and shared by every
 * cell (`sharedPy()`), so NumPy is downloaded and initialised once per page, not once per cell.
 *
 * Runs are serialised: a worker executes one cell at a time anyway, and a queue keeps each run's timeout honest
 * (the clock starts when the request is posted, not while it waits behind another cell).
 *
 * Failure handling is the client's job because only the client can see a worker that never answers:
 * - the first run of a worker may take long (download + NumPy), so it gets `loadMs`; a timeout there means Python
 *   could not be loaded (`fail: 'load'`);
 * - later runs get `runMs`; a timeout there means the code itself ran too long (`fail: 'timeout'`), e.g. an endless
 *   loop. A worker stuck in Python cannot be interrupted from outside without SharedArrayBuffer (which GitHub Pages
 *   cannot enable: no COOP/COEP headers), so the worker is terminated and the next run starts a fresh one;
 * - a load failure reported by the worker or a worker `error` event also drops the worker, so pressing Run again is
 *   the retry.
 * Results carry a `Fail` code, not English text: the cell renders the message in the page locale.
 */
import type { Fail, PyReply, PyRequest } from './protocol';

/** Result of one run: printed output, and on failure its kind and technical detail. */
export interface RunResult {
	out: string;
	err?: string;
	fail?: Fail;
}

/** The part of `Worker` the client uses; tests pass fakes. The client assigns `onmessage` / `onerror` directly. */
export interface WorkerLike {
	postMessage(msg: PyRequest): void;
	terminate(): void;
	onmessage: ((e: { data: PyReply }) => void) | null;
	onerror: ((e: { message?: string }) => void) | null;
}

export interface PyTimeouts {
	/** First run of a fresh worker: download + start of Python, NumPy, the code. */
	loadMs: number;
	/** Every later run: just the code. */
	runMs: number;
}

export const DEFAULT_TIMEOUTS: PyTimeouts = { loadMs: 120_000, runMs: 30_000 };

export interface PyClient {
	run(code: string): Promise<RunResult>;
	/** Whether the current worker has answered at least once (Python is loaded; the next run is fast). */
	readonly loaded: boolean;
	/** Terminates the worker; a later `run()` starts a new one. */
	dispose(): void;
}

export function createPyClient(spawn: () => WorkerLike, timeouts: PyTimeouts = DEFAULT_TIMEOUTS): PyClient {
	let worker: WorkerLike | null = null;
	let loaded = false;
	let nextId = 1;
	let tail: Promise<unknown> = Promise.resolve();

	const drop = () => {
		if (worker) {
			worker.onmessage = null;
			worker.onerror = null;
			worker.terminate();
		}
		worker = null;
		loaded = false;
	};

	const exec = (code: string) =>
		new Promise<RunResult>((resolve) => {
			if (!worker) worker = spawn();
			const w = worker;
			const id = nextId++;
			const wasLoaded = loaded;
			const finish = (r: RunResult) => {
				clearTimeout(timer);
				w.onmessage = null;
				w.onerror = null;
				resolve(r);
			};
			const timer = setTimeout(() => {
				drop();
				finish({ out: '', fail: wasLoaded ? 'timeout' : 'load' });
			}, wasLoaded ? timeouts.runMs : timeouts.loadMs);
			w.onmessage = ({ data }) => {
				if (data.id !== id) return; // a late reply of a run that already timed out
				if (data.fail === 'load') drop();
				else loaded = true;
				finish({ out: data.out, err: data.err, fail: data.fail });
			};
			w.onerror = (e) => {
				drop();
				finish({ out: '', err: e.message, fail: 'load' });
			};
			w.postMessage({ id, code });
		});

	return {
		run(code) {
			const r = tail.then(() => exec(code));
			tail = r;
			return r;
		},
		get loaded() {
			return loaded;
		},
		dispose: drop,
	};
}

let shared: PyClient | null = null;

/** The page's one client. The worker module is fetched by `new Worker(...)`, i.e. only on the first Run. */
export function sharedPy(): PyClient {
	// The literal `new Worker(new URL(..., import.meta.url), { type: 'module' })` form is what Vite detects to bundle
	// the worker as its own chunk.
	return (shared ??= createPyClient(
		() => new Worker(new URL('./py.worker.ts', import.meta.url), { type: 'module' }) as unknown as WorkerLike,
	));
}
