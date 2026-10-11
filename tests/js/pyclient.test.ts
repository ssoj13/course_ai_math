// In-browser Python: the page/worker protocol (src/viz/py/pyclient.ts) with fake workers (no network, no Pyodide),
// plus the pure helpers of protocol.ts and fence.ts and the lint that lesson cells wrap exactly one python fence.
import { describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { createPyClient, type WorkerLike } from '../../src/viz/py/pyclient';
import { cleanTrace, type PyReply, type PyRequest } from '../../src/viz/py/protocol';
import { decodeAttr, fenceCode, MAX_ROWS, outputRows } from '../../src/viz/py/fence';


/**
 * A fake worker scripted per request by `answer`: `undefined` = silent (never even ready), `'hang'` = ready but no
 * reply (code that never ends), a reply = `ready` then the reply, except a load failure, which comes without `ready`
 * (the worker could not start Python).
 */
function fake(answer: (m: PyRequest, n: number) => Omit<PyReply, 'id'> | 'hang' | undefined) {
	const sent: PyRequest[] = [];
	let terminated = 0;
	const w: WorkerLike = {
		onmessage: null,
		onerror: null,
		terminate: () => void terminated++,
		postMessage(m) {
			sent.push(m);
			const r = answer(m, sent.length);
			if (r === undefined) return;
			queueMicrotask(() => {
				if (r === 'hang' || r.fail !== 'load') w.onmessage?.({ data: { id: m.id, ready: true } });
				if (r !== 'hang') w.onmessage?.({ data: { id: m.id, ...r } });
			});
		},
	};
	return { w, sent, terminated: () => terminated };
}

const T = { loadMs: 30, runMs: 20 };

describe('pyclient', () => {
	test('does not spawn a worker before the first run', () => {
		let spawned = 0;
		createPyClient(() => (spawned++, fake(() => ({ out: '' })).w), T);
		expect(spawned).toBe(0);
	});

	test('echoes output from the worker protocol, reuses one worker and reports loaded to watchers', async () => {
		let spawned = 0;
		const f = fake((m) => ({ out: `ran ${m.code}\n` }));
		const py = createPyClient(() => (spawned++, f.w), T);
		const seen: boolean[] = [];
		py.watch((v) => seen.push(v));
		expect(py.loaded).toBe(false);
		expect(await py.run('print(11)')).toEqual({ out: 'ran print(11)\n', err: undefined, fail: undefined });
		expect(py.loaded).toBe(true);
		await py.run('x');
		expect(spawned).toBe(1);
		expect(f.sent.map((m) => m.id)).toEqual([1, 2]);
		expect(seen).toEqual([true]); // changes only, not every run
		py.dispose();
		expect(seen).toEqual([true, false]);
	});

	test('a worker that is never ready fails as "load" and is terminated; Run again respawns (retry)', async () => {
		const workers = [fake(() => undefined), fake(() => ({ out: '11\n' }))];
		let i = 0;
		const py = createPyClient(() => workers[i++]!.w, T);
		const r = await py.run('print(11)');
		expect(r.fail).toBe('load');
		expect(workers[0]!.terminated()).toBe(1);
		expect((await py.run('print(11)')).out).toBe('11\n');
		expect(i).toBe(2);
	});

	test('code that never ends is a run timeout on runMs, even as the first run of a fresh worker', async () => {
		const first = fake(() => 'hang');
		const second = fake(() => ({ out: 'again\n' }));
		const queue = [first, second];
		// The load clock is far longer than the run clock: the result must come from the run clock.
		const py = createPyClient(() => queue.shift()!.w, { loadMs: 5_000, runMs: 20 });
		const t0 = performance.now();
		expect(await py.run('while True: pass')).toEqual({ out: '', fail: 'timeout' });
		expect(performance.now() - t0).toBeLessThan(1_000);
		expect(first.terminated()).toBe(1);
		expect(py.loaded).toBe(false);
		expect((await py.run('b')).out).toBe('again\n');
	});

	test('a worker-reported load failure drops the worker; a Python error keeps it', async () => {
		const workers = [
			fake(() => ({ out: '', err: 'TypeError: Failed to fetch', fail: 'load' })),
			fake(() => ({ out: 'partial\n', err: 'ZeroDivisionError: division by zero', fail: 'py' })),
		];
		let i = 0;
		const py = createPyClient(() => workers[i++]!.w, T);
		expect(await py.run('x')).toEqual({ out: '', err: 'TypeError: Failed to fetch', fail: 'load' });
		expect(workers[0]!.terminated()).toBe(1);
		const r = await py.run('1/0');
		expect(r.fail).toBe('py');
		expect(r.out).toBe('partial\n');
		expect(workers[1]!.terminated()).toBe(0);
		expect(py.loaded).toBe(true);
	});

	test('a worker error event (script failed to load) resolves as "load"', async () => {
		const w: WorkerLike = {
			onmessage: null,
			onerror: null,
			terminate() {},
			postMessage() {
				queueMicrotask(() => w.onerror?.({ message: 'NetworkError' }));
			},
		};
		expect(await createPyClient(() => w, T).run('x')).toEqual({ out: '', err: 'NetworkError', fail: 'load' });
	});

	test('a worker that cannot be created is a "load" failure and does not poison the queue', async () => {
		let calls = 0;
		const ok = fake(() => ({ out: 'fine\n' }));
		const py = createPyClient(() => {
			if (calls++ === 0) throw new Error('Refused to create a worker (CSP worker-src)');
			return ok.w;
		}, T);
		expect(await py.run('a')).toEqual({ out: '', err: 'Refused to create a worker (CSP worker-src)', fail: 'load' });
		expect((await py.run('b')).out).toBe('fine\n');
	});

	test('runs are serialised: the second request is posted after the first reply', async () => {
		const order: string[] = [];
		const w: WorkerLike = {
			onmessage: null,
			onerror: null,
			terminate() {},
			postMessage(m) {
				order.push(`post ${m.code}`);
				setTimeout(() => {
					order.push(`reply ${m.code}`);
					w.onmessage?.({ data: { id: m.id, out: m.code } });
				}, 5);
			},
		};
		const py = createPyClient(() => w, T);
		const [a, b] = await Promise.all([py.run('a'), py.run('b')]);
		expect([a.out, b.out]).toEqual(['a', 'b']);
		expect(order).toEqual(['post a', 'reply a', 'post b', 'reply b']);
	});

	test('a late reply of a timed-out run is ignored', async () => {
		const f = fake(() => undefined);
		const py = createPyClient(() => f.w, T);
		const r = await py.run('x');
		f.w.onmessage?.({ data: { id: 1, out: 'late' } }); // handler already detached: nothing happens
		expect(r).toEqual({ out: '', fail: 'load' });
	});
});

describe('protocol helpers', () => {
	test('cleanTrace drops Pyodide runner frames and keeps the reader\'s frames', () => {
		const trace = [
			'Traceback (most recent call last):',
			'  File "/lib/python314.zip/_pyodide/_base.py", line 597, in eval_code_async',
			'    await CodeRunner(',
			'    ...<9 lines>...',
			'  File "/lib/python314.zip/_pyodide/_base.py", line 411, in run_async',
			'    coroutine = eval(self.code, globals, locals)',
			'  File "<exec>", line 3, in <module>',
			'    print(1 / 0)',
			'          ~~^~~',
			'ZeroDivisionError: division by zero',
		].join('\n');
		expect(cleanTrace(trace)).toBe(
			[
				'Traceback (most recent call last):',
				'  File "<exec>", line 3, in <module>',
				'    print(1 / 0)',
				'          ~~^~~',
				'ZeroDivisionError: division by zero',
			].join('\n'),
		);
	});
});

describe('fence', () => {
	// Shape of an Expressive Code block as rendered in dist/ (copy payload lines joined by U+007F).
	const ec = (code: string, lang = 'python') =>
		`<div class="expressive-code"><figure class="frame"><pre data-language="${lang}"><code>…</code></pre>` +
		`<div class="copy"><button title="Copy" data-code="${code}"><div></div></button></div></figure></div>`;

	test('recovers the exact code text, entities and blank lines included', () => {
		const html = ec('import numpy as np\u007f\u007fprint(f&quot;{x:4d}° &lt;&amp;&gt; &#39;a&#39;&quot;)\u007f');
		expect(fenceCode(html)).toBe('import numpy as np\n\nprint(f"{x:4d}° <&> \'a\'")\n');
	});

	test('rejects zero or two blocks, another language, and a missing copy payload', () => {
		expect(() => fenceCode('<p>no code</p>')).toThrow(/exactly one fenced code block, found 0/);
		expect(() => fenceCode(ec('a') + ec('b'))).toThrow(/found 2/);
		expect(() => fenceCode(ec('a', 'ts'))).toThrow(/must be ```python/);
		expect(() => fenceCode('<pre data-language="python"><code>x</code></pre>')).toThrow(/copy payload/);
	});

	test('decodeAttr handles numeric references and refuses unknown names', () => {
		expect(decodeAttr('&#x3C;&#60;&LT;')).toBe('<<<');
		expect(() => decodeAttr('&nbsp;')).toThrow(/unknown character reference/);
	});

	test('outputRows: prints, literal loops, documented matrix rows, clamped to 2..MAX_ROWS', () => {
		expect(outputRows('x = 1')).toBe(2);
		expect(outputRows('print(1)\nprint(2)\nprint(3)')).toBe(3);
		expect(outputRows('print(1)\n'.repeat(40))).toBe(MAX_ROWS);
		// A print in a loop over a literal tuple prints once per item; code after the loop counts once again.
		expect(outputRows('for d in (0, 30, 60, 90):\n    r = d\n    print(r)\nprint("end")')).toBe(5);
		expect(outputRows('for i in range(2, 5):\n    for j in [1, 2]:\n        print(i, j)')).toBe(6);
		expect(outputRows('for i in range(n):\n    print(i)\nprint(1)\nprint(2)')).toBe(3); // unknown count: once
		// A matrix whose rows are documented in the comment lines below its print.
		expect(outputRows('print(np.round(S, 2))\n# [[1.   0.99]\n#  [0.99 1.  ]]\nprint(1)')).toBe(3);
	});
});

describe('lesson cells', () => {
	const DOCS = join(import.meta.dir, '../../src/content/docs');
	const mdx = (dir: string): string[] =>
		readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
			e.isDirectory() ? mdx(join(dir, e.name)) : e.name.endsWith('.mdx') ? [join(dir, e.name)] : [],
		);
	const cells = (src: string) => [...src.matchAll(/^<PyCell\b[^>]*>\r?\n([\s\S]*?)^<\/PyCell>$/gm)].map((m) => m[1] ?? '');

	test('every <PyCell> wraps exactly one ```python fence and nothing else', () => {
		const bad: string[] = [];
		for (const file of mdx(DOCS)) {
			for (const body of cells(readFileSync(file, 'utf8'))) {
				if (!/^\s*```python\r?\n[\s\S]*?\r?\n```\s*$/.test(body) || (body.match(/^```/gm)?.length ?? 0) !== 2) {
					bad.push(relative(DOCS, file));
				}
			}
		}
		expect(bad).toEqual([]);
	});

	test('Russian pages run the same code as their English source', () => {
		const ru = join(DOCS, 'ru');
		for (const file of mdx(ru)) {
			const en = join(DOCS, relative(ru, file));
			expect({ file: relative(DOCS, file), cells: cells(readFileSync(file, 'utf8')) }).toEqual({
				file: relative(DOCS, file),
				cells: cells(readFileSync(en, 'utf8')),
			});
		}
	});
});
