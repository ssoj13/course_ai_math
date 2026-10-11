/**
 * Message protocol between the page (`pyclient.ts`) and the Pyodide worker (`py.worker.ts`), plus the pure helpers
 * both sides share. DOM-free and worker-free, so the tests import it directly.
 *
 * One request `{ id, code }` gets exactly one reply `{ id, out, err?, fail? }`. There is no separate init message:
 * the worker loads Pyodide inside the handling of its first request, so a page that never presses Run never
 * downloads anything (C5 / Task 8: "Pyodide starts on Run").
 */

/**
 * Pyodide release served from jsDelivr. Checked 2026-10-11: `pyodide.mjs` of this version answers 200 and its
 * `pyodide-lock.json` reports CPython 3.14.2 with numpy 2.4.6 (Pyodide's version tracks CPython since 314.x).
 */
export const PYODIDE_VERSION = '314.0.7';

/** ES module entry of the pinned release; `loadPyodide()` finds the rest of the distribution next to it. */
export const PYODIDE_URL = `https://cdn.jsdelivr.net/pyodide/v${PYODIDE_VERSION}/full/pyodide.mjs`;

/**
 * One-time download of the first run, rounded up, for the first-run note. Measured 2026-10-11 on the production preview
 * (encoded response bodies from jsDelivr): pyodide.asm.wasm 3.44 MB, python_stdlib.zip 2.51 MB, numpy wheel 2.93 MB,
 * pyodide.asm.mjs 0.26 MB, lock + loader 0.03 MB = 9.18 MB (task8-report.md).
 */
export const DOWNLOAD_MB = 10;

/** Why a run failed. `load`: Python or a package could not be downloaded; `py`: the code raised; `timeout`: client side. */
export type Fail = 'load' | 'py' | 'timeout';

/** Page -> worker. */
export interface PyRequest {
	id: number;
	code: string;
}

/** Worker -> page: exactly one per request. `out` is stdout and stderr in the order Python wrote them. */
export interface PyReply {
	id: number;
	out: string;
	/** Technical detail of a failure (Python traceback, network error); shown verbatim, never translated. */
	err?: string;
	fail?: Exclude<Fail, 'timeout'>;
}

/**
 * Drops Pyodide's own frames from a Python traceback. Every exception raised by user code first passes through
 * `/lib/python3XX.zip/_pyodide/...` (the runner that executes the cell); those frames say nothing about the reader's
 * code and push the real line ("File "<exec>", line 3") out of view. A frame is the `  File ...` line plus the
 * indented source lines under it; only frames whose path starts with `/lib/` are removed.
 */
export function cleanTrace(trace: string): string {
	const lines = trace.split('\n');
	const kept: string[] = [];
	let skipping = false;
	for (const line of lines) {
		if (/^ {2}File "/.test(line)) skipping = line.startsWith('  File "/lib/');
		else if (!/^ {3,}/.test(line)) skipping = false;
		if (!skipping) kept.push(line);
	}
	return kept.join('\n');
}
