// <lab-py> (src/viz/py/cell.ts) states with a scripted client: no worker, no network. The markup mirrors
// src/components/PyCell.astro; the last test checks that every data-ref the element uses exists there.
import './dom';
import { afterEach, describe, expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { DICT } from '../../src/i18n/ui';
// Side-effect import (defines <lab-py>); the type import alone would be erased.
import '../../src/viz/py/cell';
import type { LabPy } from '../../src/viz/py/cell';
import type { PyClient, RunResult } from '../../src/viz/py/pyclient';

const CODE = 'import numpy as np\nprint(np.dot([1, 2], [3, 4]))   # 11\n';

function mount(lang = 'en'): LabPy {
	document.documentElement.lang = lang;
	// Parsed detached, then attached: like a deferred module script, connectedCallback must see the children.
	const host = document.createElement('div');
	host.innerHTML = `
<lab-py class="lab-card" data-code="${CODE.replace(/"/g, '&quot;')}">
	<div data-ref="src"><pre data-language="python"><code>highlighted</code></pre></div>
	<textarea data-ref="edit" hidden></textarea>
	<div>
		<button type="button" data-ref="run" disabled>Run</button>
		<button type="button" data-ref="edit-btn" aria-pressed="false" disabled>Edit</button>
		<button type="button" data-ref="reset" disabled>Reset</button>
		<a data-ref="colab" hidden></a>
	</div>
	<p data-ref="status" role="status">first</p>
	<pre class="is-empty" data-ref="out">placeholder</pre>
</lab-py>`;
	document.body.append(host);
	return host.querySelector('lab-py') as LabPy;
}

/** A client whose next results are queued by the test; `loaded` flips after the first success like the real one. */
function client(results: RunResult[], loadedAtStart = false) {
	const codes: string[] = [];
	let release: (() => void) | null = null;
	let loaded = loadedAtStart;
	const py: PyClient = {
		get loaded() {
			return loaded;
		},
		dispose() {},
		run(code) {
			codes.push(code);
			const r = results.shift()!;
			return new Promise((resolve) => {
				release = () => {
					if (!r.fail) loaded = true;
					resolve(r);
				};
			});
		},
	};
	return { py, codes, finish: () => release?.() };
}

const $ = <T extends Element = HTMLElement>(el: Element, ref: string) => el.querySelector(`[data-ref="${ref}"]`) as unknown as T;
const click = (el: Element, ref: string) => $<HTMLButtonElement>(el, ref).click();
const tick = () => new Promise((r) => setTimeout(r, 0));

afterEach(() => {
	document.body.innerHTML = '';
});

describe('<lab-py>', () => {
	test('hydration enables Run and Edit; Reset stays disabled until there is something to reset', () => {
		const el = mount();
		expect($<HTMLButtonElement>(el, 'run').disabled).toBe(false);
		expect($<HTMLButtonElement>(el, 'edit-btn').disabled).toBe(false);
		expect($<HTMLButtonElement>(el, 'reset').disabled).toBe(true);
	});

	test('first run: loading -> output and "done"; buttons are disabled while busy', async () => {
		const el = mount();
		const c = client([{ out: '11\n' }]);
		el.py = c.py;
		click(el, 'run');
		expect($(el, 'status').textContent).toBe(DICT.en['py.loading']);
		expect($<HTMLButtonElement>(el, 'run').disabled).toBe(true);
		expect($(el, 'out').getAttribute('aria-busy')).toBe('true');
		expect(c.codes).toEqual([CODE]);
		c.finish();
		await tick();
		expect($(el, 'out').textContent).toBe('11\n');
		expect($(el, 'out').classList.contains('is-empty')).toBe(false);
		expect($(el, 'status').className).toContain('is-ok');
		expect($(el, 'status').textContent).toMatch(/^Done in \d+\.\d\d s\.$/);
		expect($<HTMLButtonElement>(el, 'run').disabled).toBe(false);
		expect($<HTMLButtonElement>(el, 'reset').disabled).toBe(false);
		// A second run on a loaded client says "Running", not "Loading".
		const c2 = client([{ out: '11\n' }], true);
		el.py = c2.py;
		click(el, 'run');
		expect($(el, 'status').textContent).toBe(DICT.en['py.running']);
		c2.finish();
		await tick();
	});

	test('Python exception: output and traceback shown, error status, Run stays available', async () => {
		const el = mount();
		const c = client([{ out: 'partial', err: 'ZeroDivisionError: division by zero', fail: 'py' }]);
		el.py = c.py;
		click(el, 'run');
		c.finish();
		await tick();
		expect($(el, 'out').textContent).toBe('partial\nZeroDivisionError: division by zero');
		expect($(el, 'status').className).toContain('is-err');
		expect($(el, 'status').textContent).toBe(DICT.en['py.failPy']);
		expect($<HTMLButtonElement>(el, 'run').disabled).toBe(false);
	});

	test('load failure and timeout: localized retry messages (Russian page); retry runs again', async () => {
		const el = mount('ru');
		const c = client([{ out: '', err: 'TypeError: Failed to fetch', fail: 'load' }, { out: '', fail: 'timeout' }, { out: 'ok\n' }]);
		el.py = c.py;
		click(el, 'run');
		c.finish();
		await tick();
		expect($(el, 'status').textContent).toBe(DICT.ru['py.failLoad']);
		expect($(el, 'out').textContent).toBe('TypeError: Failed to fetch');
		expect($(el, 'colab').hidden).toBe(true); // no notebook given: no dead link
		click(el, 'run');
		c.finish();
		await tick();
		expect($(el, 'status').textContent).toBe(DICT.ru['py.failTimeout'].replace('{s}', '30'));
		click(el, 'run');
		c.finish();
		await tick();
		expect($(el, 'out').textContent).toBe('ok\n');
		expect(c.codes.length).toBe(3);
	});

	test('a failure offers the Colab notebook when the cell names one', async () => {
		const el = mount();
		$(el, 'colab').setAttribute('href', 'https://colab.research.google.com/github/x');
		const c = client([{ out: '', fail: 'load' }]);
		el.py = c.py;
		click(el, 'run');
		c.finish();
		await tick();
		expect($(el, 'colab').hidden).toBe(false);
	});

	test('Edit swaps in a textarea with the code, Ctrl+Enter runs the edited text, Reset restores everything', async () => {
		const el = mount();
		const c = client([{ out: '7\n' }]);
		el.py = c.py;
		click(el, 'edit-btn');
		const box = $<HTMLTextAreaElement>(el, 'edit');
		expect(box.hidden).toBe(false);
		expect($(el, 'src').hidden).toBe(true);
		expect(box.value).toBe(CODE);
		expect(document.activeElement).toBe(box);
		expect($(el, 'edit-btn').getAttribute('aria-pressed')).toBe('true');
		expect($<HTMLButtonElement>(el, 'reset').disabled).toBe(false);
		box.value = 'print(7)';
		box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, bubbles: true, cancelable: true }));
		expect(c.codes).toEqual(['print(7)']);
		c.finish();
		await tick();
		expect($(el, 'out').textContent).toBe('7\n');
		click(el, 'reset');
		expect(box.hidden).toBe(true);
		expect($(el, 'src').hidden).toBe(false);
		expect($(el, 'out').textContent).toBe(DICT.en['py.placeholder']);
		expect($(el, 'out').classList.contains('is-empty')).toBe(true);
		expect($(el, 'status').textContent).toBe(DICT.en['py.ready']);
		expect($<HTMLButtonElement>(el, 'reset').disabled).toBe(true);
		expect(document.activeElement).toBe($(el, 'run'));
	});

	test('every data-ref of the element exists in PyCell.astro', () => {
		const astro = readFileSync(join(import.meta.dir, '../../src/components/PyCell.astro'), 'utf8');
		const cell = readFileSync(join(import.meta.dir, '../../src/viz/py/cell.ts'), 'utf8');
		const refs = new Set([...cell.matchAll(/ref(?:<\w+>)?\('([\w-]+)'\)/g)].map((m) => m[1]));
		expect(refs.size).toBeGreaterThan(5);
		for (const r of refs) expect(astro).toContain(`data-ref="${r}"`);
	});
});
