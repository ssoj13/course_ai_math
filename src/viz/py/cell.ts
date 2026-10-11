/**
 * `<lab-py>`: the client side of PyCell.astro. The server renders everything the reader sees before a click (the
 * highlighted code, disabled buttons, the first-run note and an output pane of reserved height); this element only
 * enables the buttons and runs the code through the page's shared Pyodide client.
 *
 * States, shown in the one-line status (`role=status`, so screen readers hear them):
 *   idle (first-run note or "loaded") -> loading (first run) / running -> done | failLoad | failPy | failTimeout.
 * An idle cell follows the shared client: once any cell of the page has loaded Python, every idle cell says so.
 * Every failure leaves Run enabled: pressing it again is the retry (the client has already dropped a dead worker).
 *
 * Editing: "Edit" swaps the highlighted block for a textarea of the same height holding the code; Ctrl/Cmd+Enter
 * runs it. "Reset" restores the lesson's code, the highlighted view and the empty output.
 */
import { t, docLocale } from '../../i18n/core';
import { LabElement, define } from '../../lib/element';
import { sharedPy, type PyClient, type RunResult } from './pyclient';
import { DOWNLOAD_MB } from './protocol';

type Status = { text: string; kind?: 'ok' | 'err' };

export class LabPy extends LabElement {
	/** The Python runtime; the page-wide shared client unless a test assigns one before the element connects. */
	py: PyClient | null = null;

	private get client(): PyClient {
		// Creating the shared client spawns nothing: the worker starts on its first run.
		return (this.py ??= sharedPy());
	}

	private readonly locale = docLocale();
	private busy = false;
	private editing = false;
	/** The status shows the idle note (first-run note or "loaded"), which follows the client's state. */
	private idle = true;
	private unwatch: (() => void) | null = null;

	private get code(): string {
		return this.editing ? this.ref<HTMLTextAreaElement>('edit').value : (this.dataset.code ?? '');
	}

	protected mount(): void {
		for (const name of ['run', 'edit-btn']) this.ref<HTMLButtonElement>(name).disabled = false;
		this.syncButtons();
		this.unwatch = this.client.watch(() => {
			if (this.idle) this.idleNote();
		});
		if (this.idle) this.idleNote();
		this.on(this, 'click', (e) => {
			const btn = (e.target as HTMLElement).closest<HTMLButtonElement>('button[data-ref]');
			if (!btn || btn.disabled) return;
			if (btn.dataset.ref === 'run') void this.run();
			else if (btn.dataset.ref === 'edit-btn') this.edit();
			else if (btn.dataset.ref === 'reset') this.reset();
		});
		this.on(this, 'keydown', (e) => {
			if (e.key === 'Enter' && (e.ctrlKey || e.metaKey) && e.target === this.ref('edit')) {
				e.preventDefault();
				void this.run();
			}
		});
	}

	protected unmount(): void {
		this.unwatch?.();
		this.unwatch = null;
	}

	/** Runs the current code (queued behind other cells of the page by the shared client). */
	async run(): Promise<void> {
		if (this.busy) return;
		const py = this.client;
		this.busy = true;
		this.idle = false;
		this.syncButtons();
		const out = this.ref('out');
		out.setAttribute('aria-busy', 'true');
		this.status({ text: t(this.locale, py.loaded ? 'py.running' : 'py.loading') });
		const started = performance.now();
		let r: RunResult;
		try {
			r = await py.run(this.code);
		} catch (e) {
			// The client settles every run itself; this only keeps Run usable if that contract is ever broken.
			r = { out: '', err: e instanceof Error ? e.message : String(e), fail: 'load' };
		} finally {
			this.busy = false;
			out.removeAttribute('aria-busy');
		}
		this.show(r, (performance.now() - started) / 1000);
		this.syncButtons();
	}

	private show(r: RunResult, secs: number): void {
		const out = this.ref('out');
		out.classList.remove('is-empty');
		// Python's own text (output, traceback, network error) is shown verbatim: it is what the reader debugs.
		out.textContent = r.err ? (r.out && !r.out.endsWith('\n') ? `${r.out}\n` : r.out) + r.err : r.out;
		const link = this.ref<HTMLAnchorElement>('colab');
		link.hidden = !(r.fail && link.getAttribute('href'));
		if (!r.fail) this.status({ text: t(this.locale, 'py.done', { s: secs.toFixed(2) }), kind: 'ok' });
		else if (r.fail === 'py') this.status({ text: t(this.locale, 'py.failPy'), kind: 'err' });
		else if (r.fail === 'timeout') {
			this.status({ text: t(this.locale, 'py.failTimeout', { s: this.client.runMs / 1000 }), kind: 'err' });
		} else this.status({ text: t(this.locale, 'py.failLoad'), kind: 'err' });
	}

	private edit(): void {
		const src = this.ref('src');
		const box = this.ref<HTMLTextAreaElement>('edit');
		// Same height as the highlighted block it replaces, so the swap does not move the page.
		box.style.height = `${src.getBoundingClientRect().height}px`; // sub-pixel exact, offsetHeight rounds
		box.value = this.dataset.code ?? '';
		src.hidden = true;
		box.hidden = false;
		this.editing = true;
		this.idle = false;
		box.focus();
		this.status({ text: t(this.locale, 'py.editing') });
		this.syncButtons();
	}

	private reset(): void {
		const box = this.ref<HTMLTextAreaElement>('edit');
		const wasEditing = this.editing;
		this.editing = false;
		box.hidden = true;
		this.ref('src').hidden = false;
		const out = this.ref('out');
		out.classList.add('is-empty');
		out.textContent = t(this.locale, 'py.placeholder');
		this.ref('colab').hidden = true;
		this.idle = true;
		this.idleNote();
		this.syncButtons();
		// Focus would otherwise stay on the now hidden textarea or the disabled Reset button.
		if (wasEditing || this.contains(document.activeElement)) this.ref<HTMLButtonElement>('run').focus();
	}

	private idleNote(): void {
		this.status({
			text: this.client.loaded ? t(this.locale, 'py.ready') : t(this.locale, 'py.first', { mb: DOWNLOAD_MB }),
		});
	}

	private status({ text, kind }: Status): void {
		const el = this.ref('status');
		// classList, not className: the element also carries Astro's scoped-style class (its reserved min-height).
		el.classList.toggle('is-ok', kind === 'ok');
		el.classList.toggle('is-err', kind === 'err');
		el.textContent = text;
	}

	private syncButtons(): void {
		const run = this.ref<HTMLButtonElement>('run');
		run.disabled = this.busy;
		const editBtn = this.ref<HTMLButtonElement>('edit-btn');
		editBtn.disabled = this.busy || this.editing;
		editBtn.setAttribute('aria-pressed', String(this.editing));
		const dirty = this.editing || !this.ref('out').classList.contains('is-empty');
		this.ref<HTMLButtonElement>('reset').disabled = this.busy || !dirty;
	}
}

define('lab-py', LabPy);
