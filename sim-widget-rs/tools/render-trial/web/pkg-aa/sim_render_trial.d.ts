/* tslint:disable */
/* eslint-disable */

export class CosinesTrial {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    static create(canvas: HTMLCanvasElement): Promise<CosinesTrial>;
    dispose(): void;
    fit_view(perspective: boolean): void;
    render(width: number, height: number, dpr: number): string;
    reset_values(): void;
    seek(fraction: number): void;
    set_locale(locale: string): void;
    set_parameter(name: string, value: number): void;
    set_playing(playing: boolean): void;
}

/**
 * Browser trial uses a real host-owned surface and separately captures production pixels.
 */
export function run_portable(canvas: HTMLCanvasElement): Promise<string>;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_cosinestrial_free: (a: number, b: number) => void;
    readonly cosinestrial_create: (a: number) => number;
    readonly cosinestrial_dispose: (a: number) => void;
    readonly cosinestrial_fit_view: (a: number, b: number, c: number) => void;
    readonly cosinestrial_render: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly cosinestrial_reset_values: (a: number, b: number) => void;
    readonly cosinestrial_seek: (a: number, b: number, c: number) => void;
    readonly cosinestrial_set_locale: (a: number, b: number, c: number, d: number) => void;
    readonly cosinestrial_set_parameter: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly cosinestrial_set_playing: (a: number, b: number, c: number) => void;
    readonly run_portable: (a: number) => number;
    readonly __wasm_bindgen_func_elem_1226: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1226_13: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_1226_14: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_2743: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_2745: (a: number, b: number, c: number, d: number) => void;
    readonly __wbindgen_export: (a: number, b: number) => number;
    readonly __wbindgen_export2: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_export3: (a: number) => void;
    readonly __wbindgen_export4: (a: number, b: number, c: number) => void;
    readonly __wbindgen_export5: (a: number, b: number) => void;
    readonly __wbindgen_add_to_stack_pointer: (a: number) => number;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;

/**
 * Instantiates the given `module`, which can either be bytes or
 * a precompiled `WebAssembly.Module`.
 *
 * @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
 *
 * @returns {InitOutput}
 */
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
 * If `module_or_path` is {RequestInfo} or {URL}, makes a request and
 * for everything else, calls `WebAssembly.instantiate` directly.
 *
 * @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
 *
 * @returns {Promise<InitOutput>}
 */
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
