import { createElement, type ReactNode } from 'react';

/**
 * Minimal inline markup for lesson strings that live in the dictionary (timeline steps, challenge texts):
 * `**bold**` and `*emphasis*`, nothing else. Lesson prose in MDX already has Markdown; these strings are rendered by
 * React components from plain text, where a literal `**a**` would reach the reader (LC audit E-1). Why not Markdown:
 * the only markup the course needs here is bold vectors and the odd emphasised word, and a full parser would pull
 * a dependency into the scene bundle. Pure: no DOM.
 */
const TOKEN = /\*\*(.+?)\*\*|\*(.+?)\*/g;

/** React nodes for `text`: `**x**` → `<strong>`, `*x*` → `<em>`; everything else is text. */
export function rich(text: string): ReactNode[] {
	const out: ReactNode[] = [];
	let last = 0;
	for (const m of text.matchAll(TOKEN)) {
		if (m.index > last) out.push(text.slice(last, m.index));
		const [, bold, em] = m;
		out.push(createElement(bold !== undefined ? 'strong' : 'em', { key: m.index }, bold ?? em));
		last = m.index + m[0].length;
	}
	if (last < text.length) out.push(text.slice(last));
	return out;
}

/** The same text without markers, for ARIA values and plain-text contexts. */
export const plain = (text: string): string => text.replace(TOKEN, (_, bold: string | undefined, em: string | undefined) => bold ?? em ?? '');
