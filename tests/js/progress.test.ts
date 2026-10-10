import { expect, test } from 'bun:test';

// progress.ts resolves lesson URLs at import time, which needs Astro's BASE_URL (bun aliases import.meta.env to process.env).
process.env.BASE_URL = '/math-course/';
const { describe: describeProgress } = await import('../../src/lib/progress');

test('describe omits the optional part when the course has no optional modules', () => {
	expect(describeProgress({})).toBe('0/12 core');
	expect(describeProgress({ pythagoras: true, vector: true })).toBe('2/12 core');
});
