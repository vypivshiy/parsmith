mod common;

use std::path::PathBuf;
use std::process::Command;

use common::*;
use parsmith::{CodegenOptions, HttpIo, PythonModels, Target, generate_source};

const SCHEMA_JSON_GJSON_AND_HOISTING: &str = r####"
@doc "JSON projection coverage: GJSON dot-paths, escaped dots, array indices, count, projection, inline hoisting, and dict schemas."

json GeoLocation {
    latitude float from="lat"
    longitude float from="lng"
}

(array)json TagItemList {
    tag_id int from="id"
    label str
}

(dict)json TranslationsMap {
    @key str
    @value {
        title str from="wire_name"
        active bool
        badge str @omitempty from="meta.badge"
        raw_token @skip
    }
}

json ReleaseCatalog {
    release_id int from="@id"
    os_version str from="os\\.version"
    nested_dot str from="meta.build\\.number"
    backslash_key str from="a\\\\b"
    hash_key str from="\\#"
    first_score int from="rating_scores.0"
    tag_names (array)str from="tags.#.name"
    tag_count int from="tags.#"
    missing_note str? from="meta.notes.0.text"
    missing_count int? from="missing_arr.#"
    missing_proj (array)str? from="missing_arr.#.name"
    omitted_val int @omitempty from="not_found.key"
    secret_debug @skip
    location GeoLocation?
    tag_items TagItemList?
    author {
        author_name str from="dc:creator"
        verified bool
    }
    links (array)LinkItem {
        id int
        relation str
    }
    channels (dict) {
        @key str
        @value {
            endpoint str from="stream_endpoint"
            codec str?
            internal_flag @skip
        }
    }
    translations TranslationsMap?
}

(raw)struct CatalogExtractor {
    catalog {
        jsonify ReleaseCatalog
    }
    envelope_catalog {
        jsonify ReleaseCatalog path="payload\\.data"
    }
    translations_only {
        jsonify TranslationsMap
    }
    tags_only {
        jsonify TagItemList
    }
}
"####;

const SCHEMA_HTML_FETCH_REQUESTS: &str = r####"
@doc "HTML and Raw structs with @request, typed placeholders, query array styles, and response-join."

struct ArticlePage {
    @doc "Article HTML page parser with sync and async fetch."

    @request doc="Fetch article by ID and query filters." """
    GET /articles/{{id:int}}?q={{q:str?}}&tags={{tags:str[]?|csv}}&pipes={{pipes:int[]?|pipe}}&spaces={{spaces:str[]?|space}}&brackets={{brackets:int[]?|bracket}}&repeats={{repeats:str[]?}} HTTP/1.1
    Host: example.com
    X-Static: parsmith-test
    X-Trace: {{trace:str?}}
    Cookie: sid={{sid:str}}; theme={{theme:str?}}
    """

    @request name="by-slug" """
    POST /articles/slug/{{slug:str}} HTTP/1.1
    Host: example.com
    Content-Type: application/x-www-form-urlencoded

    mode=preview&token={{token:str?}}
    """

    heading {
        css "h1"
        text
        trim
    }

    summary {
        css "p.summary"
        text
        trim
        fallback ""
    }
}

(raw)struct ChunkedLog {
    @request response-path="payload.lines" response-join="\n" """
    GET /logs/{{stream_id:str}} HTTP/1.1
    Host: example.com
    """

    lines {
        split "\n"
    }
}
"####;

#[test]
fn snapshot_python_json_projectors_typeddict() {
    let opts = CodegenOptions::new(Target::Python).with_models(PythonModels::TypedDict);
    let res = generate_source(
        SCHEMA_JSON_GJSON_AND_HOISTING,
        Some("json_gjson.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_json_projectors_dataclass() {
    let opts = CodegenOptions::new(Target::Python).with_models(PythonModels::Dataclass);
    let res = generate_source(
        SCHEMA_JSON_GJSON_AND_HOISTING,
        Some("json_gjson_dc.kdl"),
        &opts,
    );
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    insta::assert_snapshot!(res.primary_code().unwrap());
}

#[test]
fn snapshot_python_html_fetch_requests() {
    let opts = CodegenOptions::new(Target::Python);
    let res = generate_source(SCHEMA_HTML_FETCH_REQUESTS, Some("html_fetch.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(!code.contains("import importlib"));
    insta::assert_snapshot!(code);
}

#[test]
fn snapshot_python_rest_client_typeddict_both() {
    let opts = CodegenOptions::new(Target::Python)
        .with_models(PythonModels::TypedDict)
        .with_http_io(HttpIo::Both);
    let res = generate_source(SCHEMA_REST_CLIENT, Some("rest_client.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(!code.contains("import importlib"));
    insta::assert_snapshot!(code);
}

#[test]
fn snapshot_python_rest_client_dataclass_sync() {
    let opts = CodegenOptions::new(Target::Python)
        .with_models(PythonModels::Dataclass)
        .with_http_io(HttpIo::Sync);
    let res = generate_source(SCHEMA_REST_CLIENT, Some("rest_client_dc_sync.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(!code.contains("import importlib"));
    insta::assert_snapshot!(code);
}

#[test]
fn snapshot_python_rest_client_async_only() {
    let opts = CodegenOptions::new(Target::Python).with_http_io(HttpIo::Async);
    let res = generate_source(SCHEMA_REST_CLIENT, Some("rest_client_async.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    let code = res.primary_code().unwrap();
    assert!(!code.contains("import importlib"));
    insta::assert_snapshot!(code);
}

#[test]
fn snapshot_python_rest_client_separate_runtime() {
    let opts = CodegenOptions::new(Target::Python)
        .with_separate_runtime(true)
        .with_runtime_name("parsmith_runtime");
    let res = generate_source(SCHEMA_REST_CLIENT, Some("rest_sep.kdl"), &opts);
    assert!(!res.has_errors(), "{:?}", res.diagnostics);
    assert_eq!(res.files.len(), 2);
    assert!(!res.files["rest_sep.py"].contains("import importlib"));
    assert!(!res.files["parsmith_runtime.py"].contains("import importlib"));
    insta::assert_snapshot!("rest_separate_runtime_parser", &res.files["rest_sep.py"]);
    insta::assert_snapshot!(
        "rest_separate_runtime_module",
        &res.files["parsmith_runtime.py"]
    );
}

#[test]
fn python_json_and_rest_toolchain_pytest_and_mypy_strict() {
    let tmp = TempTestDir::new("json_rest_toolchain");

    let mut generated_files: Vec<PathBuf> = Vec::new();

    // 1. JSON Projectors (TypedDict & Dataclass)
    let res_json_td = generate_source(
        SCHEMA_JSON_GJSON_AND_HOISTING,
        Some("m_json_td.kdl"),
        &CodegenOptions::new(Target::Python).with_models(PythonModels::TypedDict),
    );
    assert!(!res_json_td.has_errors(), "{:?}", res_json_td.diagnostics);
    for (fname, code) in &res_json_td.files {
        generated_files.push(tmp.write(fname, code));
    }

    let res_json_dc = generate_source(
        SCHEMA_JSON_GJSON_AND_HOISTING,
        Some("m_json_dc.kdl"),
        &CodegenOptions::new(Target::Python).with_models(PythonModels::Dataclass),
    );
    assert!(!res_json_dc.has_errors(), "{:?}", res_json_dc.diagnostics);
    for (fname, code) in &res_json_dc.files {
        generated_files.push(tmp.write(fname, code));
    }

    // 2. HTML & Raw @request fetch (TypedDict & Dataclass)
    let res_fetch_td = generate_source(
        SCHEMA_HTML_FETCH_REQUESTS,
        Some("m_fetch_td.kdl"),
        &CodegenOptions::new(Target::Python).with_models(PythonModels::TypedDict),
    );
    assert!(!res_fetch_td.has_errors(), "{:?}", res_fetch_td.diagnostics);
    for (fname, code) in &res_fetch_td.files {
        assert!(
            !code.contains("import importlib"),
            "{fname} must not import importlib"
        );
        generated_files.push(tmp.write(fname, code));
    }

    let res_fetch_dc = generate_source(
        SCHEMA_HTML_FETCH_REQUESTS,
        Some("m_fetch_dc.kdl"),
        &CodegenOptions::new(Target::Python).with_models(PythonModels::Dataclass),
    );
    assert!(!res_fetch_dc.has_errors(), "{:?}", res_fetch_dc.diagnostics);
    for (fname, code) in &res_fetch_dc.files {
        assert!(
            !code.contains("import importlib"),
            "{fname} must not import importlib"
        );
        generated_files.push(tmp.write(fname, code));
    }

    // 3. Monadic REST Client (TypedDict, Dataclass, Sync-only, Async-only, and Separate Runtime -R)
    let res_rest_td = generate_source(
        SCHEMA_REST_CLIENT,
        Some("m_rest_td.kdl"),
        &CodegenOptions::new(Target::Python).with_models(PythonModels::TypedDict),
    );
    assert!(!res_rest_td.has_errors(), "{:?}", res_rest_td.diagnostics);
    for (fname, code) in &res_rest_td.files {
        assert!(
            !code.contains("import importlib"),
            "{fname} must not import importlib"
        );
        generated_files.push(tmp.write(fname, code));
    }

    let res_rest_dc = generate_source(
        SCHEMA_REST_CLIENT,
        Some("m_rest_dc.kdl"),
        &CodegenOptions::new(Target::Python).with_models(PythonModels::Dataclass),
    );
    assert!(!res_rest_dc.has_errors(), "{:?}", res_rest_dc.diagnostics);
    for (fname, code) in &res_rest_dc.files {
        assert!(
            !code.contains("import importlib"),
            "{fname} must not import importlib"
        );
        generated_files.push(tmp.write(fname, code));
    }

    let res_rest_sync = generate_source(
        SCHEMA_REST_CLIENT,
        Some("m_rest_sync.kdl"),
        &CodegenOptions::new(Target::Python).with_http_io(HttpIo::Sync),
    );
    assert!(
        !res_rest_sync.has_errors(),
        "{:?}",
        res_rest_sync.diagnostics
    );
    for (fname, code) in &res_rest_sync.files {
        assert!(
            !code.contains("import importlib"),
            "{fname} must not import importlib"
        );
        generated_files.push(tmp.write(fname, code));
    }

    let res_rest_async = generate_source(
        SCHEMA_REST_CLIENT,
        Some("m_rest_async.kdl"),
        &CodegenOptions::new(Target::Python).with_http_io(HttpIo::Async),
    );
    assert!(
        !res_rest_async.has_errors(),
        "{:?}",
        res_rest_async.diagnostics
    );
    for (fname, code) in &res_rest_async.files {
        assert!(
            !code.contains("import importlib"),
            "{fname} must not import importlib"
        );
        generated_files.push(tmp.write(fname, code));
    }

    let res_rest_sep = generate_source(
        SCHEMA_REST_CLIENT,
        Some("m_rest_sep.kdl"),
        &CodegenOptions::new(Target::Python)
            .with_separate_runtime(true)
            .with_runtime_name("parsmith_runtime"),
    );
    assert!(!res_rest_sep.has_errors(), "{:?}", res_rest_sep.diagnostics);
    for (fname, code) in &res_rest_sep.files {
        assert!(
            !code.contains("import importlib"),
            "{fname} must not import importlib"
        );
        generated_files.push(tmp.write(fname, code));
    }

    // 4. Write comprehensive pytest + mypy --strict test suite
    let test_suite_py = r##"from __future__ import annotations

import asyncio
from contextlib import AbstractAsyncContextManager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import threading
from typing import Any
import urllib.error

import aiohttp
import httpx
import pytest
import requests

import m_json_td
import m_json_dc
import m_fetch_td
import m_fetch_dc
import m_rest_td
import m_rest_dc
import m_rest_sync
import m_rest_async
import m_rest_sep
import parsmith_runtime


class MockSyncTransport:
    def __init__(
        self,
        handler: Any,
    ) -> None:
        self._handler = handler
        self.last_request: Any = None

    def send(self, req: Any) -> Any:
        self.last_request = req
        return self._handler(req)


class MockAsyncTransport:
    def __init__(
        self,
        handler: Any,
    ) -> None:
        self._handler = handler
        self.last_request: Any = None

    async def send(self, req: Any) -> Any:
        self.last_request = req
        return await self._handler(req)


def test_json_allowlist_projection_gjson_typeddict_and_dataclass() -> None:
    payload = {
        "@id": 42,
        "os.version": "24.04",
        "meta": {
            "build.number": "b-900",
            "notes": [{"text": "first note"}],
        },
        "a\\b": "slash-ok",
        "#": "hash-val",
        "rating_scores": [98, 85, 70],
        "tags": [
            {"name": "action", "id": 1},
            {"id": 2},
            {"name": None, "id": 3},
            {"name": "sci-fi", "id": 4},
        ],
        "secret_debug": "must_be_dropped",
        "undeclared_wire_key": "must_also_be_dropped",
        "location": {"lat": 35.68, "lng": 139.76, "extra_coord": 999},
        "tag_items": [
            {"id": 10, "label": "featured", "noise": True},
            {"id": 20, "label": "hd"},
        ],
        "author": {
            "dc:creator": "Alice Author",
            "verified": True,
            "ignored": 123,
        },
        "links": [
            {"id": 1, "relation": "sequel", "extra": "drop"},
        ],
        "channels": {
            "main": {
                "stream_endpoint": "https://cdn.example/main.m3u8",
                "codec": "h264",
                "internal_flag": "secret",
                "extra": 1,
            },
            "backup": {
                "stream_endpoint": "https://cdn.example/backup.m3u8",
                "codec": None,
            },
        },
        "translations": {
            "en": {
                "wire_name": "English",
                "active": True,
                "raw_token": "tok_1",
                "extra": 0,
            },
            "ja": {
                "wire_name": "Japanese",
                "active": False,
                "meta": {"badge": "original"},
                "raw_token": "tok_2",
            },
        },
    }
    raw_str = json.dumps(payload)
    enveloped_str = json.dumps({"payload.data": payload})
    tr_str = json.dumps(payload["translations"])
    tags_str = json.dumps(payload["tag_items"])

    # 1. TypedDict mode
    cat_td = m_json_td.rt_project_release_catalog(json.loads(raw_str))
    assert cat_td["release_id"] == 42
    assert cat_td["os_version"] == "24.04"
    assert cat_td["nested_dot"] == "b-900"
    assert cat_td["backslash_key"] == "slash-ok"
    assert cat_td["hash_key"] == "hash-val"
    assert cat_td["first_score"] == 98
    assert cat_td["tag_names"] == ["action", "sci-fi"]
    assert cat_td["tag_count"] == 4
    assert cat_td["missing_note"] == "first note"
    assert cat_td["missing_count"] is None
    assert cat_td["missing_proj"] is None
    assert "omitted_val" not in cat_td
    assert "secret_debug" not in cat_td
    assert "undeclared_wire_key" not in cat_td
    assert cat_td["location"] == {"latitude": 35.68, "longitude": 139.76}
    assert cat_td["tag_items"] == [
        {"tag_id": 10, "label": "featured"},
        {"tag_id": 20, "label": "hd"},
    ]
    assert cat_td["author"] == {"author_name": "Alice Author", "verified": True}
    assert cat_td["links"] == [{"id": 1, "relation": "sequel"}]
    assert cat_td["channels"] == {
        "main": {"endpoint": "https://cdn.example/main.m3u8", "codec": "h264"},
        "backup": {"endpoint": "https://cdn.example/backup.m3u8", "codec": None},
    }
    assert cat_td["translations"] == {
        "en": {"title": "English", "active": True},
        "ja": {"title": "Japanese", "active": False, "badge": "original"},
    }

    # Verify jsonify with path="payload\.data"
    env_td = m_json_td.rt_project_release_catalog(
        m_json_td.rt_resolve_dotpath(json.loads(enveloped_str), r"payload\.data", is_optional=False)
    )
    assert env_td == cat_td
    assert m_json_td.rt_project_translations_map(json.loads(tr_str)) == cat_td["translations"]
    assert m_json_td.rt_project_tag_item_list(json.loads(tags_str)) == cat_td["tag_items"]

    # 2. Dataclass mode
    cat_dc = m_json_dc.rt_project_release_catalog(json.loads(raw_str))
    assert isinstance(cat_dc, m_json_dc.ReleaseCatalogJson)
    assert cat_dc.release_id == 42
    assert cat_dc.os_version == "24.04"
    assert cat_dc.nested_dot == "b-900"
    assert cat_dc.backslash_key == "slash-ok"
    assert cat_dc.hash_key == "hash-val"
    assert cat_dc.tag_names == ["action", "sci-fi"]
    assert cat_dc.tag_count == 4
    assert cat_dc.omitted_val is None
    assert isinstance(cat_dc.location, m_json_dc.GeoLocationJson)
    assert cat_dc.location.latitude == 35.68
    assert cat_dc.location.longitude == 139.76
    assert isinstance(cat_dc.author, m_json_dc.ReleaseCatalogAuthorJson)
    assert cat_dc.author.author_name == "Alice Author"
    assert cat_dc.channels["main"].endpoint == "https://cdn.example/main.m3u8"
    assert cat_dc.translations is not None
    assert cat_dc.translations["ja"].badge == "original"
    assert cat_dc.translations["en"].badge is None


def test_json_projector_contract_violation_exceptions() -> None:
    # Container mismatch -> RtJsonSchemaError (subclass of ValueError)
    for bad_input, type_name in [
        (None, "NoneType"),
        ([1, 2], "list"),
        (123, "int"),
        ("str_val", "str"),
    ]:
        with pytest.raises(
            m_json_td.RtJsonSchemaError,
            match=f"GeoLocation: Expected dict, got {type_name}",
        ):
            m_json_td.rt_project_geo_location(bad_input)

    with pytest.raises(
        m_json_td.RtJsonSchemaError,
        match="TagItemList: Expected list, got dict",
    ):
        m_json_td.rt_project_tag_item_list({"id": 1})

    # Missing required field -> RtJsonFieldMissingError
    with pytest.raises(
        m_json_td.RtJsonFieldMissingError,
        match=r"GeoLocation\.latitude: Required JSON field 'lat' \(mapped to 'latitude'\) is missing",
    ):
        m_json_td.rt_project_geo_location({"lng": 20.0})

    # Null non-nullable field -> RtJsonFieldMissingError
    with pytest.raises(
        m_json_td.RtJsonFieldMissingError,
        match=r"GeoLocation\.latitude: Field 'lat' is null, but 'latitude' is not nullable",
    ):
        m_json_td.rt_project_geo_location({"lat": None, "lng": 20.0})

    # Path error -> RtJsonPathError
    with pytest.raises(
        m_json_td.RtJsonPathError,
        match=r"User\.age: Missing key 'age' in path 'user\.age'",
    ):
        m_json_td.rt_resolve_dotpath(
            {"user": {"name": "Alice"}}, "user.age", is_optional=False, target="User.age"
        )

    with pytest.raises(
        m_json_td.RtJsonPathError,
        match="Index 3 out of bounds",
    ):
        m_json_td.rt_resolve_dotpath({"items": ["a"]}, "items.3", is_optional=False)


def test_html_and_raw_request_fetch_sync_and_async() -> None:
    def sync_handler(req: m_fetch_td.HttpRequest) -> m_fetch_td.RawResponse:
        if req.url.startswith("https://example.com/articles/slug/"):
            return m_fetch_td.RawResponse(
                status=200,
                headers={"content-type": "text/html"},
                text="<html><body><h1>Slug Article</h1><p class='summary'>From POST</p></body></html>",
            )
        if req.url.startswith("https://example.com/logs/"):
            return m_fetch_td.RawResponse(
                status=200,
                headers={"content-type": "application/json"},
                text=json.dumps({"payload": {"lines": ["line-1", "line-2", "line-3"]}}),
            )
        return m_fetch_td.RawResponse(
            status=200,
            headers={"content-type": "text/html"},
            text="<html><body><h1>Article 7</h1><p class='summary'>Summary 7</p></body></html>",
        )

    transport = MockSyncTransport(sync_handler)
    page = m_fetch_td.ArticlePage.fetch(
        transport,
        id=7,
        sid="sess_1",
        tags=["a", "b"],
        pipes=[1, 2],
        spaces=["x", "y"],
        brackets=[10, 20],
        repeats=["r1", "r2"],
        headers={"X-Caller": "custom"},
    )
    assert page.parse() == {"heading": "Article 7", "summary": "Summary 7"}

    sent: m_fetch_td.HttpRequest = transport.last_request
    assert sent.method == "GET"
    assert sent.url == "https://example.com/articles/7"
    assert sent.params == {
        "tags": "a,b",
        "pipes": "1|2",
        "spaces": "x y",
        "brackets[]": ["10", "20"],
        "repeats": ["r1", "r2"],
    }
    assert "q" not in sent.params
    assert sent.headers == {"X-Static": "parsmith-test", "X-Caller": "custom"}
    assert "X-Trace" not in sent.headers
    assert sent.cookies == {"sid": "sess_1"}
    assert "theme" not in sent.cookies

    # Named @request (fetch_by_slug) with form body
    slug_page = m_fetch_dc.ArticlePage.fetch_by_slug(transport, slug="hello-world")
    assert slug_page.parse() == m_fetch_dc.ArticlePageType(
        heading="Slug Article", summary="From POST"
    )
    assert transport.last_request.method == "POST"
    assert transport.last_request.body_kind == "form"
    assert transport.last_request.body == {"mode": "preview"}

    # response-path + response-join on (raw)struct
    log_parsed = m_fetch_td.ChunkedLog.fetch(transport, stream_id="s99").parse()
    assert log_parsed == {"lines": ["line-1", "line-2", "line-3"]}

    # Async fetch
    async def async_handler(req: m_fetch_td.HttpRequest) -> m_fetch_td.RawResponse:
        return sync_handler(req)

    async_transport = MockAsyncTransport(async_handler)
    page_async = asyncio.run(
        m_fetch_td.ArticlePage.async_fetch(async_transport, id=7, sid="s2")
    )
    assert page_async.parse() == {"heading": "Article 7", "summary": "Summary 7"}


def test_via_urllib_against_local_http_server_and_rest_monad() -> None:
    class TestHandler(BaseHTTPRequestHandler):
        def log_message(self, format: str, *args: Any) -> None:
            pass

        def do_GET(self) -> None:
            if self.path.startswith("/articles/"):
                body = b"<html><body><h1>Live HTTP Title</h1><p class='summary'>Live Summary</p></body></html>"
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            elif self.path.startswith("/error404"):
                body = json.dumps({"message": "Not here"}).encode("utf-8")
                self.send_response(404)
                self.send_header("Content-Type", "application/json")
                self.send_header("X-Err-Header", "404-hdr")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
            else:
                body = json.dumps({"ok": True, "Cookie": self.headers.get("Cookie", "")}).encode("utf-8")
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

        def do_POST(self) -> None:
            length = int(self.headers.get("Content-Length", "0"))
            raw = self.rfile.read(length).decode("utf-8")
            self.send_response(201)
            self.send_header("Content-Type", "application/json")
            out = raw.encode("utf-8")
            self.send_header("Content-Length", str(len(out)))
            self.end_headers()
            self.wfile.write(out)

    server = ThreadingHTTPServer(("127.0.0.1", 0), TestHandler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()

    try:
        ut = m_rest_td.via_urllib(timeout=5.0)

        # 1. 200 GET with params & cookies
        r200 = ut.send(
            m_rest_td.HttpRequest(
                method="GET",
                url=f"http://127.0.0.1:{port}/test",
                params={"a": "1", "b": ["x", "y"]},
                cookies={"sid": "abc"},
            )
        )
        assert r200.status == 200
        assert json.loads(r200.text)["Cookie"] == "sid=abc"

        # 2. 201 POST with JSON body
        r201 = ut.send(
            m_rest_td.HttpRequest(
                method="POST",
                url=f"http://127.0.0.1:{port}/post",
                body={"name": "Alice", "nickname": None},
                body_kind="json",
            )
        )
        assert r201.status == 201
        assert json.loads(r201.text) == {"name": "Alice", "nickname": None}

        # 3. 404 HTTPError normalized into RawResponse (not raising TransportError!)
        r404 = ut.send(
            m_rest_td.HttpRequest(
                method="GET",
                url=f"http://127.0.0.1:{port}/error404",
            )
        )
        assert r404.status == 404
        assert r404.headers.get("x-err-header") == "404-hdr"
        assert json.loads(r404.text) == {"message": "Not here"}

        # 4. Connection failure raises TransportError wrapping URLError
        with pytest.raises(m_rest_td.TransportError) as exc_info:
            ut.send(
                m_rest_td.HttpRequest(
                    method="GET",
                    url="http://127.0.0.1:1/unreachable",
                    extra={"timeout": 0.2},
                )
            )
        assert isinstance(exc_info.value.cause, urllib.error.URLError)
    finally:
        server.shutdown()
        server.server_close()


def test_protocol_adapter_factories_httpx_requests_aiohttp() -> None:
    # Verify via_httpx, via_httpx_async, via_requests, via_aiohttp with duck-typed clients
    # (including an independent httpx2-like client/error) and strict exception boundaries.
    class FakeHttpxError(Exception):
        pass

    class FakeHttpx2Error(Exception):
        pass

    class FakeRequestsError(Exception):
        pass

    class FakeAiohttpError(Exception):
        pass

    class FakeResp:
        def __init__(self, status_code: int, headers: dict[str, str], text: str) -> None:
            self.status_code = status_code
            self.headers = headers
            self._text = text

        @property
        def text(self) -> str:
            return self._text

    class FakeAiohttpResp:
        def __init__(
            self,
            status: int,
            headers: dict[str, str],
            body: str,
            text_error: Exception | None = None,
        ) -> None:
            self.status = status
            self.headers = headers
            self._body = body
            self._text_error = text_error

        async def text(self) -> str:
            if self._text_error is not None:
                raise self._text_error
            return self._body

    class FakeAiohttpCtx(AbstractAsyncContextManager[FakeAiohttpResp]):
        def __init__(self, resp: FakeAiohttpResp) -> None:
            self._resp = resp

        async def __aenter__(self) -> FakeAiohttpResp:
            return self._resp

        async def __aexit__(self, exc_type: Any, exc: Any, tb: Any) -> None:
            return None

    class FakeHttpxClient:
        def __init__(self, error: Exception | None = None, lib: str = "httpx") -> None:
            self._error = error
            self._lib = lib

        def request(
            self,
            method: str,
            url: str,
            *,
            content: Any = None,
            data: Any = None,
            json: Any = None,
            params: Any = None,
            headers: Any = None,
            cookies: Any = None,
        ) -> FakeResp:
            if self._error is not None:
                raise self._error
            return FakeResp(200, {"X-Lib": self._lib}, '{"ok": true}')

    class FakeAsyncHttpxClient:
        def __init__(self, error: Exception | None = None, lib: str = "httpx-async") -> None:
            self._error = error
            self._lib = lib

        async def request(
            self,
            method: str,
            url: str,
            *,
            content: Any = None,
            data: Any = None,
            json: Any = None,
            params: Any = None,
            headers: Any = None,
            cookies: Any = None,
        ) -> FakeResp:
            if self._error is not None:
                raise self._error
            return FakeResp(200, {"X-Lib": self._lib}, '{"ok": true}')

    class FakeRequestsSession:
        def __init__(self, error: Exception | None = None) -> None:
            self._error = error

        def request(
            self,
            method: str,
            url: str,
            *,
            params: Any = None,
            data: Any = None,
            headers: Any = None,
            cookies: Any = None,
            json: Any = None,
        ) -> FakeResp:
            if self._error is not None:
                raise self._error
            return FakeResp(200, {"X-Lib": "requests"}, '{"ok": true}')

    class FakeAiohttpSession:
        def __init__(
            self,
            error: Exception | None = None,
            text_error: Exception | None = None,
        ) -> None:
            self._error = error
            self._text_error = text_error

        def request(
            self,
            method: str,
            url: str,
            *,
            params: Any = None,
            data: Any = None,
            headers: Any = None,
            cookies: Any = None,
            json: Any = None,
        ) -> AbstractAsyncContextManager[FakeAiohttpResp]:
            if self._error is not None:
                raise self._error
            return FakeAiohttpCtx(
                FakeAiohttpResp(200, {"X-Lib": "aiohttp"}, '{"ok": true}', self._text_error)
            )

    req = m_rest_td.HttpRequest(method="GET", url="https://api.example.com")

    # 1. via_httpx: success, httpx error, httpx2-like error, TypeError/AttributeError passthrough
    resp = m_rest_td.via_httpx(FakeHttpxClient()).send(req)
    assert resp.status == 200
    assert resp.headers["x-lib"] == "httpx"
    assert resp.text == '{"ok": true}'

    resp_h2 = m_rest_td.via_httpx(FakeHttpxClient(lib="httpx2")).send(req)
    assert resp_h2.headers["x-lib"] == "httpx2"

    with pytest.raises(m_rest_td.TransportError) as httpx_exc:
        m_rest_td.via_httpx(FakeHttpxClient(FakeHttpxError("httpx down"))).send(req)
    assert isinstance(httpx_exc.value.cause, FakeHttpxError)

    with pytest.raises(m_rest_td.TransportError) as httpx2_exc:
        m_rest_td.via_httpx(FakeHttpxClient(FakeHttpx2Error("httpx2 down"))).send(req)
    assert isinstance(httpx2_exc.value.cause, FakeHttpx2Error)

    with pytest.raises(TypeError, match="bad kwarg"):
        m_rest_td.via_httpx(FakeHttpxClient(TypeError("bad kwarg"))).send(req)

    with pytest.raises(AttributeError, match="missing attr"):
        m_rest_td.via_httpx(FakeHttpxClient(AttributeError("missing attr"))).send(req)

    # 2. via_httpx_async: success, httpx/httpx2 errors, TypeError/AttributeError passthrough
    async def _check_async_adapters() -> None:
        aresp = await m_rest_td.via_httpx_async(FakeAsyncHttpxClient()).send(req)
        assert aresp.status == 200
        assert aresp.headers["x-lib"] == "httpx-async"

        with pytest.raises(m_rest_td.TransportError) as ahttpx_exc:
            await m_rest_td.via_httpx_async(
                FakeAsyncHttpxClient(FakeHttpx2Error("async httpx2 down"))
            ).send(req)
        assert isinstance(ahttpx_exc.value.cause, FakeHttpx2Error)

        with pytest.raises(TypeError, match="async bad kwarg"):
            await m_rest_td.via_httpx_async(
                FakeAsyncHttpxClient(TypeError("async bad kwarg"))
            ).send(req)

        with pytest.raises(AttributeError, match="async missing attr"):
            await m_rest_td.via_httpx_async(
                FakeAsyncHttpxClient(AttributeError("async missing attr"))
            ).send(req)

        # 4. via_aiohttp: success, client error, text read error, TypeError/AttributeError passthrough
        aio_resp = await m_rest_td.via_aiohttp(FakeAiohttpSession()).send(req)
        assert aio_resp.status == 200
        assert aio_resp.headers["x-lib"] == "aiohttp"
        assert aio_resp.text == '{"ok": true}'

        with pytest.raises(m_rest_td.TransportError) as aio_exc:
            await m_rest_td.via_aiohttp(
                FakeAiohttpSession(FakeAiohttpError("aiohttp down"))
            ).send(req)
        assert isinstance(aio_exc.value.cause, FakeAiohttpError)

        with pytest.raises(m_rest_td.TransportError) as aio_text_exc:
            await m_rest_td.via_aiohttp(
                FakeAiohttpSession(text_error=FakeAiohttpError("aiohttp read failed"))
            ).send(req)
        assert isinstance(aio_text_exc.value.cause, FakeAiohttpError)

        with pytest.raises(TypeError, match="aiohttp bad kwarg"):
            await m_rest_td.via_aiohttp(
                FakeAiohttpSession(TypeError("aiohttp bad kwarg"))
            ).send(req)

        with pytest.raises(AttributeError, match="aiohttp missing attr"):
            await m_rest_td.via_aiohttp(
                FakeAiohttpSession(AttributeError("aiohttp missing attr"))
            ).send(req)

    asyncio.run(_check_async_adapters())

    # 3. via_requests: success, requests error, TypeError/AttributeError passthrough
    rresp = m_rest_td.via_requests(FakeRequestsSession()).send(req)
    assert rresp.status == 200
    assert rresp.headers["x-lib"] == "requests"

    with pytest.raises(m_rest_td.TransportError) as req_exc:
        m_rest_td.via_requests(FakeRequestsSession(FakeRequestsError("requests down"))).send(req)
    assert isinstance(req_exc.value.cause, FakeRequestsError)

    with pytest.raises(TypeError, match="requests bad kwarg"):
        m_rest_td.via_requests(FakeRequestsSession(TypeError("requests bad kwarg"))).send(req)

    with pytest.raises(AttributeError, match="requests missing attr"):
        m_rest_td.via_requests(FakeRequestsSession(AttributeError("requests missing attr"))).send(req)


def test_html_fetch_http_status_error_and_rest_error_dispatch() -> None:
    for status, body in [(404, "Page not found"), (500, "Internal Server Error")]:
        raw_err = m_fetch_td.RawResponse(
            status=status,
            headers={"x-err": f"status-{status}"},
            text=body,
        )

        def _sync_err(req: m_fetch_td.HttpRequest, _r: m_fetch_td.RawResponse = raw_err) -> m_fetch_td.RawResponse:
            return _r

        with pytest.raises(m_fetch_td.HttpStatusError) as sync_exc:
            m_fetch_td.ArticlePage.fetch(MockSyncTransport(_sync_err), id=7, sid="s1")
        err = sync_exc.value
        assert not isinstance(err, m_fetch_td.TransportError)
        assert err.status == status
        assert err.headers == {"x-err": f"status-{status}"}
        assert err.text == body
        assert err.response == raw_err

        async def _async_err(
            req: m_fetch_td.HttpRequest,
            _r: m_fetch_td.RawResponse = raw_err,
        ) -> m_fetch_td.RawResponse:
            return _r

        with pytest.raises(m_fetch_td.HttpStatusError) as async_exc:
            asyncio.run(
                m_fetch_td.ArticlePage.async_fetch(MockAsyncTransport(_async_err), id=7, sid="s1")
            )
        aerr = async_exc.value
        assert not isinstance(aerr, m_fetch_td.TransportError)
        assert aerr.status == status
        assert aerr.headers == {"x-err": f"status-{status}"}
        assert aerr.text == body
        assert aerr.response == raw_err

    # Meanwhile, (rest)struct endpoints MUST still route 404/500 to @error / UnknownErr
    def _rest_404(req: Any) -> m_rest_td.RawResponse:
        return m_rest_td.RawResponse(
            status=404,
            headers={},
            text=json.dumps({"message": "No such user"}),
        )

    res_404 = m_rest_td.UserApi.get_user(MockSyncTransport(_rest_404), id=99)
    assert isinstance(res_404, m_rest_td.UserApiErr404)
    assert res_404.status == 404
    assert res_404.value == {"message": "No such user"}

    def _rest_500(req: Any) -> m_rest_td.RawResponse:
        return m_rest_td.RawResponse(
            status=500,
            headers={},
            text=json.dumps({"detail": "boom"}),
        )

    res_500 = m_rest_td.UserApi.get_user(MockSyncTransport(_rest_500), id=99)
    assert isinstance(res_500, m_rest_td.UnknownErr)
    assert res_500.status == 500
    assert res_500.body == {"detail": "boom"}


def test_real_http_clients_satisfy_protocols_strict_typing() -> None:
    with httpx.Client() as hc:
        t_httpx: m_rest_td.SyncTransport = m_rest_td.via_httpx(hc)
        t_httpx_sep: parsmith_runtime.SyncTransport = parsmith_runtime.via_httpx(hc)
        assert isinstance(t_httpx, m_rest_td.HttpxTransport)
        assert isinstance(t_httpx_sep, parsmith_runtime.HttpxTransport)

    with requests.Session() as rs:
        t_req: m_rest_td.SyncTransport = m_rest_td.via_requests(rs)
        t_req_sep: parsmith_runtime.SyncTransport = parsmith_runtime.via_requests(rs)
        assert isinstance(t_req, m_rest_td.RequestsTransport)
        assert isinstance(t_req_sep, parsmith_runtime.RequestsTransport)

    async def _check_async_clients() -> None:
        async with httpx.AsyncClient() as ahc:
            t_ahttpx: m_rest_td.AsyncTransport = m_rest_td.via_httpx_async(ahc)
            t_ahttpx_sep: parsmith_runtime.AsyncTransport = parsmith_runtime.via_httpx_async(ahc)
            assert isinstance(t_ahttpx, m_rest_td.AsyncHttpxTransport)
            assert isinstance(t_ahttpx_sep, parsmith_runtime.AsyncHttpxTransport)

        async with aiohttp.ClientSession() as asess:
            t_aio: m_rest_td.AsyncTransport = m_rest_td.via_aiohttp(asess)
            t_aio_sep: parsmith_runtime.AsyncTransport = parsmith_runtime.via_aiohttp(asess)
            assert isinstance(t_aio, m_rest_td.AiohttpTransport)
            assert isinstance(t_aio_sep, parsmith_runtime.AiohttpTransport)

    asyncio.run(_check_async_clients())


def test_rest_monad_ok_errors_unknown_transport_decode_and_programmer_bugs() -> None:
    # 1. 200 OK with TypedDict, Dataclass, and Separate Runtime (-R)
    def ok_handler(req: Any) -> Any:
        return m_rest_td.RawResponse(
            status=200,
            headers={"x-req-id": "r1"},
            text=json.dumps(
                {"id": 10, "name": "Alice", "nickname": None, "undeclared": "ignored"}
            ),
        )

    res_td = m_rest_td.UserApi.get_user(MockSyncTransport(ok_handler), id=10)
    assert isinstance(res_td, m_rest_td.Ok)
    assert res_td.ok is True
    assert res_td.is_ok is True
    assert res_td.status == 200
    assert res_td.headers == {"x-req-id": "r1"}
    assert res_td.value == {"id": 10, "name": "Alice", "nickname": None}

    res_dc = m_rest_dc.UserApi.get_user(MockSyncTransport(ok_handler), id=10)
    assert isinstance(res_dc, m_rest_dc.Ok)
    assert res_dc.ok is True
    assert isinstance(res_dc.value, m_rest_dc.UserJson)
    assert res_dc.value.id == 10
    assert res_dc.value.name == "Alice"
    assert res_dc.value.nickname is None
    assert res_dc.value.role is None

    res_sep = m_rest_sep.UserApi.get_user(MockSyncTransport(ok_handler), id=10)
    assert isinstance(res_sep, parsmith_runtime.Ok)
    assert res_sep.ok is True
    assert res_sep.value == {"id": 10, "name": "Alice", "nickname": None}

    # 2. POST JSON body preserves null for optional placeholder while omitting None params/headers/cookies
    post_tx = MockSyncTransport(ok_handler)
    res_create = m_rest_td.UserApi.create_user(
        post_tx,
        tag="v1",
        sid="s_abc",
        name="Alice",
        active=True,
        headers={"X-Custom": "1"},
    )
    assert isinstance(res_create, m_rest_td.Ok)
    sent_post: m_rest_td.HttpRequest = post_tx.last_request
    assert sent_post.method == "POST"
    assert sent_post.params == {"tag": "v1"}
    assert sent_post.headers == {"Content-Type": "application/json", "X-Custom": "1"}
    assert sent_post.cookies == {"sid": "s_abc"}
    assert sent_post.body_kind == "json"
    assert sent_post.body == {"name": "Alice", "nickname": None, "active": True}

    # 3. Enveloped response-path="data.user" and void endpoint
    def env_handler(req: Any) -> Any:
        return m_rest_td.RawResponse(
            status=200,
            headers={},
            text=json.dumps({"data": {"user": {"id": 5, "name": "Bob", "nickname": "B"}}}),
        )

    res_env = m_rest_td.UserApi.get_profile_enveloped(MockSyncTransport(env_handler))
    assert isinstance(res_env, m_rest_td.Ok)
    assert res_env.value == {"id": 5, "name": "Bob", "nickname": "B"}

    def void_handler(req: Any) -> Any:
        return m_rest_td.RawResponse(status=204, headers={}, text="")

    res_ping = m_rest_td.UserApi.ping(MockSyncTransport(void_handler))
    assert isinstance(res_ping, m_rest_td.Ok)
    assert res_ping.status == 204
    assert res_ping.value is None

    # 4. @error 200 dot-path condition ("error.code")
    def err200_handler(req: Any) -> Any:
        return m_rest_td.RawResponse(
            status=200,
            headers={},
            text=json.dumps({"error": {"code": 1001, "message": "Quota exceeded"}}),
        )

    res_err200 = m_rest_td.UserApi.get_user(MockSyncTransport(err200_handler), id=1)
    assert isinstance(res_err200, m_rest_td.UserApiErr200Code)
    assert res_err200.ok is False
    assert res_err200.status == 200
    assert res_err200.value == {"code": 1001, "message": "Quota exceeded"}

    # 5. @error:not-found 404 SpecificNotFoundError vs fallthrough to @error 404 GenericError
    def err404_specific_handler(req: Any) -> Any:
        return m_rest_td.RawResponse(
            status=404,
            headers={},
            text=json.dumps({"error": "Not Found", "code": 404, "noise": 1}),
        )

    res_404_spec = m_rest_td.UserApi.get_user(MockSyncTransport(err404_specific_handler), id=1)
    assert isinstance(res_404_spec, m_rest_td.UserApiErr404NotFound)
    assert res_404_spec.ok is False
    assert res_404_spec.value == {"error": "Not Found", "code": 404}

    def err404_fallthrough_handler(req: Any) -> Any:
        # Matches error="Not Found" but lacks 'code', so SpecificNotFoundError fails projection and falls through to GenericError
        return m_rest_td.RawResponse(
            status=404,
            headers={},
            text=json.dumps({"error": "Not Found", "message": "Fallback generic 404"}),
        )

    res_404_gen = m_rest_td.UserApi.get_user(MockSyncTransport(err404_fallthrough_handler), id=1)
    assert isinstance(res_404_gen, m_rest_td.UserApiErr404)
    assert res_404_gen.ok is False
    assert res_404_gen.value == {"message": "Fallback generic 404"}

    # 6. UnknownErr on unmatched non-2xx status (e.g. 503)
    def err503_handler(req: Any) -> Any:
        return m_rest_td.RawResponse(
            status=503,
            headers={"retry-after": "10"},
            text=json.dumps({"reason": "maintenance"}),
        )

    res_503 = m_rest_td.UserApi.get_user(MockSyncTransport(err503_handler), id=1)
    assert isinstance(res_503, m_rest_td.UnknownErr)
    assert res_503.ok is False
    assert res_503.status == 503
    assert res_503.body == {"reason": "maintenance"}

    # 7. DecodeErr on 200 OK with malformed JSON or schema mismatch
    def bad_json_200(req: Any) -> Any:
        return m_rest_td.RawResponse(
            status=200,
            headers={"content-type": "text/html"},
            text="<html>502 Bad Gateway disguised as 200</html>",
        )

    res_decode_json = m_rest_td.UserApi.get_user(MockSyncTransport(bad_json_200), id=1)
    assert isinstance(res_decode_json, m_rest_td.DecodeErr)
    assert res_decode_json.ok is False
    assert res_decode_json.status == 200
    assert res_decode_json.body == "<html>502 Bad Gateway disguised as 200</html>"
    assert isinstance(res_decode_json.exception, json.JSONDecodeError)

    def mismatch_200(req: Any) -> Any:
        return m_rest_td.RawResponse(
            status=200,
            headers={},
            text=json.dumps({"id": 10}),  # missing required 'name'
        )

    res_decode_proj = m_rest_td.UserApi.get_user(MockSyncTransport(mismatch_200), id=10)
    assert isinstance(res_decode_proj, m_rest_td.DecodeErr)
    assert res_decode_proj.ok is False
    assert res_decode_proj.status == 200
    assert res_decode_proj.body == {"id": 10}
    assert "Required JSON field 'name'" in res_decode_proj.cause
    assert isinstance(res_decode_proj.exception, m_rest_td.RtJsonFieldMissingError)

    # DecodeErr when matched @error 400 fails its declared ApiError schema contract
    def mismatch_400(req: Any) -> Any:
        return m_rest_td.RawResponse(
            status=400,
            headers={},
            text=json.dumps({"unexpected": "shape"}),
        )

    res_decode_400 = m_rest_td.UserApi.get_user(MockSyncTransport(mismatch_400), id=10)
    assert isinstance(res_decode_400, m_rest_td.DecodeErr)
    assert res_decode_400.ok is False
    assert res_decode_400.status == 400
    assert res_decode_400.body == {"unexpected": "shape"}

    # 8. TransportErr when transport raises TransportError (preserving exception)
    net_cause = ConnectionResetError("peer reset")

    def transport_fail(req: Any) -> Any:
        raise m_rest_td.TransportError(net_cause)

    res_trans = m_rest_td.UserApi.get_user(MockSyncTransport(transport_fail), id=1)
    assert isinstance(res_trans, m_rest_td.TransportErr)
    assert res_trans.ok is False
    assert res_trans.status == 0
    assert res_trans.exception is net_cause
    assert "peer reset" in res_trans.cause

    # 9. Programmer bug (TypeError) in broken transport MUST NOT be swallowed into TransportErr!
    def broken_transport(req: Any) -> Any:
        raise TypeError("programmer bug inside custom transport")

    with pytest.raises(TypeError, match="programmer bug inside custom transport"):
        m_rest_td.UserApi.get_user(MockSyncTransport(broken_transport), id=1)

    # 10. Verify --http-io sync and --http-io async method surfaces
    assert hasattr(m_rest_sync.UserApi, "get_user")
    assert not hasattr(m_rest_sync.UserApi, "async_get_user")
    assert not hasattr(m_rest_async.UserApi, "get_user")
    assert hasattr(m_rest_async.UserApi, "async_get_user")

    async def async_ok(req: Any) -> Any:
        return m_rest_async.RawResponse(
            status=200,
            headers={},
            text=json.dumps([{"id": 1, "title": "Async Article"}]),
        )

    res_async = asyncio.run(
        m_rest_async.UserApi.async_list_articles(MockAsyncTransport(async_ok))
    )
    assert isinstance(res_async, m_rest_async.Ok)
    assert res_async.value == [{"article_id": 1, "headline": "Async Article"}]
"##;
    let test_suite_path = tmp.write("test_json_rest_suite.py", test_suite_py);
    generated_files.push(test_suite_path.clone());

    // 5. Run `uv run mypy --strict` on all generated files + test suite
    let mut mypy_cmd = Command::new("uv");
    mypy_cmd.arg("run").arg("mypy").arg("--strict");
    for file in &generated_files {
        mypy_cmd.arg(file);
    }
    let mypy_out = mypy_cmd.output().expect("execute uv run mypy --strict");
    assert!(
        mypy_out.status.success(),
        "mypy --strict failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&mypy_out.stdout),
        String::from_utf8_lossy(&mypy_out.stderr)
    );

    // 6. Run `uv run pytest` on the generated test suite
    let pytest_out = Command::new("uv")
        .args(["run", "pytest", "-q"])
        .arg(&test_suite_path)
        .env("PYTHONPATH", &tmp.path)
        .output()
        .expect("execute uv run pytest");
    assert!(
        pytest_out.status.success(),
        "pytest failed!\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&pytest_out.stdout),
        String::from_utf8_lossy(&pytest_out.stderr)
    );
}

#[test]
fn python_json_value_constants_typeddict_and_dataclass() {
    let schema = r#"
define SOURCE="github"

json ReleaseCard {
    source str value="{{SOURCE}}"
    channel str value="{{SOURCE}}"
    kind str value="event"
    version int value=1
    weight float value=2.5
    is_live bool value=#false
    id str from="node_id"
}
"#;
    let res_td = generate_source(
        schema,
        Some("release_card.kdl"),
        &CodegenOptions::new(Target::Python).with_models(PythonModels::TypedDict),
    );
    assert!(!res_td.has_errors(), "{:?}", res_td.diagnostics);
    let code_td = res_td.primary_code().unwrap();
    assert!(code_td.contains("'source': 'github',"));
    assert!(code_td.contains("'channel': 'github',"));
    assert!(code_td.contains("'kind': 'event',"));
    assert!(code_td.contains("'version': 1,"));
    assert!(code_td.contains("'weight': 2.5,"));
    assert!(code_td.contains("'is_live': False,"));
    assert!(code_td.contains("'id': rt_json_req(_d, 'node_id', 'ReleaseCard.id'),"));
    assert!(!code_td.contains("rt_json_req(_d, 'source'"));

    let res_dc = generate_source(
        schema,
        Some("release_card_dc.kdl"),
        &CodegenOptions::new(Target::Python).with_models(PythonModels::Dataclass),
    );
    assert!(!res_dc.has_errors(), "{:?}", res_dc.diagnostics);
    let code_dc = res_dc.primary_code().unwrap();
    assert!(code_dc.contains("source='github',"));
    assert!(code_dc.contains("channel='github',"));
    assert!(code_dc.contains("kind='event',"));
    assert!(code_dc.contains("version=1,"));
    assert!(code_dc.contains("weight=2.5,"));
    assert!(code_dc.contains("is_live=False,"));
    assert!(code_dc.contains("id=rt_json_req(_d, 'node_id', 'ReleaseCard.id'),"));
}
