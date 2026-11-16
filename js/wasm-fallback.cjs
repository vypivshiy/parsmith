"use strict";

const fs = require("node:fs");
const path = require("node:path");

const wasm = require("./wasm/parsmith_wasm.js");

const VALID_TARGETS = {
  python: ["python", null, null],
  py: ["python", null, null],
  "py-bs4": ["python", "bs4", null],
  "python-bs4": ["python", "bs4", null],
  "py-beautifulsoup": ["python", "bs4", null],
  "python-beautifulsoup": ["python", "bs4", null],
  "py-beautifulsoup4": ["python", "bs4", null],
  "python-beautifulsoup4": ["python", "bs4", null],
  "py-lxml": ["python", "lxml", null],
  "python-lxml": ["python", "lxml", null],
  "py-parsel": ["python", "parsel", null],
  "python-parsel": ["python", "parsel", null],
  "py-selectolax": ["python", "selectolax", null],
  "python-selectolax": ["python", "selectolax", null],
  "py-slax": ["python", "selectolax", null],
  "python-slax": ["python", "selectolax", null],
  ts: ["ts", null, null],
  typescript: ["ts", null, null],
  js: ["js", null, null],
  javascript: ["js", null, null],
  go: ["go", null, null],
  golang: ["go", null, null],
  rust: ["rust", null, null],
  rs: ["rust", null, null],
  elixir: ["elixir", null, null],
  ex: ["elixir", null, null],
  php: ["php", null, null],
  "php-native": ["php", null, "native"],
  "php-ext-dom": ["php", null, "native"],
  "php-dom": ["php", null, "native"],
  "php-dom-crawler": ["php", null, "dom-crawler"],
  "php-dom_crawler": ["php", null, "dom-crawler"],
  "php-crawler": ["php", null, "dom-crawler"],
  "php-symfony": ["php", null, "dom-crawler"],
  csharp: ["csharp", null, null],
  cs: ["csharp", null, null],
  "c#": ["csharp", null, null],
  kotlin: ["kotlin", null, null],
  kt: ["kotlin", null, null],
};

const VALID_HTML_LIBS = {
  bs4: "bs4",
  beautifulsoup: "bs4",
  beautifulsoup4: "bs4",
  lxml: "lxml",
  parsel: "parsel",
  selectolax: "selectolax",
  slax: "selectolax",
};

const VALID_PHP_HTML_LIBS = {
  native: "native",
  "ext-dom": "native",
  ext_dom: "native",
  dom: "native",
  "dom-crawler": "dom-crawler",
  dom_crawler: "dom-crawler",
  domcrawler: "dom-crawler",
  crawler: "dom-crawler",
  symfony: "dom-crawler",
};

const VALID_MODELS = {
  typeddict: "typeddict",
  "typed-dict": "typeddict",
  typed_dict: "typeddict",
  dataclass: "dataclass",
  "data-class": "dataclass",
  data_class: "dataclass",
  dataclasses: "dataclass",
};

const VALID_BS4_PARSERS = {
  lxml: "lxml",
  "html.parser": "html.parser",
  html5lib: "html5lib",
};

const VALID_HTTP_IO = {
  both: "both",
  sync: "sync",
  async: "async",
};

function parseTargetShorthand(s) {
  const key = String(s).trim().toLowerCase();
  if (!Object.prototype.hasOwnProperty.call(VALID_TARGETS, key)) {
    throw new Error(
      `Invalid target '${s}'. Expected one of: python, ts, js, go, rust, elixir, php, csharp, kotlin, py-bs4, py-lxml, py-parsel, py-selectolax, py-slax, php-native, php-dom-crawler`
    );
  }
  return VALID_TARGETS[key];
}

function parseHtmlLib(s) {
  const key = String(s).trim().toLowerCase();
  if (!Object.prototype.hasOwnProperty.call(VALID_HTML_LIBS, key)) {
    throw new Error(
      `Invalid --html-lib '${s}'. Expected one of: bs4, lxml, parsel, selectolax`
    );
  }
  return VALID_HTML_LIBS[key];
}

function parsePhpHtmlLib(s) {
  const key = String(s).trim().toLowerCase();
  if (!Object.prototype.hasOwnProperty.call(VALID_PHP_HTML_LIBS, key)) {
    throw new Error(
      `Invalid PHP --html-lib '${s}'. Expected one of: native, dom-crawler`
    );
  }
  return VALID_PHP_HTML_LIBS[key];
}

function parseModels(s) {
  const key = String(s).trim().toLowerCase();
  if (!Object.prototype.hasOwnProperty.call(VALID_MODELS, key)) {
    throw new Error(
      `Invalid --models '${s}'. Expected one of: typeddict, dataclass`
    );
  }
  return VALID_MODELS[key];
}

function parseBs4Parser(s) {
  const key = String(s).trim().toLowerCase();
  if (!Object.prototype.hasOwnProperty.call(VALID_BS4_PARSERS, key)) {
    throw new Error(
      `Invalid --bs4-parser '${s}'. Expected one of: lxml, html.parser, html5lib`
    );
  }
  return VALID_BS4_PARSERS[key];
}

function parseHttpIo(s) {
  const key = String(s).trim().toLowerCase();
  if (!Object.prototype.hasOwnProperty.call(VALID_HTTP_IO, key)) {
    throw new Error(
      `Invalid --http-io '${s}'. Expected one of: both, sync, async`
    );
  }
  return VALID_HTTP_IO[key];
}

function deepClone(val) {
  return val === undefined ? undefined : JSON.parse(JSON.stringify(val));
}

class Diagnostic {
  constructor(bundle) {
    this._rawJson = JSON.stringify(bundle.raw);
    this._camel = bundle.camel;
    this._snake = bundle.snake;
    this._renderedDefault = bundle.renderedDefault;
  }
  get code() {
    return this._camel.code;
  }
  get codeName() {
    return this._camel.codeName;
  }
  get severity() {
    return this._camel.severity;
  }
  get message() {
    return this._camel.message;
  }
  get path() {
    return this._camel.path;
  }
  get line() {
    return this._camel.line;
  }
  get col() {
    return this._camel.col;
  }
  get endLine() {
    return this._camel.endLine;
  }
  get endCol() {
    return this._camel.endCol;
  }
  get hint() {
    return this._camel.hint;
  }
  get notes() {
    return [...this._camel.notes];
  }
  get label() {
    return this._camel.label;
  }
  get isError() {
    return this._camel.severity === "error";
  }
  get isWarning() {
    return this._camel.severity === "warning";
  }
  render(source) {
    if (source === undefined || source === null) {
      return this._renderedDefault;
    }
    return wasm.sdk_render_diagnostic(this._rawJson, String(source));
  }
  toObject() {
    return deepClone(this._camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._snake);
  }
  toString() {
    return this.render();
  }
}

class CheckResult {
  constructor(payload) {
    this._ok = payload.ok;
    this._diagnostics = payload.diagnostics.map((b) => new Diagnostic(b));
    this._formatText = payload.formatText;
    this._formatJson = payload.formatJson;
  }
  get diagnostics() {
    return [...this._diagnostics];
  }
  get errors() {
    return this._diagnostics.filter((d) => d.isError);
  }
  get warnings() {
    return this._diagnostics.filter((d) => d.isWarning);
  }
  get ok() {
    return this._ok;
  }
  get errorCount() {
    return this.errors.length;
  }
  get warningCount() {
    return this.warnings.length;
  }
  hasErrors() {
    return this.errorCount > 0;
  }
  hasWarnings() {
    return this.warningCount > 0;
  }
  isOk() {
    return this._ok;
  }
  formatText() {
    return this._formatText;
  }
  formatJson() {
    return this._formatJson;
  }
  toObject() {
    return {
      ok: this._ok,
      diagnostics: this._diagnostics.map((d) => d.toObject()),
    };
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return {
      ok: this._ok,
      diagnostics: this._diagnostics.map((d) => d.toDict()),
    };
  }
  toString() {
    return `CheckResult(ok=${this._ok}, errors=${this.errorCount}, warnings=${this.warningCount})`;
  }
}

function applyTargetWithDefaultRuntime(opts, parsedTarget, keepRuntimeName) {
  if (!keepRuntimeName) {
    if (parsedTarget === "python" && opts.runtime_name === "parsmith_runtime") {
      opts.runtime_name = ".parsmith_runtime";
    } else if (
      parsedTarget !== "python" &&
      opts.runtime_name === ".parsmith_runtime"
    ) {
      opts.runtime_name = "parsmith_runtime";
    }
  }
  opts.target = parsedTarget;
}

class CodegenOptions {
  constructor(targetOrOptions, options) {
    const parsed = parseCodegenArgs(targetOrOptions, options, false);
    this._inner = parsed.opts;
  }
  get target() {
    return this._inner.target;
  }
  set target(val) {
    const [t, pyLib, phpLib] = parseTargetShorthand(val);
    applyTargetWithDefaultRuntime(this._inner, t, false);
    if (pyLib) this._inner.html_lib = pyLib;
    if (phpLib) this._inner.php_html_lib = phpLib;
  }
  get htmlLib() {
    return this._inner.html_lib;
  }
  set htmlLib(val) {
    this._inner.html_lib = parseHtmlLib(val);
  }
  get phpHtmlLib() {
    return this._inner.php_html_lib;
  }
  set phpHtmlLib(val) {
    this._inner.php_html_lib = parsePhpHtmlLib(val);
  }
  get php_html_lib() {
    return this._inner.php_html_lib;
  }
  set php_html_lib(val) {
    this._inner.php_html_lib = parsePhpHtmlLib(val);
  }
  get models() {
    return this._inner.models;
  }
  set models(val) {
    this._inner.models = parseModels(val);
  }
  get bs4Parser() {
    return this._inner.bs4_parser;
  }
  set bs4Parser(val) {
    this._inner.bs4_parser = parseBs4Parser(val);
  }
  get httpIo() {
    return this._inner.http_io;
  }
  set httpIo(val) {
    this._inner.http_io = parseHttpIo(val);
  }
  get separateRuntime() {
    return this._inner.separate_runtime;
  }
  set separateRuntime(val) {
    this._inner.separate_runtime = Boolean(val);
  }
  get runtimeName() {
    return this._inner.runtime_name;
  }
  set runtimeName(val) {
    this._inner.runtime_name = String(val);
  }
  get package() {
    return this._inner.package;
  }
  set package(val) {
    this._inner.package = String(val);
  }
  get moduleStem() {
    return this._inner.module_stem;
  }
  set moduleStem(val) {
    this._inner.module_stem = val == null ? null : String(val);
  }
  get prologue() {
    return this._inner.prologue;
  }
  set prologue(val) {
    this._inner.prologue = val == null ? null : String(val);
  }
  get epilogue() {
    return this._inner.epilogue;
  }
  set epilogue(val) {
    this._inner.epilogue = val == null ? null : String(val);
  }
  toObject() {
    return {
      target: this.target,
      htmlLib: this.htmlLib,
      phpHtmlLib: this.phpHtmlLib,
      models: this.models,
      bs4Parser: this.bs4Parser,
      httpIo: this.httpIo,
      separateRuntime: this.separateRuntime,
      runtimeName: this.runtimeName,
      package: this.package,
      moduleStem: this.moduleStem,
      prologue: this.prologue,
      epilogue: this.epilogue,
    };
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._inner);
  }
  toString() {
    return `CodegenOptions(target=${JSON.stringify(this.target)}, htmlLib=${JSON.stringify(this.htmlLib)}, phpHtmlLib=${JSON.stringify(this.phpHtmlLib)}, models=${JSON.stringify(this.models)}, bs4Parser=${JSON.stringify(this.bs4Parser)}, httpIo=${JSON.stringify(this.httpIo)}, separateRuntime=${this.separateRuntime}, runtimeName=${JSON.stringify(this.runtimeName)}, package=${JSON.stringify(this.package)})`;
  }
}

class GenerateResult {
  constructor(payload) {
    this._ok = payload.ok;
    this._files = payload.files || {};
    this._primaryCode = payload.primaryCode ?? null;
    this._diagnostics = payload.diagnostics.map((b) => new Diagnostic(b));
    this._formatText = payload.formatText;
    this._formatJson = payload.formatJson;
  }
  get files() {
    return { ...this._files };
  }
  get diagnostics() {
    return [...this._diagnostics];
  }
  get errors() {
    return this._diagnostics.filter((d) => d.isError);
  }
  get warnings() {
    return this._diagnostics.filter((d) => d.isWarning);
  }
  get ok() {
    return this._ok;
  }
  get code() {
    return this._primaryCode;
  }
  primaryCode() {
    return this._primaryCode;
  }
  getFile(name) {
    return Object.prototype.hasOwnProperty.call(this._files, name)
      ? this._files[name]
      : null;
  }
  hasFile(name) {
    return Object.prototype.hasOwnProperty.call(this._files, name);
  }
  hasErrors() {
    return this.errors.length > 0;
  }
  hasWarnings() {
    return this.warnings.length > 0;
  }
  isOk() {
    return this._ok;
  }
  formatText() {
    return this._formatText;
  }
  formatJson() {
    return this._formatJson;
  }
  toObject() {
    return {
      ok: this._ok,
      files: this.files,
      diagnostics: this._diagnostics.map((d) => d.toObject()),
    };
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return {
      ok: this._ok,
      files: this.files,
      diagnostics: this._diagnostics.map((d) => d.toDict()),
    };
  }
  toString() {
    const names = Object.keys(this._files);
    return `GenerateResult(ok=${this._ok}, files=${JSON.stringify(names)}, diagnostics=${this._diagnostics.length})`;
  }
}

class TypeInfo {
  constructor(b) {
    this._b = b;
  }
  get base() {
    return this._b.camel.base;
  }
  get dslName() {
    return this._b.camel.dslName;
  }
  get isArray() {
    return this._b.camel.isArray;
  }
  get isOptional() {
    return this._b.camel.isOptional;
  }
  get isDict() {
    return this._b.camel.isDict;
  }
  get reference() {
    return this._b.camel.reference;
  }
  get keyType() {
    return this._b.keyType ? new TypeInfo(this._b.keyType) : null;
  }
  get valueType() {
    return this._b.valueType ? new TypeInfo(this._b.valueType) : null;
  }
  get omitempty() {
    return this._b.camel.omitempty;
  }
  get skip() {
    return this._b.camel.skip;
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `TypeInfo(base=${JSON.stringify(this.base)}, isArray=${this.isArray}, isOptional=${this.isOptional}, reference=${JSON.stringify(this.reference)})`;
  }
}

class FieldInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get typeInfo() {
    return new TypeInfo(this._b.typeInfo);
  }
  get hasFallback() {
    return this._b.camel.hasFallback;
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `FieldInfo(name=${JSON.stringify(this.name)}, typeInfo=${this.typeInfo.toString()}, hasFallback=${this.hasFallback})`;
  }
}

class InitFieldInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get typeInfo() {
    return new TypeInfo(this._b.typeInfo);
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `InitFieldInfo(name=${JSON.stringify(this.name)}, typeInfo=${this.typeInfo.toString()})`;
  }
}

class CheckMethodInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get returnType() {
    return new TypeInfo(this._b.returnType);
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `CheckMethodInfo(name=${JSON.stringify(this.name)}, returnType=${this.returnType.toString()})`;
  }
}

class PlaceholderInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get typeName() {
    return this._b.camel.typeName;
  }
  get isArray() {
    return this._b.camel.isArray;
  }
  get isOptional() {
    return this._b.camel.isOptional;
  }
  get style() {
    return this._b.camel.style;
  }
  get token() {
    return this._b.camel.token;
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `PlaceholderInfo(name=${JSON.stringify(this.name)}, typeName=${JSON.stringify(this.typeName)}, isArray=${this.isArray}, isOptional=${this.isOptional}, style=${JSON.stringify(this.style)})`;
  }
}

class RequestInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get method() {
    return this._b.camel.method;
  }
  get url() {
    return this._b.camel.url;
  }
  get doc() {
    return this._b.camel.doc;
  }
  get responseSchema() {
    return this._b.camel.responseSchema;
  }
  get responsePath() {
    return this._b.camel.responsePath;
  }
  get responseJoin() {
    return this._b.camel.responseJoin;
  }
  get resultAliasName() {
    return this._b.camel.resultAliasName;
  }
  get bodyKind() {
    return this._b.camel.bodyKind;
  }
  get placeholders() {
    return this._b.placeholders.map((p) => new PlaceholderInfo(p));
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `RequestInfo(name=${JSON.stringify(this.name)}, method=${JSON.stringify(this.method)}, url=${JSON.stringify(this.url)}, bodyKind=${JSON.stringify(this.bodyKind)})`;
  }
}

class RestEndpointInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get method() {
    return this._b.camel.method;
  }
  get url() {
    return this._b.camel.url;
  }
  get doc() {
    return this._b.camel.doc;
  }
  get responseSchema() {
    return this._b.camel.responseSchema;
  }
  get responsePath() {
    return this._b.camel.responsePath;
  }
  get responseIsArray() {
    return this._b.camel.responseIsArray;
  }
  get resultAliasName() {
    return this._b.camel.resultAliasName;
  }
  get bodyKind() {
    return this._b.camel.bodyKind;
  }
  get placeholders() {
    return this._b.placeholders.map((p) => new PlaceholderInfo(p));
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `RestEndpointInfo(name=${JSON.stringify(this.name)}, method=${JSON.stringify(this.method)}, url=${JSON.stringify(this.url)}, resultAliasName=${JSON.stringify(this.resultAliasName)})`;
  }
}

class ErrorVariantInfo {
  constructor(b) {
    this._b = b;
  }
  get variantName() {
    return this._b.camel.variantName;
  }
  get name() {
    return this._b.camel.name;
  }
  get status() {
    return this._b.camel.status;
  }
  get schemaName() {
    return this._b.camel.schemaName;
  }
  get suffix() {
    return this._b.camel.suffix;
  }
  get requiredKeys() {
    return [...this._b.camel.requiredKeys];
  }
  get conditions() {
    return deepClone(this._b.camel.conditions);
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `ErrorVariantInfo(variantName=${JSON.stringify(this.variantName)}, status=${this.status}, schemaName=${JSON.stringify(this.schemaName)}, suffix=${JSON.stringify(this.suffix)})`;
  }
}

class StructInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get kind() {
    return this._b.camel.kind;
  }
  get doc() {
    return this._b.camel.doc;
  }
  get keepOrder() {
    return this._b.camel.keepOrder;
  }
  get fields() {
    return this._b.fields.map((f) => new FieldInfo(f));
  }
  get initFields() {
    return this._b.initFields.map((i) => new InitFieldInfo(i));
  }
  get checks() {
    return this._b.checks.map((c) => new CheckMethodInfo(c));
  }
  get hasPreValidate() {
    return this._b.camel.hasPreValidate;
  }
  get keyType() {
    return this._b.keyType ? new TypeInfo(this._b.keyType) : null;
  }
  get valueType() {
    return this._b.valueType ? new TypeInfo(this._b.valueType) : null;
  }
  get requests() {
    return this._b.requests.map((r) => new RequestInfo(r));
  }
  get restEndpoints() {
    return this._b.restEndpoints.map((e) => new RestEndpointInfo(e));
  }
  get errors() {
    return this._b.errors.map((e) => new ErrorVariantInfo(e));
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `StructInfo(name=${JSON.stringify(this.name)}, kind=${JSON.stringify(this.kind)}, fields=${this._b.fields.length}, requests=${this._b.requests.length}, errors=${this._b.errors.length})`;
  }
}

class FnParamInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get typeInfo() {
    return new TypeInfo(this._b.typeInfo);
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `FnParamInfo(name=${JSON.stringify(this.name)}, typeInfo=${this.typeInfo.toString()})`;
  }
}

class FunctionInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get isRaw() {
    return this._b.camel.isRaw;
  }
  get doc() {
    return this._b.camel.doc;
  }
  get params() {
    return this._b.params.map((p) => new FnParamInfo(p));
  }
  get returnType() {
    return new TypeInfo(this._b.returnType);
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `FunctionInfo(name=${JSON.stringify(this.name)}, isRaw=${this.isRaw}, returnType=${this.returnType.toString()})`;
  }
}

class JsonFieldInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get typeInfo() {
    return new TypeInfo(this._b.typeInfo);
  }
  get alias() {
    return this._b.camel.alias;
  }
  get doc() {
    return this._b.camel.doc;
  }
  get isOptional() {
    return this._b.camel.isOptional;
  }
  get omitempty() {
    return this._b.camel.omitempty;
  }
  get skip() {
    return this._b.camel.skip;
  }
  get isDict() {
    return this._b.camel.isDict;
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `JsonFieldInfo(name=${JSON.stringify(this.name)}, typeInfo=${this.typeInfo.toString()}, alias=${JSON.stringify(this.alias)}, isOptional=${this.isOptional}, omitempty=${this.omitempty}, skip=${this.skip})`;
  }
}

class JsonDefInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get kind() {
    return this._b.camel.kind;
  }
  get doc() {
    return this._b.camel.doc;
  }
  get isArray() {
    return this._b.camel.isArray;
  }
  get isDict() {
    return this._b.camel.isDict;
  }
  get isHoisted() {
    return this._b.camel.isHoisted;
  }
  get path() {
    return this._b.camel.path;
  }
  get fields() {
    return this._b.fields.map((f) => new JsonFieldInfo(f));
  }
  get keyType() {
    return this._b.keyType ? new TypeInfo(this._b.keyType) : null;
  }
  get valueType() {
    return this._b.valueType ? new TypeInfo(this._b.valueType) : null;
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `JsonDefInfo(name=${JSON.stringify(this.name)}, kind=${JSON.stringify(this.kind)}, isHoisted=${this.isHoisted}, fields=${this._b.fields.length})`;
  }
}

class ExtensionOpInfo {
  constructor(b) {
    this._b = b;
  }
  get name() {
    return this._b.camel.name;
  }
  get qualifiedName() {
    return this._b.camel.qualifiedName;
  }
  get inputType() {
    return this._b.camel.inputType;
  }
  get outputType() {
    return this._b.camel.outputType;
  }
  get isGeneric() {
    return this._b.camel.isGeneric;
  }
  get targets() {
    return [...this._b.camel.targets];
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `ExtensionOpInfo(qualifiedName=${JSON.stringify(this.qualifiedName)}, input=${JSON.stringify(this.inputType)}, output=${JSON.stringify(this.outputType)}, targets=${JSON.stringify(this.targets)})`;
  }
}

class ExtensionInfo {
  constructor(b) {
    this._b = b;
  }
  get namespace() {
    return this._b.camel.namespace;
  }
  get name() {
    return this._b.camel.name;
  }
  get operations() {
    return this._b.operations.map((o) => new ExtensionOpInfo(o));
  }
  toObject() {
    return deepClone(this._b.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._b.snake);
  }
  toString() {
    return `ExtensionInfo(namespace=${JSON.stringify(this.namespace)}, operations=${this._b.operations.length})`;
  }
}

class ModuleInfo {
  constructor(root) {
    this._r = root;
    this._diagnostics = root.diagnostics.map((d) => new Diagnostic(d));
  }
  get doc() {
    return this._r.camel.doc;
  }
  get sourceFile() {
    return this._r.camel.sourceFile;
  }
  get structKinds() {
    return deepClone(this._r.camel.structKinds);
  }
  get structs() {
    return this._r.structs.map((s) => new StructInfo(s));
  }
  get functions() {
    return this._r.functions.map((f) => new FunctionInfo(f));
  }
  get jsonDefs() {
    return this._r.jsonDefs.map((j) => new JsonDefInfo(j));
  }
  get extensions() {
    return this._r.extensions.map((e) => new ExtensionInfo(e));
  }
  get diagnostics() {
    return [...this._diagnostics];
  }
  get errors() {
    return this._diagnostics.filter((d) => d.isError);
  }
  get warnings() {
    return this._diagnostics.filter((d) => d.isWarning);
  }
  get ok() {
    return this._r.camel.ok;
  }
  hasErrors() {
    return this.errors.length > 0;
  }
  hasWarnings() {
    return this.warnings.length > 0;
  }
  isOk() {
    return this._r.camel.ok;
  }
  toObject() {
    return deepClone(this._r.camel);
  }
  toJSON() {
    return this.toObject();
  }
  toDict() {
    return deepClone(this._r.snake);
  }
  toJson() {
    return this._r.toJson;
  }
  formatText() {
    return this._r.formatText;
  }
  formatJson() {
    return this._r.formatJson;
  }
  toString() {
    return `ModuleInfo(ok=${this.ok}, structs=${this._r.structs.length}, functions=${this._r.functions.length}, jsonDefs=${this._r.jsonDefs.length}, extensions=${this._r.extensions.length})`;
  }
}

// ============================================================================
// Argument & VFS helpers matching crates/parsmith-js
// ============================================================================

function extractRawSourceOrPath(val) {
  if (typeof val === "string") {
    return [val, false];
  }
  if (
    typeof val === "object" &&
    val !== null &&
    Object.prototype.hasOwnProperty.call(val, "__parsmithPath__")
  ) {
    return [String(val.__parsmithPath__), true];
  }
  throw new TypeError("expected string or file: URL for sourceOrPath");
}

function normalizeVirtualPath(p) {
  const unified = String(p).replace(/\\/g, "/");
  const isAbs = unified.startsWith("/");
  const stack = [];
  for (const seg of unified.split("/")) {
    if (!seg || seg === ".") continue;
    if (seg === "..") {
      if (stack.length > 0 && stack[stack.length - 1] !== "..") {
        stack.pop();
        continue;
      }
      if (!isAbs) stack.push("..");
    } else {
      stack.push(seg);
    }
  }
  return isAbs ? `/${stack.join("/")}` : stack.join("/");
}

function extractSourcesMap(val) {
  if (val === undefined || val === null) return null;
  if (typeof val !== "object" || Array.isArray(val)) {
    throw new TypeError(
      "sources must be a Record<string, string> or Map<string, string>"
    );
  }
  const out = {};
  if (
    typeof val.entries === "function" &&
    typeof val.get === "function"
  ) {
    for (const [k, v] of val.entries()) {
      let kStr;
      try {
        [kStr] = extractRawSourceOrPath(k);
      } catch {
        throw new TypeError("sources keys must be string or file: URL");
      }
      if (typeof v !== "string") {
        throw new TypeError("sources values must be strings");
      }
      out[kStr] = v;
    }
    return out;
  }
  for (const k of Object.keys(val)) {
    const v = val[k];
    if (typeof v !== "string") {
      throw new TypeError("sources values must be strings");
    }
    out[k] = v;
  }
  return out;
}

function extractTargets(val) {
  if (val === undefined || val === null) return null;
  if (typeof val === "string") return [val];
  if (Array.isArray(val)) {
    for (const item of val) {
      if (typeof item !== "string") {
        throw new TypeError("targets elements must be strings");
      }
    }
    return val.length === 0 ? null : val;
  }
  throw new TypeError("targets must be a string or an array of strings");
}

function extractOptString(val, fieldName) {
  if (val === undefined || val === null) return null;
  if (typeof val === "string") return val;
  throw new TypeError(`'${fieldName}' must be a string`);
}

function extractReqString(val, fieldName) {
  if (typeof val === "string") return val;
  throw new TypeError(`'${fieldName}' must be a string`);
}

function extractReqBool(val, fieldName) {
  if (typeof val === "boolean") return val;
  throw new TypeError(`'${fieldName}' must be a boolean`);
}

function isSingleLineKdlFilename(s) {
  return (
    !s.includes("\n") &&
    !s.includes("\r") &&
    !s.includes("{") &&
    s.trim().endsWith(".kdl")
  );
}

function resolveInputMode(rawStr, isPathlike, vfsMap) {
  if (vfsMap) {
    const norm = normalizeVirtualPath(rawStr);
    const hasKey = Object.keys(vfsMap).some(
      (k) => normalizeVirtualPath(k) === norm
    );
    if (hasKey || isPathlike || isSingleLineKdlFilename(rawStr)) {
      return ["path", rawStr];
    }
    return ["source", rawStr];
  }
  if (
    isPathlike ||
    (!rawStr.includes("\n") &&
      !rawStr.includes("\r") &&
      !rawStr.includes("{") &&
      (fs.existsSync(rawStr) || rawStr.trim().endsWith(".kdl")))
  ) {
    return ["path", rawStr];
  }
  return ["source", rawStr];
}

function collectDiskSourcesForEntry(entryPath) {
  const sources = {};
  const visited = new Set();

  function walk(filePath, virtualKey) {
    const absPath = path.resolve(filePath);
    if (visited.has(absPath)) return;
    visited.add(absPath);

    if (!fs.existsSync(absPath) || !fs.statSync(absPath).isFile()) {
      return;
    }
    const content = fs.readFileSync(absPath, "utf-8");
    sources[virtualKey] = content;

    const importRegex = /\bimport\s+"([^"]+)"/g;
    let match;
    while ((match = importRegex.exec(content)) !== null) {
      const imp = match[1];
      const childDisk = path.resolve(path.dirname(absPath), imp);
      const childVirt = normalizeVirtualPath(
        path.posix.join(
          path.posix.dirname(virtualKey.replace(/\\/g, "/")),
          imp.replace(/\\/g, "/")
        )
      );
      walk(childDisk, childVirt);
    }
  }

  walk(entryPath, entryPath);
  return Object.keys(sources).length > 0 ? sources : null;
}

function applyOptionsFromObject(obj, state, allowCallContextKeys) {
  if (obj instanceof CodegenOptions) {
    state.opts = { ...obj._inner };
    state.optionsHadHtmlLib = true;
    state.optionsHadPhpHtmlLib = true;
    state.optionsHadRuntimeName = true;
    return;
  }
  for (const key of Object.keys(obj)) {
    const v = obj[key];
    if (v === undefined) continue;
    switch (key) {
      case "target": {
        const s = extractReqString(v, "target");
        const [parsedTarget, shorthandPyLib, shorthandPhpLib] =
          parseTargetShorthand(s);
        applyTargetWithDefaultRuntime(
          state.opts,
          parsedTarget,
          state.optionsHadRuntimeName
        );
        if (shorthandPyLib && !state.optionsHadHtmlLib) {
          state.opts.html_lib = shorthandPyLib;
        }
        if (shorthandPhpLib && !state.optionsHadPhpHtmlLib) {
          state.opts.php_html_lib = shorthandPhpLib;
        }
        break;
      }
      case "htmlLib":
      case "html_lib":
      case "html-lib":
      case "lib": {
        const s = extractReqString(v, key);
        state.pendingHtmlLib = s;
        state.optionsHadHtmlLib = true;
        break;
      }
      case "phpHtmlLib":
      case "php_html_lib":
      case "php-html-lib": {
        const s = extractReqString(v, key);
        state.opts.php_html_lib = parsePhpHtmlLib(s);
        state.optionsHadPhpHtmlLib = true;
        break;
      }
      case "models": {
        const s = extractReqString(v, "models");
        state.opts.models = parseModels(s);
        break;
      }
      case "bs4Parser":
      case "bs4_parser":
      case "bs4-parser": {
        const s = extractReqString(v, key);
        state.opts.bs4_parser = parseBs4Parser(s);
        break;
      }
      case "httpIo":
      case "http_io":
      case "http-io": {
        const s = extractReqString(v, key);
        state.opts.http_io = parseHttpIo(s);
        break;
      }
      case "separateRuntime":
      case "separate_runtime":
      case "separate-runtime": {
        state.opts.separate_runtime = extractReqBool(v, key);
        break;
      }
      case "runtimeName":
      case "runtime_name":
      case "runtime-name": {
        state.opts.runtime_name = extractReqString(v, key);
        state.optionsHadRuntimeName = true;
        break;
      }
      case "package":
      case "namespace": {
        state.opts.package = extractReqString(v, key);
        break;
      }
      case "moduleStem":
      case "module_stem":
      case "module-stem": {
        state.opts.module_stem = extractOptString(v, key);
        break;
      }
      case "prologue": {
        state.opts.prologue = extractOptString(v, "prologue");
        break;
      }
      case "epilogue": {
        state.opts.epilogue = extractOptString(v, "epilogue");
        break;
      }
      default: {
        if (allowCallContextKeys && key === "options") {
          if (v !== null && v !== undefined) {
            if (typeof v !== "object" || Array.isArray(v)) {
              throw new TypeError(
                "options must be a CodegenOptions instance or an object"
              );
            }
            applyOptionsFromObject(v, state, false);
          }
        } else if (allowCallContextKeys && key === "sources") {
          state.vfsMap = extractSourcesMap(v);
        } else if (allowCallContextKeys && key === "path") {
          state.logicalPath = extractOptString(v, "path");
        } else {
          throw new Error(`Unknown codegen option '${key}'`);
        }
      }
    }
  }
}

function parseCodegenArgs(targetOrOptions, options, allowCallContextKeys) {
  const state = {
    opts: {
      target: "ts",
      html_lib: "bs4",
      php_html_lib: "native",
      models: "typeddict",
      bs4_parser: "lxml",
      http_io: "both",
      separate_runtime: false,
      runtime_name: "parsmith_runtime",
      package: "main",
      module_stem: null,
      prologue: null,
      epilogue: null,
    },
    optionsHadHtmlLib: false,
    optionsHadPhpHtmlLib: false,
    optionsHadRuntimeName: false,
    pendingHtmlLib: null,
    vfsMap: null,
    logicalPath: null,
  };

  let explicitTarget = null;
  let firstWasObject = false;

  if (targetOrOptions !== undefined && targetOrOptions !== null) {
    if (typeof targetOrOptions === "string") {
      explicitTarget = targetOrOptions;
    } else if (
      typeof targetOrOptions === "object" &&
      !Array.isArray(targetOrOptions)
    ) {
      firstWasObject = true;
      applyOptionsFromObject(targetOrOptions, state, allowCallContextKeys);
    } else {
      throw new TypeError(
        "targetOrOptions must be a target string, CodegenOptions, or options object"
      );
    }
  }

  if (options !== undefined && options !== null) {
    if (typeof options === "object" && !Array.isArray(options)) {
      if (firstWasObject && !allowCallContextKeys) {
        throw new TypeError(
          "cannot pass two options objects to CodegenOptions"
        );
      }
      applyOptionsFromObject(options, state, allowCallContextKeys);
    } else {
      throw new TypeError(
        "options must be a CodegenOptions instance or an object"
      );
    }
  }

  if (explicitTarget !== null) {
    const [parsedTarget, shorthandPyLib, shorthandPhpLib] =
      parseTargetShorthand(explicitTarget);
    applyTargetWithDefaultRuntime(
      state.opts,
      parsedTarget,
      state.optionsHadRuntimeName
    );
    if (shorthandPyLib && !state.optionsHadHtmlLib) {
      state.opts.html_lib = shorthandPyLib;
    }
    if (shorthandPhpLib && !state.optionsHadPhpHtmlLib) {
      state.opts.php_html_lib = shorthandPhpLib;
    }
  }

  if (state.pendingHtmlLib !== null) {
    if (state.opts.target === "php" && !state.optionsHadPhpHtmlLib) {
      try {
        state.opts.php_html_lib = parsePhpHtmlLib(state.pendingHtmlLib);
      } catch {
        state.opts.html_lib = parseHtmlLib(state.pendingHtmlLib);
      }
    } else {
      state.opts.html_lib = parseHtmlLib(state.pendingHtmlLib);
    }
  }

  return state;
}

function version() {
  return wasm.version();
}

function expandDiskInputPaths(rawPaths, skipUnderscoreInDirs) {
  const expanded = [];
  for (const p of rawPaths) {
    if (fs.existsSync(p) && fs.statSync(p).isDirectory()) {
      const entries = fs
        .readdirSync(p)
        .filter((name) => {
          if (!name.toLowerCase().endsWith(".kdl")) return false;
          if (skipUnderscoreInDirs && name.startsWith("_")) return false;
          const full = path.join(p, name);
          return fs.existsSync(full) && fs.statSync(full).isFile();
        })
        .sort();
      for (const name of entries) {
        expanded.push(path.join(p, name));
      }
    } else {
      expanded.push(p);
    }
  }
  return expanded;
}

function resolveBatchOrSingleInput(sourceOrPath, vfsMap, skipUnderscoreInDirs) {
  if (Array.isArray(sourceOrPath)) {
    const rawPaths = sourceOrPath.map((item) => {
      try {
        return extractRawSourceOrPath(item)[0];
      } catch {
        throw new TypeError(
          "array elements for sourceOrPath must be string or file: URL"
        );
      }
    });
    const expanded = vfsMap
      ? rawPaths
      : expandDiskInputPaths(rawPaths, skipUnderscoreInDirs);
    if (!vfsMap) {
      const merged = {};
      for (const ep of expanded) {
        const sub = collectDiskSourcesForEntry(ep);
        if (sub) Object.assign(merged, sub);
      }
      vfsMap = Object.keys(merged).length > 0 ? merged : null;
    }
    return {
      isPath: true,
      strVal: JSON.stringify(expanded),
      vfsMap,
    };
  }

  const [rawStr, isPathlike] = extractRawSourceOrPath(sourceOrPath);
  if (
    !vfsMap &&
    fs.existsSync(rawStr) &&
    fs.statSync(rawStr).isDirectory()
  ) {
    const expanded = expandDiskInputPaths([rawStr], skipUnderscoreInDirs);
    const merged = {};
    for (const ep of expanded) {
      const sub = collectDiskSourcesForEntry(ep);
      if (sub) Object.assign(merged, sub);
    }
    return {
      isPath: true,
      strVal: JSON.stringify(expanded),
      vfsMap: Object.keys(merged).length > 0 ? merged : null,
    };
  }

  const [mode, strVal] = resolveInputMode(rawStr, isPathlike, vfsMap);
  const isPath = mode === "path";
  if (isPath && !vfsMap) {
    vfsMap = collectDiskSourcesForEntry(strVal);
  }
  return { isPath, strVal, vfsMap };
}

function check(sourceOrPath, options) {
  let vfsMap = null;
  let targetsVec = null;
  let logicalPath = null;

  if (options !== undefined && options !== null) {
    if (typeof options !== "object" || Array.isArray(options)) {
      throw new TypeError("check options must be an object");
    }
    for (const key of Object.keys(options)) {
      const v = options[key];
      if (v === undefined) continue;
      if (key === "sources") {
        vfsMap = extractSourcesMap(v);
      } else if (key === "targets") {
        targetsVec = extractTargets(v);
      } else if (key === "path") {
        logicalPath = extractOptString(v, "path");
      } else {
        throw new Error(`Unknown check option '${key}'`);
      }
    }
  }

  const resolved = resolveBatchOrSingleInput(sourceOrPath, vfsMap, false);
  const rawJson = wasm.sdk_check(
    resolved.strVal,
    resolved.isPath,
    logicalPath ?? undefined,
    resolved.vfsMap ? JSON.stringify(resolved.vfsMap) : undefined,
    targetsVec ? JSON.stringify(targetsVec) : undefined
  );
  return new CheckResult(JSON.parse(rawJson));
}

function generate(sourceOrPath, targetOrOptions, options) {
  const parsed = parseCodegenArgs(targetOrOptions, options, true);
  const resolved = resolveBatchOrSingleInput(
    sourceOrPath,
    parsed.vfsMap,
    true
  );
  const rawJson = wasm.sdk_generate(
    resolved.strVal,
    resolved.isPath,
    parsed.logicalPath ?? undefined,
    resolved.vfsMap ? JSON.stringify(resolved.vfsMap) : undefined,
    JSON.stringify(parsed.opts)
  );
  return new GenerateResult(JSON.parse(rawJson));
}

function inspectSchema(sourceOrPath, options) {
  const [rawStr, isPathlike] = extractRawSourceOrPath(sourceOrPath);
  let vfsMap = null;
  let logicalPath = null;

  if (options !== undefined && options !== null) {
    if (typeof options !== "object" || Array.isArray(options)) {
      throw new TypeError("inspectSchema options must be an object");
    }
    for (const key of Object.keys(options)) {
      const v = options[key];
      if (v === undefined) continue;
      if (key === "sources") {
        vfsMap = extractSourcesMap(v);
      } else if (key === "path") {
        logicalPath = extractOptString(v, "path");
      } else {
        throw new Error(`Unknown inspectSchema option '${key}'`);
      }
    }
  }

  const [mode, strVal] = resolveInputMode(rawStr, isPathlike, vfsMap);
  const isPath = mode === "path";
  if (isPath && !vfsMap) {
    vfsMap = collectDiskSourcesForEntry(strVal);
  }
  const rawJson = wasm.sdk_inspect(
    strVal,
    isPath,
    logicalPath ?? undefined,
    vfsMap ? JSON.stringify(vfsMap) : undefined
  );
  return new ModuleInfo(JSON.parse(rawJson));
}

function jsonToKdl(jsonInput, nameOrOptions) {
  let name = "JsonResponse";
  if (nameOrOptions !== undefined && nameOrOptions !== null) {
    if (typeof nameOrOptions === "string") {
      name = nameOrOptions;
    } else if (
      typeof nameOrOptions === "object" &&
      !Array.isArray(nameOrOptions)
    ) {
      for (const key of Object.keys(nameOrOptions)) {
        const v = nameOrOptions[key];
        if (v === undefined) continue;
        if (key === "name") {
          const s = extractOptString(v, "name");
          if (s !== null) name = s;
        } else {
          throw new Error(`Unknown jsonToKdl option '${key}'`);
        }
      }
    } else {
      throw new TypeError(
        "second argument to jsonToKdl must be a string or an options object"
      );
    }
  }
  const res = JSON.parse(wasm.json_to_kdl(String(jsonInput), name));
  if (!res.ok) {
    throw new Error(res.error || "Invalid JSON input");
  }
  return res.kdl;
}

module.exports = {
  version,
  check,
  generate,
  inspectSchema,
  inspect_schema: inspectSchema,
  jsonToKdl,
  json_to_kdl: jsonToKdl,
  Diagnostic,
  CheckResult,
  CodegenOptions,
  GenerateResult,
  TypeInfo,
  FieldInfo,
  InitFieldInfo,
  CheckMethodInfo,
  PlaceholderInfo,
  RequestInfo,
  RestEndpointInfo,
  ErrorVariantInfo,
  StructInfo,
  FnParamInfo,
  FunctionInfo,
  JsonFieldInfo,
  JsonDefInfo,
  ExtensionOpInfo,
  ExtensionInfo,
  ModuleInfo,
};
