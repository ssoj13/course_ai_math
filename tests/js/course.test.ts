import { describe, expect, test } from 'bun:test';
import { adjacent, lessons, modules } from '../../src/lib/course';

describe('course manifest', () => {
	test('lessons are numbered globally in course order', () => {
		expect(lessons.map((l) => l.num)).toEqual(lessons.map((_, i) => i + 1));
		expect(lessons[0].slug).toBe('why-this-course');
		expect(modules[0].slug).toBe('start');
		expect(modules[1].lessons[0].slug).toBe('squares-and-plane');
		expect(lessons.at(-1)?.slug).toBe('tensor-road');
		expect(modules.at(-1)?.slug).toBe('tensors');
	});
	test('slugs are unique', () => {
		expect(new Set(lessons.map((l) => l.slug)).size).toBe(lessons.length);
	});
	test('prev/next cross module boundaries', () => {
		expect(adjacent('how-it-connects').next?.slug).toBe('squares-and-plane');
		expect(adjacent('law-of-cosines').next?.slug).toBe('vector');
		expect(adjacent('vector').prev?.slug).toBe('law-of-cosines');
		expect(adjacent('waves-dft').next?.slug).toBe('tensor-road');
	});
	test('branch flag is carried through', () => {
		expect(modules[2].lessons.find((l) => l.slug === 'cross-product')?.branch).toBe(true);
	});
});
