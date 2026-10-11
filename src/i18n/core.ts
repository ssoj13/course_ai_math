/** Dictionary-free runtime; server ui.ts registers dictionaries, clients read exact locale JSON slices. */
import { asLocale, type Locale } from '../lib/course-rules.mjs';
// Type-only: erased from every bundle, so client code that imports core.ts never pulls the dictionary in.
import type { UiKey } from './ui';

export type { Locale } from '../lib/course-rules.mjs';
export type { UiKey } from './ui';

/** Placeholder values of a dictionary string. */
export type Vars = Readonly<Record<string, string | number>>;

/** Keys whose values hold markup (`th()` only). */
export type HtmlKey = Extract<UiKey, `${string}Html`>;
/** Keys whose values are plain text (`t()` only). */
export type TextKey = Exclude<UiKey, HtmlKey>;

/**
 * A message whose language is decided where it is shown: library code without a locale
 * returns these, widgets render them with `tm(locale, msg)`.
 */
export interface Msg {
	readonly key: TextKey;
	readonly vars?: Vars;
}

/** Builds a `Msg`. */
export const msg = (key: TextKey, vars?: Vars): Msg => ({ key, vars });

/** Strings of one locale, keyed by dictionary key. */
type Strings = Readonly<Record<string, string>>;

/** Full dictionary, registered by `ui.ts` on the server; never set in the browser (the test forbids client imports). */
let dict: Readonly<Record<Locale, Strings>> | null = null;

/** Registers the full dictionary (called once by `ui.ts`). */
export function useDict(d: Readonly<Record<Locale, Strings>>): void {
	dict = d;
}

/** Browser: strings of this page per locale, merged from every `<script type="application/json" data-i18n>` slice. */
const pageStrings = new Map<Locale, Record<string, string>>();

/** Attribute that marks a slice block; its value is the slice's locale. Shared with UiStrings.astro. */
export const SLICE_ATTR = 'data-i18n';

function readSlices(): void {
	for (const el of document.querySelectorAll<HTMLScriptElement>(`script[${SLICE_ATTR}]`)) {
		const locale = asLocale(el.getAttribute(SLICE_ATTR) ?? '');
		const into = pageStrings.get(locale) ?? {};
		Object.assign(into, JSON.parse(el.textContent ?? '{}') as Strings);
		pageStrings.set(locale, into);
	}
}

/** The string of `key`, or `undefined` when it is not available here (browser: not shipped with this page). */
function find(locale: Locale, key: UiKey): string | undefined {
	if (dict) return dict[locale][key];
	if (typeof document === 'undefined') {
		throw new Error(
			`i18n: no dictionary for "${key}": import i18n/ui.ts on server; workers post keys to page`,
		);
	}
	// Slices are read lazily and re-read on a miss, so a block added after the first lookup is still found.
	return pageStrings.get(locale)?.[key] ?? (readSlices(), pageStrings.get(locale)?.[key]);
}

function lookup(locale: Locale, key: UiKey): string {
	const s = find(locale, key);
	if (s !== undefined) return s;
	if (dict) throw new Error(`i18n: unknown key "${key}"`);
	throw new Error(
		`i18n: "${key}" (${locale}) missing slice: add "${key.slice(0, key.indexOf('.'))}" to UiStrings ns`,
	);
}

/**
 * Whether `key` can be rendered in `locale` here. Only for diagnostics that must not throw (an Error's English
 * `message` in a Russian page, where English strings are not shipped); UI text uses `t()`, which throws.
 */
export function has(locale: Locale, key: UiKey): boolean {
	return find(asLocale(locale), key) !== undefined;
}

function fill(locale: Locale, key: UiKey, vars: Vars, esc: (s: string) => string): string {
	return lookup(asLocale(locale), key).replace(/\{(\w+)\}/g, (_, name: string) => {
		const v = vars[name];
		if (v === undefined) throw new Error(`i18n: "${key}" needs a value for {${name}}`);
		return esc(String(v));
	});
}

const escHtml = (s: string) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c);

/**
 * Translated plain-text UI string with `{name}` placeholders filled from `vars`; throws on a placeholder without a value.
 * @example t('ru', 'course.lesson', { num: 3 }) // "Урок 3"
 */
export function t(locale: Locale, key: TextKey, vars: Vars = {}): string {
	return fill(locale, key, vars, (s) => s);
}

/**
 * Translated markup string (`…Html` key) for `set:html` / `innerHTML`; placeholder values are HTML-escaped, so
 * hrefs and command lines can be passed as they are.
 */
export function th(locale: Locale, key: HtmlKey, vars: Vars = {}): string {
	return fill(locale, key, vars, escHtml);
}

/** Renders a `Msg` in a locale. */
export function tm(locale: Locale, m: Msg): string {
	return t(locale, m.key, m.vars);
}

/** Locale of the current document in client scripts: `<html lang>` as set by Starlight (`en`, `ru`). */
export function docLocale(): Locale {
	return asLocale(document.documentElement.lang.split('-')[0] ?? '');
}
