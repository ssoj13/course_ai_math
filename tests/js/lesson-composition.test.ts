import { expect, test } from 'bun:test';
import { readFileSync } from 'node:fs';

/**
 * Page composition of the law-of-cosines lesson: every card's controls come directly after the card they drive.
 * The LC-2 timeline once sat under the detail card while the proof it steps played in the card above it, so a reader
 * moved the slider and saw nothing change next to it (user report).
 */
const ORDER = [
	'LawScene scene={0}', 'LawControls scene={0}', 'LawDetail ', 'LawDetailControls ',
	'LawScene scene={1}', 'LawControls scene={1}',
	'LawScene scene={2}', 'LawControls scene={2}', 'LawDetail proof', 'LawDetailControls proof',
	'LawScene scene={3}', 'LawControls scene={3}',
];

for (const page of ['src/content/docs/ground/law-of-cosines.mdx', 'src/content/docs/ru/ground/law-of-cosines.mdx']) {
	test('LC cards are followed by their own controls: ' + page, () => {
		const tags = [...readFileSync(page, 'utf8').matchAll(/<(Law\w+(?: proof)?(?: scene=\{\d\})?)[ />]/g)].map((m) => m[1]);
		const norm = tags.map((x) => (x === 'LawDetail' || x === 'LawDetailControls' ? x + ' ' : x));
		expect(norm).toEqual(ORDER);
	});
}
