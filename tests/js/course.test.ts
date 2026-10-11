import { describe, expect, test } from 'bun:test';
import { adjacent, lessons, modules } from '../../src/lib/course';

describe('course manifest', () => {
	test('lessons are numbered globally in course order', () => {
		expect(lessons.map((l) => l.num)).toEqual(lessons.map((_, i) => i + 1));
		expect(lessons[0].slug).toBe('squares-and-plane');
		expect(lessons.at(-1)?.slug).toBe('waves-dft');
	});
	test('slugs are unique', () => {
		expect(new Set(lessons.map((l) => l.slug)).size).toBe(lessons.length);
	});
	test('prev/next cross module boundaries', () => {
		expect(adjacent('law-of-cosines').next?.slug).toBe('vector');
		expect(adjacent('vector').prev?.slug).toBe('law-of-cosines');
	});
	test('branch flag is carried through', () => {
		expect(modules[1].lessons.find((l) => l.slug === 'cross-product')?.branch).toBe(true);
	});
});
