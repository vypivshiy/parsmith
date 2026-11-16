"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { fileURLToPath } = require("node:url");

function getCargoCdylibName() {
  switch (process.platform) {
    case "win32":
      return "parsmith_js.dll";
    case "darwin":
      return "libparsmith_js.dylib";
    default:
      return "libparsmith_js.so";
  }
}

function loadNativeAddon() {
  const jsDir = __dirname;
  const repoRoot = path.resolve(jsDir, "..");

  if (process.env.PARSMITH_FORCE_WASM === "1") {
    return require("./wasm-fallback.cjs");
  }

  // 1. Check for prebuilt .node artifacts inside js/ and Cargo target/{release,debug} shared libraries
  const nodeCandidates = [
    path.join(jsDir, `parsmith.${process.platform}-${process.arch}.node`),
    path.join(jsDir, "parsmith.node"),
    path.join(jsDir, "index.node"),
  ];

  const libName = getCargoCdylibName();
  const targetDir = path.join(repoRoot, "target");
  const cargoCandidates = [
    path.join(targetDir, "release", libName),
    path.join(targetDir, "debug", libName),
  ];

  const existingCandidates = [...nodeCandidates, ...cargoCandidates]
    .filter((p) => fs.existsSync(p))
    .sort((a, b) => fs.statSync(b).mtimeMs - fs.statSync(a).mtimeMs);

  if (existingCandidates.length > 0) {
    const mod = { exports: {} };
    process.dlopen(mod, existingCandidates[0]);
    return mod.exports;
  }

  // 3. Universal WebAssembly fallback (works on all OS/architectures)
  const wasmEntry = path.join(jsDir, "wasm", "parsmith_wasm.js");
  if (fs.existsSync(wasmEntry)) {
    return require("./wasm-fallback.cjs");
  }

  throw new Error(
    `Failed to load parsmith-js native addon or WASM fallback. Run 'cargo build -p parsmith-js' first. Searched:\n` +
      [...nodeCandidates, ...cargoCandidates, wasmEntry]
        .map((p) => `  - ${p}`)
        .join("\n")
  );
}

const native = loadNativeAddon();

function isWhatwgUrl(val) {
  return (
    val instanceof URL ||
    (typeof val === "object" &&
      val !== null &&
      typeof val.protocol === "string" &&
      typeof val.href === "string")
  );
}

function normalizePathInput(input) {
  if (isWhatwgUrl(input)) {
    if (input.protocol !== "file:") {
      throw new TypeError("URL must use the 'file:' protocol");
    }
    return { __parsmithPath__: fileURLToPath(input) };
  }
  if (Array.isArray(input)) {
    return input.map((item) => normalizePathInput(item));
  }
  return input;
}

function normalizeOptionsSources(opts) {
  if (
    typeof opts !== "object" ||
    opts === null ||
    opts instanceof native.CodegenOptions
  ) {
    return opts;
  }
  if (opts.sources instanceof Map) {
    const mapped = new Map();
    for (const [k, v] of opts.sources.entries()) {
      const normK = isWhatwgUrl(k)
        ? normalizePathInput(k).__parsmithPath__
        : k;
      mapped.set(normK, v);
    }
    return { ...opts, sources: mapped };
  }
  return opts;
}

function check(sourceOrPath, options) {
  return native.check(
    normalizePathInput(sourceOrPath),
    normalizeOptionsSources(options)
  );
}

function generate(sourceOrPath, targetOrOptions, options) {
  return native.generate(
    normalizePathInput(sourceOrPath),
    normalizeOptionsSources(targetOrOptions),
    normalizeOptionsSources(options)
  );
}

function inspectSchema(sourceOrPath, options) {
  return native.inspectSchema(
    normalizePathInput(sourceOrPath),
    normalizeOptionsSources(options)
  );
}

const __version__ = native.version();

module.exports = {
  ...native,
  __version__,
  check,
  generate,
  inspectSchema,
  inspect_schema: inspectSchema,
};
