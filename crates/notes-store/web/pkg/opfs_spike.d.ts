/* tslint:disable */
/* eslint-disable */

/**
 * Open the OPFS database, seed it on first run, and report what is stored.
 *
 * `reset` removes the rows so a test run starts from an empty database.
 */
export function run(reset: boolean): Promise<string>;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly main: (a: number, b: number) => number;
    readonly run: (a: number) => any;
    readonly rust_sqlite_wasm_abort: () => void;
    readonly rust_sqlite_wasm_assert_fail: (a: number, b: number, c: number, d: number) => void;
    readonly rust_sqlite_wasm_calloc: (a: number, b: number) => number;
    readonly rust_sqlite_wasm_malloc: (a: number) => number;
    readonly rust_sqlite_wasm_free: (a: number) => void;
    readonly rust_sqlite_wasm_getentropy: (a: number, b: number) => number;
    readonly rust_sqlite_wasm_localtime: (a: number) => number;
    readonly rust_sqlite_wasm_realloc: (a: number, b: number) => number;
    readonly sqlite3_os_end: () => number;
    readonly sqlite3_os_init: () => number;
    readonly wasm_bindgen_37d9c22d7905d9f8___convert__closures_____invoke___js_sys_f18b74f8dba2bf99___Function_fn_wasm_bindgen_37d9c22d7905d9f8___JsValue_____wasm_bindgen_37d9c22d7905d9f8___sys__Undefined___js_sys_f18b74f8dba2bf99___Function_fn_wasm_bindgen_37d9c22d7905d9f8___JsValue_____wasm_bindgen_37d9c22d7905d9f8___sys__Undefined_______true_: (a: number, b: number, c: any, d: any) => void;
    readonly wasm_bindgen_37d9c22d7905d9f8___convert__closures_____invoke___wasm_bindgen_37d9c22d7905d9f8___JsValue__core_37ee0ce0c4cd57d5___result__Result_____wasm_bindgen_37d9c22d7905d9f8___JsError___true_: (a: number, b: number, c: any) => [number, number];
    readonly wasm_bindgen_37d9c22d7905d9f8___convert__closures_____invoke___bool__true_: (a: number, b: number) => number;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_exn_store: (a: number) => void;
    readonly __externref_table_alloc: () => number;
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_destroy_closure: (a: number, b: number) => void;
    readonly __externref_table_dealloc: (a: number) => void;
    readonly __wbindgen_start: () => void;
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
