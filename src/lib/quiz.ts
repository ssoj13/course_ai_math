/**
 * Self-check quiz rules, shared by <Quiz> (build-time validation: a broken quiz fails the build) and
 * tests/js/quiz-lint.test.ts (structure plus the course-wide "longest option" statistics).
 */

export interface Question {
	q: string;
	options: string[];
	answer: number;
	explain: string;
}

/** Every question has exactly this many options (course quiz rules, notes/fix-wave-1.md). */
export const OPTION_COUNT = 4;

/**
 * Structural problems of one question (empty list = valid). Takes `unknown` because the value comes
 * from MDX props or from the lint's evaluation of the MDX source, not from typed code.
 */
export function questionErrors(item: unknown): string[] {
	const errs: string[] = [];
	const it = (item ?? {}) as Partial<Record<keyof Question, unknown>>;
	const nonEmpty = (v: unknown): v is string => typeof v === 'string' && v.trim() !== '';
	if (!nonEmpty(it.q)) errs.push('empty `q`');
	if (!Array.isArray(it.options) || it.options.length !== OPTION_COUNT) {
		errs.push(`needs exactly ${OPTION_COUNT} options (has ${Array.isArray(it.options) ? it.options.length : 'none'})`);
	} else if (!it.options.every(nonEmpty)) {
		errs.push('every option must be a non-empty string');
	}
	const n = Array.isArray(it.options) ? it.options.length : 0;
	if (!Number.isInteger(it.answer) || (it.answer as number) < 0 || (it.answer as number) >= n) {
		errs.push(`answer index ${String(it.answer)} is out of range 0..${n - 1}`);
	}
	if (!nonEmpty(it.explain)) errs.push('empty `explain`');
	return errs;
}

/** Length in code points, so a non-BMP character counts once. */
const len = (s: string): number => [...s].length;

/**
 * True when the correct option is strictly longer than every distractor: the classic test-taking
 * giveaway (pick the longest answer) that the quiz rules limit.
 */
export function correctIsLongest(q: Question): boolean {
	const right = len(q.options[q.answer]);
	return q.options.every((o, i) => i === q.answer || len(o) < right);
}

/** Per lesson: at most one third of the questions may have the longest-correct giveaway (floor, min 1). */
export const giveawayLimit = (questions: number): number => Math.max(1, Math.floor(questions / 3));

/** Across the whole course: at most this share of questions may have the giveaway. */
export const COURSE_GIVEAWAY_SHARE = 0.3;
