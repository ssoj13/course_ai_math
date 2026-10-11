// Translation staleness stamps: every Russian page records, in its frontmatter field `translatedFrom`, the git blob id of
// the English source it was translated from. tests/js/i18n-sync.test.ts recomputes the id of the current English file and
// fails when they differ, so a Russian page can never silently lag behind an English edit.
//
// Usage (after the Russian text matches its English source again):
//   bun run i18n:stamp src/content/docs/ru/performance/serving-tuning.mdx [more ru pages…]
//   bun run i18n:stamp --all
//
// Why a git blob id and not a date or a plain hash: it is exactly what `git hash-object <english file>` prints, so a
// reviewer can run `git diff <stamp> <current>` on the two blob ids and see what changed in English since translation.
import { createHash } from 'node:crypto';
import { existsSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { isAbsolute, join, relative, resolve } from 'node:path';

export const ROOT = join(import.meta.dir, '..');
export const DOCS = join(ROOT, 'src', 'content', 'docs');
export const LOCALE = 'ru';
export const LOCALE_DOCS = join(DOCS, LOCALE);
/** Frontmatter key; the docs schema in src/content.config.ts declares it. */
export const STAMP_KEY = 'translatedFrom';

/**
 * Git blob id (SHA-1) of a file's content as git stores it: `sha1("blob <byte length>\0" + bytes)` over the text with
 * CRLF normalized to LF (the repo's `.gitattributes` is `text=auto eol=lf`, so this equals `git hash-object` and the id
 * in the index whatever the checkout's line endings are).
 */
export function blobSha(text: string): string {
	const bytes = Buffer.from(text.replace(/\r\n/g, '\n'), 'utf8');
	return createHash('sha1').update(`blob ${bytes.length}\0`).update(bytes).digest('hex');
}

const FRONTMATTER = /^---(\r?\n)([\s\S]*?)\r?\n---(?:\r?\n|$)/;
// `(?=\r?$)`: with the `m` flag `$` sits before "\n", so a CRLF line still matches without consuming its "\r".
const STAMP_LINE = new RegExp(`^${STAMP_KEY}:[ \\t]*["']?([^"'\\r\\n]*?)["']?[ \\t]*(?=\\r?$)`, 'm');

/** The `translatedFrom` value of a page, or undefined when the page has no frontmatter or no stamp. */
export function readStamp(src: string): string | undefined {
	const fm = FRONTMATTER.exec(src);
	return fm ? STAMP_LINE.exec(fm[2] ?? '')?.[1] : undefined;
}

/**
 * Returns `src` with `translatedFrom: <sha>` in its frontmatter: an existing stamp line is replaced in place, otherwise
 * the line is appended as the last frontmatter line (with the file's own line ending). Every other byte is kept.
 */
export function writeStamp(src: string, sha: string): string {
	const fm = FRONTMATTER.exec(src);
	if (!fm) throw new Error('no frontmatter block (--- … ---) at the top of the file');
	const [, eol = '\n', body = ''] = fm;
	const line = `${STAMP_KEY}: ${sha}`;
	const next = STAMP_LINE.test(body) ? body.replace(STAMP_LINE, line) : `${body}${eol}${line}`;
	const start = 3 + eol.length; // after "---" + eol
	return src.slice(0, start) + next + src.slice(start + body.length);
}

/** Repo-relative path with forward slashes (for messages). */
export const repoPath = (file: string) => relative(ROOT, file).replaceAll('\\', '/');

/** English source of a Russian page: src/content/docs/ru/<id>.mdx -> src/content/docs/<id>.mdx. */
export function englishOf(ruFile: string): string {
	const rel = relative(LOCALE_DOCS, ruFile);
	if (rel.startsWith('..') || isAbsolute(rel)) throw new Error(`${ruFile} is not under ${repoPath(LOCALE_DOCS)}/`);
	return join(DOCS, rel);
}

export const ruPages = (dir = LOCALE_DOCS): string[] =>
	readdirSync(dir, { withFileTypes: true }).flatMap((e) =>
		e.isDirectory() ? ruPages(join(dir, e.name)) : e.name.endsWith('.mdx') ? [join(dir, e.name)] : [],
	);

/** Stamps one Russian page with the current blob id of its English source; returns true when the file changed. */
export function stamp(ruFile: string): boolean {
	const en = englishOf(ruFile);
	if (!existsSync(en)) throw new Error(`${repoPath(ruFile)}: English source ${repoPath(en)} does not exist`);
	const src = readFileSync(ruFile, 'utf8');
	const out = writeStamp(src, blobSha(readFileSync(en, 'utf8')));
	if (out === src) return false;
	writeFileSync(ruFile, out);
	return true;
}

if (import.meta.main) {
	const args = process.argv.slice(2);
	if (args.length === 0) {
		console.error('usage: bun run i18n:stamp <src/content/docs/ru/…/page.mdx>… | --all');
		process.exit(2);
	}
	const files = args.includes('--all') ? ruPages() : args.map((a) => resolve(a));
	let failed = false;
	for (const f of files) {
		try {
			if (!existsSync(f)) throw new Error(`${repoPath(f)} does not exist`);
			console.log(`${stamp(f) ? 'stamped ' : 'current '} ${repoPath(f)}`);
		} catch (e) {
			console.error(`error    ${(e as Error).message}`);
			failed = true;
		}
	}
	process.exit(failed ? 1 : 0);
}
