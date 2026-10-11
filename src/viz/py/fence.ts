/**
 * Build-time helpers of PyCell.astro. A lesson keeps its Python as an ordinary fenced block inside `<PyCell>`:
 * the MDX text of the code is the single source (the reader sees it highlighted by Expressive Code, the cell runs
 * exactly it, and the notebook generator of Task 10 reads the same fences). These helpers recover the code text from
 * the rendered slot and size the output pane.
 */

/** Expressive Code writes the raw code into its copy button, lines joined by U+007F (DEL). */
const EC_CODE = /\sdata-code="([^"]*)"/g;
const EC_LINE_SEP = /\u007f/g;

const ENTITIES: Readonly<Record<string, string>> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" };

/** Decodes the character references an HTML serializer may put into an attribute value. */
export function decodeAttr(s: string): string {
	return s.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (m, ref: string) => {
		if (ref[0] === '#') {
			const cp = ref[1] === 'x' || ref[1] === 'X' ? parseInt(ref.slice(2), 16) : parseInt(ref.slice(1), 10);
			return String.fromCodePoint(cp);
		}
		const named = ENTITIES[ref.toLowerCase()];
		if (named === undefined) throw new Error(`PyCell: unknown character reference ${m}`);
		return named;
	});
}

/**
 * The code of the one ```python fence in a rendered `<PyCell>` slot. Throws (failing the build) unless the slot holds
 * exactly one code block, it is Python, and Expressive Code's copy payload is present: a cell that silently ran
 * nothing, or ran other text than the reader sees, would be worse than a broken build.
 */
export function fenceCode(html: string): string {
	const pres = [...html.matchAll(/<pre\b[^>]*>/g)].map((m) => m[0]);
	if (pres.length !== 1) throw new Error(`PyCell: expected exactly one fenced code block, found ${pres.length}`);
	if (!/\sdata-language="python"/.test(pres[0] ?? '')) {
		throw new Error(`PyCell: the fenced block must be \`\`\`python, got ${pres[0]}`);
	}
	const codes = [...html.matchAll(EC_CODE)];
	if (codes.length !== 1) {
		throw new Error(`PyCell: expected one Expressive Code copy payload (data-code), found ${codes.length}`);
	}
	return decodeAttr(codes[0]?.[1] ?? '').replace(EC_LINE_SEP, '\n');
}

/** Tallest pane; longer output (rare in lessons) scrolls. */
export const MAX_ROWS = 24;

/** Iterations of a `for` header whose iterable is a literal: `(0, 30, 60)`, `[a, b]`, `range(7)`, `range(2, 9)`. */
function loopCount(header: string): number {
	const it = /\bin\s+(.+?):\s*(?:#.*)?$/.exec(header)?.[1]?.trim() ?? '';
	const range = /^range\(\s*(-?\d+)\s*(?:,\s*(-?\d+)\s*)?\)$/.exec(it);
	if (range) return Math.max(0, range[2] === undefined ? Number(range[1]) : Number(range[2]) - Number(range[1]));
	const lit = /^[([](.*)[)\]]$/.exec(it);
	if (lit && !/[([]/.test(lit[1] ?? '')) return (lit[1] ?? '').split(',').filter((s) => s.trim()).length;
	return 1; // unknown iterable: count the body once
}

/** Rows of a printed matrix as documented in the comment lines right under its print (`# [[1. 0.]` / `#  [0. 1.]]`). */
function commentRows(lines: readonly string[], from: number): number {
	let n = 0;
	while (/^\s*#\s*\[/.test(lines[from + n] ?? '')) n++;
	return n;
}

/**
 * Height of the output pane in text lines, reserved before any run so that output arriving seconds after the click
 * (Pyodide download) does not push the page down. Each `print(` counts one line, times the iterations of the literal
 * `for` loops around it, or the rows of a matrix written in the comment lines under it. An explicit `rows` on the
 * cell overrides the estimate; output longer than the pane scrolls inside it, it never resizes the pane.
 */
export function outputRows(code: string): number {
	const lines = code.split('\n');
	const loops: { indent: number; n: number }[] = [];
	let rows = 0;
	lines.forEach((line, i) => {
		if (!line.trim() || /^\s*#/.test(line)) return;
		const indent = line.length - line.trimStart().length;
		while (loops.length && indent <= (loops.at(-1)?.indent ?? -1)) loops.pop();
		const mult = loops.reduce((m, l) => m * l.n, 1);
		const prints = line.match(/\bprint\(/g)?.length ?? 0;
		if (prints) rows += mult * Math.max(prints, commentRows(lines, i + 1));
		if (/^\s*for\b/.test(line)) loops.push({ indent, n: loopCount(line) });
	});
	return Math.min(Math.max(rows, 2), MAX_ROWS);
}
