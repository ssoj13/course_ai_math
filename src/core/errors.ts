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

/** Bad einsum grammar. `token` is the offending text and `pos` its character offset in the op string. */
export class SpecError extends CoreError {
	constructor(
		msg: string,
		readonly op: string,
		readonly token: string,
		readonly pos: number,
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
