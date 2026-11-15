import initWasm, {
  compile_and_lint,
  json_to_kdl,
  prepare_test_html,
  version as wasmVersion,
} from "../wasm-pkg/parsmith_wasm.js";
import type {
  CompileAndLintResult,
  HealthCheckStatus,
  JsonToKdlResult,
  PrepareTestHtmlResult,
  SelectorCheckSpec,
  SelectorHealthCheck,
  StructHealthReport,
  TargetSpecPayload,
  TestHtmlResult,
} from "../types";

let wasmInitPromise: Promise<string> | null = null;

function ensureXPathResultGlobals(): void {
  const g = globalThis as Record<string, unknown>;
  if (typeof g.XPathResult === "undefined") {
    g.XPathResult = {
      ANY_TYPE: 0,
      NUMBER_TYPE: 1,
      STRING_TYPE: 2,
      BOOLEAN_TYPE: 3,
      UNORDERED_NODE_ITERATOR_TYPE: 4,
      ORDERED_NODE_ITERATOR_TYPE: 5,
      UNORDERED_NODE_SNAPSHOT_TYPE: 6,
      ORDERED_NODE_SNAPSHOT_TYPE: 7,
      ANY_UNORDERED_NODE_TYPE: 8,
      FIRST_ORDERED_NODE_TYPE: 9,
    };
  }
}

/**
 * Initialize the `parsmith-wasm` module once and return the compiler version string.
 * Supports both browser (`fetch` asset URL) and Node/Vitest (`node:fs/promises`) runtimes.
 */
export function ensureParsmithWasm(): Promise<string> {
  if (!wasmInitPromise) {
    wasmInitPromise = (async () => {
      ensureXPathResultGlobals();
      const nodeProcess = (
        globalThis as {
          process?: { versions?: { node?: string }; cwd?: () => string };
        }
      ).process;
      if (typeof nodeProcess !== "undefined" && nodeProcess?.versions?.node) {
        const fsModuleId = "node:fs/promises";
        const pathModuleId = "node:path";
        const fs = (await import(/* @vite-ignore */ fsModuleId)) as {
          readFile: (path: string | URL) => Promise<Uint8Array>;
        };
        const pathMod = (await import(/* @vite-ignore */ pathModuleId)) as {
          resolve: (...segments: string[]) => string;
        };
        const candidates: Array<string | URL> = [
          new URL("../wasm-pkg/parsmith_wasm_bg.wasm", import.meta.url),
        ];
        if (typeof nodeProcess.cwd === "function") {
          const cwd = nodeProcess.cwd();
          candidates.push(
            pathMod.resolve(cwd, "src/wasm-pkg/parsmith_wasm_bg.wasm"),
            pathMod.resolve(
              cwd,
              "playground/src/wasm-pkg/parsmith_wasm_bg.wasm",
            ),
          );
        }
        let wasmBytes: Uint8Array | null = null;
        for (const candidate of candidates) {
          try {
            wasmBytes = await fs.readFile(candidate);
            break;
          } catch {
            // Try next candidate path
          }
        }
        if (wasmBytes) {
          await initWasm({ module_or_path: wasmBytes });
        } else {
          await initWasm();
        }
      } else {
        await initWasm();
      }
      return wasmVersion();
    })().catch((err: unknown) => {
      wasmInitPromise = null;
      throw err;
    });
  }
  return wasmInitPromise;
}

/**
 * Validate and compile a KDL 2.0 schema for the requested target options via `parsmith-wasm`.
 */
export async function compileAndLint(
  kdlSrc: string,
  target?: Partial<TargetSpecPayload>,
): Promise<CompileAndLintResult> {
  await ensureParsmithWasm();
  const raw = compile_and_lint(kdlSrc, JSON.stringify(target ?? {}));
  return JSON.parse(raw) as CompileAndLintResult;
}

/**
 * Infer KDL 2.0 `json` schema definitions from a sample JSON payload via `parsmith-wasm`.
 */
export async function jsonToKdl(
  jsonSrc: string,
  schemaName?: string,
): Promise<JsonToKdlResult> {
  await ensureParsmithWasm();
  const normalizedName =
    schemaName && schemaName.trim() ? schemaName.trim() : undefined;
  const raw = json_to_kdl(jsonSrc, normalizedName);
  return JSON.parse(raw) as JsonToKdlResult;
}

function evaluateCssCount(doc: Document, query: string): number {
  try {
    return doc.querySelectorAll(query).length;
  } catch {
    return -1;
  }
}

function evaluateXpathCount(doc: Document, query: string): number {
  try {
    const snap = doc.evaluate(
      query,
      doc,
      null,
      7 /* XPathResult.ORDERED_NODE_SNAPSHOT_TYPE */,
      null,
    );
    return snap.snapshotLength;
  } catch {
    try {
      const anyRes = doc.evaluate(
        query,
        doc,
        null,
        0 /* XPathResult.ANY_TYPE */,
        null,
      );
      if (anyRes.resultType === 1) {
        return Number.isNaN(anyRes.numberValue) || anyRes.numberValue === 0
          ? 0
          : 1;
      }
      if (anyRes.resultType === 2) {
        return anyRes.stringValue ? 1 : 0;
      }
      if (anyRes.resultType === 3) {
        return anyRes.booleanValue ? 1 : 0;
      }
      return -1;
    } catch {
      return -1;
    }
  }
}

/**
 * Evaluate extracted AST selector check descriptors against an HTML string
 * using browser-native `DOMParser` and `document.evaluate` (`XPathResult`).
 */
export function evaluateStructHealth(
  structName: string,
  structKind: string,
  selectors: SelectorCheckSpec[],
  htmlSrc: string,
): StructHealthReport {
  ensureXPathResultGlobals();

  if (structKind === "raw") {
    return {
      struct_name: structName,
      passed: true,
      checks: [
        {
          path: structName,
          selector_type: "raw",
          query: "",
          matches: 0,
          status: "warn",
          message: "(raw)struct has no HTML selectors to check",
        },
      ],
    };
  }

  const doc = new DOMParser().parseFromString(htmlSrc, "text/html");
  const checks: SelectorHealthCheck[] = [];

  for (const spec of selectors) {
    const isCss = spec.selector_type.startsWith("css");
    const isXpath = spec.selector_type.startsWith("xpath");
    if (!isCss && !isXpath) {
      continue;
    }
    const isRemove = spec.selector_type.endsWith("-remove");
    const queries = spec.queries.length > 0 ? spec.queries : [spec.report_query];

    let count = 0;
    for (const q of queries) {
      count = isCss ? evaluateCssCount(doc, q) : evaluateXpathCount(doc, q);
      if (count === -1 || count > 0) {
        break;
      }
    }

    if (count === -1) {
      checks.push({
        path: spec.path,
        selector_type: spec.selector_type,
        query: spec.report_query,
        matches: 0,
        status: "fail",
        message: "invalid selector syntax",
      });
      continue;
    }

    const hasFallback =
      spec.fallback_value !== null && spec.fallback_value !== undefined;

    let status: HealthCheckStatus;
    if (isRemove) {
      status = count > 0 ? "ok" : "warn";
    } else if (count > 0) {
      status = "ok";
    } else if (hasFallback) {
      status = "warn";
    } else {
      status = "fail";
    }

    let message: string;
    if (status === "ok") {
      message = `(${count} matches)`;
    } else if (status === "warn" && hasFallback && count === 0) {
      message = `(0 matches, fallback=${spec.fallback_value})`;
    } else if (status === "warn") {
      message = `(${count} matches)`;
    } else {
      message = "(0 matches)";
    }

    checks.push({
      path: spec.path,
      selector_type: spec.selector_type,
      query: spec.report_query,
      matches: count,
      status,
      message,
    });
  }

  return {
    struct_name: structName,
    passed: !checks.some((c) => c.status === "fail"),
    checks,
  };
}

/**
 * Execute a generated standalone JavaScript parser module (`parser.js`) against `inputSrc`.
 */
export function executeGeneratedJsParser(
  jsCode: string,
  className: string,
  inputSrc: string,
): unknown {
  ensureXPathResultGlobals();
  if (!/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(className)) {
    throw new Error(`Invalid generated parser class name '${className}'`);
  }
  const transformedCode = jsCode.replace(
    /^export\s+(?=(?:async\s+)?(?:class|function|const|let|var)\b)/gm,
    "",
  );
  const runner = new Function(
    "inputSrc",
    `${transformedCode}\nreturn new ${className}(inputSrc).parse();`,
  ) as (src: string) => unknown;
  const rawResult = runner(inputSrc);
  if (rawResult === undefined) {
    return null;
  }
  return JSON.parse(JSON.stringify(rawResult));
}

/**
 * Compile `kdlSrc` to JavaScript via `parsmith-wasm`, evaluate selector health checks,
 * and run `new <ClassName>(htmlSrc).parse()` in-process.
 */
export async function testHtml(
  kdlSrc: string,
  htmlSrc: string,
  structName?: string | null,
): Promise<TestHtmlResult> {
  await ensureParsmithWasm();
  const normalizedStruct =
    structName && structName.trim() ? structName.trim() : undefined;
  const rawPrep = prepare_test_html(kdlSrc, normalizedStruct);
  const prep = JSON.parse(rawPrep) as PrepareTestHtmlResult;

  if (!prep.ok || !prep.struct_name || !prep.class_name || !prep.js_code) {
    return {
      ok: false,
      struct_name: prep.struct_name,
      run_result: null,
      run_error: prep.error ?? "Failed to prepare HTML test",
      elapsed_ms: 0,
      health: null,
    };
  }

  const health = evaluateStructHealth(
    prep.struct_name,
    prep.struct_kind ?? "item",
    prep.selectors,
    htmlSrc,
  );

  const t0 = performance.now();
  try {
    const runResult = executeGeneratedJsParser(
      prep.js_code,
      prep.class_name,
      htmlSrc,
    );
    const elapsedMs = Math.round((performance.now() - t0) * 1000) / 1000;
    return {
      ok: true,
      struct_name: prep.struct_name,
      run_result: runResult,
      run_error: null,
      elapsed_ms: elapsedMs,
      health,
    };
  } catch (err: unknown) {
    const elapsedMs = Math.round((performance.now() - t0) * 1000) / 1000;
    const errorMsg =
      err instanceof Error ? `${err.name}: ${err.message}` : String(err);
    return {
      ok: false,
      struct_name: prep.struct_name,
      run_result: null,
      run_error: errorMsg,
      elapsed_ms: elapsedMs,
      health,
    };
  }
}
