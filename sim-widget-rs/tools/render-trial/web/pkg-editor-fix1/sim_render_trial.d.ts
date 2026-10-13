/* tslint:disable */
/* eslint-disable */

export class CosinesTrial {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    advance(fraction: number): void;
    camera_gesture(button: number, space: boolean, alt: boolean, shift: boolean): string;
    clear_hover(): void;
    static create(canvas: HTMLCanvasElement): Promise<CosinesTrial>;
    cycle_pick(step: number): string;
    dispose(): void;
    fit_view(perspective: boolean): void;
    navigate(kind: string, from_x: number, from_y: number, to_x: number, to_y: number, precision: boolean): void;
    nudge_selected(x: number, y: number, z: number): void;
    playback_state(): string;
    pointer_begin(x: number, y: number, touch: boolean, pointer: number): boolean;
    pointer_end(pointer: number, cancel: boolean): void;
    pointer_hover(x: number, y: number, touch: boolean): string;
    pointer_move(x: number, y: number, pointer: number): void;
    render(width: number, height: number, dpr: number): string;
    reset_values(): void;
    seek(fraction: number): void;
    select_handle(name: string): void;
    set_constraint(profile: string): void;
    set_locale(locale: string): void;
    set_parameter(name: string, value: number): void;
    set_playing(playing: boolean): void;
    set_viewport_rect(left: number, top: number, width: number, height: number, dpr: number): void;
    undo_input(redo: boolean): void;
    wheel_dolly(x: number, y: number, delta: number, precision: boolean): void;
}

export class EditorTrial {
    private constructor();
    free(): void;
    [Symbol.dispose](): void;
    cancel(): void;
    static create(canvas: HTMLCanvasElement): Promise<EditorTrial>;
    dispose(): void;
    event(source: string): void;
    render(width: number, height: number, dpr: number, now: number): Promise<string>;
    set_canvas_rect(left: number, top: number): void;
}

/**
 * Browser trial uses a real host-owned surface and separately captures production pixels.
 */
export function run_portable(canvas: HTMLCanvasElement): Promise<string>;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly __wbg_cosinestrial_free: (a: number, b: number) => void;
    readonly __wbg_editortrial_free: (a: number, b: number) => void;
    readonly cosinestrial_advance: (a: number, b: number, c: number) => void;
    readonly cosinestrial_camera_gesture: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly cosinestrial_clear_hover: (a: number, b: number) => void;
    readonly cosinestrial_create: (a: number) => number;
    readonly cosinestrial_cycle_pick: (a: number, b: number, c: number) => void;
    readonly cosinestrial_dispose: (a: number) => void;
    readonly cosinestrial_fit_view: (a: number, b: number, c: number) => void;
    readonly cosinestrial_navigate: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => void;
    readonly cosinestrial_nudge_selected: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly cosinestrial_playback_state: (a: number, b: number) => void;
    readonly cosinestrial_pointer_begin: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly cosinestrial_pointer_end: (a: number, b: number, c: number, d: number) => void;
    readonly cosinestrial_pointer_hover: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly cosinestrial_pointer_move: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly cosinestrial_render: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly cosinestrial_reset_values: (a: number, b: number) => void;
    readonly cosinestrial_seek: (a: number, b: number, c: number) => void;
    readonly cosinestrial_select_handle: (a: number, b: number, c: number, d: number) => void;
    readonly cosinestrial_set_constraint: (a: number, b: number, c: number, d: number) => void;
    readonly cosinestrial_set_locale: (a: number, b: number, c: number, d: number) => void;
    readonly cosinestrial_set_parameter: (a: number, b: number, c: number, d: number, e: number) => void;
    readonly cosinestrial_set_playing: (a: number, b: number, c: number) => void;
    readonly cosinestrial_set_viewport_rect: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => void;
    readonly cosinestrial_undo_input: (a: number, b: number, c: number) => void;
    readonly cosinestrial_wheel_dolly: (a: number, b: number, c: number, d: number, e: number, f: number) => void;
    readonly editortrial_cancel: (a: number, b: number) => void;
    readonly editortrial_create: (a: number) => number;
    readonly editortrial_dispose: (a: number) => void;
    readonly editortrial_event: (a: number, b: number, c: number, d: number) => void;
    readonly editortrial_render: (a: number, b: number, c: number, d: number, e: number) => number;
    readonly editortrial_set_canvas_rect: (a: number, b: number, c: number, d: number) => void;
    readonly run_portable: (a: number) => number;
    readonly __wasm_bindgen_func_elem_14298: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_14298_35: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_14298_36: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_23790: (a: number, b: number, c: number, d: number) => void;
    readonly __wasm_bindgen_func_elem_23792: (a: number, b: number, c: number, d: number) => void;
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
