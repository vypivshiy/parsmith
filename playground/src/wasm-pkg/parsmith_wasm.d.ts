/* tslint:disable */
/* eslint-disable */

/**
 * Validates and compiles a KDL 2.0 schema for the target configuration in `target_json`,
 * returning a JSON-serialized [`CompileAndLintResult`].
 */
export function compile_and_lint(kdl_src: string, target_json: string): string;

/**
 * Infers a KDL 2.0 `json` schema from `json_src` using `schema_name` (defaulting to `"JsonResponse"`),
 * returning a JSON-serialized [`JsonToKdlResult`].
 */
export function json_to_kdl(json_src: string, schema_name?: string | null): string;

/**
 * Compiles `kdl_src` to standalone JavaScript (`parser.js`) and extracts AST selector check
 * descriptors for `struct_name`, returning a JSON-serialized [`PrepareTestHtmlResult`].
 */
export function prepare_test_html(kdl_src: string, struct_name?: string | null): string;

/**
 * Executes `parsmith.check()` for the Node.js WASM fallback.
 */
export function sdk_check(entry_or_source: string, is_path: boolean, logical_path?: string | null, sources_json?: string | null, targets_json?: string | null): string;

/**
 * Executes `parsmith.generate()` for the Node.js WASM fallback.
 */
export function sdk_generate(entry_or_source: string, is_path: boolean, logical_path: string | null | undefined, sources_json: string | null | undefined, options_json: string): string;

/**
 * Executes `parsmith.inspectSchema()` for the Node.js WASM fallback.
 */
export function sdk_inspect(entry_or_source: string, is_path: boolean, logical_path?: string | null, sources_json?: string | null): string;

/**
 * Renders a serialized `parsmith::Diagnostic` with an optional custom source text.
 */
export function sdk_render_diagnostic(diag_json: string, source?: string | null): string;

/**
 * Returns the `parsmith` compiler version string.
 */
export function version(): string;

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
    readonly memory: WebAssembly.Memory;
    readonly compile_and_lint: (a: number, b: number, c: number, d: number) => [number, number];
    readonly json_to_kdl: (a: number, b: number, c: number, d: number) => [number, number];
    readonly prepare_test_html: (a: number, b: number, c: number, d: number) => [number, number];
    readonly sdk_check: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => [number, number, number, number];
    readonly sdk_generate: (a: number, b: number, c: number, d: number, e: number, f: number, g: number, h: number, i: number) => [number, number, number, number];
    readonly sdk_inspect: (a: number, b: number, c: number, d: number, e: number, f: number, g: number) => [number, number, number, number];
    readonly sdk_render_diagnostic: (a: number, b: number, c: number, d: number) => [number, number];
    readonly version: () => [number, number];
    readonly __wbindgen_externrefs: WebAssembly.Table;
    readonly __wbindgen_malloc: (a: number, b: number) => number;
    readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
    readonly __wbindgen_free: (a: number, b: number, c: number) => void;
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
