import { useCallback, useEffect, useState } from "react";
import { useCopyFeedback } from "../hooks/useCopyFeedback";
import type { JsonToKdlResult } from "../types";

export const DEFAULT_JSON_SCHEMA_NAME = "JsonResponse";

export const DEFAULT_SAMPLE_JSON = JSON.stringify(
  {
    id: 101,
    title: "Parsmith Compiler",
    available: true,
    price: 29.99,
    tags: ["kdl", "wasm", "scraper"],
    author: {
      username: "parsmith",
      verified: true,
    },
  },
  null,
  2,
);

/**
 * Prepend a generated KDL `json` snippet to an existing KDL schema buffer.
 */
export function insertKdlSnippetIntoSchema(
  existingSchema: string,
  kdlSnippet: string,
): string {
  const trimmedSnippet = kdlSnippet.trim();
  if (!trimmedSnippet) {
    return existingSchema;
  }
  if (!existingSchema.trim()) {
    return `${trimmedSnippet}\n`;
  }
  return `${trimmedSnippet}\n\n${existingSchema}`;
}

/**
 * Replace the entire KDL schema buffer with a generated KDL snippet.
 */
export function replaceSchemaWithKdlSnippet(kdlSnippet: string): string {
  const trimmedSnippet = kdlSnippet.trim();
  return trimmedSnippet ? `${trimmedSnippet}\n` : "";
}

export interface JsonToKdlModalProps {
  isOpen: boolean;
  onClose: () => void;
  onInsertIntoSchema: (kdlSnippet: string) => void;
  onReplaceSchema: (kdlSnippet: string) => void;
  jsonToKdl: (
    jsonSrc: string,
    schemaName?: string,
  ) => Promise<JsonToKdlResult>;
}

export function JsonToKdlModal({
  isOpen,
  onClose,
  onInsertIntoSchema,
  onReplaceSchema,
  jsonToKdl,
}: JsonToKdlModalProps) {
  const [schemaName, setSchemaName] = useState<string>(
    DEFAULT_JSON_SCHEMA_NAME,
  );
  const [jsonInput, setJsonInput] = useState<string>(DEFAULT_SAMPLE_JSON);
  const [previewKdl, setPreviewKdl] = useState<string>("");
  const [error, setError] = useState<string | null>(null);
  const { copied, copyText } = useCopyFeedback(1500);

  useEffect(() => {
    if (!isOpen) {
      return;
    }
    let cancelled = false;
    const effectiveName = schemaName.trim() || DEFAULT_JSON_SCHEMA_NAME;

    jsonToKdl(jsonInput, effectiveName)
      .then((res) => {
        if (cancelled) return;
        if (res.ok) {
          setPreviewKdl(res.kdl);
          setError(null);
        } else {
          setPreviewKdl("");
          setError(res.error ?? "Invalid JSON input");
        }
      })
      .catch((err: unknown) => {
        if (cancelled) return;
        const msg = err instanceof Error ? err.message : String(err);
        setPreviewKdl("");
        setError(msg);
      });

    return () => {
      cancelled = true;
    };
  }, [isOpen, jsonInput, schemaName, jsonToKdl]);

  useEffect(() => {
    if (!isOpen) {
      return;
    }
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        onClose();
      }
    };
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [isOpen, onClose]);

  const handleInsert = useCallback(() => {
    if (!previewKdl.trim() || error) {
      return;
    }
    onInsertIntoSchema(previewKdl);
    onClose();
  }, [previewKdl, error, onInsertIntoSchema, onClose]);

  const handleReplace = useCallback(() => {
    if (!previewKdl.trim() || error) {
      return;
    }
    onReplaceSchema(previewKdl);
    onClose();
  }, [previewKdl, error, onReplaceSchema, onClose]);

  if (!isOpen) {
    return null;
  }

  const canApply = Boolean(previewKdl.trim()) && !error;

  return (
    <div
      className="modal-backdrop"
      role="presentation"
      onClick={(e) => {
        if (e.target === e.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        className="json-to-kdl-modal"
        role="dialog"
        aria-modal="true"
        aria-labelledby="json-to-kdl-modal-title"
      >
        <div className="json-to-kdl-modal-header">
          <div className="pane-title-group">
            <span id="json-to-kdl-modal-title" className="pane-title">
              JSON → KDL Schema Generator
            </span>
            <span className="pane-subtitle">
              Infer KDL 2.0 json definitions from sample JSON
            </span>
          </div>
          <button
            type="button"
            className="json-to-kdl-close-icon-btn"
            aria-label="Close modal"
            onClick={onClose}
          >
            ✕
          </button>
        </div>

        <div className="json-to-kdl-controls-row">
          <label
            className="target-option-label"
            htmlFor="json-to-kdl-name-input"
          >
            <span>Root Schema Name</span>
            <input
              id="json-to-kdl-name-input"
              type="text"
              className="target-option-input"
              data-testid="json-to-kdl-name-input"
              value={schemaName}
              placeholder={DEFAULT_JSON_SCHEMA_NAME}
              onChange={(e) => setSchemaName(e.target.value)}
            />
          </label>
        </div>

        {error && (
          <div
            className="target-error-banner"
            role="alert"
            data-testid="json-to-kdl-error"
          >
            <strong>JSON Parse Error:</strong> {error}
          </div>
        )}

        <div className="json-to-kdl-split">
          <div className="json-to-kdl-column">
            <div className="html-section-header">
              <span className="html-section-title">Sample JSON Input</span>
              <span className="html-section-meta">application/json</span>
            </div>
            <textarea
              className="json-to-kdl-textarea"
              data-testid="json-to-kdl-input"
              value={jsonInput}
              onChange={(e) => setJsonInput(e.target.value)}
              spellCheck={false}
              placeholder="Paste sample JSON object or array here…"
            />
          </div>

          <div className="json-to-kdl-column">
            <div className="html-section-header">
              <span className="html-section-title">Generated KDL Preview</span>
              <span className="html-section-meta">KDL 2.0</span>
            </div>
            <pre
              className="json-to-kdl-preview"
              data-testid="json-to-kdl-preview"
            >
              {previewKdl}
            </pre>
          </div>
        </div>

        <div className="json-to-kdl-modal-footer">
          <button
            type="button"
            className="copy-code-btn"
            data-testid="json-to-kdl-copy-btn"
            disabled={!canApply}
            onClick={() => void copyText(previewKdl)}
          >
            {copied ? "Copied!" : "Copy KDL"}
          </button>

          <div className="json-to-kdl-footer-actions">
            <button
              type="button"
              className="html-action-btn html-action-secondary"
              data-testid="json-to-kdl-close-btn"
              onClick={onClose}
            >
              Close
            </button>
            <button
              type="button"
              className="html-action-btn html-action-secondary"
              data-testid="json-to-kdl-replace-btn"
              disabled={!canApply}
              onClick={handleReplace}
            >
              Replace Schema
            </button>
            <button
              type="button"
              className="html-action-btn html-action-primary"
              data-testid="json-to-kdl-insert-btn"
              disabled={!canApply}
              onClick={handleInsert}
            >
              Insert into Schema
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}
