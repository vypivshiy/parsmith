import { useEffect, useMemo, useState } from "react";
import Editor from "@monaco-editor/react";
import { useCopyFeedback } from "../hooks/useCopyFeedback";
import {
  getMonacoLanguageForFile,
  normalizeTargetConfig,
  type Bs4ParserEngine,
  type CompileAndLintResult,
  type HttpIoMode,
  type PhpHtmlLib,
  type PythonHtmlLib,
  type PythonModels,
  type TargetConfig,
  type TargetLang,
} from "../types";

export interface CodeOutputPaneProps {
  target: TargetConfig;
  onChangeTarget: (nextTarget: TargetConfig) => void;
  result: CompileAndLintResult | null;
  theme: "vs-dark" | "light";
}

const TARGET_LANGUAGES: Array<{ id: TargetLang; label: string }> = [
  { id: "python", label: "Python" },
  { id: "ts", label: "TypeScript" },
  { id: "js", label: "JavaScript" },
  { id: "go", label: "Go" },
  { id: "rust", label: "Rust" },
  { id: "elixir", label: "Elixir" },
  { id: "php", label: "PHP" },
  { id: "csharp", label: "C#" },
  { id: "kotlin", label: "Kotlin" },
];

export function CodeOutputPane({
  target,
  onChangeTarget,
  result,
  theme,
}: CodeOutputPaneProps) {
  const fileNames = useMemo(
    () => (result?.files ? Object.keys(result.files) : []),
    [result],
  );

  const [selectedFile, setSelectedFile] = useState<string>("");
  const { copied, copyText } = useCopyFeedback(1500);

  useEffect(() => {
    if (fileNames.length === 0) {
      setSelectedFile("");
      return;
    }
    if (!selectedFile || !fileNames.includes(selectedFile)) {
      setSelectedFile(fileNames[0]);
    }
  }, [fileNames, selectedFile]);

  const activeFileName =
    selectedFile && fileNames.includes(selectedFile)
      ? selectedFile
      : (fileNames[0] ?? "");
  const activeCode =
    activeFileName && result?.files ? (result.files[activeFileName] ?? "") : "";
  const monacoLanguage = getMonacoLanguageForFile(activeFileName, target.lang);

  const updateTarget = <K extends keyof TargetConfig>(
    key: K,
    value: TargetConfig[K],
  ) => {
    onChangeTarget(normalizeTargetConfig({ ...target, [key]: value }));
  };

  return (
    <div className="code-output-pane">
      <div className="target-controls-bar">
        <div
          className="target-lang-selector"
          role="group"
          aria-label="Target language"
        >
          {TARGET_LANGUAGES.map((langItem) => (
            <button
              key={langItem.id}
              type="button"
              className={`target-lang-btn ${
                target.lang === langItem.id ? "active" : ""
              }`}
              onClick={() => updateTarget("lang", langItem.id)}
            >
              {langItem.label}
            </button>
          ))}
        </div>

        {target.lang === "python" && (
          <div
            className="target-options-group"
            data-testid="python-target-options"
          >
            <label className="target-option-label">
              <span>HTML Backend</span>
              <select
                value={target.lib}
                onChange={(e) =>
                  updateTarget("lib", e.target.value as PythonHtmlLib)
                }
              >
                <option value="bs4">bs4</option>
                <option value="lxml">lxml</option>
                <option value="parsel">parsel</option>
                <option value="selectolax">selectolax</option>
              </select>
            </label>

            <label className="target-option-label">
              <span>Models</span>
              <select
                value={target.models}
                onChange={(e) =>
                  updateTarget("models", e.target.value as PythonModels)
                }
              >
                <option value="typeddict">typeddict</option>
                <option value="dataclass">dataclass</option>
              </select>
            </label>

            {target.lib === "bs4" && (
              <label className="target-option-label">
                <span>BS4 Parser Engine</span>
                <select
                  value={target.bs4_parser}
                  onChange={(e) =>
                    updateTarget(
                      "bs4_parser",
                      e.target.value as Bs4ParserEngine,
                    )
                  }
                >
                  <option value="lxml">lxml</option>
                  <option value="html.parser">html.parser</option>
                  <option value="html5lib">html5lib</option>
                </select>
              </label>
            )}

            <label className="target-option-label">
              <span>HTTP I/O</span>
              <select
                value={target.http_io}
                onChange={(e) =>
                  updateTarget("http_io", e.target.value as HttpIoMode)
                }
              >
                <option value="both">both</option>
                <option value="sync">sync</option>
                <option value="async">async</option>
              </select>
            </label>

            <label className="target-option-checkbox">
              <input
                type="checkbox"
                checked={target.separate_runtime}
                onChange={(e) =>
                  updateTarget("separate_runtime", e.target.checked)
                }
              />
              <span>Separate runtime (parsmith_runtime.py)</span>
            </label>
          </div>
        )}

        {target.lang === "go" && (
          <div className="target-options-group" data-testid="go-target-options">
            <label className="target-option-label">
              <span>Package</span>
              <input
                type="text"
                className="target-option-input"
                data-testid="go-package-input"
                value={target.go_package}
                placeholder="main"
                onChange={(e) =>
                  onChangeTarget({ ...target, go_package: e.target.value })
                }
                onBlur={() => onChangeTarget(normalizeTargetConfig(target))}
              />
            </label>

            <label className="target-option-checkbox">
              <input
                type="checkbox"
                checked={target.separate_runtime}
                onChange={(e) =>
                  updateTarget("separate_runtime", e.target.checked)
                }
              />
              <span>Separate runtime (parsmith_runtime.go)</span>
            </label>
          </div>
        )}

        {target.lang === "rust" && (
          <div
            className="target-options-group"
            data-testid="rust-target-options"
          >
            <label className="target-option-checkbox">
              <input
                type="checkbox"
                checked={target.separate_runtime}
                onChange={(e) =>
                  updateTarget("separate_runtime", e.target.checked)
                }
              />
              <span>Separate runtime (parsmith_runtime.rs)</span>
            </label>
          </div>
        )}

        {target.lang === "elixir" && (
          <div
            className="target-options-group"
            data-testid="elixir-target-options"
          >
            <label className="target-option-label">
              <span>Module Prefix</span>
              <input
                type="text"
                className="target-option-input"
                data-testid="elixir-package-input"
                value={target.go_package === "main" ? "" : target.go_package}
                placeholder="(none)"
                onChange={(e) =>
                  onChangeTarget({
                    ...target,
                    go_package: e.target.value || "main",
                  })
                }
                onBlur={() => onChangeTarget(normalizeTargetConfig(target))}
              />
            </label>

            <label className="target-option-checkbox">
              <input
                type="checkbox"
                checked={target.separate_runtime}
                onChange={(e) =>
                  updateTarget("separate_runtime", e.target.checked)
                }
              />
              <span>Separate runtime (parsmith_runtime.ex)</span>
            </label>
          </div>
        )}

        {target.lang === "php" && (
          <div
            className="target-options-group"
            data-testid="php-target-options"
          >
            <label className="target-option-label">
              <span>HTML Backend</span>
              <select
                value={target.php_html_lib ?? "native"}
                onChange={(e) =>
                  updateTarget("php_html_lib", e.target.value as PhpHtmlLib)
                }
              >
                <option value="native">native (ext-dom)</option>
                <option value="dom-crawler">dom-crawler (Symfony)</option>
              </select>
            </label>

            <label className="target-option-label">
              <span>Namespace</span>
              <input
                type="text"
                className="target-option-input"
                data-testid="php-package-input"
                value={target.go_package === "main" ? "" : target.go_package}
                placeholder="(none)"
                onChange={(e) =>
                  onChangeTarget({
                    ...target,
                    go_package: e.target.value || "main",
                  })
                }
                onBlur={() => onChangeTarget(normalizeTargetConfig(target))}
              />
            </label>

            <label className="target-option-checkbox">
              <input
                type="checkbox"
                checked={target.separate_runtime}
                onChange={(e) =>
                  updateTarget("separate_runtime", e.target.checked)
                }
              />
              <span>Separate runtime (parsmith_runtime.php)</span>
            </label>
          </div>
        )}

        {target.lang === "csharp" && (
          <div
            className="target-options-group"
            data-testid="csharp-target-options"
          >
            <label className="target-option-label">
              <span>Namespace</span>
              <input
                type="text"
                className="target-option-input"
                data-testid="csharp-package-input"
                value={target.go_package === "main" ? "" : target.go_package}
                placeholder="(none)"
                onChange={(e) =>
                  onChangeTarget({
                    ...target,
                    go_package: e.target.value || "main",
                  })
                }
                onBlur={() => onChangeTarget(normalizeTargetConfig(target))}
              />
            </label>

            <label className="target-option-checkbox">
              <input
                type="checkbox"
                checked={target.separate_runtime}
                onChange={(e) =>
                  updateTarget("separate_runtime", e.target.checked)
                }
              />
              <span>Separate runtime (parsmith_runtime.cs)</span>
            </label>
          </div>
        )}

        {target.lang === "kotlin" && (
          <div
            className="target-options-group"
            data-testid="kotlin-target-options"
          >
            <label className="target-option-label">
              <span>Package</span>
              <input
                type="text"
                className="target-option-input"
                data-testid="kotlin-package-input"
                value={target.go_package === "main" ? "" : target.go_package}
                placeholder="(none)"
                onChange={(e) =>
                  onChangeTarget({
                    ...target,
                    go_package: e.target.value || "main",
                  })
                }
                onBlur={() => onChangeTarget(normalizeTargetConfig(target))}
              />
            </label>

            <label className="target-option-checkbox">
              <input
                type="checkbox"
                checked={target.separate_runtime}
                onChange={(e) =>
                  updateTarget("separate_runtime", e.target.checked)
                }
              />
              <span>Separate runtime (parsmith_runtime.kt)</span>
            </label>
          </div>
        )}
      </div>

      <div className="file-tabs-bar">
        <div className="file-tabs" role="tablist" aria-label="Generated files">
          {fileNames.map((fname) => (
            <button
              key={fname}
              type="button"
              role="tab"
              aria-selected={fname === activeFileName}
              className={`file-tab-btn ${
                fname === activeFileName ? "active" : ""
              }`}
              onClick={() => setSelectedFile(fname)}
            >
              {fname}
            </button>
          ))}
        </div>

        <button
          type="button"
          className="copy-code-btn"
          onClick={() => void copyText(activeCode)}
          disabled={!activeCode}
        >
          {copied ? "Copied!" : "Copy"}
        </button>
      </div>

      {result?.target_error && (
        <div className="target-error-banner" role="alert">
          <strong>Target Resolution Error:</strong> {result.target_error}
        </div>
      )}

      {result && !result.ok && !result.target_error && (
        <div className="codegen-blocked-banner" role="status">
          Fix schema validation errors in the KDL editor to view generated code.
        </div>
      )}

      <div className="editor-container">
        <Editor
          height="100%"
          language={monacoLanguage}
          theme={theme}
          value={activeCode}
          options={{
            readOnly: true,
            minimap: { enabled: false },
            fontSize: 13,
            lineNumbers: "on",
            scrollBeyondLastLine: false,
            automaticLayout: true,
            wordWrap: "off",
          }}
        />
      </div>
    </div>
  );
}
