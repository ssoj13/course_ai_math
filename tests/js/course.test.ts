import { describe, expect, test } from 'bun:test';
import { adjacent, lessonBySlug, lessons, modules } from '../../src/lib/course';
import { numberCourse, numbered } from '../../src/lib/course-rules.mjs';

describe('course manifest', () => {
	test('lessons follow course order', () => {
		expect(lessons[0].slug).toBe('why-this-course');
		expect(modules[0].slug).toBe('start');
		expect(modules[1].lessons[0].slug).toBe('squares-and-plane');
		expect(lessons.at(-1)?.slug).toBe('tensor-road');
		expect(modules.at(-1)?.slug).toBe('tensors');
	});
	test('CURRICULUM numbering: start is unnumbered, Ground is Part 0, explicit parts hold their number', () => {
		expect(modules.map((m) => [m.slug, m.part])).toEqual([['start', null], ['ground', 0], ['vectors', 1], ['matrices', 2], ['tensors', 3]]);
		expect(lessons.filter((l) => l.moduleSlug === 'start').map((l) => l.id)).toEqual([null, null]);
		expect(lessonBySlug('squares-and-plane')?.id).toBe('0.1');
		expect(lessonBySlug('law-of-cosines')?.id).toBe('0.5');
		expect(lessonBySlug('cosine-similarity')?.id).toBe('1.7');
		expect(lessonBySlug('cross-product')?.id).toBe('1.8');
		expect(lessonBySlug('linear-transformations')?.id).toBe('2.1');
		expect(lessonBySlug('matrix-columns')?.id).toBe('2.2');
		expect(lessonBySlug('tensor-road')?.id).toBe('3.1');
		for (const l of lessons) expect(l.id === null).toBe(l.part === null);
		expect(numbered('0.3', 'Pythagoras')).toBe('0.3 Pythagoras');
		expect(numbered(0, 'Ground')).toBe('0. Ground');
		expect(numbered(null, 'Start here')).toBe('Start here');
	});
	test('numberCourse rejects contradictory or non-increasing parts', () => {
		const m = (x: object) => ({ lessons: [{}], ...x });
		expect(numberCourse([m({}), m({}), m({ part: 5 }), m({})]).map((n) => n.part)).toEqual([0, 1, 5, 6]);
		expect(() => numberCourse([m({ unnumbered: true, part: 0 })])).toThrow(/exclude/);
		expect(() => numberCourse([m({}), m({ part: 0 })])).toThrow(/above 0/);
		expect(() => numberCourse([m({ part: 1.5 })])).toThrow(/integer/);
	});
	test('slugs are unique', () => {
		expect(new Set(lessons.map((l) => l.slug)).size).toBe(lessons.length);
	});
	test('prev/next cross module boundaries', () => {
		expect(adjacent('how-it-connects').next?.slug).toBe('squares-and-plane');
		expect(adjacent('law-of-cosines').next?.slug).toBe('vector');
		expect(adjacent('vector').prev?.slug).toBe('law-of-cosines');
		expect(adjacent('waves-dft').next?.slug).toBe('linear-transformations');
		expect(adjacent('matrix-columns').next?.slug).toBe('tensor-road');
	});
	test('branch flag is carried through', () => {
		expect(modules[2].lessons.find((l) => l.slug === 'cross-product')?.branch).toBe(true);
	});
});
