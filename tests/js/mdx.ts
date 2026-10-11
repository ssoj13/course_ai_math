/**
 * Minimal MDX scanner for the content tests (i18n sync): fenced code blocks, imports, JSX component tags with their
 * attributes, course links and MEASURED placeholders. Not a full MDX parser: it understands exactly the constructs
 * the course content uses (see AGENTS.md "Content rules") and throws on anything it cannot read, so a test never
 * passes by skipping what it failed to parse.
 */

/** A fenced code block: info-string language and body with the fence's own indentation removed. */
export interface Fence {
	lang: string;
	code: string;
	line: number;
}

/** One JSX attribute: string literal, `{expression}` (raw source) or a bare boolean flag. */
export type Attr = { kind: 'string'; value: string } | { kind: 'expr'; value: string } | { kind: 'flag' };

export interface Tag {
	name: string;
	attrs: Record<string, Attr>;
	line: number;
}

/**
 * Returns the text of the `{…}` expression that starts at `open` (which must be `{`), skipping braces inside string
 * literals and comments, so a `}` in an option text does not end the expression early. (Same algorithm as
 * tests/js/quiz-lint.test.ts, which predates this module.)
 */
export function balancedExpr(src: string, open: number): string {
	let depth = 0;
	for (let i = open; i < src.length; i++) {
		const c = src[i];
		if (c === '"' || c === "'" || c === '`') {
			for (i++; i < src.length && src[i] !== c; i++) if (src[i] === '\\') i++;
		} else if (c === '/' && src[i + 1] === '/') {
			i = src.indexOf('\n', i);
			if (i < 0) break;
		} else if (c === '/' && src[i + 1] === '*') {
			i = src.indexOf('*/', i + 2) + 1;
			if (i <= 0) break;
		} else if (c === '{') depth++;
		else if (c === '}' && --depth === 0) return src.slice(open + 1, i);
	}
	throw new Error(`unbalanced braces in the expression starting at offset ${open}`);
}

/** Evaluates a literal JSX expression (arrays/objects of literals) the way MDX would; throws if it references names. */
export function evalLiteral(expr: string): unknown {
	return new Function(`"use strict"; return (${expr});`)();
}

const lineAt = (src: string, offset: number): number => src.slice(0, offset).split('\n').length;

/** Splits frontmatter (`---` block at the top) from the body. */
export function splitFrontmatter(src: string): { frontmatter: string; body: string; bodyLine: number } {
	const m = /^---\r?\n([\s\S]*?)\r?\n---\r?\n/.exec(src);
	if (!m) return { frontmatter: '', body: src, bodyLine: 1 };
	return { frontmatter: m[1] ?? '', body: src.slice(m[0].length), bodyLine: lineAt(src, m[0].length) };
}

/**
 * Extracts fenced code blocks and returns the body with every fence blanked out (line count preserved), so later
 * scans do not see components or links that only appear inside code.
 */
export function extractFences(body: string, firstLine = 1): { fences: Fence[]; prose: string } {
	const lines = body.split(/\r?\n/);
	const fences: Fence[] = [];
	const prose: string[] = [];
	for (let i = 0; i < lines.length; i++) {
		const open = /^(\s*)(`{3,}|~{3,})(.*)$/.exec(lines[i] ?? '');
		if (!open) {
			prose.push(lines[i] ?? '');
			continue;
		}
		const [, indent = '', marker = '', info = ''] = open;
		const start = i;
		const code: string[] = [];
		for (i++; i < lines.length; i++) {
			const l = lines[i] ?? '';
			if (l.trim().startsWith(marker) && /^[`~]+$/.test(l.trim()) && l.trim().length >= marker.length) break;
			code.push(l.startsWith(indent) ? l.slice(indent.length) : l.trimStart());
		}
		if (i >= lines.length) throw new Error(`unclosed code fence at line ${firstLine + start}`);
		fences.push({ lang: info.trim().split(/\s+/)[0] ?? '', code: code.join('\n'), line: firstLine + start });
		for (let k = start; k <= i; k++) prose.push('');
	}
	return { fences, prose: prose.join('\n') };
}

/**
 * Blanks inline code spans (same length, so offsets and lines stay valid): "`<Quiz>`" in prose or "`lesson:x`" in an
 * explanation is text about a construct, not the construct.
 */
export function blankInlineCode(prose: string): string {
	return prose.replace(/`[^`\n]*`/g, (m) => ' '.repeat(m.length));
}

/** `import X from '…'` / `import { A, B } from '…'` lines: names and specifier. */
export function imports(prose: string): { names: string[]; from: string }[] {
	return [...prose.matchAll(/^import\s+([\s\S]+?)\s+from\s+['"]([^'"]+)['"];?\s*$/gm)].map((m) => ({
		names: (m[1] ?? '')
			.replace(/[{}]/g, ' ')
			.split(/[\s,]+/)
			.filter(Boolean)
			.sort(),
		from: m[2] ?? '',
	}));
}

/**
 * Every JSX element of an imported component (`names`), in document order, with parsed attributes. Restricting to
 * imported names keeps prose like "x <T" or "<GiB>" in text from being mistaken for a tag.
 */
export function componentTags(prose: string, names: ReadonlySet<string>, firstLine = 1): Tag[] {
	const tags: Tag[] = [];
	for (const m of prose.matchAll(/<([A-Z][\w.]*)(?=[\s/>])/g)) {
		if (!names.has(m[1] ?? '')) continue;
		const attrs: Record<string, Attr> = {};
		let i = m.index + m[0].length;
		for (;;) {
			while (/\s/.test(prose[i] ?? '')) i++;
			if (prose.startsWith('/>', i) || prose[i] === '>') break;
			const name = /^[\w:-]+/.exec(prose.slice(i))?.[0];
			if (!name) throw new Error(`line ${firstLine - 1 + lineAt(prose, i)}: cannot parse attributes of <${m[1]}>`);
			i += name.length;
			if (prose[i] !== '=') {
				attrs[name] = { kind: 'flag' };
				continue;
			}
			i++;
			const q = prose[i];
			if (q === '"' || q === "'") {
				const end = prose.indexOf(q, i + 1);
				attrs[name] = { kind: 'string', value: prose.slice(i + 1, end) };
				i = end + 1;
			} else if (q === '{') {
				const expr = balancedExpr(prose, i);
				attrs[name] = { kind: 'expr', value: expr };
				i += expr.length + 2;
			} else throw new Error(`line ${firstLine - 1 + lineAt(prose, i)}: unsupported value of ${name} in <${m[1]}>`);
		}
		tags.push({ name: m[1] ?? '', attrs, line: firstLine - 1 + lineAt(prose, m.index) });
	}
	return tags;
}

/** Slug-level targets of `lesson:`/`page:` links (markdown links, link definitions and frontmatter `link:` values). */
export function courseLinks(prose: string, frontmatter = ''): string[] {
	const out = new Set<string>();
	for (const m of prose.matchAll(/\]\(\s*<?((?:lesson|page):[\w-]+)/g)) out.add(m[1] ?? '');
	for (const m of prose.matchAll(/^\s*\[[^\]]+\]:\s*<?((?:lesson|page):[\w-]+)/gm)) out.add(m[1] ?? '');
	for (const m of frontmatter.matchAll(/\blink:\s*['"]?((?:lesson|page):[\w-]+)/g)) out.add(m[1] ?? '');
	return [...out].sort();
}

/** `MEASURED: id` / `MEASURED[stand,…]: id` placeholders (in MDX comments), in document order. */
export function measuredPlaceholders(prose: string): string[] {
	return [...prose.matchAll(/MEASURED(\[[^\]]*\])?:\s*([^\s—*]+)/g)].map((m) => `MEASURED${m[1] ?? ''}: ${m[2]}`);
}

/** TeX stays identical across translations; ignore code fences/spans and escaped dollar signs. */
export function mathExpressions(body: string): string[] {
 const prose = blankInlineCode(extractFences(body).prose);
 return [...prose.matchAll(/(?<!\\)\$\$[\s\S]*?(?<!\\)\$\$|(?<!\\)\$(?!\$)[^\n]*?(?<!\\)\$/g)].map(m=>m[0]);
}
