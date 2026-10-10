/** Feature check before mounting the shared canvas; without WebGL the captions/formulas/tables still teach. */
export function webglAvailable(make: () => HTMLCanvasElement = () => document.createElement('canvas')): boolean {
	try {
		const c = make();
		return !!(c.getContext('webgl2') ?? c.getContext('webgl'));
	} catch {
		return false;
	}
}
