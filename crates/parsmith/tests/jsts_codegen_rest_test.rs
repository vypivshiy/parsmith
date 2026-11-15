mod common;

use std::fs;
use std::process::Command;

use common::*;
use parsmith::{CodegenOptions, Target, generate_source};

#[test]
fn snapshot_ts_and_js_html_fetch_requests() {
    let ts_res = generate_source(
        SCHEMA_HTML_FETCH_REQUESTS,
        Some("html_fetch.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!ts_res.has_errors(), "{:?}", ts_res.diagnostics);
    insta::assert_snapshot!("ts_html_fetch_requests", &ts_res.files["html_fetch.ts"]);

    let js_res = generate_source(
        SCHEMA_HTML_FETCH_REQUESTS,
        Some("html_fetch.kdl"),
        &CodegenOptions::new(Target::JavaScript),
    );
    assert!(!js_res.has_errors(), "{:?}", js_res.diagnostics);
    assert_eq!(js_res.files.len(), 2);
    insta::assert_snapshot!("js_html_fetch_requests", &js_res.files["html_fetch.js"]);
    insta::assert_snapshot!("dts_html_fetch_requests", &js_res.files["html_fetch.d.ts"]);
}

#[test]
fn snapshot_ts_and_js_rest_client() {
    let ts_res = generate_source(
        SCHEMA_REST_CLIENT,
        Some("rest_client.kdl"),
        &CodegenOptions::new(Target::TypeScript),
    );
    assert!(!ts_res.has_errors(), "{:?}", ts_res.diagnostics);
    insta::assert_snapshot!("ts_rest_client", &ts_res.files["rest_client.ts"]);

    let js_res = generate_source(
        SCHEMA_REST_CLIENT,
        Some("rest_client.kdl"),
        &CodegenOptions::new(Target::JavaScript),
    );
    assert!(!js_res.has_errors(), "{:?}", js_res.diagnostics);
    assert_eq!(js_res.files.len(), 2);
    insta::assert_snapshot!("js_rest_client", &js_res.files["rest_client.js"]);
    insta::assert_snapshot!("dts_rest_client", &js_res.files["rest_client.d.ts"]);
}

#[test]
fn jsts_rest_toolchain_node_execution_and_tsc_strict() {
    let tmp = TempTestDir::new("rest_toolchain");

    let schemas = [
        ("m_fetch.kdl", SCHEMA_HTML_FETCH_REQUESTS),
        ("m_rest.kdl", SCHEMA_REST_CLIENT),
    ];

    let ts_dir = tmp.path.join("ts_out");
    let js_dir = tmp.path.join("js_out");
    fs::create_dir_all(&ts_dir).unwrap();
    fs::create_dir_all(&js_dir).unwrap();

    // 1. Generate schemas in standalone TypeScript (`.ts`) mode
    let ts_opts = CodegenOptions::new(Target::TypeScript);
    for (name, src) in &schemas {
        let res = generate_source(src, Some(name), &ts_opts);
        assert!(
            !res.has_errors(),
            "TS generation failed for {name}: {:?}",
            res.diagnostics
        );
        for (fname, code) in &res.files {
            tmp.write(&format!("ts_out/{fname}"), code);
        }
    }

    // 2. Generate schemas in JavaScript + Declaration (`.js` + `.d.ts`) mode
    let js_opts = CodegenOptions::new(Target::JavaScript);
    for (name, src) in &schemas {
        let res = generate_source(src, Some(name), &js_opts);
        assert!(
            !res.has_errors(),
            "JS generation failed for {name}: {:?}",
            res.diagnostics
        );
        assert_eq!(res.files.len(), 2, "Expected .js and .d.ts for {name}");
        for (fname, code) in &res.files {
            tmp.write(&format!("js_out/{fname}"), code);
        }
    }

    // 3. Write strict TypeScript consumer verifying discriminated unions, viaFetch/viaAxios, and overloads
    let consumer_ts = r#"import {
  ArticlePage,
  type ArticlePageType,
  ChunkedLog,
  type ChunkedLogType,
  type HttpRequest,
  type RawResponse,
  type HttpTransport,
  type RtRequestOptions,
  TransportError,
  FetchTransport,
  AxiosTransport,
  viaFetch,
  viaAxios,
  rtBuildRequest,
  rtExtractParams,
  rtExtractPath,
  rtFetch,
} from "./m_fetch.js";
import {
  UserApi,
  type UserJson,
  type ArticleListJson,
  type ApiErrorJson,
  type SpecificNotFoundErrorJson,
  type GenericErrorJson,
  type Ok,
  type UnknownErr,
  type TransportErr,
  type DecodeErr,
  type UserApiGetUserResult,
  type GetUserResult,
  type UserApiCreateUserResult,
  type UserApiListArticlesResult,
  type UserApiPingResult,
  type UserApiErr200Code,
  type UserApiErr404NotFound,
  type UserApiErr404,
  type UserApiErr400,
  RT_USER_API_MATCHERS,
  rtDispatchResponse,
  rtRestCall,
} from "./m_rest.js";

export async function verifyRestAndTransportTypes(transport: HttpTransport): Promise<void> {
  const ft: HttpTransport = viaFetch();
  const at: HttpTransport = viaAxios({
    async request(_cfg) {
      return { status: 200, headers: {}, data: "{}" };
    },
  });
  const _opts: RtRequestOptions = { headers: { "X-Test": "1" } };
  const _classes = [TransportError, FetchTransport, AxiosTransport, ft, at, _opts, rtBuildRequest, rtExtractParams, rtExtractPath, rtFetch, RT_USER_API_MATCHERS, rtDispatchResponse, rtRestCall];

  // 1. HTML & Raw @request fetch (both positional and named params object overloads)
  const p1: ArticlePage = await ArticlePage.fetch(transport, 7, "sess_1", null, null, null, ["a", "b"]);
  const p2: ArticlePage = await ArticlePage.fetch(
    transport,
    {
      id: 7,
      sid: "sess_1",
      tags: ["a", "b"],
      pipes: [1, 2],
      spaces: ["x", "y"],
      brackets: [10, 20],
      repeats: ["r1", "r2"],
    },
    { headers: { "X-Caller": "custom" } },
  );
  const d1: ArticlePageType = p1.parse();
  const d2: ArticlePageType = p2.parse();

  const slug1: ArticlePage = await ArticlePage.fetchBySlug(transport, "hello-world");
  const slug2: ArticlePage = await ArticlePage.fetch_by_slug(transport, { slug: "hello-world", token: "t1" });

  const log1: ChunkedLog = await ChunkedLog.fetch(transport, "s99");
  const log2: ChunkedLog = await ChunkedLog.fetch(transport, { streamId: "s99" });
  const log3: ChunkedLog = await ChunkedLog.fetch(transport, { stream_id: "s99" });
  const logData: ChunkedLogType = log1.parse();

  // 2. Monadic REST client & discriminated union narrowing
  const res: UserApiGetUserResult = await UserApi.getUser(transport, 10, true);
  const resAlias: GetUserResult = await UserApi.getUser(transport, { id: 10, includeMeta: false });

  if (res.ok) {
    const _okCheck: Ok<UserJson> = res;
    const _val: UserJson = res.value;
    const _id: number = _val.id;
    void [_okCheck, _id];
  } else {
    switch (res.kind) {
      case "error": {
        switch (res.variant) {
          case "UserApiErr200Code": {
            const _e200: UserApiErr200Code = res;
            const _apiErr: ApiErrorJson = res.value;
            void [_e200, _apiErr.code];
            break;
          }
          case "UserApiErr404NotFound": {
            const _e404nf: UserApiErr404NotFound = res;
            const _nf: SpecificNotFoundErrorJson = res.value;
            void [_e404nf, _nf.error];
            break;
          }
          case "UserApiErr404": {
            const _e404: UserApiErr404 = res;
            const _gen: GenericErrorJson = res.value;
            void [_e404, _gen.message];
            break;
          }
          case "UserApiErr400": {
            const _e400: UserApiErr400 = res;
            const _apiErr: ApiErrorJson = res.value;
            void [_e400, _apiErr.message];
            break;
          }
        }
        break;
      }
      case "unknown": {
        const _u: UnknownErr = res;
        const _b: unknown = _u.body;
        void _b;
        break;
      }
      case "transport": {
        const _t: TransportErr = res;
        const _zero: 0 = _t.status;
        void [_zero, _t.cause, _t.exception];
        break;
      }
      case "decode": {
        const _d: DecodeErr = res;
        void [_d.status, _d.body, _d.cause, _d.exception];
        break;
      }
    }
  }

  const created: UserApiCreateUserResult = await UserApi.createUser(transport, {
    tag: "v1",
    sid: "s_1",
    name: "Alice",
    active: true,
  });
  const articles: UserApiListArticlesResult = await UserApi.listArticles(transport);
  if (articles.ok) {
    const _list: ArticleListJson[] = articles.value;
    void _list;
  }
  const pingRes: UserApiPingResult = await UserApi.ping(transport);
  if (pingRes.ok) {
    const _nullVal: null = pingRes.value;
    void _nullVal;
  }

  const _reqCheck: HttpRequest = {
    method: "GET",
    url: "https://example.com",
    headers: {},
    params: {},
    cookies: {},
  };
  const _respCheck: RawResponse = { status: 200, headers: {}, text: "" };

  void [_classes, d1, d2, slug1, slug2, log2, log3, logData, resAlias, created, _reqCheck, _respCheck];
}
"#;
    tmp.write("js_out/check_rest_consumer.ts", consumer_ts);
    tmp.write("ts_out/check_rest_consumer.ts", consumer_ts);

    let tsconfig = r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "ESNext",
    "moduleResolution": "Bundler",
    "lib": ["ES2022", "DOM"],
    "strict": true,
    "noEmit": true,
    "skipLibCheck": true
  },
  "include": ["./*.ts", "./*.d.ts"]
}
"#;
    let ts_config_path = tmp.write("ts_out/tsconfig.json", tsconfig);
    let js_config_path = tmp.write("js_out/tsconfig.json", tsconfig);

    // 4. Run `tsc --noEmit --strict` on both `ts_out` (`.ts`) and `js_out` (`.js + .d.ts`)
    let tsc_bin = if cfg!(windows) { "tsc.cmd" } else { "tsc" };

    let tsc_ts_out = Command::new(tsc_bin)
        .args(["-p", ts_config_path.to_str().unwrap()])
        .output()
        .expect("execute tsc on ts_out");
    assert!(
        tsc_ts_out.status.success(),
        "tsc --strict failed on .ts files!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tsc_ts_out.stdout),
        String::from_utf8_lossy(&tsc_ts_out.stderr)
    );

    let tsc_js_out = Command::new(tsc_bin)
        .args(["-p", js_config_path.to_str().unwrap()])
        .output()
        .expect("execute tsc on js_out");
    assert!(
        tsc_js_out.status.success(),
        "tsc --strict failed on .js + .d.ts files!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&tsc_js_out.stdout),
        String::from_utf8_lossy(&tsc_js_out.stderr)
    );

    // 5. Execute generated `.js` and `.ts` modules in Node.js verifying viaFetch, viaAxios, @request fetch, and REST monad
    let runner_body = r##"import assert from "node:assert/strict";
import { createRequire } from "node:module";

const require = createRequire(process.env.PARSMITH_JSDOM_PKG || import.meta.url);
const { JSDOM } = require("jsdom");

const dom = new JSDOM("<!doctype html><html><body></body></html>", { url: "http://localhost" });
globalThis.window = dom.window;
globalThis.document = dom.window.document;
globalThis.DOMParser = dom.window.DOMParser;
globalThis.XPathResult = dom.window.XPathResult;
globalThis.Element = dom.window.Element;
globalThis.Document = dom.window.Document;

import * as mFetch from "./m_fetch.EXT";
import * as mRest from "./m_rest.EXT";

class MockTransport {
  constructor(handler) {
    this._handler = handler;
    this.lastRequest = null;
  }

  async send(req) {
    this.lastRequest = req;
    return await this._handler(req);
  }
}

// 1. Test viaFetch adapter (query params, cookies, json/form/raw bodies, header normalization, and TransportError wrapping)
{
  let capturedUrl = "";
  let capturedInit = null;

  const mockFetchOk = async (url, init) => {
    capturedUrl = String(url);
    capturedInit = init;
    const h = new Map([
      ["Content-Type", "application/json"],
      ["X-Server-Header", "srv-val"],
    ]);
    return {
      status: 201,
      headers: h,
      text: async () => '{"created":true}',
    };
  };

  const fetchTransport = mFetch.viaFetch(mockFetchOk);
  assert.ok(fetchTransport instanceof mFetch.FetchTransport);

  const rawResp = await fetchTransport.send({
    method: "POST",
    url: "https://api.example.com/items",
    headers: { "X-Custom": "1" },
    params: { q: "search", tag: ["a", "b"] },
    cookies: { sid: "s1", theme: "dark" },
    body: { title: "Hello" },
    bodyKind: "json",
    extra: { mode: "cors" },
  });

  assert.equal(capturedUrl, "https://api.example.com/items?q=search&tag=a&tag=b");
  assert.equal(capturedInit.method, "POST");
  assert.equal(capturedInit.mode, "cors");
  assert.equal(capturedInit.headers["X-Custom"], "1");
  assert.equal(capturedInit.headers["Cookie"], "sid=s1; theme=dark");
  assert.equal(capturedInit.headers["Content-Type"], "application/json");
  assert.equal(capturedInit.body, '{"title":"Hello"}');

  assert.equal(rawResp.status, 201);
  assert.deepEqual(rawResp.headers, {
    "content-type": "application/json",
    "x-server-header": "srv-val",
  });
  assert.equal(rawResp.text, '{"created":true}');

  // Form body via FetchTransport
  await fetchTransport.send({
    method: "POST",
    url: "https://api.example.com/form",
    headers: {},
    params: {},
    cookies: {},
    body: { a: "1", b: "two words" },
    bodyKind: "form",
  });
  assert.equal(capturedInit.headers["Content-Type"], "application/x-www-form-urlencoded");
  assert.equal(capturedInit.body, "a=1&b=two+words");

  // Network error in fetchFn wraps into TransportError preserving cause
  const netErr = new Error("ECONNREFUSED fetch failure");
  const failingFetch = mFetch.viaFetch(async () => {
    throw netErr;
  });
  await assert.rejects(
    () =>
      failingFetch.send({
        method: "GET",
        url: "https://api.example.com/down",
        headers: {},
        params: {},
        cookies: {},
      }),
    (err) => err instanceof mFetch.TransportError && err.cause === netErr
  );
}

// 2. Test viaAxios adapter (config normalization, validateStatus, err.response conversion, and TransportError wrapping)
{
  let lastAxiosConfig = null;
  const axiosOk = mRest.viaAxios({
    async request(config) {
      lastAxiosConfig = config;
      return {
        status: 200,
        headers: { "X-Axios": "yes", "Content-Type": "application/json" },
        data: { id: 1, name: "Alice", nickname: null },
      };
    },
  });
  assert.ok(axiosOk instanceof mRest.AxiosTransport);

  const r1 = await mRest.UserApi.getUser(axiosOk, 1, true);
  assert.equal(r1.ok, true);
  assert.equal(r1.kind, "ok");
  assert.equal(r1.status, 200);
  assert.equal(r1.headers["x-axios"], "yes");
  assert.deepEqual(r1.value, { id: 1, name: "Alice", nickname: null });
  assert.equal(lastAxiosConfig.method, "GET");
  assert.equal(lastAxiosConfig.url, "https://api.example.com/users/1");
  assert.deepEqual(lastAxiosConfig.params, { include_meta: "true" });
  assert.equal(typeof lastAxiosConfig.validateStatus, "function");
  assert.equal(lastAxiosConfig.validateStatus(500), true);

  // Axios instance that rejects with err.response (e.g. if a custom interceptor rejects on 404)
  const axiosRejectWithResp = mRest.viaAxios({
    async request(_config) {
      const err = new Error("Request failed with status code 404");
      err.response = {
        status: 404,
        headers: { "X-Err": "from-axios-err" },
        data: JSON.stringify({ error: "Not Found", code: 404 }),
      };
      throw err;
    },
  });
  const r404 = await mRest.UserApi.getUser(axiosRejectWithResp, 99);
  assert.equal(r404.ok, false);
  assert.equal(r404.kind, "error");
  assert.equal(r404.variant, "UserApiErr404NotFound");
  assert.equal(r404.status, 404);
  assert.equal(r404.headers["x-err"], "from-axios-err");
  assert.deepEqual(r404.value, { error: "Not Found", code: 404 });

  // Axios network error without err.response -> TransportError -> TransportErr
  const axiosNetCause = new Error("ETIMEDOUT axios timeout");
  const axiosNetFail = mRest.viaAxios({
    async request(_config) {
      throw axiosNetCause;
    },
  });
  const rTrans = await mRest.UserApi.getUser(axiosNetFail, 1);
  assert.equal(rTrans.ok, false);
  assert.equal(rTrans.kind, "transport");
  assert.equal(rTrans.status, 0);
  assert.equal(rTrans.exception, axiosNetCause);
  assert.ok(rTrans.cause.includes("ETIMEDOUT axios timeout"));
}

// 3. Test @request fetch methods on HTML & Raw structs (ArticlePage & ChunkedLog)
{
  const tx = new MockTransport(async (req) => {
    if (req.url.startsWith("https://example.com/articles/slug/")) {
      return {
        status: 200,
        headers: { "content-type": "text/html" },
        text: "<html><body><h1>Slug Article</h1><p class='summary'>From POST</p></body></html>",
      };
    }
    if (req.url.startsWith("https://example.com/logs/")) {
      return {
        status: 200,
        headers: { "content-type": "application/json" },
        text: JSON.stringify({ payload: { lines: ["line-1", "line-2", "line-3"] } }),
      };
    }
    return {
      status: 200,
      headers: { "content-type": "text/html" },
      text: "<html><body><h1>Article 7</h1><p class='summary'>Summary 7</p></body></html>",
    };
  });

  const page = await mFetch.ArticlePage.fetch(
    tx,
    {
      id: 7,
      sid: "sess_1",
      tags: ["a", "b"],
      pipes: [1, 2],
      spaces: ["x", "y"],
      brackets: [10, 20],
      repeats: ["r1", "r2"],
    },
    { headers: { "X-Caller": "custom" } }
  );
  assert.deepEqual(page.parse(), { heading: "Article 7", summary: "Summary 7" });

  const sent = tx.lastRequest;
  assert.equal(sent.method, "GET");
  assert.equal(sent.url, "https://example.com/articles/7");
  assert.deepEqual(sent.params, {
    tags: "a,b",
    pipes: "1|2",
    spaces: "x y",
    "brackets[]": ["10", "20"],
    repeats: ["r1", "r2"],
  });
  assert.equal("q" in sent.params, false);
  assert.deepEqual(sent.headers, { "X-Static": "parsmith-test", "X-Caller": "custom" });
  assert.equal("X-Trace" in sent.headers, false);
  assert.deepEqual(sent.cookies, { sid: "sess_1" });
  assert.equal("theme" in sent.cookies, false);

  // Named @request (fetchBySlug & fetch_by_slug) with form body and URL path encoding
  const slugPage = await mFetch.ArticlePage.fetchBySlug(tx, "hello world");
  assert.deepEqual(slugPage.parse(), { heading: "Slug Article", summary: "From POST" });
  assert.equal(tx.lastRequest.method, "POST");
  assert.equal(tx.lastRequest.url, "https://example.com/articles/slug/hello%20world");
  assert.equal(tx.lastRequest.bodyKind, "form");
  assert.deepEqual(tx.lastRequest.body, { mode: "preview" });

  const slugPage2 = await mFetch.ArticlePage.fetch_by_slug(tx, { slug: "hello-2", token: "tok_9" });
  assert.deepEqual(slugPage2.parse(), { heading: "Slug Article", summary: "From POST" });
  assert.deepEqual(tx.lastRequest.body, { mode: "preview", token: "tok_9" });

  // response-path + response-join on (raw)struct
  const logParsed = (await mFetch.ChunkedLog.fetch(tx, { streamId: "s99" })).parse();
  assert.deepEqual(logParsed, { lines: ["line-1", "line-2", "line-3"] });
}

// 4. Test Monadic REST Client (Ok, @error variants, fallthrough, UnknownErr, DecodeErr, TransportErr, and programmer errors)
{
  // 4a. 200 OK + allowlist projection
  const okTx = new MockTransport(async () => ({
    status: 200,
    headers: { "x-req-id": "r1" },
    text: JSON.stringify({ id: 10, name: "Alice", nickname: null, undeclared: "ignored" }),
  }));
  const resOk = await mRest.UserApi.getUser(okTx, 10);
  assert.deepEqual(resOk, {
    ok: true,
    kind: "ok",
    status: 200,
    headers: { "x-req-id": "r1" },
    value: { id: 10, name: "Alice", nickname: null },
  });
  assert.equal("undeclared" in resOk.value, false);

  // 4b. POST JSON body preserves null for optional placeholder while omitting null/undefined params/headers/cookies
  const resCreate = await mRest.UserApi.createUser(
    okTx,
    {
      tag: "v1",
      sid: "s_abc",
      name: "Alice",
      active: true,
    },
    { headers: { "X-Custom": "1" } }
  );
  assert.equal(resCreate.ok, true);
  assert.equal(okTx.lastRequest.method, "POST");
  assert.deepEqual(okTx.lastRequest.params, { tag: "v1" });
  assert.deepEqual(okTx.lastRequest.headers, {
    "Content-Type": "application/json",
    "X-Custom": "1",
  });
  assert.deepEqual(okTx.lastRequest.cookies, { sid: "s_abc" });
  assert.equal(okTx.lastRequest.bodyKind, "json");
  assert.deepEqual(okTx.lastRequest.body, {
    name: "Alice",
    nickname: null,
    active: true,
  });

  // 4c. Enveloped response-path="data.user", array response, and void endpoint
  const envTx = new MockTransport(async () => ({
    status: 200,
    headers: {},
    text: JSON.stringify({ data: { user: { id: 5, name: "Bob", nickname: "B" } } }),
  }));
  const resEnv = await mRest.UserApi.getProfileEnveloped(envTx);
  assert.equal(resEnv.ok, true);
  assert.deepEqual(resEnv.value, { id: 5, name: "Bob", nickname: "B" });

  const listTx = new MockTransport(async () => ({
    status: 200,
    headers: {},
    text: JSON.stringify([{ id: 1, title: "First", extra: 0 }, { id: 2, title: "Second" }]),
  }));
  const resList = await mRest.UserApi.listArticles(listTx);
  assert.equal(resList.ok, true);
  assert.deepEqual(resList.value, [
    { article_id: 1, headline: "First" },
    { article_id: 2, headline: "Second" },
  ]);

  const voidTx = new MockTransport(async () => ({
    status: 204,
    headers: {},
    text: "",
  }));
  const resPing = await mRest.UserApi.ping(voidTx);
  assert.deepEqual(resPing, {
    ok: true,
    kind: "ok",
    status: 204,
    headers: {},
    value: null,
  });

  // 4d. @error 200 dot-path condition ("error.code")
  const err200Tx = new MockTransport(async () => ({
    status: 200,
    headers: {},
    text: JSON.stringify({ error: { code: 1001, message: "Quota exceeded" } }),
  }));
  const resErr200 = await mRest.UserApi.getUser(err200Tx, 1);
  assert.deepEqual(resErr200, {
    ok: false,
    kind: "error",
    variant: "UserApiErr200Code",
    status: 200,
    headers: {},
    value: { code: 1001, message: "Quota exceeded" },
  });

  // 4e. @error:not-found 404 SpecificNotFoundError vs fallthrough to @error 404 GenericError
  const err404SpecTx = new MockTransport(async () => ({
    status: 404,
    headers: {},
    text: JSON.stringify({ error: "Not Found", code: 404, noise: 1 }),
  }));
  const res404Spec = await mRest.UserApi.getUser(err404SpecTx, 1);
  assert.deepEqual(res404Spec, {
    ok: false,
    kind: "error",
    variant: "UserApiErr404NotFound",
    status: 404,
    headers: {},
    value: { error: "Not Found", code: 404 },
  });

  const err404FallthroughTx = new MockTransport(async () => ({
    status: 404,
    headers: {},
    text: JSON.stringify({ error: "Not Found", message: "Fallback generic 404" }),
  }));
  const res404Gen = await mRest.UserApi.getUser(err404FallthroughTx, 1);
  assert.deepEqual(res404Gen, {
    ok: false,
    kind: "error",
    variant: "UserApiErr404",
    status: 404,
    headers: {},
    value: { message: "Fallback generic 404" },
  });

  // 4f. UnknownErr on unmatched non-2xx status (503)
  const err503Tx = new MockTransport(async () => ({
    status: 503,
    headers: { "retry-after": "10" },
    text: JSON.stringify({ reason: "maintenance" }),
  }));
  const res503 = await mRest.UserApi.getUser(err503Tx, 1);
  assert.deepEqual(res503, {
    ok: false,
    kind: "unknown",
    status: 503,
    headers: { "retry-after": "10" },
    body: { reason: "maintenance" },
  });

  // 4g. DecodeErr on 200 OK with malformed JSON or projector schema mismatch
  const badJson200Tx = new MockTransport(async () => ({
    status: 200,
    headers: { "content-type": "text/html" },
    text: "<html>502 Bad Gateway disguised as 200</html>",
  }));
  const resDecodeJson = await mRest.UserApi.getUser(badJson200Tx, 1);
  assert.equal(resDecodeJson.ok, false);
  assert.equal(resDecodeJson.kind, "decode");
  assert.equal(resDecodeJson.status, 200);
  assert.equal(resDecodeJson.body, "<html>502 Bad Gateway disguised as 200</html>");
  assert.ok(resDecodeJson.exception instanceof SyntaxError);

  const mismatch200Tx = new MockTransport(async () => ({
    status: 200,
    headers: {},
    text: JSON.stringify({ id: 10 }), // missing required 'name'
  }));
  const resDecodeProj = await mRest.UserApi.getUser(mismatch200Tx, 10);
  assert.equal(resDecodeProj.ok, false);
  assert.equal(resDecodeProj.kind, "decode");
  assert.equal(resDecodeProj.status, 200);
  assert.deepEqual(resDecodeProj.body, { id: 10 });
  assert.ok(resDecodeProj.cause.includes("Required JSON field 'name'"));
  assert.ok(resDecodeProj.exception instanceof mRest.RtJsonFieldMissingError);

  // DecodeErr when matched @error 400 fails its declared ApiError schema contract
  const mismatch400Tx = new MockTransport(async () => ({
    status: 400,
    headers: {},
    text: JSON.stringify({ unexpected: "shape" }),
  }));
  const resDecode400 = await mRest.UserApi.getUser(mismatch400Tx, 10);
  assert.equal(resDecode400.ok, false);
  assert.equal(resDecode400.kind, "decode");
  assert.equal(resDecode400.status, 400);
  assert.deepEqual(resDecode400.body, { unexpected: "shape" });

  // 4h. TransportErr when transport throws TransportError
  const rawCause = new Error("socket hang up");
  const transFailTx = new MockTransport(async () => {
    throw new mRest.TransportError(rawCause);
  });
  const resTrans = await mRest.UserApi.getUser(transFailTx, 1);
  assert.equal(resTrans.ok, false);
  assert.equal(resTrans.kind, "transport");
  assert.equal(resTrans.status, 0);
  assert.equal(resTrans.exception, rawCause);
  assert.ok(resTrans.cause.includes("socket hang up"));

  // 4i. Programmer bug (TypeError) in broken transport MUST NOT be swallowed into TransportErr!
  const brokenTx = new MockTransport(async () => {
    throw new TypeError("programmer bug inside custom transport");
  });
  await assert.rejects(
    () => mRest.UserApi.getUser(brokenTx, 1),
    (err) => err instanceof TypeError && err.message.includes("programmer bug inside custom transport")
  );
}
"##;

    let js_runner_path = tmp.write(
        "js_out/run_rest_suite.mjs",
        &runner_body.replace(".EXT", ".js"),
    );
    let ts_runner_path = tmp.write(
        "ts_out/run_rest_suite.ts",
        &runner_body.replace(".EXT", ".ts"),
    );
    let jsdom_pkg = resolve_jsdom_pkg();

    let node_js_out = Command::new("node")
        .env("PARSMITH_JSDOM_PKG", &jsdom_pkg)
        .arg(&js_runner_path)
        .output()
        .expect("execute node on js_out/run_rest_suite.mjs");
    assert!(
        node_js_out.status.success(),
        "node execution failed on .js modules!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&node_js_out.stdout),
        String::from_utf8_lossy(&node_js_out.stderr)
    );

    let node_ts_out = Command::new("node")
        .env("PARSMITH_JSDOM_PKG", &jsdom_pkg)
        .arg(&ts_runner_path)
        .output()
        .expect("execute node on ts_out/run_rest_suite.ts");
    assert!(
        node_ts_out.status.success(),
        "node execution failed on .ts modules!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&node_ts_out.stdout),
        String::from_utf8_lossy(&node_ts_out.stderr)
    );
}
