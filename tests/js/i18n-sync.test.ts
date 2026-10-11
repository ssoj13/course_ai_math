// Structural sync between English content (src/content/docs/**) and its Russian translation (src/content/docs/ru/**,
// same relative path). Prose is free to differ; everything a reader or the build relies on must not:
//   - imported names and the files they come from,
//   - components in order with their data props (text props such as label/title/caption are translatable),
//   - the set of <UiLabel k="…"/> keys (widget labels quoted in prose; a set, because Russian word order may move them),
//   - <Quiz>: number of questions, answer indices and option counts,
//   - fenced code blocks (code and comments in code stay English by project rule),
//   - MEASURED / MEASURED[...] placeholder ids,
//   - the set of lesson:/page: link targets (slug level; #anchors are per language).
// A missing Russian file is allowed only when its content id is listed in src/i18n/untranslated.json, and a listed
// id must not have a Russian file (keeps the list honest).
// Staleness: every Russian page carries `translatedFrom` = git blob id of the English source it matches
// (scripts/i18n-stamp.ts); a different current id means English changed after translation. Run:
// `bun test tests/js/i18n-sync.test.ts`.
import { expect, test } from 'bun:test';
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { dirname, join, relative, resolve } from 'node:path';
import { blobSha, readStamp, STAMP_KEY, writeStamp } from '../../scripts/i18n-stamp';
import {
	blankInlineCode,
 mathExpressions,
	componentTags,
	courseLinks,
	evalLiteral,
	extractFences,
	imports,
	measuredPlaceholders,
	splitFrontmatter,
	type Tag,
} from './mdx';

const ROOT = join(import.meta.dir, '..', '..');
const SRC = join(ROOT, 'src');
const DOCS = join(SRC, 'content', 'docs');
/** Translated locales: content under `src/content/docs/<locale>/`, untranslated list per locale. */
const LOCALE = 'ru';
const LOCALE_DOCS = join(DOCS, LOCALE);
const UNTRANSLATED_FILE = join(SRC, 'i18n', 'untranslated.json');

/** Inline prose component naming a widget label (src/components/UiLabel.astro): compared as a key set, not in order. */
const UI_LABEL = 'UiLabel';

/** Attribute and object keys that carry reader-facing text: translated, so not compared. */
const TEXT_KEYS = new Set(['label', 'title', 'caption', 'description', 'text', 'tagline', 'alt', 'q', 'options', 'explain']);

const mdxFiles = (dir: string): string[] =>
	readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
		e.isDirectory() ? mdxFiles(join(dir, e.name)) : e.name.endsWith('.mdx') ? [join(dir, e.name)] : [],
	);

const idOf = (base: string, file: string) => relative(base, file).replaceAll('\\', '/').replace(/\.mdx$/, '');

const enIds = mdxFiles(DOCS)
	.filter((f) => !f.startsWith(LOCALE_DOCS))
	.map((f) => idOf(DOCS, f))
	.sort();
const ruIds = existsSync(LOCALE_DOCS) ? mdxFiles(LOCALE_DOCS).map((f) => idOf(LOCALE_DOCS, f)).sort() : [];
const untranslated: string[] = JSON.parse(readFileSync(UNTRANSLATED_FILE, 'utf8'));

/** Removes translatable text fields from a literal prop value, keeping its data shape. */
function dataOnly(v: unknown): unknown {
	if (Array.isArray(v)) return v.map(dataOnly);
	if (v && typeof v === 'object') {
		return Object.fromEntries(
			Object.entries(v)
				.filter(([k]) => !TEXT_KEYS.has(k))
				.map(([k, x]) => [k, dataOnly(x)]),
		);
	}
	return v;
}

/** Comparable signature of one component use: name + data props (Quiz: question shape and answers). */
function signature(tag: Tag, where: string): string {
	if (tag.name === 'Quiz') {
		const q = tag.attrs.questions;
		if (q?.kind !== 'expr') throw new Error(`${where}:${tag.line}: <Quiz> without a questions={…} expression`);
		const list = evalLiteral(q.value) as { answer: unknown; options: unknown[] }[];
		return `Quiz questions=${list.length} answers=[${list.map((x) => x.answer).join(',')}] options=[${list.map((x) => x.options.length).join(',')}]`;
	}
	const props = Object.entries(tag.attrs)
		.filter(([k]) => !TEXT_KEYS.has(k))
		.sort(([a], [b]) => a.localeCompare(b))
		.map(([k, a]) => {
			if (a.kind === 'flag') return k;
			if (a.kind === 'string') return `${k}=${JSON.stringify(a.value)}`;
			let value: string;
			try {
				value = JSON.stringify(dataOnly(evalLiteral(a.value)));
			} catch {
				value = `{${a.value.replace(/\s+/g, ' ').trim()}}`; // references a name (e.g. {lessons.length}): compare the source
			}
			return `${k}=${value}`;
		});
	return [tag.name, ...props].join(' ');
}

interface Fingerprint {
 math: string[];
	imports: string[];
	components: string[];
	labels: string[];
	code: string[];
	measured: string[];
	links: string[];
}

function fingerprint(file: string): Fingerprint {
	const where = relative(ROOT, file).replaceAll('\\', '/');
	const { frontmatter, body, bodyLine } = splitFrontmatter(readFileSync(file, 'utf8'));
	const { fences, prose } = extractFences(body, bodyLine);
	const imps = imports(prose);
	const names = new Set(imps.flatMap((i) => i.names));
	const text = blankInlineCode(prose);
	const tags = componentTags(text, names, bodyLine);
	const labels = tags.filter((t) => t.name === UI_LABEL).map((t) => {
		const k = t.attrs.k;
		if (k?.kind !== 'string') throw new Error(`${where}:${t.line}: <${UI_LABEL}> needs a literal k="namespace.key"`);
		return k.value;
	});
	return {
  math: mathExpressions(body),
		// Specifiers resolved against the file, so `../../x` (English) and `../../../x` (Russian) compare equal.
		imports: imps
			.map((i) => `${i.names.join(', ')} from ${i.from.startsWith('.') ? relative(SRC, resolve(dirname(file), i.from)).replaceAll('\\', '/') : i.from}`)
			.sort(),
		components: tags.filter((t) => t.name !== UI_LABEL).map((t) => signature(t, where)),
		labels: [...new Set(labels)].sort(),
		code: fences.map((f) => `[${f.lang}]\n${f.code}`),
		measured: measuredPlaceholders(prose),
		links: courseLinks(text, frontmatter),
	};
}

/** Human-readable difference of two lists: first mismatch for ordered lists, missing/extra for sets. */
function diffList(name: string, en: string[], ru: string[], ordered: boolean): string[] {
	if (JSON.stringify(en) === JSON.stringify(ru)) return [];
	if (!ordered) {
		const missing = en.filter((x) => !ru.includes(x));
		const extra = ru.filter((x) => !en.includes(x));
		return [`  ${name}:`, ...missing.map((x) => `    missing in ru: ${x}`), ...extra.map((x) => `    only in ru:    ${x}`)];
	}
	// Every differing position (capped), so one fix-and-rerun cycle shows all problems of a file.
	const MAX = 10;
	const show = (s: string | undefined) => (s === undefined ? '(none)' : s.length > 400 ? `${s.slice(0, 400)}…` : s);
	const at = Array.from({ length: Math.max(en.length, ru.length) }, (_, k) => k).filter((k) => en[k] !== ru[k]);
	return [
		`  ${name}: ${en.length} in en, ${ru.length} in ru; ${at.length} position(s) differ`,
		...at.slice(0, MAX).flatMap((k) => [`    #${k + 1} en: ${show(en[k])}`, `    #${k + 1} ru: ${show(ru[k])}`]),
		...(at.length > MAX ? [`    … ${at.length - MAX} more`] : []),
	];
}

test('untranslated.json lists exactly the English pages without a Russian file', () => {
	const errors: string[] = [];
	const dupes = untranslated.filter((id, i) => untranslated.indexOf(id) !== i);
	if (dupes.length) errors.push(`duplicates: ${dupes.join(', ')}`);
	for (const id of untranslated) {
		if (!enIds.includes(id)) errors.push(`listed but no English page: ${id}`);
		if (ruIds.includes(id)) errors.push(`listed but translated (remove it from the list): ${LOCALE}/${id}.mdx exists`);
	}
	for (const id of enIds) {
		if (!ruIds.includes(id) && !untranslated.includes(id)) errors.push(`no ${LOCALE}/${id}.mdx and not listed as untranslated`);
	}
	for (const id of ruIds) if (!enIds.includes(id)) errors.push(`${LOCALE}/${id}.mdx has no English original`);
	if (errors.length) throw new Error(`src/i18n/untranslated.json:\n${errors.map((e) => `  ${e}`).join('\n')}`);
});

test('Russian pages keep the structure of their English originals', () => {
	const report: string[] = [];
	const pairs = ruIds.filter((id) => enIds.includes(id));
	for (const id of pairs) {
		const en = fingerprint(join(DOCS, `${id}.mdx`));
		const ru = fingerprint(join(LOCALE_DOCS, `${id}.mdx`));
		const lines = [
			...diffList('imports', en.imports, ru.imports, false),
			...diffList('components (order and data props)', en.components, ru.components, true),
			...diffList(`<${UI_LABEL}> keys`, en.labels, ru.labels, false),
			...diffList('fenced code blocks', en.code, ru.code, true),
			...diffList('MEASURED placeholders', en.measured, ru.measured, true),
			...diffList('lesson:/page: link targets', en.links, ru.links, false),
		];
		if (lines.length) report.push(`${LOCALE}/${id}.mdx vs ${id}.mdx`, ...lines);
	}
	expect(pairs.length).toBe(ruIds.length); // Infrastructure stage: all bodies may honestly remain fallback.
	if (report.length) throw new Error(`\n${report.join('\n')}`);
});

test('every English page parses (so a future translation can be checked)', () => {
	for (const id of enIds) expect(() => fingerprint(join(DOCS, `${id}.mdx`))).not.toThrow();
});

/** Whether git has this blob locally (a shallow clone or rewritten history may lack an old English blob). */
function gitHas(sha: string): boolean {
	return spawnSync('git', ['cat-file', '-e', `${sha}^{blob}`], { cwd: ROOT }).status === 0;
}

/** Staleness verdict for one Russian page, or undefined when its stamp equals the current English blob id. */
function staleness(id: string, ruSrc: string, enSrc: string, has: (sha: string) => boolean): string | undefined {
	const ruPath = `src/content/docs/${LOCALE}/${id}.mdx`;
	const enPath = `src/content/docs/${id}.mdx`;
	const stamp = readStamp(ruSrc);
	const current = blobSha(enSrc);
	if (!stamp) return `${LOCALE}/${id}.mdx has no ${STAMP_KEY}: check it against ${enPath}, then run bun run i18n:stamp ${ruPath}`;
	if (stamp === current) return undefined;
	// `git diff <blob> <blob>` needs both objects locally: the old one can be missing (shallow clone, rewritten history),
	// the current one is missing while the English edit is not committed yet.
	const review = !has(stamp)
		? `git log -p -- ${enPath} (blob ${stamp} is not in the local object store: shallow clone or rewritten history)`
		: has(current)
			? `git diff ${stamp} ${current} (blob ids)`
			: `git diff ${stamp} HEAD:${enPath} plus git diff -- ${enPath} (the English edit is not committed yet)`;
	return `${LOCALE}/${id}.mdx is stale: English changed since translation. Review: ${review}, then update the Russian text and run bun run i18n:stamp ${ruPath}`;
}

test('every Russian page is stamped with the current blob id of its English source', () => {
	const stale = ruIds
		.filter((id) => enIds.includes(id))
		.map((id) => staleness(id, readFileSync(join(LOCALE_DOCS, `${id}.mdx`), 'utf8'), readFileSync(join(DOCS, `${id}.mdx`), 'utf8'), gitHas))
		.filter((m): m is string => m !== undefined);
	if (stale.length) throw new Error(`\n${stale.join('\n')}`);
});

test('staleness guard on fixtures: blob ids, stamping, stale verdicts', () => {
	// Known git ids: `printf '' | git hash-object --stdin` and `printf 'hello\n' | git hash-object --stdin`.
	expect(blobSha('')).toBe('e69de29bb2d1d6434b8b29ae775ad8c2e48c5391');
	expect(blobSha('hello\n')).toBe('ce013625030ba8dba906f756967f9e9ca394464a');
	expect(blobSha('a\r\nb\r\n')).toBe(blobSha('a\nb\n'));

	const en = '---\ntitle: "Lesson"\n---\n\nText.\n';
	const ru = '---\ntitle: "Урок"\ndescription: "Описание"\n---\n\nТекст.\n';
	const sha = blobSha(en);
	const stamped = writeStamp(ru, sha);
	expect(stamped).toBe(`---\ntitle: "Урок"\ndescription: "Описание"\n${STAMP_KEY}: ${sha}\n---\n\nТекст.\n`);
	expect(writeStamp(stamped, sha)).toBe(stamped); // idempotent
	expect(writeStamp(ru.replaceAll('\n', '\r\n'), sha)).toBe(stamped.replaceAll('\n', '\r\n')); // keeps CRLF files CRLF
	expect(readStamp(stamped)).toBe(sha);
	expect(staleness('x/y', stamped, en, () => true)).toBeUndefined();
	expect(staleness('x/y', stamped, en.replaceAll('\n', '\r\n'), () => true)).toBeUndefined(); // checkout EOL is irrelevant

	const edited = en.replace('Text.', 'Text, edited.');
	expect(staleness('x/y', stamped, edited, () => true)).toBe(
		`ru/x/y.mdx is stale: English changed since translation. Review: git diff ${sha} ${blobSha(edited)} (blob ids), then update the Russian text and run bun run i18n:stamp src/content/docs/ru/x/y.mdx`,
	);
	expect(staleness('x/y', stamped, edited, () => false)).toContain('git log -p -- src/content/docs/x/y.mdx');
	expect(staleness('x/y', stamped, edited, (s) => s === sha)).toContain(`git diff ${sha} HEAD:src/content/docs/x/y.mdx`);
	expect(staleness('x/y', ru, en, () => true)).toContain(`has no ${STAMP_KEY}`);
	expect(writeStamp(writeStamp(stamped, blobSha(edited)), sha)).toBe(stamped); // restamping replaces in place
});

test('blobSha equals `git hash-object` on the real English pages', () => {
	if (spawnSync('git', ['--version']).status !== 0) return; // no git: the fixture vectors above still pin the algorithm
	const files = enIds.map((id) => join(DOCS, `${id}.mdx`));
	const out = spawnSync('git', ['hash-object', '--', ...files], { cwd: ROOT, encoding: 'utf8' });
	expect(out.status).toBe(0);
	expect(out.stdout.trim().split(/\r?\n/)).toEqual(files.map((f) => blobSha(readFileSync(f, 'utf8'))));
});

test('math sync catches changed TeX and ignores translated prose and code examples',()=>{
 const en='A vector $a\\cdot b$.\n$$\nx^2+y^2\n$$\n\x60$ignored$\x60';
 const ru='Вектор $a\\cdot b$.\n$$\nx^2+y^2\n$$\n\x60$другое$\x60';
 expect(mathExpressions(en)).toEqual(mathExpressions(ru));
 expect(mathExpressions(ru.replace('y^2','y^3'))).not.toEqual(mathExpressions(en));
});
