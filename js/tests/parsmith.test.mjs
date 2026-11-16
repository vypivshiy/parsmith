import assert from "node:assert/strict";
import fs from "node:fs";
import { createRequire } from "node:module";
import os from "node:os";
import path from "node:path";
import { describe, it } from "node:test";
import { pathToFileURL } from "node:url";

import parsmithDefault, * as parsmith from "../index.mjs";
import {
  CheckMethodInfo,
  CheckResult,
  CodegenOptions,
  Diagnostic,
  ErrorVariantInfo,
  ExtensionInfo,
  ExtensionOpInfo,
  FieldInfo,
  FnParamInfo,
  FunctionInfo,
  GenerateResult,
  InitFieldInfo,
  JsonDefInfo,
  JsonFieldInfo,
  ModuleInfo,
  PlaceholderInfo,
  RequestInfo,
  RestEndpointInfo,
  StructInfo,
  TypeInfo,
  __version__,
  check,
  generate,
  inspectSchema,
  inspect_schema,
  jsonToKdl,
  json_to_kdl,
  version,
} from "../index.mjs";

const require = createRequire(import.meta.url);
const cjsParsmith = require("../index.cjs");

function withTempDir(fn) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "parsmith-js-test-"));
  try {
    return fn(dir);
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
}

describe("parsmith-js N-API SDK", () => {
  it("exports version and all 20 SDK classes in both ESM and CommonJS", () => {
    assert.equal(typeof __version__, "string");
    assert.ok(__version__.length > 0);
    assert.equal(version(), __version__);
    assert.equal(parsmithDefault.version(), __version__);
    assert.equal(cjsParsmith.version(), __version__);
    assert.equal(cjsParsmith.__version__, __version__);

    const classes = [
      [Diagnostic, "Diagnostic"],
      [CheckResult, "CheckResult"],
      [CodegenOptions, "CodegenOptions"],
      [GenerateResult, "GenerateResult"],
      [TypeInfo, "TypeInfo"],
      [FieldInfo, "FieldInfo"],
      [InitFieldInfo, "InitFieldInfo"],
      [CheckMethodInfo, "CheckMethodInfo"],
      [PlaceholderInfo, "PlaceholderInfo"],
      [RequestInfo, "RequestInfo"],
      [RestEndpointInfo, "RestEndpointInfo"],
      [ErrorVariantInfo, "ErrorVariantInfo"],
      [StructInfo, "StructInfo"],
      [FnParamInfo, "FnParamInfo"],
      [FunctionInfo, "FunctionInfo"],
      [JsonFieldInfo, "JsonFieldInfo"],
      [JsonDefInfo, "JsonDefInfo"],
      [ExtensionOpInfo, "ExtensionOpInfo"],
      [ExtensionInfo, "ExtensionInfo"],
      [ModuleInfo, "ModuleInfo"],
    ];

    for (const [Cls, expectedName] of classes) {
      assert.equal(typeof Cls, "function", `Missing class ${expectedName}`);
      assert.equal(Cls.name, expectedName);
      assert.equal(cjsParsmith[expectedName], Cls);
    }
  });

  it("validates inline valid and invalid KDL with check()", () => {
    const validSchema = `
    struct BookCard {
        title { css "h3 > a"; attr "title"; trim }
        price { css ".price"; text; re #"(\\d+\\.\\d+)"#; to-float; fallback 0.0 }
    }
    `;
    const res = check(validSchema);
    assert.equal(res instanceof CheckResult, true);
    assert.equal(res.ok, true);
    assert.equal(res.hasErrors(), false);
    assert.equal(res.hasWarnings(), false);
    assert.equal(res.isOk(), true);
    assert.equal(res.errorCount, 0);
    assert.equal(res.warningCount, 0);
    assert.deepEqual(res.diagnostics, []);
    assert.deepEqual(res.errors, []);
    assert.deepEqual(res.warnings, []);
    assert.equal(res.formatText(), "");
    assert.deepEqual(JSON.parse(res.formatJson()), { diagnostics: [] });
    assert.deepEqual(res.toObject(), { ok: true, diagnostics: [] });
    assert.deepEqual(res.toDict(), { ok: true, diagnostics: [] });
    assert.deepEqual(JSON.parse(JSON.stringify(res)), {
      ok: true,
      diagnostics: [],
    });

    const invalidSchema = `
    json Item {
        name str "legacy_name"
    }
    struct Broken {
        bad_field { trim }
    }
    `;
    const badRes = check(invalidSchema, { path: "broken.kdl" });
    assert.equal(badRes.ok, false);
    assert.equal(badRes.hasErrors(), true);
    assert.equal(badRes.hasWarnings(), true);
    assert.equal(badRes.errorCount, 1);
    assert.equal(badRes.warningCount, 1);
    assert.equal(badRes.diagnostics.length, 2);

    const errDiag = badRes.errors[0];
    assert.equal(errDiag instanceof Diagnostic, true);
    assert.equal(errDiag.code, "E100");
    assert.equal(errDiag.codeName, "TYPE_MISMATCH");
    assert.equal(errDiag.severity, "error");
    assert.equal(errDiag.isError, true);
    assert.equal(errDiag.isWarning, false);
    assert.equal(errDiag.path, "broken.kdl");
    assert.equal(errDiag.line, 6);
    assert.ok(errDiag.col > 0);
    assert.ok(errDiag.endLine >= errDiag.line);
    assert.ok(errDiag.endCol >= errDiag.col);
    assert.notEqual(errDiag.hint, null);
    assert.equal(Array.isArray(errDiag.notes), true);

    const rendered = errDiag.render();
    assert.ok(rendered.includes("error[E100]"));
    assert.ok(rendered.includes("broken.kdl:6:"));
    assert.ok(rendered.includes("bad_field { trim }"));
    assert.equal(errDiag.toString(), rendered);

    const customRendered = errDiag.render(invalidSchema);
    assert.ok(customRendered.includes("bad_field { trim }"));

    // Dual serialization: toObject() -> camelCase, toDict() -> snake_case
    const diagObj = errDiag.toObject();
    assert.equal(diagObj.code, "E100");
    assert.equal(diagObj.codeName, "TYPE_MISMATCH");
    assert.equal(diagObj.endLine, errDiag.endLine);
    assert.equal("code_name" in diagObj, false);
    assert.deepEqual(JSON.parse(JSON.stringify(errDiag)), diagObj);

    const diagDict = errDiag.toDict();
    assert.equal(diagDict.code, "E100");
    assert.equal(diagDict.code_name, "TYPE_MISMATCH");
    assert.equal(diagDict.end_line, errDiag.endLine);
    assert.equal("codeName" in diagDict, false);

    const warnDiag = badRes.warnings[0];
    assert.equal(warnDiag.code, "W011");
    assert.equal(warnDiag.severity, "warning");
    assert.equal(warnDiag.isWarning, true);
    assert.equal(warnDiag.isError, false);

    const textOut = badRes.formatText();
    assert.ok(textOut.includes("error[E100]"));
    assert.ok(textOut.includes("warning[W011]"));

    const jsonOut = JSON.parse(badRes.formatJson());
    assert.equal(jsonOut.diagnostics.length, 2);
  });

  it("supports filesystem paths, file: URLs, and target filtering in check()", () => {
    withTempDir((tmpDir) => {
      const schemaText = `
      struct MyParser {
          self { css "span.fn"; text }
      }
      `;
      const schemaFile = path.join(tmpDir, "reserved_rust.kdl");
      fs.writeFileSync(schemaFile, schemaText, "utf-8");

      // 1. Default 4-ecosystem validation catches Rust keyword collision (E403)
      const resAll = check(schemaFile);
      assert.equal(resAll.hasErrors(), true);
      assert.equal(
        resAll.errors.some((d) => d.code === "E403"),
        true
      );

      // 2. Also test passing a WHATWG file: URL
      const fileUrl = pathToFileURL(schemaFile);
      const resUrl = check(fileUrl);
      assert.equal(resUrl.hasErrors(), true);
      assert.equal(
        resUrl.errors.some((d) => d.code === "E403"),
        true
      );

      // 3. Target-filtered check for Python only succeeds with zero errors
      const resPy = check(schemaFile, { targets: ["python"] });
      assert.equal(resPy.ok, true);
      assert.equal(resPy.hasErrors(), false);

      // Single string target is also accepted
      const resPySingle = check(schemaFile, { targets: "python" });
      assert.equal(resPySingle.ok, true);

      // 4. Non-existent .kdl path reports E003
      const resMissing = check(path.join(tmpDir, "does_not_exist.kdl"));
      assert.equal(resMissing.hasErrors(), true);
      assert.equal(resMissing.errors[0].code, "E003");

      // 5. Non-file: URL and invalid types throw TypeError
      assert.throws(
        () => check(new URL("https://example.com/schema.kdl")),
        TypeError
      );
      assert.throws(() => check(123), TypeError);
      assert.throws(
        () => check(schemaFile, { sources: { "a.kdl": 123 } }),
        TypeError
      );
    });
  });

  it("compiles VFS diamond imports with both Record and Map sources and executes generated JS", async () => {
    const sourcesObj = {
      "common/ext.kdl": `
      extension TextExt {
          clean {
              sig str str
              py { emit "{{out}} = {{in}}.strip().upper()" }
              js { emit "const {{out}} = {{in}}.trim().toUpperCase();" }
          }
      }
      define PRICE_CSS=".price-val"
      `,
      "models/author.kdl": `
      import "../common/ext.kdl" {
          (extension)TextExt
      }
      struct AuthorBadge {
          name { css ".author"; text; !TextExt.clean }
      }
      `,
      "models/pricing.kdl": `
      import "../common/ext.kdl" {
          (define)PRICE_CSS
      }
      json DiscountInfo {
          percent int
          code str? @omitempty
      }
      struct PriceTag {
          amount { css PRICE_CSS; text; to-float; fallback 0.0 }
      }
      `,
      "pages/product.kdl": `
      import "../common/ext.kdl" {
          (extension)TextExt
      }
      import "../models/author.kdl" {
          (struct)AuthorBadge
      }
      import "../models/pricing.kdl" {
          (struct)PriceTag
          (json)DiscountInfo
      }
      struct ProductPage {
          title { css "h1.title"; text; !TextExt.clean }
          author { css ".author-box"; nested AuthorBadge }
          pricing { css ".price-box"; nested PriceTag }
          discount { css "script#discount"; text; jsonify DiscountInfo }
      }
      `,
    };

    // 1. Check by VFS entry key using Record<string, string>
    const checkRes = check("pages/product.kdl", { sources: sourcesObj });
    assert.equal(checkRes.ok, true, checkRes.formatText());

    // 2. Check by VFS entry key using Map<string, string>
    const sourcesMap = new Map(Object.entries(sourcesObj));
    const checkMapRes = check("pages/product.kdl", { sources: sourcesMap });
    assert.equal(checkMapRes.ok, true, checkMapRes.formatText());

    // 3. Check inline KDL source importing from VFS
    const inlineEntry = `
    import "./pages/product.kdl" {
        (struct)ProductPage
    }
    struct StoreFront {
        featured { css ".featured"; nested ProductPage }
    }
    `;
    const inlineCheck = check(inlineEntry, { sources: sourcesMap });
    assert.equal(inlineCheck.ok, true, inlineCheck.formatText());

    // 4. Default target in generate() is "ts"
    const genTsDefault = generate("pages/product.kdl", { sources: sourcesObj });
    assert.equal(genTsDefault.ok, true, genTsDefault.formatText());
    assert.equal(genTsDefault.hasFile("product.ts"), true);
    assert.equal(genTsDefault.hasFile("nonexistent.ts"), false);
    assert.equal(genTsDefault.getFile("nonexistent.ts"), null);
    const tsCode = genTsDefault.getFile("product.ts");
    assert.equal(typeof tsCode, "string");
    assert.equal(genTsDefault.primaryCode(), tsCode);
    assert.equal(genTsDefault.code, tsCode);
    assert.ok(tsCode.includes("export class ProductPage"));

    // 5. Generate Python code from VFS diamond graph
    const genPy = generate("pages/product.kdl", "python", {
      sources: sourcesObj,
    });
    assert.equal(genPy.ok, true, genPy.formatText());
    assert.equal(genPy.hasFile("product.py"), true);
    assert.ok(genPy.getFile("product.py").includes("class ProductPage:"));
  });

  it("executes generated JS REST client end-to-end and supports Python separateRuntime", async () => {
    const schema = `
    json UserProfile {
        id int
        username str from="user.name"
        role str? @omitempty
    }

    json ApiErrorPayload {
        message str
        code int
    }

    (rest)struct UserApiClient {
        @request name="get_user" response="UserProfile" """
        GET /v1/users/{{user_id:int}} HTTP/1.1
        Host: api.example.com
        """
        @error:not-found 404 ApiErrorPayload
    }
    `;

    // 1. Test CodegenOptions class construction & getters/setters
    const opts = new CodegenOptions("python", {
      models: "dataclass",
      bs4Parser: "html.parser",
      httpIo: "sync",
      separateRuntime: true,
      runtimeName: "custom_rt",
      moduleStem: "user_api",
      prologue: "# PROLOGUE_MARKER",
      epilogue: "# EPILOGUE_MARKER",
    });
    assert.equal(opts.target, "python");
    assert.equal(opts.models, "dataclass");
    assert.equal(opts.bs4Parser, "html.parser");
    assert.equal(opts.httpIo, "sync");
    assert.equal(opts.separateRuntime, true);
    assert.equal(opts.runtimeName, "custom_rt");
    assert.equal(opts.moduleStem, "user_api");
    assert.equal(opts.prologue, "# PROLOGUE_MARKER");
    assert.equal(opts.epilogue, "# EPILOGUE_MARKER");

    // Dual serialization on CodegenOptions
    assert.equal(opts.toObject().separateRuntime, true);
    assert.equal(opts.toDict().separate_runtime, true);

    const resPy = generate(schema, opts);
    assert.equal(resPy.ok, true, resPy.formatText());
    assert.deepEqual(Object.keys(resPy.files).sort(), [
      "custom_rt.py",
      "user_api.py",
    ]);
    assert.ok(resPy.getFile("user_api.py").includes("# PROLOGUE_MARKER"));
    assert.ok(resPy.getFile("user_api.py").includes("# EPILOGUE_MARKER"));
    assert.ok(resPy.getFile("user_api.py").includes("from custom_rt import"));

    // 2. Generate JS REST client and execute it directly in Node.js
    const resJs = generate(schema, "js", { moduleStem: "user_api_js" });
    assert.equal(resJs.ok, true, resJs.formatText());
    assert.deepEqual(Object.keys(resJs.files).sort(), [
      "user_api_js.d.ts",
      "user_api_js.js",
    ]);

    const jsCode = resJs.getFile("user_api_js.js");
    const modUrl =
      "data:text/javascript;base64," +
      Buffer.from(jsCode, "utf-8").toString("base64");
    const apiMod = await import(modUrl);

    class MockTransport {
      constructor(status, body) {
        this.status = status;
        this.body = body;
        this.lastUrl = "";
      }
      async send(req) {
        this.lastUrl = req.url;
        return {
          status: this.status,
          headers: { "content-type": "application/json" },
          text: this.body,
        };
      }
    }

    // 200 OK
    const tOk = new MockTransport(
      200,
      '{"id": 7, "user": {"name": "bob"}}'
    );
    const outOk = await apiMod.UserApiClient.getUser(tOk, { userId: 7 });
    assert.equal(outOk.ok, true);
    assert.equal(outOk.value.id, 7);
    assert.equal(outOk.value.username, "bob");
    assert.equal("role" in outOk.value, false);
    assert.equal(tOk.lastUrl, "https://api.example.com/v1/users/7");

    // 404 NotFound
    const t404 = new MockTransport(
      404,
      '{"message": "User missing", "code": 40401}'
    );
    const out404 = await apiMod.UserApiClient.getUser(t404, { userId: 999 });
    assert.equal(out404.ok, false);
    assert.equal(out404.status, 404);
    assert.equal(out404.value.message, "User missing");
    assert.equal(out404.value.code, 40401);

    // 200 Malformed JSON -> DecodeErr
    const tBad = new MockTransport(200, '{"id": "not-an-int"}');
    const outBad = await apiMod.UserApiClient.getUser(tBad, { userId: 7 });
    assert.equal(outBad.ok, false);
    assert.equal(outBad.kind, "decode");
    assert.equal(outBad.status, 200);
  });

  it("supports Python htmlLib options, target shorthands, Go, and Rust codegen", () => {
    const schema = `
    struct Article {
        title { css "h1"; text; trim }
        tags { css-all "ul.tags > li"; text }
    }
    `;

    // Default CodegenOptions has target="ts" and htmlLib="bs4"
    const optsDefault = new CodegenOptions();
    assert.equal(optsDefault.target, "ts");
    assert.equal(optsDefault.htmlLib, "bs4");
    assert.equal(optsDefault.toObject().htmlLib, "bs4");
    assert.equal(optsDefault.toDict().html_lib, "bs4");

    const optsLxml = new CodegenOptions("python", { htmlLib: "lxml" });
    assert.equal(optsLxml.target, "python");
    assert.equal(optsLxml.htmlLib, "lxml");
    optsLxml.htmlLib = "slax";
    assert.equal(optsLxml.htmlLib, "selectolax");

    const optsShorthand = new CodegenOptions("py-parsel");
    assert.equal(optsShorthand.target, "python");
    assert.equal(optsShorthand.htmlLib, "parsel");

    for (const [shorthand, expectedImport] of [
      ["py-bs4", "from bs4 import BeautifulSoup, Tag"],
      ["python-bs4", "from bs4 import BeautifulSoup, Tag"],
      ["py-lxml", "from lxml import html"],
      ["python-lxml", "from lxml import html"],
      ["py-parsel", "from parsel import Selector, SelectorList"],
      ["python-parsel", "from parsel import Selector, SelectorList"],
      [
        "py-selectolax",
        "from selectolax.lexbor import LexborHTMLParser, LexborNode",
      ],
      [
        "py-slax",
        "from selectolax.lexbor import LexborHTMLParser, LexborNode",
      ],
    ]) {
      const res = generate(schema, shorthand);
      assert.equal(res.ok, true);
      assert.ok(res.getFile("schema.py").includes(expectedImport));
    }

    // Explicit htmlLib overrides target shorthand
    const resOverride = generate(schema, "py-bs4", { htmlLib: "parsel" });
    assert.equal(resOverride.ok, true);
    assert.ok(
      resOverride
        .getFile("schema.py")
        .includes("from parsel import Selector, SelectorList")
    );

    // Go & Rust generation
    const resGo = generate(schema, "go", {
      package: "custompkg",
      moduleStem: "article_go",
    });
    assert.equal(resGo.ok, true, resGo.formatText());
    assert.ok(resGo.getFile("article_go.go").includes("package custompkg"));

    const resGoSep = generate(schema, "golang", {
      separateRuntime: true,
      runtimeName: "my_rt",
      package: "sep_pkg",
    });
    assert.equal(resGoSep.ok, true, resGoSep.formatText());
    assert.deepEqual(Object.keys(resGoSep.files).sort(), [
      "my_rt.go",
      "schema.go",
    ]);
    assert.ok(resGoSep.getFile("my_rt.go").includes("func rtCssSelect("));

    const resRust = generate(schema, "rust", { moduleStem: "article_rs" });
    assert.equal(resRust.ok, true, resRust.formatText());
    assert.ok(resRust.getFile("article_rs.rs").includes("pub mod rt {"));

    const resRsSep = generate(schema, "rs", {
      separate_runtime: true,
      "runtime-name": "my_rt",
    });
    assert.equal(resRsSep.ok, true, resRsSep.formatText());
    assert.deepEqual(Object.keys(resRsSep.files).sort(), [
      "my_rt.rs",
      "schema.rs",
    ]);
    assert.ok(resRsSep.getFile("my_rt.rs").includes("pub struct RtError"));

    // Invalid Go package -> E002
    const badPkg = generate(schema, "go", { package: "123invalid" });
    assert.equal(badPkg.ok, false);
    assert.equal(
      badPkg.errors.some((d) => d.code === "E002"),
      true
    );

    // Invalid target or option -> throws Error
    assert.throws(() => generate(schema, "cobol"), /Invalid target/);
    assert.throws(
      () => generate(schema, "python", { htmlLib: "cheerio" }),
      /Invalid --html-lib/
    );
    assert.throws(
      () => generate(schema, "ts", { unknownOption: 123 }),
      /Unknown codegen option 'unknownOption'/
    );

    // Elixir, PHP (native & dom-crawler), C#, and Kotlin targets
    const optsPhp = new CodegenOptions("php");
    assert.equal(optsPhp.target, "php");
    assert.equal(optsPhp.phpHtmlLib, "native");
    assert.equal(optsPhp.php_html_lib, "native");
    assert.equal(optsPhp.toObject().phpHtmlLib, "native");
    assert.equal(optsPhp.toDict().php_html_lib, "native");
    optsPhp.phpHtmlLib = "dom-crawler";
    assert.equal(optsPhp.phpHtmlLib, "dom-crawler");

    const optsPhpShorthand = new CodegenOptions("php-dom-crawler");
    assert.equal(optsPhpShorthand.target, "php");
    assert.equal(optsPhpShorthand.phpHtmlLib, "dom-crawler");
    optsPhpShorthand.target = "php-native";
    assert.equal(optsPhpShorthand.phpHtmlLib, "native");

    const resEx = generate(schema, "ex", {
      package: "MyApp.Parsers",
      separateRuntime: true,
      runtimeName: "rt_ex",
      moduleStem: "article_ex",
    });
    assert.equal(resEx.ok, true, resEx.formatText());
    assert.deepEqual(Object.keys(resEx.files).sort(), [
      "article_ex.ex",
      "rt_ex.ex",
    ]);
    assert.ok(
      resEx.getFile("article_ex.ex").includes("defmodule MyApp.Parsers.Article do")
    );
    assert.ok(
      resEx.getFile("rt_ex.ex").includes("defmodule MyApp.Parsers.RtEx do")
    );

    const resPhpNative = generate(schema, "php", {
      namespace: "App\\Parsers",
    });
    assert.equal(resPhpNative.ok, true, resPhpNative.formatText());
    assert.ok(
      resPhpNative.getFile("schema.php").includes("namespace App\\Parsers;")
    );
    assert.ok(resPhpNative.getFile("schema.php").includes("\\DOMDocument"));

    const resPhpCrawler = generate(schema, "php", {
      phpHtmlLib: "dom-crawler",
      separateRuntime: true,
      runtimeName: "rt_php",
    });
    assert.equal(resPhpCrawler.ok, true, resPhpCrawler.formatText());
    assert.deepEqual(Object.keys(resPhpCrawler.files).sort(), [
      "rt_php.php",
      "schema.php",
    ]);
    assert.ok(
      resPhpCrawler
        .getFile("schema.php")
        .includes("Symfony\\Component\\DomCrawler\\Crawler")
    );

    const resPhpShorthand = generate(schema, "php-dom-crawler");
    assert.equal(resPhpShorthand.ok, true);
    assert.ok(
      resPhpShorthand
        .getFile("schema.php")
        .includes("Symfony\\Component\\DomCrawler\\Crawler")
    );

    const resPhpHtmlLibAlias = generate(schema, "php", {
      htmlLib: "dom-crawler",
    });
    assert.equal(resPhpHtmlLibAlias.ok, true);
    assert.ok(
      resPhpHtmlLibAlias
        .getFile("schema.php")
        .includes("Symfony\\Component\\DomCrawler\\Crawler")
    );

    const resCs = generate(schema, "cs", {
      package: "MyApp.Parsers",
      separateRuntime: true,
      runtimeName: "RtModule",
      moduleStem: "ArticleCs",
    });
    assert.equal(resCs.ok, true, resCs.formatText());
    assert.deepEqual(Object.keys(resCs.files).sort(), [
      "ArticleCs.cs",
      "RtModule.cs",
    ]);
    assert.ok(
      resCs.getFile("ArticleCs.cs").includes("namespace MyApp.Parsers;")
    );
    assert.ok(
      resCs.getFile("ArticleCs.cs").includes("public sealed class Article")
    );

    const resKt = generate(schema, "kt", {
      package: "com.example.parsers",
      separateRuntime: true,
      runtimeName: "rt_kt",
      moduleStem: "ArticleKt",
    });
    assert.equal(resKt.ok, true, resKt.formatText());
    assert.deepEqual(Object.keys(resKt.files).sort(), [
      "ArticleKt.kt",
      "rt_kt.kt",
    ]);
    assert.ok(
      resKt.getFile("ArticleKt.kt").includes("package com.example.parsers")
    );
    assert.ok(resKt.getFile("ArticleKt.kt").includes("class Article("));
  });

  it("introspects schemas into ModuleInfo and all 15 child classes with dual serialization", () => {
    const vfs = {
      "shared.kdl": `
      extension StrExt {
          slugify {
              sig str str
              py { emit "{{out}} = {{in}}.lower()" }
              js { emit "const {{out}} = {{in}}.toLowerCase();" }
          }
          truthy {
              sig T bool
              py { emit "{{out}} = bool({{in}})" }
              js { emit "const {{out}} = Boolean({{in}});" }
          }
      }
      json Author {
          id int
          display_name str from="profile.display_name"
      }
      `,
      "catalog.kdl": `
      @doc "Catalog schema"
      import "./shared.kdl" {
          (extension)StrExt
          (json)Author
      }
      (dict)json Translations {
          @key str
          @value {
              stream_url str from="url"
              author Author?
              internal_token str @skip
              backup_url str? @omitempty
          }
      }
      json ApiError {
          code int
          detail str
      }
      (list)struct BookCard {
          @split-doc { css-all "article.book" }
          title { css "h3 > a"; attr "title"; trim }
          slug { css "h3 > a"; text; !StrExt.slugify }
          price { css ".price"; text; to-float; fallback 0.0 }
      }
      struct CatalogPage {
          @doc "Main catalog page"
          @init {
              raw_state { css "script#state"; text }
          }
          @pre-validate {
              assert "missing main" { css "main" }
          }
          @check has_books {
              css "article.book"
              !StrExt.truthy
          }
          @request name="fetch-page" """
          curl 'https://books.example.com/page/{{page:int}}?tag={{tags:str[]|csv}}&q={{query:str?}}'
          """
          heading { css "h1"; text; trim }
          books { nested BookCard }
          translations { @raw_state; jsonify Translations path="data.translations" }
      }
      (rest)struct CatalogApi {
          @request name="get-translations" response="Translations" response-path="data" """
          POST /v1/translations/{{anime_id:int}} HTTP/1.1
          Host: api.example.com
          Content-Type: application/json

          {"include_inactive": {{include_inactive:bool}}}
          """
          @error:not-found 404 ApiError detail="Not found"
          @error 500 ApiError "code"
      }
      fn extract_heading {
          @doc "Extracts heading"
          css "h1"
          text
          trim
      }
      `,
    };

    const info = inspectSchema("catalog.kdl", { sources: vfs });
    assert.equal(info instanceof ModuleInfo, true);
    assert.equal(info.ok, true);
    assert.equal(info.isOk(), true);
    assert.equal(info.hasErrors(), false);
    assert.equal(info.doc, "Catalog schema");
    assert.equal(info.sourceFile, "catalog.kdl");
    assert.deepEqual(info.structKinds, {
      BookCard: "list",
      CatalogPage: "item",
      CatalogApi: "rest",
    });

    //Also verify snake_case alias inspect_schema
    const infoAlias = inspect_schema("catalog.kdl", { sources: vfs });
    assert.equal(infoAlias.sourceFile, "catalog.kdl");

    // 1. StructInfo, FieldInfo, TypeInfo
    assert.equal(info.structs.length, 3);
    const bookCard = info.structs[0];
    assert.equal(bookCard instanceof StructInfo, true);
    assert.equal(bookCard.name, "BookCard");
    assert.equal(bookCard.kind, "list");
    assert.equal(bookCard.fields.length, 3);

    const priceField = bookCard.fields[2];
    assert.equal(priceField instanceof FieldInfo, true);
    assert.equal(priceField.name, "price");
    assert.equal(priceField.hasFallback, true);
    assert.equal(priceField.typeInfo instanceof TypeInfo, true);
    assert.equal(priceField.typeInfo.base, "float");
    assert.equal(priceField.typeInfo.dslName, "FLOAT");
    assert.equal(priceField.typeInfo.isArray, false);

    // 2. InitFieldInfo & CheckMethodInfo
    const catalogPage = info.structs[1];
    assert.equal(catalogPage.name, "CatalogPage");
    assert.equal(catalogPage.kind, "item");
    assert.equal(catalogPage.doc, "Main catalog page");
    assert.equal(catalogPage.hasPreValidate, true);
    assert.equal(catalogPage.initFields.length, 1);

    const initF = catalogPage.initFields[0];
    assert.equal(initF instanceof InitFieldInfo, true);
    assert.equal(initF.name, "raw_state");
    assert.equal(initF.typeInfo.base, "string");

    assert.equal(catalogPage.checks.length, 1);
    const chk = catalogPage.checks[0];
    assert.equal(chk instanceof CheckMethodInfo, true);
    assert.equal(chk.name, "has_books");
    assert.equal(chk.returnType.base, "bool");

    const booksField = catalogPage.fields[1];
    assert.equal(booksField.name, "books");
    assert.equal(booksField.typeInfo.base, "nested");
    assert.equal(booksField.typeInfo.reference, "BookCard");
    assert.equal(booksField.typeInfo.isArray, true);

    // 3. RequestInfo & PlaceholderInfo
    assert.equal(catalogPage.requests.length, 1);
    const req = catalogPage.requests[0];
    assert.equal(req instanceof RequestInfo, true);
    assert.equal(req.name, "fetch-page");
    assert.equal(req.method, "GET");
    assert.equal(req.bodyKind, "empty");
    assert.equal(req.placeholders.length, 3);

    const phTags = req.placeholders[1];
    assert.equal(phTags instanceof PlaceholderInfo, true);
    assert.equal(phTags.name, "tags");
    assert.equal(phTags.typeName, "str");
    assert.equal(phTags.isArray, true);
    assert.equal(phTags.isOptional, false);
    assert.equal(phTags.style, "csv");
    assert.equal(phTags.token, "{{tags[]|csv}}");

    // 4. RestEndpointInfo & ErrorVariantInfo
    const catalogApi = info.structs[2];
    assert.equal(catalogApi.name, "CatalogApi");
    assert.equal(catalogApi.kind, "rest");
    assert.equal(catalogApi.requests.length, 1);
    assert.equal(catalogApi.restEndpoints.length, 1);

    const ep = catalogApi.restEndpoints[0];
    assert.equal(ep instanceof RestEndpointInfo, true);
    assert.equal(ep.name, "get-translations");
    assert.equal(ep.method, "POST");
    assert.equal(ep.responseSchema, "Translations");
    assert.equal(ep.responsePath, "data");
    assert.equal(ep.resultAliasName, "GetTranslationsResult");
    assert.equal(ep.bodyKind, "json");
    assert.equal(ep.placeholders.length, 2);

    assert.equal(catalogApi.errors.length, 2);
    const err404 = catalogApi.errors[0];
    assert.equal(err404 instanceof ErrorVariantInfo, true);
    assert.equal(err404.variantName, "CatalogApiErr404NotFound");
    assert.equal(err404.name, "CatalogApiErr404NotFound");
    assert.equal(err404.status, 404);
    assert.equal(err404.schemaName, "ApiError");
    assert.equal(err404.suffix, "not-found");
    assert.deepEqual(err404.conditions, { detail: "Not found" });

    const err500 = catalogApi.errors[1];
    assert.equal(err500.variantName, "CatalogApiErr500Code");
    assert.equal(err500.status, 500);
    assert.deepEqual(err500.requiredKeys, ["code"]);

    // 5. FunctionInfo & FnParamInfo
    assert.equal(info.functions.length, 1);
    const fnInfo = info.functions[0];
    assert.equal(fnInfo instanceof FunctionInfo, true);
    assert.equal(fnInfo.name, "extract_heading");
    assert.equal(fnInfo.isRaw, false);
    assert.equal(fnInfo.doc, "Extracts heading");
    assert.equal(fnInfo.returnType.base, "string");
    const param0 = fnInfo.params[0];
    assert.equal(param0 instanceof FnParamInfo, true);
    assert.equal(param0.name, "document");
    assert.equal(param0.typeInfo.base, "document");

    // 6. JsonDefInfo & JsonFieldInfo
    const jsonNames = info.jsonDefs.map((j) => j.name);
    assert.deepEqual(jsonNames, [
      "Author",
      "TranslationsValue",
      "Translations",
      "ApiError",
    ]);
    const hoistedDef = info.jsonDefs[1];
    assert.equal(hoistedDef instanceof JsonDefInfo, true);
    assert.equal(hoistedDef.name, "TranslationsValue");
    assert.equal(hoistedDef.isHoisted, true);
    const streamF = hoistedDef.fields[0];
    assert.equal(streamF instanceof JsonFieldInfo, true);
    assert.equal(streamF.name, "stream_url");
    assert.equal(streamF.alias, "url");
    const tokenF = hoistedDef.fields[2];
    assert.equal(tokenF.name, "internal_token");
    assert.equal(tokenF.skip, true);

    // 7. ExtensionInfo & ExtensionOpInfo
    assert.equal(info.extensions.length, 1);
    const extInfo = info.extensions[0];
    assert.equal(extInfo instanceof ExtensionInfo, true);
    assert.equal(extInfo.namespace, "StrExt");
    assert.equal(extInfo.name, "StrExt");
    const opInfo = extInfo.operations[0];
    assert.equal(opInfo instanceof ExtensionOpInfo, true);
    assert.equal(opInfo.qualifiedName, "StrExt.slugify");
    assert.equal(opInfo.inputType, "STRING");
    assert.equal(opInfo.outputType, "STRING");
    assert.equal(opInfo.isGeneric, false);
    assert.ok(opInfo.targets.includes("py"));
    assert.ok(opInfo.targets.includes("ts"));

    // 8. Dual Serialization Contract: toObject()/toJSON() -> camelCase, toDict()/toJson() -> snake_case
    const asObj = info.toObject();
    assert.equal(asObj.ok, true);
    assert.equal(asObj.sourceFile, "catalog.kdl");
    assert.equal(asObj.structs.length, 3);
    assert.equal(asObj.jsonDefs.length, 4);
    assert.equal("source_file" in asObj, false);
    assert.deepEqual(JSON.parse(JSON.stringify(info)), asObj);

    const asDict = info.toDict();
    assert.equal(asDict.ok, true);
    assert.equal(asDict.source_file, "catalog.kdl");
    assert.equal(asDict.structs.length, 3);
    assert.equal(asDict.json_defs.length, 4);
    assert.equal("sourceFile" in asDict, false);

    const asJson = JSON.parse(info.toJson());
    assert.equal(asJson.source_file, "catalog.kdl");

    // 9. Broken schema inspection
    const badInfo = inspectSchema("struct Broken { f { trim } }", {
      path: "bad.kdl",
    });
    assert.equal(badInfo.ok, false);
    assert.equal(badInfo.hasErrors(), true);
    assert.equal(badInfo.errors.length, 1);
    assert.equal(badInfo.errors[0].code, "E100");
    assert.ok(badInfo.formatText().includes("error[E100]"));
  });

  it("bootstraps schemas with jsonToKdl() and executes generated JS projector", async () => {
    const samplePayload = JSON.stringify({
      "@id": 101,
      "user-name": "charlie",
      tags: ["rust", "ts"],
      metrics: { score: 98.5, verified: true },
    });

    const kdlSchema = jsonToKdl(samplePayload, "UserResponse");
    assert.ok(kdlSchema.includes("json UserResponse"));
    assert.ok(kdlSchema.includes('id int from="@id"'));
    assert.ok(kdlSchema.includes('user_name str from="user-name"'));

    // Also test options object and snake_case alias json_to_kdl
    const kdlAlias = json_to_kdl(samplePayload, { name: "UserResponse" });
    assert.equal(kdlAlias, kdlSchema);

    const fullSchema = `${kdlSchema}
    (raw)struct PayloadExtractor {
        payload { jsonify UserResponse }
    }
    `;
    const chk = check(fullSchema);
    assert.equal(chk.ok, true, chk.formatText());

    const genJs = generate(fullSchema, "js");
    assert.equal(genJs.ok, true, genJs.formatText());

    const modUrl =
      "data:text/javascript;base64," +
      Buffer.from(genJs.getFile("schema.js"), "utf-8").toString("base64");
    const mod = await import(modUrl);
    const parsed = new mod.PayloadExtractor(samplePayload).parse();
    assert.deepEqual(parsed, {
      payload: {
        id: 101,
        user_name: "charlie",
        tags: ["rust", "ts"],
        metrics: { score: 98.5, verified: true },
      },
    });

    assert.throws(() => jsonToKdl("{broken json"));
  });

  it("supports directory and multi-path batch check() and generate() with two-pass separateRuntime", () => {
    withTempDir((tmpDir) => {
      const commonPath = path.join(tmpDir, "_common.kdl");
      fs.writeFileSync(
        commonPath,
        `
        define HOST="https://kodik.example"
        define SEARCH_REQ="curl '{{HOST}}/search?q={{query:str}}'"
        `,
        "utf-8"
      );

      const definesOnlyPath = path.join(tmpDir, "defines_only.kdl");
      fs.writeFileSync(definesOnlyPath, 'define UNUSED_ONLY="1"\n', "utf-8");

      const kodikPath = path.join(tmpDir, "kodik.kdl");
      fs.writeFileSync(
        kodikPath,
        `
        import "./_common.kdl" {
            (define)HOST
            (define)SEARCH_REQ
        }
        struct KodikPage {
            @request SEARCH_REQ
            title { css "h1.title"; text; trim }
        }
        `,
        "utf-8"
      );

      const yummyPath = path.join(tmpDir, "yummy_anime.kdl");
      fs.writeFileSync(
        yummyPath,
        `
        json AnimeInfo {
            id int
            title str
        }
        (rest)struct YummyClient {
            @request name="get_anime" response="AnimeInfo" """
            GET /v1/anime/{{anime_id:int}} HTTP/1.1
            Host: api.yummy.example
            """
        }
        `,
        "utf-8"
      );

      const optsPy = new CodegenOptions("python");
      assert.equal(optsPy.runtimeName, ".parsmith_runtime");
      optsPy.target = "go";
      assert.equal(optsPy.runtimeName, "parsmith_runtime");

      const chkDir = check(tmpDir);
      assert.equal(chkDir.ok, true, chkDir.formatText());

      const chkArr = check([kodikPath, pathToFileURL(yummyPath)]);
      assert.equal(chkArr.ok, true, chkArr.formatText());

      const resDir = generate(tmpDir, "python", { separateRuntime: true });
      assert.equal(resDir.ok, true, resDir.formatText());
      assert.deepEqual(Object.keys(resDir.files).sort(), [
        "kodik.py",
        "parsmith_runtime.py",
        "yummy_anime.py",
      ]);
      assert.equal(resDir.hasFile("_common.py"), false);
      assert.equal(resDir.hasFile("defines_only.py"), false);
      assert.ok(
        resDir.getFile("kodik.py").includes("from .parsmith_runtime import ")
      );
      assert.ok(
        resDir
          .getFile("yummy_anime.py")
          .includes("from .parsmith_runtime import ")
      );
      const rtDir = resDir.getFile("parsmith_runtime.py");
      assert.ok(rtDir.includes("async def rt_fetch_async("));
      assert.ok(rtDir.includes("async def rt_rest_call_async("));

      const resArr = generate(
        [pathToFileURL(yummyPath), kodikPath],
        "python",
        { separateRuntime: true }
      );
      assert.equal(resArr.ok, true, resArr.formatText());
      assert.deepEqual(Object.keys(resArr.files).sort(), [
        "kodik.py",
        "parsmith_runtime.py",
        "yummy_anime.py",
      ]);
      const rtArr = resArr.getFile("parsmith_runtime.py");
      assert.ok(rtArr.includes("async def rt_fetch_async("));
      assert.ok(rtArr.includes("async def rt_rest_call_async("));
    });
  });
});
