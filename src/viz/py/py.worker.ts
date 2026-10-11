/**
 * Pyodide worker: runs the lesson's Python cells off the main thread (a NumPy import or a long loop never freezes
 * the page or its scenes). Pyodide itself is NOT bundled: it is imported from jsDelivr at the pinned
 * `PYODIDE_VERSION` (protocol.ts) inside the handling of the first request, so a reader who never presses Run
 * downloads nothing. The browser HTTP cache keeps the ~10 MB runtime for later pages.
 *
 * Every cell runs in a fresh globals dict: a lesson's cells are self-contained (each starts with its imports), the
 * printed numbers must match the comments in the lesson no matter which cell ran before, and "Reset" really resets.
 * Packages are loaded on demand from the cell's imports (`loadPackagesFromImports`): NumPy only when a cell uses it.
 */
import { cleanTrace, PYODIDE_URL, type PyMessage, type PyRequest } from './protocol';

/** The part of Pyodide's API this worker uses (the package's types are not a dependency: it is loaded at runtime). */
interface PyProxyDict {
	destroy(): void;
}
interface Pyodide {
	setStdout(o: { write: (buf: Uint8Array) => number; isatty?: boolean }): void;
	setStderr(o: { write: (buf: Uint8Array) => number; isatty?: boolean }): void;
	loadPackagesFromImports(
		code: string,
		o: { messageCallback: (m: string) => void; errorCallback: (m: string) => void },
	): Promise<unknown>;
	runPythonAsync(code: string, o: { globals: PyProxyDict }): Promise<unknown>;
	globals: { get(name: 'dict'): () => PyProxyDict };
}

let boot: Promise<Pyodide> | null = null;

/** Loads Pyodide once; a failed load is forgotten so a later request can try again. */
function pyodide(): Promise<Pyodide> {
	boot ??= (import(/* @vite-ignore */ PYODIDE_URL) as Promise<{ loadPyodide: () => Promise<Pyodide> }>)
		.then((m) => m.loadPyodide())
		.catch((e: unknown) => {
			boot = null;
			throw e;
		});
	return boot;
}

const post = (m: PyMessage) => (self as unknown as { postMessage(m: PyMessage): void }).postMessage(m);
const detail = (e: unknown) => (e instanceof Error ? e.message : String(e));

async function handle({ id, code }: PyRequest): Promise<void> {
	let py: Pyodide;
	try {
		py = await pyodide();
	} catch (e) {
		post({ id, out: '', err: detail(e), fail: 'load' });
		return;
	}

	// stdout and stderr share one buffer so a warning shows up where Python raised it. `isatty: true` makes Python
	// line-buffer stdout (a non-tty stdout is block-buffered and would print after stderr).
	let out = '';
	const sink = () => {
		const dec = new TextDecoder();
		return {
			isatty: true,
			write: (buf: Uint8Array) => {
				out += dec.decode(buf, { stream: true });
				return buf.length;
			},
		};
	};
	py.setStdout(sink());
	py.setStderr(sink());

	// Package download failures are reported through errorCallback rather than always rejecting; both mean "load".
	const loadErrs: string[] = [];
	try {
		await py.loadPackagesFromImports(code, { messageCallback: () => {}, errorCallback: (m) => loadErrs.push(m) });
	} catch (e) {
		loadErrs.push(detail(e));
	}
	if (loadErrs.length) {
		post({ id, out, err: loadErrs.join('\n'), fail: 'load' });
		return;
	}

	post({ id, ready: true }); // from here on the client times the code, not the download
	const globals = py.globals.get('dict')();
	try {
		await py.runPythonAsync(code, { globals });
		post({ id, out });
	} catch (e) {
		post({ id, out, err: cleanTrace(detail(e)), fail: 'py' });
	} finally {
		globals.destroy();
	}
}

// Requests are handled one after another: stdout redirection and the globals are per request.
let queue: Promise<void> = Promise.resolve();
self.onmessage = (e: MessageEvent<PyRequest>) => {
	queue = queue.then(() => handle(e.data));
};
