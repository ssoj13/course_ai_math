/**
 * Page side of the in-browser Python: one Pyodide worker per page, spawned on the first `run()` and shared by every
 * cell (`sharedPy()`), so NumPy is downloaded and initialised once per page, not once per cell.
 *
 * Runs are serialised: a worker executes one cell at a time anyway, and a queue keeps each run's timeout honest
 * (the clock starts when the request is posted, not while it waits behind another cell). A failed run never blocks
 * the queue: every run settles with a `RunResult`, even when the worker cannot be created.
 *
 * Failure handling is the client's job because only the client can see a worker that never answers. Each run has
 * two clocks, split by the worker's `ready` notice (protocol.ts):
 * - until `ready` (download, start of Python, the cell's packages): `loadMs`; a timeout means Python could not be
 *   loaded (`fail: 'load'`);
 * - after `ready` (the code itself): `runMs`; a timeout means the code ran too long (`fail: 'timeout'`), e.g. an
 *   endless loop. A worker stuck in Python cannot be interrupted from outside without SharedArrayBuffer (which GitHub
 *   Pages cannot enable: no COOP/COEP headers), so the worker is terminated and the next run starts a fresh one;
 * - a load failure reported by the worker, a worker `error` event or a worker that cannot be created also drop the
 *   worker, so pressing Run again is the retry.
 * Results carry a `Fail` code, not English text: the cell renders the message in the page locale.
 */
import { isReady, type Fail, type PyMessage, type PyRequest } from './protocol';

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
	onmessage: ((e: { data: PyMessage }) => void) | null;
	onerror: ((e: { message?: string }) => void) | null;
}

export interface PyTimeouts {
	/** Until the worker's `ready` notice: download and start of Python, the cell's packages. */
	loadMs: number;
	/** After `ready`: the code. */
	runMs: number;
}

export const DEFAULT_TIMEOUTS: PyTimeouts = { loadMs: 120_000, runMs: 30_000 };

export interface PyClient {
	run(code: string): Promise<RunResult>;
	/** Whether the current worker has Python running (the next run starts without a download). */
	readonly loaded: boolean;
	/** The run clock (`runMs`), for the timeout message. */
	readonly runMs: number;
	/** Calls `fn` whenever `loaded` changes (every cell of the page updates its idle note); returns the unsubscribe. */
	watch(fn: (loaded: boolean) => void): () => void;
	/** Terminates the worker; a later `run()` starts a new one. */
	dispose(): void;
}

export function createPyClient(spawn: () => WorkerLike, timeouts: PyTimeouts = DEFAULT_TIMEOUTS): PyClient {
	let worker: WorkerLike | null = null;
	let loaded = false;
	let nextId = 1;
	let tail: Promise<unknown> = Promise.resolve();
	const watchers = new Set<(loaded: boolean) => void>();

	const setLoaded = (v: boolean) => {
		if (v === loaded) return;
		loaded = v;
		for (const fn of watchers) fn(v);
	};

	const drop = () => {
		if (worker) {
			worker.onmessage = null;
			worker.onerror = null;
			worker.terminate();
		}
		worker = null;
		setLoaded(false);
	};

	const exec = (code: string) =>
		new Promise<RunResult>((resolve) => {
			try {
				worker ??= spawn();
			} catch (e) {
				// e.g. a CSP `worker-src` that forbids the worker: the same "cannot load" path as a failed download.
				resolve({ out: '', err: e instanceof Error ? e.message : String(e), fail: 'load' });
				return;
			}
			const w = worker;
			const id = nextId++;
			let started = false; // the worker's `ready` notice for this run arrived
			let timer: ReturnType<typeof setTimeout>;
			const arm = (ms: number) => {
				clearTimeout(timer);
				timer = setTimeout(() => {
					drop();
					finish({ out: '', fail: started ? 'timeout' : 'load' });
				}, ms);
			};
			const finish = (r: RunResult) => {
				clearTimeout(timer);
				w.onmessage = null;
				w.onerror = null;
				resolve(r);
			};
			w.onmessage = ({ data }) => {
				if (data.id !== id) return; // a late message of a run that already timed out
				if (isReady(data)) {
					started = true;
					setLoaded(true);
					arm(timeouts.runMs);
					return;
				}
				if (data.fail === 'load') drop();
				finish({ out: data.out, err: data.err, fail: data.fail });
			};
			w.onerror = (e) => {
				drop();
				finish({ out: '', err: e.message, fail: 'load' });
			};
			arm(timeouts.loadMs);
			w.postMessage({ id, code });
		});

	return {
		run(code) {
			const go = () => exec(code);
			// `go` on both branches: a queue that rejected once would otherwise reject every later run.
			const r = tail.then(go, go);
			tail = r;
			return r;
		},
		get loaded() {
			return loaded;
		},
		runMs: timeouts.runMs,
		watch(fn) {
			watchers.add(fn);
			return () => watchers.delete(fn);
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
