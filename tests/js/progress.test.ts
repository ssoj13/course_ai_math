import '../../src/i18n/ui';
import { expect, test } from 'bun:test';
import course from '../../src/data/course.json';

// progress.ts resolves lesson URLs at import time, which needs Astro's BASE_URL (bun aliases import.meta.env to process.env).
process.env.BASE_URL = '/course_ai_math/';
const { describe: describeProgress } = await import('../../src/lib/progress');

// Derived from course.json so inserting a lesson does not break this gate.
const coreTotal = course.modules.filter((m) => !(m as { optional?: boolean }).optional).flatMap((m) => m.lessons).length;

test('describe omits the optional part when the course has no optional modules', () => {
	expect(describeProgress({})).toBe(`0/${coreTotal} core`);
	expect(describeProgress({ pythagoras: true, vector: true })).toBe(`2/${coreTotal} core`);
});
