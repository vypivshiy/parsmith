import { useCallback, useEffect, useRef, useState } from "react";
import type {
  CompileAndLintResult,
  JsonToKdlResult,
  TargetSpecPayload,
  TestHtmlResult,
  WorkerStatus,
} from "../types";
import {
  compileAndLint as wasmCompileAndLint,
  ensureParsmithWasm,
  jsonToKdl as wasmJsonToKdl,
  testHtml as wasmTestHtml,
} from "./wasmClient";

export interface ParsmithRuntimeHandle {
  status: WorkerStatus;
  version: string | null;
  workerError: string | null;
  isCompiling: boolean;
  isTestingHtml: boolean;
  compileAndLint: (
    kdlSrc: string,
    target: TargetSpecPayload,
  ) => Promise<CompileAndLintResult>;
  testHtml: (
    kdlSrc: string,
    htmlSrc: string,
    structName?: string | null,
  ) => Promise<TestHtmlResult>;
  jsonToKdl: (
    jsonSrc: string,
    schemaName?: string,
  ) => Promise<JsonToKdlResult>;
}

export function useParsmithRuntime(): ParsmithRuntimeHandle {
  const [status, setStatus] = useState<WorkerStatus>("loading");
  const [version, setVersion] = useState<string | null>(null);
  const [workerError, setWorkerError] = useState<string | null>(null);
  const [isCompiling, setIsCompiling] = useState<boolean>(false);
  const [isTestingHtml, setIsTestingHtml] = useState<boolean>(false);

  const compilePendingCountRef = useRef<number>(0);
  const testPendingCountRef = useRef<number>(0);

  useEffect(() => {
    let cancelled = false;
    ensureParsmithWasm()
      .then((ver) => {
        if (!cancelled) {
          setVersion(ver);
          setStatus("ready");
          setWorkerError(null);
        }
      })
      .catch((err: unknown) => {
        if (!cancelled) {
          setStatus("error");
          setWorkerError(
            err instanceof Error
              ? err.message
              : String(err || "Failed to initialize parsmith-wasm"),
          );
        }
      });

    return () => {
      cancelled = true;
    };
  }, []);

  const runTrackedTask = useCallback(
    async <T>(
      counterRef: React.MutableRefObject<number>,
      setBusy: (busy: boolean) => void,
      fallbackMsg: string,
      task: () => Promise<T>,
    ): Promise<T> => {
      counterRef.current += 1;
      setBusy(true);
      try {
        const res = await task();
        setWorkerError(null);
        return res;
      } catch (err: unknown) {
        const msg =
          err instanceof Error ? err.message : String(err || fallbackMsg);
        setWorkerError(msg);
        throw err instanceof Error ? err : new Error(msg);
      } finally {
        counterRef.current = Math.max(0, counterRef.current - 1);
        setBusy(counterRef.current > 0);
      }
    },
    [],
  );

  const compileAndLint = useCallback(
    (
      kdlSrc: string,
      target: TargetSpecPayload,
    ): Promise<CompileAndLintResult> =>
      runTrackedTask(
        compilePendingCountRef,
        setIsCompiling,
        "parsmith-wasm compile_and_lint failed",
        () => wasmCompileAndLint(kdlSrc, target),
      ),
    [runTrackedTask],
  );

  const testHtml = useCallback(
    (
      kdlSrc: string,
      htmlSrc: string,
      structName?: string | null,
    ): Promise<TestHtmlResult> =>
      runTrackedTask(
        testPendingCountRef,
        setIsTestingHtml,
        "parsmith-wasm test_html failed",
        () => wasmTestHtml(kdlSrc, htmlSrc, structName),
      ),
    [runTrackedTask],
  );

  const jsonToKdl = useCallback(
    async (
      jsonSrc: string,
      schemaName?: string,
    ): Promise<JsonToKdlResult> => {
      return wasmJsonToKdl(jsonSrc, schemaName);
    },
    [],
  );

  return {
    status,
    version,
    workerError,
    isCompiling,
    isTestingHtml,
    compileAndLint,
    testHtml,
    jsonToKdl,
  };
}
