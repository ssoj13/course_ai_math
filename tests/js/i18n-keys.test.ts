// UI dictionary checks (src/i18n/ui.ts) against the code that uses it. TypeScript is not run by `astro build`, so a
// mistyped key would only surface as a runtime throw in the reader's browser; this static scan catches it first:
//   - every dictionary-looking string literal ('<namespace>.<name>') in components, lib code and pages is a key,
//   - t()/msg() take plain-text keys and th() takes `…Html` keys, never a key built from a template string,
//   - every key is used somewhere (no stale entries); lesson prose counts too: <UiLabel k="…"/> in .mdx content,
//   - en and ru have the same keys, the same {placeholders} and, for `…Html` keys, the same tags; plain-text values
//     hold no markup.
import { afterEach, beforeEach, describe, expect, test } from 'bun:test';
import { readdirSync, readFileSync } from 'node:fs';
import { join, relative } from 'node:path';
import { clientGraph, namespaces as usedNamespaces } from './i18n-clients';
import { DICT, msg, slice, t, th, tm } from '../../src/i18n/ui';

const ROOT = join(import.meta.dir, '..', '..');
const SRC = join(ROOT, 'src');
/** Code that renders UI strings. Content (.mdx) never calls t(). */
const SCAN_DIRS = ['components', 'lib', 'pages', 'i18n', 'viz'].map((d) => join(SRC, d));
const SCAN_EXT = /\.(astro|tsx?|mjs)$/;

const files = (dir: string): string[] =>
	readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
		e.isDirectory() ? files(join(dir, e.name)) : SCAN_EXT.test(e.name) ? [join(dir, e.name)] : [],
	);

const isDictFile = (f: string) => relative(SRC, f).replaceAll('\\', '/').startsWith('i18n/');
const sources = SCAN_DIRS.flatMap(files).map((f) => ({ file: relative(ROOT, f).replaceAll('\\', '/'), text: readFileSync(f, 'utf8'), dict: isDictFile(f) }));
const code = sources.filter((s) => !s.dict);

/** Widget labels quoted in lesson prose: `<UiLabel k="ns.key" />` (src/components/UiLabel.astro) in .mdx content. */
const DOCS = join(SRC, 'content', 'docs');
const mdxFiles = (dir: string): string[] =>
	readdirSync(dir, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? mdxFiles(join(dir, e.name)) : e.name.endsWith('.mdx') ? [join(dir, e.name)] : []));
const uiLabels = mdxFiles(DOCS).flatMap((f) =>
	[...readFileSync(f, 'utf8').matchAll(/<UiLabel\s+k=(['"])([^'"]*)\1\s*\/>/g)].map((m) => ({ file: relative(ROOT, f).replaceAll('\\', '/'), key: m[2] ?? '' })),
);

const en = DICT.en;
const ru = DICT.ru;
const keys = Object.keys(en);
const keySet = new Set(keys);
const namespaces = [...new Set(keys.map((k) => k.slice(0, k.indexOf('.'))))];
/** A quoted string that looks like a dictionary key of a known namespace. */
const KEY_LITERAL = new RegExp(`(['"\`])(?![\\w.]*\\.(?:ts|mjs|js|astro|json|py|mdx?)\\1)((?:${namespaces.join('|')})\\.[A-Za-z0-9_.]+)\\1`, 'g');

const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
const tags = (s: string) => [...s.matchAll(/<\/?([a-zA-Z][\w-]*)/g)].map((m) => m[0]).sort();

describe('dictionary', () => {
	test('en and ru have the same keys', () => {
		expect(Object.keys(ru).sort()).toEqual([...keys].sort());
	});

	test('no value is empty', () => {
		const empty = keys.filter((k) => !en[k as keyof typeof en].trim() || !ru[k as keyof typeof ru].trim());
		expect(empty).toEqual([]);
	});

	test('placeholders are the same in en and ru', () => {
		const bad = keys.filter((k) => placeholders(en[k as keyof typeof en]).join() !== placeholders(ru[k as keyof typeof ru]).join());
		expect(bad).toEqual([]);
	});

	test('…Html values carry the same tags in en and ru; plain values carry none', () => {
		const bad: string[] = [];
		for (const k of keys) {
			const a = tags(en[k as keyof typeof en]);
			const b = tags(ru[k as keyof typeof ru]);
			if (k.endsWith('Html') ? a.join() !== b.join() : a.length + b.length > 0) bad.push(k);
		}
		expect(bad).toEqual([]);
	});
});

describe('usage in code', () => {
	test('every key-looking literal exists in the dictionary', () => {
		const missing: string[] = [];
		for (const s of code) for (const m of s.text.matchAll(KEY_LITERAL)) if (!keySet.has(m[2] ?? '')) missing.push(`${s.file}: ${m[2]}`);
		expect(missing).toEqual([]);
	});

	test('t()/msg() use plain-text keys, th() uses …Html keys', () => {
		const bad: string[] = [];
		for (const s of code) {
			for (const m of s.text.matchAll(/\b(t|th|msg)\(([^'"`()]*(?:\([^()]*\))?[^'"`()]*?)(['"`])([^'"`]*)\3/g)) {
				const fn = m[1];
				const key = m[4] ?? '';
				// msg('key') takes the key first; t()/th() second (after the locale).
				if (fn === 'msg' ? m[2]?.trim() !== '' : !m[2]?.trim().endsWith(',')) continue;
				if (!keySet.has(key)) continue; // reported by the literal check
				if ((fn === 'th') !== key.endsWith('Html')) bad.push(`${s.file}: ${fn}('${key}')`);
			}
		}
		expect(bad).toEqual([]);
	});

	test('no key is built from a template string', () => {
		const bad: string[] = [];
		for (const s of code) for (const m of s.text.matchAll(/\b(?:t|th)\([^,()]*(?:\([^()]*\))?\s*,\s*`[^`]*\$\{|\bmsg\(\s*`[^`]*\$\{/g)) bad.push(`${s.file}: ${m[0]}`);
		expect(bad).toEqual([]);
	});

	test('<UiLabel> in lesson prose takes an existing plain-text key without placeholders', () => {
		const bad = uiLabels
			.filter(({ key }) => !keySet.has(key) || key.endsWith('Html') || placeholders(en[key as keyof typeof en]).length > 0)
			.map(({ file, key }) => `${file}: ${key}`);
		expect(bad).toEqual([]);
	});

	test('every key is used somewhere', () => {
		const used = new Set<string>(uiLabels.map((l) => l.key));
		for (const s of code) for (const m of s.text.matchAll(KEY_LITERAL)) used.add(m[2] ?? '');
		expect(keys.filter((k) => !used.has(k))).toEqual([]);
	});
});

describe('helpers', () => {
	test('t fills placeholders and throws on a missing one', () => {
		expect(t('ru', 'course.lesson', { num: 3 })).toBe('Урок 3');
		expect(() => t('en', 'course.lesson')).toThrow(/needs a value for \{num\}/);
	});

	test('tm renders a Msg in either locale', () => {
		const m = msg('course.lesson', { num: 3 });
		expect(tm('en', m)).toBe('Lesson 3');
		expect(tm('ru', m)).toBe('Урок 3');
	});

	test('th escapes placeholder values', () => {
		const html = Object.keys(en).find((k) => k.endsWith('Html') && placeholders(en[k as keyof typeof en]).length > 0);
		if (!html) return; // no markup key with placeholders yet
		const vars = Object.fromEntries(placeholders(en[html as keyof typeof en]).map((p) => [p, '<x>&"']));
		const out = th('en', html as Parameters<typeof th>[1], vars);
		expect(out).not.toContain('<x>');
		expect(out).toContain('&lt;x&gt;&amp;&quot;');
	});
});

/**
 * Client strings come from per-page slices (src/i18n/core.ts header): a widget's `<UiStrings ns={[…]} />` must ship
 * every namespace its client code can render, or the reader gets a thrown "not on this page" error. The client code of
 * a component is its `<script>` block (Astro bundles only attribute-less ones) plus everything it reaches through
 * value imports and `new URL(…, import.meta.url)` workers; the namespaces of the key literals found there must equal
 * the component's `ns` list (equal, not a superset, so a list never carries strings nobody renders).
 */
describe('client slices', () => {

 const clients = SCAN_DIRS.flatMap(files).filter(f => f.endsWith('.astro') && !f.endsWith('UiStrings.astro'))
  .map(file => ({ file, ...clientGraph(readFileSync(file, 'utf8'), file) }));
 test('React roots and Astro scripts never import the dictionary, including transitive lazy .tsx imports', () => {
  expect(clients.flatMap(c => c.imports.filter(e => /[\\/]i18n[\\/]ui\.ts$/.test(e.file)))).toEqual([]);
 });
 test('each wrapper ships exactly the namespaces of its client roots and dynamic descendants', () => {
  for (const c of clients) expect({ file: relative(ROOT, c.file), namespaces: [...new Set(c.declared)].sort() })
   .toEqual({ file: relative(ROOT, c.file), namespaces: usedNamespaces(c.texts, namespaces) });
 });
 test('UiStrings uses literal namespaces and script-close escaping', () => {
  for (const c of clients) for (const tag of c.tags) expect(tag).toMatch(/^<UiStrings\s+ns=\{\[[^]*\]\}\s*\/>$/);
  expect(readFileSync(join(SRC, 'components/UiStrings.astro'), 'utf8')).toContain(".replace(/</g, '\\\\u003c')");
 });
 test('negative fixtures detect a lazy .tsx dictionary leak and a missing status namespace', () => {
  const path = (p: string) => join(SRC, p);
  const fixture = new Map([
   [path('entry.tsx'), "export { Root } from './root';"],
   [path('root.tsx'), "import type { UiKey } from './i18n/ui'; export const Root=()=>import('./lazy');"],
   [path('lazy.tsx'), "import { t } from './i18n/core'; import { DICT } from './i18n/ui'; t(locale, 'status.idle2d');"],
   [path('i18n/core.ts'), ''],
   [path('i18n/ui.ts'), ''],
  ]);
  const graph = clientGraph("---\nimport { Root } from './entry';\n---\n<Root client:only=\"react\" /><UiStrings ns={['viewport']} />", path('Fixture.astro'),
   { exists: p => fixture.has(p), read: p => fixture.get(p)! });
  expect(graph.roots).toEqual(['Root']);
  expect(graph.imports.some(e => e.from.endsWith('lazy.tsx') && e.file === path('i18n/ui.ts'))).toBe(true);
  expect(usedNamespaces(graph.texts, namespaces)).toEqual(['status']);
  expect(graph.declared).not.toEqual(usedNamespaces(graph.texts, namespaces));
 });

});

describe('browser lookup (core.ts without the dictionary)', () => {
	/** A fresh core.ts instance: the one the tests share has the full dictionary registered by ui.ts. */
	const loadCore = async (tag: string) => (await import(`../../src/i18n/core.ts?browser=${tag}`)) as typeof import('../../src/i18n/core');
	let previous: PropertyDescriptor | undefined;
 beforeEach(() => { previous = Object.getOwnPropertyDescriptor(globalThis, 'document'); });
	const page = (lang: string, blocks: { locale: string; strings: Record<string, string> }[]) => {
		Object.defineProperty(globalThis, 'document', { configurable: true, value: {
			documentElement: { lang },
			querySelectorAll: () => blocks.map((b) => ({ getAttribute: () => b.locale, textContent: JSON.stringify(b.strings) })),
		} });
	};
	afterEach(() => {
		if (previous) Object.defineProperty(globalThis, 'document', previous);
 else Reflect.deleteProperty(globalThis, 'document');
	});

	test('reads the page slices; throws for a key or a locale the page does not ship', async () => {
		const core = await loadCore('a');
		page('ru', [{ locale: 'ru', strings: slice('ru', ['viewport', 'course']) }]);
		expect(core.docLocale()).toBe('ru');
		expect(core.t('ru', 'course.lesson', { num: 3 })).toBe('Урок 3');
		expect(core.t('ru', 'viewport.reset')).toBe(DICT.ru['viewport.reset']);
		expect(() => core.t('ru', 'progress.core', { done: 1, total: 2 })).toThrow(
			/"progress\.core" \(ru\) missing slice: add .*"progress"/,
		);
		expect(() => core.t('en', 'viewport.reset')).toThrow(/missing slice/);
		expect(core.has('en', 'viewport.reset')).toBe(false);
		expect(core.has('ru', 'viewport.reset')).toBe(true);
	});


 test('markup placeholders escape HTML and script-closing text, unknown keys remain loud', async()=>{
  const core=await loadCore('markup');
  page('ru',[{locale:'ru',strings:{'fixtureHtml':'<b>{value}</b>'}}]);
  expect(core.th('ru','fixtureHtml' as never,{value:'</script><img a="x">&'})).toBe('<b>&lt;/script&gt;&lt;img a=&quot;x&quot;&gt;&amp;</b>');
  expect(()=>core.t('ru','unknown.key' as never)).toThrow(/unknown.key.*ru.*unknown/);
 });
	test('a slice added after the first lookup is still found', async () => {
		const core = await loadCore('b');
		const blocks = [{ locale: 'en', strings: slice('en', ['viewport']) }];
		page('en', blocks);
		expect(core.t('en', 'viewport.reset')).toBe('Reset view');
		blocks.push({ locale: 'en', strings: slice('en', ['course']) });
		expect(core.t('en', 'course.lesson', { num: 2 })).toBe(DICT.en['course.lesson'].replace('{num}', '2'));
	});

	test('without a document and without ui.ts it explains what is missing', async () => {
		const core = await loadCore('c');
 Reflect.deleteProperty(globalThis, 'document');
		expect(() => core.t('en', 'viewport.reset')).toThrow(/import i18n\/ui\.ts on server/);
	});

	test('slice() rejects an unknown namespace', () => {
		expect(() => slice('en', ['nosuchwidget'])).toThrow(/unknown namespace "nosuchwidget"/);
	});
});
