import { describe, expect, it } from "vitest";
import {
  DEFAULT_JSON_SCHEMA_NAME,
  DEFAULT_SAMPLE_JSON,
  insertKdlSnippetIntoSchema,
  replaceSchemaWithKdlSnippet,
} from "../components/JsonToKdlModal";
import { diagnosticsToMarkers } from "../editor/linterMarkers";
import { PRESETS } from "../presets";
import {
  compileAndLint,
  ensureParsmithWasm,
  jsonToKdl,
} from "../runtime/wasmClient";
import {
  buildTargetSpecPayload,
  DEFAULT_TARGET_CONFIG,
  getMonacoLanguageForFile,
  normalizeTargetConfig,
  type SscDiagnostic,
  type TargetConfig,
  type TargetLang,
} from "../types";

describe("normalizeTargetConfig", () => {
  it("trims go_package and defaults empty strings to main", () => {
    expect(
      normalizeTargetConfig({
        ...DEFAULT_TARGET_CONFIG,
        go_package: "  parsers  ",
      }).go_package,
    ).toBe("parsers");

    expect(
      normalizeTargetConfig({
        ...DEFAULT_TARGET_CONFIG,
        go_package: "   ",
      }).go_package,
    ).toBe("main");
  });
});

describe("buildTargetSpecPayload", () => {
  it("includes Python-specific options when lang is python", () => {
    const cfg: TargetConfig = {
      ...DEFAULT_TARGET_CONFIG,
      lang: "python",
      lib: "bs4",
      models: "dataclass",
      bs4_parser: "html.parser",
      http_io: "sync",
      separate_runtime: true,
    };
    expect(buildTargetSpecPayload(cfg)).toEqual({
      lang: "python",
      lib: "bs4",
      models: "dataclass",
      bs4_parser: "html.parser",
      http_io: "sync",
      separate_runtime: true,
    });
  });

  it("sets bs4_parser to null when Python HTML lib is not bs4", () => {
    const cfg: TargetConfig = {
      ...DEFAULT_TARGET_CONFIG,
      lang: "python",
      lib: "selectolax",
      bs4_parser: "html5lib",
    };
    const payload = buildTargetSpecPayload(cfg);
    expect(payload.lib).toBe("selectolax");
    expect(payload.bs4_parser).toBeNull();
  });

  it("omits language-specific options for ts and js", () => {
    expect(
      buildTargetSpecPayload({
        ...DEFAULT_TARGET_CONFIG,
        lang: "ts",
        separate_runtime: true,
      }),
    ).toEqual({
      lang: "ts",
    });

    expect(
      buildTargetSpecPayload({
        ...DEFAULT_TARGET_CONFIG,
        lang: "js",
        separate_runtime: true,
      }),
    ).toEqual({
      lang: "js",
    });
  });

  it("includes package and separate_runtime for go and rust", () => {
    expect(
      buildTargetSpecPayload({
        ...DEFAULT_TARGET_CONFIG,
        lang: "go",
        go_package: "scraper",
        separate_runtime: true,
      }),
    ).toEqual({
      lang: "go",
      package: "scraper",
      separate_runtime: true,
    });

    expect(
      buildTargetSpecPayload({
        ...DEFAULT_TARGET_CONFIG,
        lang: "rust",
        separate_runtime: true,
      }),
    ).toEqual({
      lang: "rust",
      separate_runtime: true,
    });

    expect(
      buildTargetSpecPayload({
        ...DEFAULT_TARGET_CONFIG,
        lang: "elixir",
        go_package: "MyApp.Parsers",
        separate_runtime: true,
      }),
    ).toEqual({
      lang: "elixir",
      package: "MyApp.Parsers",
      separate_runtime: true,
    });

    expect(
      buildTargetSpecPayload({
        ...DEFAULT_TARGET_CONFIG,
        lang: "php",
        php_html_lib: "dom-crawler",
        go_package: "App\\Parsers",
        separate_runtime: true,
      }),
    ).toEqual({
      lang: "php",
      lib: "dom-crawler",
      package: "App\\Parsers",
      separate_runtime: true,
    });

    expect(
      buildTargetSpecPayload({
        ...DEFAULT_TARGET_CONFIG,
        lang: "csharp",
        go_package: "MyApp.Parsers",
        separate_runtime: true,
      }),
    ).toEqual({
      lang: "csharp",
      package: "MyApp.Parsers",
      separate_runtime: true,
    });

    expect(
      buildTargetSpecPayload({
        ...DEFAULT_TARGET_CONFIG,
        lang: "kotlin",
        go_package: "com.example.parsers",
        separate_runtime: true,
      }),
    ).toEqual({
      lang: "kotlin",
      package: "com.example.parsers",
      separate_runtime: true,
    });
  });
});

describe("getMonacoLanguageForFile", () => {
  it("maps emitted parser and runtime filenames to Monaco languages", () => {
    expect(getMonacoLanguageForFile("parser.py", "python")).toBe("python");
    expect(getMonacoLanguageForFile("parsmith_runtime.py", "python")).toBe(
      "python",
    );
    expect(getMonacoLanguageForFile("parser.ts", "ts")).toBe("typescript");
    expect(getMonacoLanguageForFile("parser.d.ts", "js")).toBe("typescript");
    expect(getMonacoLanguageForFile("parser.js", "js")).toBe("javascript");
    expect(getMonacoLanguageForFile("parser.go", "go")).toBe("go");
    expect(getMonacoLanguageForFile("parsmith_runtime.go", "go")).toBe("go");
    expect(getMonacoLanguageForFile("parser.rs", "rust")).toBe("rust");
    expect(getMonacoLanguageForFile("parsmith_runtime.rs", "rust")).toBe(
      "rust",
    );
    expect(getMonacoLanguageForFile("parser.ex", "elixir")).toBe("elixir");
    expect(getMonacoLanguageForFile("parser.php", "php")).toBe("php");
    expect(getMonacoLanguageForFile("parser.cs", "csharp")).toBe("csharp");
    expect(getMonacoLanguageForFile("parser.kt", "kotlin")).toBe("kotlin");
  });
});

describe("diagnosticsToMarkers", () => {
  it("converts 1-based SscDiagnostic spans and hints to Monaco markers", () => {
    const diagnostics: SscDiagnostic[] = [
      {
        code: "E100",
        severity: "error",
        message: "'to-int' does not accept DOCUMENT; expected STRING",
        hint: "insert 'text' or 'attr' before 'to-int'",
        span: {
          start: { offset: 42, line: 4, column: 9 },
          end: { offset: 48, line: 4, column: 15 },
        },
      },
      {
        code: "W011",
        severity: "warning",
        message: "Positional alias on json field is deprecated",
        hint: 'Use from="legacy_name" instead',
        span: {
          start: { offset: 0, line: 1, column: 1 },
          end: { offset: 0, line: 1, column: 1 },
        },
      },
    ];

    const markers = diagnosticsToMarkers(diagnostics);
    expect(markers).toHaveLength(2);

    expect(markers[0]).toEqual({
      severity: 8,
      startLineNumber: 4,
      startColumn: 9,
      endLineNumber: 4,
      endColumn: 15,
      code: "E100",
      message:
        "'to-int' does not accept DOCUMENT; expected STRING\nHint: insert 'text' or 'attr' before 'to-int'",
      source: "parsmith",
    });

    expect(markers[1].severity).toBe(4);
    expect(markers[1].code).toBe("W011");
    expect(markers[1].startLineNumber).toBe(1);
    expect(markers[1].startColumn).toBe(1);
    expect(markers[1].endLineNumber).toBe(1);
    expect(markers[1].endColumn).toBe(2);
    expect(markers[1].message).toContain('Hint: Use from="legacy_name"');
  });
});

describe("parsmith-wasm compileAndLint & jsonToKdl integration", () => {
  it("initializes parsmith-wasm and returns a valid version string", async () => {
    const version = await ensureParsmithWasm();
    expect(typeof version).toBe("string");
    expect(version.length).toBeGreaterThan(0);
  });

  it("compiles all 6 presets across all 9 targets without errors", async () => {
    const targets: TargetLang[] = [
      "python",
      "ts",
      "js",
      "go",
      "rust",
      "elixir",
      "php",
      "csharp",
      "kotlin",
    ];

    for (const preset of PRESETS) {
      for (const lang of targets) {
        const payload = buildTargetSpecPayload({
          ...DEFAULT_TARGET_CONFIG,
          lang,
        });
        const res = await compileAndLint(preset.kdl, payload);
        expect(
          res.ok,
          `Preset ${preset.id} failed for target ${lang}: ${res.target_error ?? JSON.stringify(res.diagnostics)}`,
        ).toBe(true);
        expect(res.target_error).toBeNull();
        expect(res.structs.length).toBeGreaterThan(0);
        expect(Object.keys(res.files).length).toBeGreaterThan(0);
      }
    }
  });

  it("supports Python dataclass models and separate_runtime across Python, Go, and Rust", async () => {
    const kdl = PRESETS[0].kdl;

    const pyRes = await compileAndLint(kdl, {
      lang: "python",
      lib: "selectolax",
      models: "dataclass",
      separate_runtime: true,
    });
    expect(pyRes.ok).toBe(true);
    expect(pyRes.files["parser.py"]).toContain("@dataclass");
    expect(pyRes.files["parsmith_runtime.py"]).toContain("class RtAssertionError");

    const jsRes = await compileAndLint(kdl, { lang: "js" });
    expect(jsRes.ok).toBe(true);
    expect(jsRes.files["parser.js"]).toContain("export function rtParseHtml");
    expect(jsRes.files["parser.d.ts"]).toContain("export declare function rtParseHtml");

    const goRes = await compileAndLint(kdl, {
      lang: "go",
      package: "scrapers",
      separate_runtime: true,
    });
    expect(goRes.ok).toBe(true);
    expect(goRes.files["parser.go"]).toContain("package scrapers");
    expect(goRes.files["parsmith_runtime.go"]).toContain("func rtCssSelect(");

    const rustRes = await compileAndLint(kdl, {
      lang: "rust",
      separate_runtime: true,
    });
    expect(rustRes.ok).toBe(true);
    expect(rustRes.files["parser.rs"]).toContain("use super::parsmith_runtime as rt;");
    expect(rustRes.files["parsmith_runtime.rs"]).toContain("pub struct RtError");
  });

  it("surfaces E100 pipeline type errors and E003 import errors with 1-based spans", async () => {
    const brokenKdl = `
struct BrokenCard {
    bad_count { css ".count"; to-int }
}
`;
    const res = await compileAndLint(brokenKdl, { lang: "python" });
    expect(res.ok).toBe(false);
    expect(res.structs).toHaveLength(1);
    expect(res.structs[0].name).toBe("BrokenCard");
    const e100 = res.diagnostics.find((d) => d.code === "E100");
    expect(e100).toBeDefined();
    expect(e100?.span.start.line).toBeGreaterThanOrEqual(1);
    expect(e100?.span.start.column).toBeGreaterThanOrEqual(1);

    const importKdl = `
import "./common.kdl" {
    (struct)SharedAuthor
}
struct Post {
    title { css "h1"; text }
}
`;
    const impRes = await compileAndLint(importKdl, { lang: "python" });
    expect(impRes.ok).toBe(false);
    expect(impRes.diagnostics.some((d) => d.code === "E003")).toBe(true);
  });

  it("converts sample JSON into KDL 2.0 json definitions via jsonToKdl", async () => {
    const sampleJson = JSON.stringify({
      id: 42,
      title: "Parsmith",
      active: true,
      tags: ["kdl", "wasm"],
    });

    const res = await jsonToKdl(sampleJson, "ReleaseItem");
    expect(res.ok).toBe(true);
    expect(res.error).toBeNull();
    expect(res.kdl).toContain("json ReleaseItem");
    expect(res.kdl).toContain("id int");
    expect(res.kdl).toContain("title str");

    const defaultRes = await jsonToKdl(sampleJson);
    expect(defaultRes.ok).toBe(true);
    expect(defaultRes.kdl).toContain("json JsonResponse");

    const badRes = await jsonToKdl("{ invalid json", "Broken");
    expect(badRes.ok).toBe(false);
    expect(badRes.kdl).toBe("");
    expect(badRes.error).toBeTruthy();
  });

  it("inserts and replaces generated KDL schemas via JsonToKdlModal helpers", async () => {
    const res = await jsonToKdl(DEFAULT_SAMPLE_JSON, DEFAULT_JSON_SCHEMA_NAME);
    expect(res.ok).toBe(true);
    expect(res.kdl).toContain(`json ${DEFAULT_JSON_SCHEMA_NAME}`);

    const existingSchema = 'struct MainPage {\n    title { css "h1"; text }\n}\n';
    const inserted = insertKdlSnippetIntoSchema(existingSchema, res.kdl);
    expect(inserted.startsWith(res.kdl.trim())).toBe(true);
    expect(inserted).toContain("\n\nstruct MainPage {");

    const compileInserted = await compileAndLint(inserted, { lang: "ts" });
    expect(compileInserted.ok).toBe(true);

    const replaced = replaceSchemaWithKdlSnippet(res.kdl);
    expect(replaced).toBe(`${res.kdl.trim()}\n`);
    expect(insertKdlSnippetIntoSchema("   ", res.kdl)).toBe(
      `${res.kdl.trim()}\n`,
    );
  });
});
