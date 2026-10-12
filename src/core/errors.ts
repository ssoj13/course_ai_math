/**
 * Typed errors of the tensor core. Every invalid spec, shape, index or dtype throws one of these instead of being
 * repaired: a silently broadcast or clamped tensor would draw a plausible but wrong scene. Callers (scenes, the
 * N-panel, the status bar) branch on the class or on `code`, never on message text.
 */
export class CoreError extends Error {
	constructor(
		readonly code: 'spec' | 'shape' | 'index' | 'dtype',
		msg: string,
	) {
		super(msg);
		this.name = new.target.name;
	}
}

/**
 * Which einsum grammar rule a `SpecError` broke: a machine-readable reason, so a UI can explain the error in the
 * reader's language (the message text is English and for developers only).
 */
export type SpecReason =
	| 'noArrow' // no "->" (implicit output is not supported)
	| 'arrows' // more than one "->"
	| 'ellipsis' // "..." is not supported
	| 'outComma' // "," in the output
	| 'space' // whitespace
	| 'char' // anything but a-z, "," and "->"
	| 'arity' // a fixed operand count was asked for and not met
	| 'repeatIn' // a letter twice inside one operand (a trace / diagonal)
	| 'repeatOut' // a letter twice in the output
	| 'unknownOut'; // an output letter no operand has

/** Bad einsum grammar. `token` is the offending text and `pos` its character offset in the op string. */
export class SpecError extends CoreError {
	constructor(
		msg: string,
		readonly op: string,
		readonly token: string,
		readonly pos: number,
		readonly reason: SpecReason,
	) {
		super('spec', `${msg} at ${pos} ("${token}") in "${op}"`);
	}
}

/** Shapes that do not fit: rank mismatch, failed broadcast, contraction extents that differ, bad reshape. */
export class ShapeError extends CoreError {
	constructor(msg: string) {
		super('shape', msg);
	}
}

/**
 * One einsum letter bound to two different extents (the "cable" between two axes of different lengths): a contraction
 * or a shared batch axis needs the same length on both sides. Carries the letter and both extents for a UI.
 */
export class ExtentError extends ShapeError {
	constructor(
		msg: string,
		readonly letter: string,
		readonly first: number,
		readonly second: number,
	) {
		super(msg);
	}
}

/** Index outside an axis, or a write into a read-only (broadcast) view. */
export class IndexError extends CoreError {
	constructor(msg: string) {
		super('index', msg);
	}
}

/** Unknown or not yet implemented arithmetic. */
export class DtypeError extends CoreError {
	constructor(msg: string) {
		super('dtype', msg);
	}
}
