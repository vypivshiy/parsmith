import { createRequire } from "node:module";
import readline from "node:readline";

function loadJsdom() {
  const candidates = [];
  if (process.env.PARSMITH_JSDOM_PKG) {
    candidates.push(process.env.PARSMITH_JSDOM_PKG);
  }
  candidates.push(new URL("../../playground/package.json", import.meta.url));
  candidates.push(import.meta.url);
  let lastErr;
  for (const cand of candidates) {
    try {
      const req = createRequire(cand);
      return req("jsdom");
    } catch (err) {
      lastErr = err;
    }
  }
  throw lastErr;
}

const { JSDOM } = loadJsdom();

function toCamelCase(name) {
  if (!name || typeof name !== "string") return name;
  return name.replace(/[-_]+([a-zA-Z0-9])/g, (_, ch) => ch.toUpperCase());
}

function toPascalCase(name) {
  if (!name || typeof name !== "string") return name;
  const camel = toCamelCase(name);
  return camel.charAt(0).toUpperCase() + camel.slice(1);
}

function resolveProp(obj, name) {
  if (!obj || !name) return undefined;
  if (name in obj) return obj[name];
  const camel = toCamelCase(name);
  if (camel in obj) return obj[camel];
  const pascal = toPascalCase(name);
  if (pascal in obj) return obj[pascal];
  return undefined;
}

function setupJsdomGlobals(html) {
  const { window } = new JSDOM(html ?? "", { url: "http://localhost" });
  globalThis.window = window;
  globalThis.document = window.document;
  globalThis.DOMParser = window.DOMParser;
  globalThis.XPathResult = window.XPathResult;
  globalThis.Element = window.Element;
  globalThis.Document = window.Document;
  return window;
}

function buildFetchTransport(mod) {
  if (typeof mod.viaFetch !== "function") {
    throw new Error("Module does not export viaFetch");
  }
  return mod.viaFetch();
}

async function handleRequest(req) {
  const action = req.action || "parse";
  const modUrl =
    "data:text/javascript;base64," +
    Buffer.from(req.code, "utf-8").toString("base64");
  const mod = await import(modUrl);

  if (action === "parse") {
    const html = req.html ?? "";
    setupJsdomGlobals(html);
    const Cls = resolveProp(mod, req.className);
    if (typeof Cls !== "function") {
      throw new Error(`Class '${req.className}' not found in generated module`);
    }
    const instance = new Cls(html);
    return instance.parse();
  }

  if (action === "fn") {
    const html = req.html ?? req.input ?? "";
    setupJsdomGlobals(html);
    const fn = resolveProp(mod, req.fnName);
    if (typeof fn !== "function") {
      throw new Error(`Function '${req.fnName}' not found in generated module`);
    }
    return fn(req.input ?? req.html ?? "");
  }

  if (action === "rest") {
    const Cls = resolveProp(mod, req.className);
    if (!Cls) {
      throw new Error(`REST class '${req.className}' not found in module`);
    }
    const methodName = req.methodName || "fetch";
    const method = resolveProp(Cls, methodName);
    if (typeof method !== "function") {
      throw new Error(
        `REST method '${methodName}' not found on '${req.className}'`,
      );
    }
    const transport = buildFetchTransport(mod);
    const res = await method.call(Cls, transport, ...(req.args || []));
    const isOk = typeof res.isOk === "boolean" ? res.isOk : Boolean(res.ok);
    const kind = res.kind ?? (isOk ? "ok" : null);
    const constructorName =
      res.variant ??
      (res.constructor && res.constructor.name !== "Object"
        ? res.constructor.name
        : kind === "ok"
          ? "Ok"
          : kind === "decode"
            ? "DecodeErr"
            : kind === "unknown"
              ? "UnknownErr"
              : kind === "transport"
                ? "TransportErr"
                : null);
    const value =
      res.value !== undefined
        ? res.value
        : res.body !== undefined
          ? res.body
          : null;
    return {
      ok: isOk,
      isOk,
      kind,
      variant: res.variant ?? constructorName,
      status: res.status ?? 0,
      headers: res.headers ?? {},
      value,
      body: res.body !== undefined ? res.body : value,
      cause: res.cause != null ? String(res.cause) : null,
      constructorName,
    };
  }

  if (action === "fetch") {
    setupJsdomGlobals("");
    const Cls = resolveProp(mod, req.className);
    if (!Cls) {
      throw new Error(`Class '${req.className}' not found in module`);
    }
    const methodName = req.methodName || "fetch";
    const method = resolveProp(Cls, methodName);
    if (typeof method !== "function") {
      throw new Error(
        `Fetch method '${methodName}' not found on '${req.className}'`,
      );
    }
    const transport = buildFetchTransport(mod);
    const parser = await method.call(Cls, transport, ...(req.args || []));
    let checkResult = null;
    if (req.checkMethod) {
      const checkFn = resolveProp(parser, req.checkMethod);
      if (typeof checkFn !== "function") {
        throw new Error(
          `Check method '${req.checkMethod}' not found on '${req.className}'`,
        );
      }
      checkResult = checkFn.call(parser);
    }
    return {
      parsed: req.skipParse ? null : parser.parse(),
      checkResult,
    };
  }

  throw new Error(`Unsupported js_worker action: ${action}`);
}

const rl = readline.createInterface({
  input: process.stdin,
  output: process.stdout,
  terminal: false,
});

for await (const line of rl) {
  if (!line.trim()) continue;
  let req = null;
  try {
    req = JSON.parse(line);
    const result = await handleRequest(req);
    process.stdout.write(
      JSON.stringify({ id: req.id, ok: true, result }) + "\n",
    );
  } catch (err) {
    const errName = err instanceof Error && err.name ? err.name : "Error";
    const errMsg =
      err instanceof Error
        ? err.name && !String(err.message).startsWith(err.name)
          ? `${err.name}: ${err.message}`
          : String(err.message)
        : String(err);
    process.stdout.write(
      JSON.stringify({
        id: req ? req.id : null,
        ok: false,
        name: errName,
        error: errMsg,
      }) + "\n",
    );
  }
}
