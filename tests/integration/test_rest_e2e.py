"""E2E integration tests for (rest)struct monadic clients against FakeServer across Python and Node.js."""

from __future__ import annotations

import asyncio
import json
from typing import Any
import urllib.error

import aiohttp
import httpx
import httpx2
import pytest
import requests

import parsmith
from tests.fake_server import FakeServer, rewrite_schema_host
from tests.integration.helpers import (
    SCHEMAS_DIR,
    assert_schema_lint_clean,
    loaded_py_module,
    normalize_for_parity,
    run_csharp_custom_program,
    run_elixir_custom_program,
    run_go_custom_program,
    run_js_rest,
    run_kotlin_custom_program,
    run_php_custom_program,
    run_rust_custom_program,
)

REST_SCHEMA_FILES: tuple[str, ...] = (
    "08_rest_basic.kdl",
    "09_rest_void.kdl",
    "10_rest_err_404.kdl",
    "11_rest_err_404_500.kdl",
    "12_rest_err_404_keys.kdl",
    "13_rest_err_200_field.kdl",
    "14_rest_int_placeholder.kdl",
    "15_rest_query_opt.kdl",
    "16_rest_header.kdl",
    "17_rest_post.kdl",
    "20_rest_prefix_form.kdl",
    "21_rest_multi_method.kdl",
    "22_rest_response_path.kdl",
    "24_rest_form_body.kdl",
    "26_multi_rest_namespace.kdl",
    "27_rest_query_params.kdl",
    "28_rest_cookies.kdl",
    "rest_dummyjson.kdl",
    "rest_petstore.kdl",
)


@pytest.mark.parametrize("schema_file", REST_SCHEMA_FILES)
def test_rest_schemas_are_lint_clean(schema_file: str) -> None:
    """Every REST schema in tests/integration/schemas/ passes parsmith.check() with zero errors/warnings."""
    assert_schema_lint_clean(schema_file)


class TestRestSuccessVoidAndDecodeErr:
    """08_rest_basic.kdl, 09_rest_void.kdl, 20_rest_prefix_form.kdl: Ok, headers, void, and DecodeErr."""

    def test_basic_200_ok_across_all_transports(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "08_rest_basic.kdl")
        expected = {"id": 42, "name": "Alice"}

        with loaded_py_module(kdl_src) as (mod, _):
            # 1. via_urllib
            r_url = mod.API.fetch(mod.via_urllib(), id="42")
            assert isinstance(r_url, mod.Ok)
            assert r_url.ok is True
            assert r_url.is_ok is True
            assert r_url.status == 200
            assert r_url.value == expected
            assert r_url.headers.get("x-request-id") == "abc123"

            # 2. via_httpx (httpx & httpx2)
            with httpx.Client() as hclient:
                r_hx = mod.API.fetch(mod.via_httpx(hclient), id="42")
                assert isinstance(r_hx, mod.Ok)
                assert r_hx.value == expected
                assert r_hx.headers.get("x-request-id") == "abc123"

            with httpx2.Client() as h2client:
                r_hx2 = mod.API.fetch(mod.via_httpx(h2client), id="42")
                assert isinstance(r_hx2, mod.Ok)
                assert r_hx2.value == expected
                assert r_hx2.headers.get("x-request-id") == "abc123"

            # 3. via_requests
            with requests.Session() as rsession:
                r_req = mod.API.fetch(mod.via_requests(rsession), id="42")
                assert isinstance(r_req, mod.Ok)
                assert r_req.value == expected
                assert r_req.headers.get("x-request-id") == "abc123"

            # 4. via_httpx_async (httpx & httpx2) & via_aiohttp
            async def _run_async() -> None:
                async with httpx.AsyncClient() as aclient:
                    r_ahx = await mod.API.async_fetch(
                        mod.via_httpx_async(aclient), id="42"
                    )
                    assert isinstance(r_ahx, mod.Ok)
                    assert r_ahx.value == expected
                    assert r_ahx.headers.get("x-request-id") == "abc123"

                async with httpx2.AsyncClient() as aclient2:
                    r_ahx2 = await mod.API.async_fetch(
                        mod.via_httpx_async(aclient2), id="42"
                    )
                    assert isinstance(r_ahx2, mod.Ok)
                    assert r_ahx2.value == expected
                    assert r_ahx2.headers.get("x-request-id") == "abc123"

                async with aiohttp.ClientSession() as asession:
                    r_aio = await mod.API.async_fetch(
                        mod.via_aiohttp(asession), id="42"
                    )
                    assert isinstance(r_aio, mod.Ok)
                    assert r_aio.value == expected
                    assert r_aio.headers.get("x-request-id") == "abc123"

            asyncio.run(_run_async())

        js_res = run_js_rest(kdl_src, "API", "fetch", args=["42"])
        assert js_res["ok"] is True
        assert js_res["kind"] == "ok"
        assert js_res["status"] == 200
        assert js_res["value"] == expected
        assert js_res["headers"].get("x-request-id") == "abc123"

    def test_void_response_returns_none(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "09_rest_void.kdl")
        with loaded_py_module(kdl_src) as (mod, _):
            res = mod.API.fetch(mod.via_urllib(), id="x")
            assert isinstance(res, mod.Ok)
            assert res.is_ok is True
            assert res.value is None

        js_res = run_js_rest(kdl_src, "API", "fetch", args=["x"])
        assert js_res["ok"] is True
        assert js_res["value"] is None

        # Also test DELETE /users/<id> and GET /ping (204 No Content)
        void_methods_kdl = fake_server.rewrite_schema(
            '''
(rest)struct VoidAPI {
    @request name=ping """
    GET /ping HTTP/1.1
    Host: api.example.com
    """
    @request name=delete-user """
    DELETE /users/{{id:int}} HTTP/1.1
    Host: api.example.com
    """
}
'''
        )
        with loaded_py_module(void_methods_kdl) as (vmod, _):
            r_ping = vmod.VoidAPI.ping(vmod.via_urllib())
            assert isinstance(r_ping, vmod.Ok)
            assert r_ping.status == 204
            assert r_ping.value is None

            r_del = vmod.VoidAPI.delete_user(vmod.via_urllib(), id=42)
            assert isinstance(r_del, vmod.Ok)
            assert r_del.status == 204
            assert r_del.value is None
            assert r_del.headers["x-echo-method"] == "DELETE"

        js_ping = run_js_rest(void_methods_kdl, "VoidAPI", "ping")
        assert js_ping["ok"] is True
        assert js_ping["status"] == 204
        assert js_ping["value"] is None

        js_del = run_js_rest(
            void_methods_kdl, "VoidAPI", "deleteUser", args=[42]
        )
        assert js_del["ok"] is True
        assert js_del["status"] == 204
        assert js_del["value"] is None

    def test_prefix_form_schema_20(self, fake_server: FakeServer) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "20_rest_prefix_form.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            r_ok = mod.API.fetch(mod.via_urllib(), id="42")
            assert isinstance(r_ok, mod.Ok)
            assert r_ok.value == {"id": 42, "name": "Alice"}

            r_err = mod.API.fetch(mod.via_urllib(), id="99")
            assert isinstance(r_err, mod.APIErr404)
            assert r_err.is_ok is False
            assert r_err.status == 404
            assert r_err.value == {"code": 404, "message": "not found"}

        js_ok = run_js_rest(kdl_src, "API", "fetch", args=["42"])
        assert js_ok["ok"] is True
        assert js_ok["value"] == {"id": 42, "name": "Alice"}

        js_err = run_js_rest(kdl_src, "API", "fetch", args=["99"])
        assert js_err["ok"] is False
        assert js_err["variant"] == "APIErr404"
        assert js_err["value"] == {"code": 404, "message": "not found"}

    def test_200_null_primitive_html_and_mismatch_return_decode_err(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "08_rest_basic.kdl")
        with loaded_py_module(kdl_src) as (mod, _):
            tx = mod.via_urllib()

            # 1. 200 with JSON null
            r_null = mod.API.fetch(tx, id="null")
            assert isinstance(r_null, mod.DecodeErr)
            assert r_null.is_ok is False
            assert r_null.status == 200
            assert r_null.body is None
            assert "User: Expected dict, got NoneType" in r_null.cause
            assert isinstance(r_null.exception, mod.RtJsonSchemaError)

            # 2. 200 with JSON primitive int
            r_prim = mod.API.fetch(tx, id="prim")
            assert isinstance(r_prim, mod.DecodeErr)
            assert r_prim.status == 200
            assert r_prim.body == 12345
            assert "User: Expected dict, got int" in r_prim.cause
            assert isinstance(r_prim.exception, mod.RtJsonSchemaError)

            # 3. 200 with HTML body (invalid JSON)
            r_bad = mod.API.fetch(tx, id="bad")
            assert isinstance(r_bad, mod.DecodeErr)
            assert r_bad.status == 200
            assert r_bad.body == "<html>not json</html>"
            assert isinstance(r_bad.exception, json.JSONDecodeError)

            # 4. 200 with missing required field
            r_mis = mod.API.fetch(tx, id="mismatch")
            assert isinstance(r_mis, mod.DecodeErr)
            assert r_mis.status == 200
            assert r_mis.body == {"id": 10}
            assert "Required JSON field 'name'" in r_mis.cause
            assert isinstance(r_mis.exception, mod.RtJsonFieldMissingError)

        js_null = run_js_rest(kdl_src, "API", "fetch", args=["null"])
        assert js_null["ok"] is False
        assert js_null["kind"] == "decode"
        assert "User: Expected object, got null" in (js_null["cause"] or "")

        js_prim = run_js_rest(kdl_src, "API", "fetch", args=["prim"])
        assert js_prim["ok"] is False
        assert js_prim["kind"] == "decode"
        assert js_prim["body"] == 12345
        assert "User: Expected object, got number" in (
            js_prim["cause"] or ""
        )

        js_bad = run_js_rest(kdl_src, "API", "fetch", args=["bad"])
        assert js_bad["ok"] is False
        assert js_bad["kind"] == "decode"
        assert js_bad["body"] == "<html>not json</html>"

        js_mis = run_js_rest(kdl_src, "API", "fetch", args=["mismatch"])
        assert js_mis["ok"] is False
        assert js_mis["kind"] == "decode"
        assert js_mis["body"] == {"id": 10}
        assert "Required JSON field 'name'" in (js_mis["cause"] or "")


class TestRestErrorDispatch:
    """10_rest_err_404.kdl, 11_rest_err_404_500.kdl, 12_rest_err_404_keys.kdl, 13_rest_err_200_field.kdl."""

    def test_schema_10_404_and_unknown_429(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "10_rest_err_404.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            r_404 = mod.API.fetch(mod.via_urllib(), id="99")
            assert isinstance(r_404, mod.APIErr404)
            assert r_404.is_ok is False
            assert r_404.status == 404
            assert r_404.value == {"code": 404, "message": "not found"}

            r_429 = mod.API.fetch(mod.via_urllib(), id="429")
            assert isinstance(r_429, mod.UnknownErr)
            assert r_429.is_ok is False
            assert r_429.status == 429
            assert r_429.value == {"error": "rate limited"}

            async def _run_async() -> None:
                async with httpx.AsyncClient() as aclient:
                    ar = await mod.API.async_fetch(
                        mod.via_httpx_async(aclient), id="99"
                    )
                    assert isinstance(ar, mod.APIErr404)
                    assert ar.value == {"code": 404, "message": "not found"}

            asyncio.run(_run_async())

        js_404 = run_js_rest(kdl_src, "API", "fetch", args=["99"])
        assert js_404["ok"] is False
        assert js_404["variant"] == "APIErr404"
        assert js_404["status"] == 404
        assert js_404["value"] == {"code": 404, "message": "not found"}

        js_429 = run_js_rest(kdl_src, "API", "fetch", args=["429"])
        assert js_429["ok"] is False
        assert js_429["kind"] == "unknown"
        assert js_429["status"] == 429
        assert js_429["body"] == {"error": "rate limited"}

    def test_schema_11_404_and_500(self, fake_server: FakeServer) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "11_rest_err_404_500.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            with httpx2.Client() as h2client:
                for tx in (mod.via_urllib(), mod.via_httpx(h2client)):
                    r_ok = mod.API.fetch(tx, id="42")
                    assert isinstance(r_ok, mod.Ok)
                    assert r_ok.status == 200
                    assert r_ok.value == {"id": 42, "name": "Alice"}

                    r_404 = mod.API.fetch(tx, id="99")
                    assert isinstance(r_404, mod.APIErr404)
                    assert r_404.status == 404
                    assert r_404.value == {"code": 404, "message": "not found"}

                    r_500 = mod.API.fetch(tx, id="500")
                    assert isinstance(r_500, mod.APIErr500)
                    assert r_500.status == 500
                    assert r_500.value == {"code": 500, "message": "internal"}

            async def _run_async_httpx2() -> None:
                async with httpx2.AsyncClient() as aclient2:
                    atx = mod.via_httpx_async(aclient2)
                    a_ok = await mod.API.async_fetch(atx, id="42")
                    assert isinstance(a_ok, mod.Ok)
                    assert a_ok.value == {"id": 42, "name": "Alice"}

                    a_404 = await mod.API.async_fetch(atx, id="99")
                    assert isinstance(a_404, mod.APIErr404)
                    assert a_404.status == 404
                    assert a_404.value == {"code": 404, "message": "not found"}

                    a_500 = await mod.API.async_fetch(atx, id="500")
                    assert isinstance(a_500, mod.APIErr500)
                    assert a_500.status == 500
                    assert a_500.value == {"code": 500, "message": "internal"}

            asyncio.run(_run_async_httpx2())

        js_500 = run_js_rest(kdl_src, "API", "fetch", args=["500"])
        assert js_500["ok"] is False
        assert js_500["variant"] == "APIErr500"
        assert js_500["status"] == 500
        assert js_500["value"] == {"code": 500, "message": "internal"}

    def test_schema_12_required_keys_match_and_fallthrough(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "12_rest_err_404_keys.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            r_match = mod.API.fetch(mod.via_urllib(), id="99")
            assert isinstance(r_match, mod.APIErr404ErrorDetail)
            assert r_match.is_ok is False
            assert r_match.value == {"error": "x", "detail": "y"}

            r_miss = mod.API.fetch(mod.via_urllib(), id="404_nokeys")
            assert isinstance(r_miss, mod.UnknownErr)
            assert r_miss.status == 404
            assert r_miss.value == {"message": "not found"}

        js_match = run_js_rest(kdl_src, "API", "fetch", args=["99"])
        assert js_match["variant"] == "APIErr404ErrorDetail"
        assert js_match["value"] == {"error": "x", "detail": "y"}

        js_miss = run_js_rest(
            kdl_src, "API", "fetch", args=["404_nokeys"]
        )
        assert js_miss["kind"] == "unknown"
        assert js_miss["body"] == {"message": "not found"}

    def test_schema_13_200_with_field_condition(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "13_rest_err_200_field.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            r_err = mod.API.fetch(mod.via_urllib(), id="err200")
            assert isinstance(r_err, mod.APIErr200Detail)
            assert r_err.is_ok is False
            assert r_err.status == 200
            assert r_err.value == {"detail": "msg"}

            r_ok = mod.API.fetch(mod.via_urllib(), id="1")
            assert isinstance(r_ok, mod.Ok)
            assert r_ok.value == {"id": 1, "name": "Alice"}

        js_err = run_js_rest(kdl_src, "API", "fetch", args=["err200"])
        assert js_err["ok"] is False
        assert js_err["variant"] == "APIErr200Detail"
        assert js_err["status"] == 200
        assert js_err["value"] == {"detail": "msg"}


class TestRestDotPathAndOrderedFallthrough:
    """Nested dot-path positional key checks and ordered @error fallthrough across all transports."""

    _SEARCH_FALLTHROUGH_SCHEMA = '''
json User {
    id int
    name str
}

json ApiError {
    code int from="error.code"
    message str from="error.message"
}

json SpecificError {
    error str
    code int
}

json GenericError {
    message str
}

(rest)struct SearchAPI {
    @error 200 ApiError "error.code"
    @error:not-found 404 SpecificError error="Ничего не найдено"
    @error 404 GenericError "message"
    @error 400 ApiError

    @request response=User """
    GET /search?mode={{mode:str?}} HTTP/1.1
    Host: api.example.com
    """
}
'''

    def test_dotpath_and_ordered_fallthrough_across_all_transports(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(self._SEARCH_FALLTHROUGH_SCHEMA)

        with loaded_py_module(kdl_src) as (mod, _):
            with (
                httpx.Client() as hclient,
                requests.Session() as rsession,
            ):
                for tx in (
                    mod.via_urllib(),
                    mod.via_httpx(hclient),
                    mod.via_requests(rsession),
                ):
                    # 1. Nested positional key "error.code" matches 200 error payload
                    r_err200 = mod.SearchAPI.fetch(tx, mode="err200")
                    assert isinstance(r_err200, mod.SearchAPIErr200Code)
                    assert r_err200.is_ok is False
                    assert r_err200.status == 200
                    assert r_err200.value == {
                        "code": 1001,
                        "message": "Quota exceeded",
                    }

                    # 2. Nested positional key "error.code" rejects {"error": {}} and {} on 200 -> Ok
                    r_ok_empty = mod.SearchAPI.fetch(tx, mode="ok_empty_err")
                    assert isinstance(r_ok_empty, mod.Ok)
                    assert r_ok_empty.value == {"id": 1, "name": "Alice"}

                    r_ok_def = mod.SearchAPI.fetch(tx, mode=None)
                    assert isinstance(r_ok_def, mod.Ok)
                    assert r_ok_def.value == {"id": 0, "name": ""}

                    # 3. Specific @error:not-found 404 matches when condition and schema both succeed
                    r_spec = mod.SearchAPI.fetch(tx, mode="404_specific")
                    assert isinstance(r_spec, mod.SearchAPIErr404NotFound)
                    assert r_spec.value == {
                        "error": "Ничего не найдено",
                        "code": 404,
                    }

                    # 4. Condition matches SpecificError, but projection fails (missing 'code') -> falls through to GenericError
                    r_fall = mod.SearchAPI.fetch(tx, mode="404_fallthrough")
                    assert isinstance(r_fall, mod.SearchAPIErr404Message)
                    assert r_fall.value == {
                        "message": "Fallback generic message"
                    }

                    # 5. Empty JSON `{}` fails 404 matchers -> falls through to UnknownErr
                    r_empty = mod.SearchAPI.fetch(tx, mode="404_empty")
                    assert isinstance(r_empty, mod.UnknownErr)
                    assert r_empty.status == 404
                    assert r_empty.value == {}

                    # 6. HTML 404 page -> falls through to UnknownErr
                    r_html = mod.SearchAPI.fetch(tx, mode="404_html")
                    assert isinstance(r_html, mod.UnknownErr)
                    assert r_html.status == 404
                    assert (
                        r_html.value == "<html><body>404 Not Found</body></html>"
                    )

                    # 7. Unconditional @error 400 with mismatching JSON body -> DecodeErr
                    r_400_mis = mod.SearchAPI.fetch(tx, mode="400_mismatch")
                    assert isinstance(r_400_mis, mod.DecodeErr)
                    assert r_400_mis.status == 400
                    assert r_400_mis.body == {"unexpected": "shape"}

            async def _run_async() -> None:
                async with (
                    httpx.AsyncClient() as aclient,
                    aiohttp.ClientSession() as asession,
                ):
                    for atx in (
                        mod.via_httpx_async(aclient),
                        mod.via_aiohttp(asession),
                    ):
                        r1 = await mod.SearchAPI.async_fetch(
                            atx, mode="err200"
                        )
                        assert isinstance(r1, mod.SearchAPIErr200Code)

                        r2 = await mod.SearchAPI.async_fetch(
                            atx, mode="404_fallthrough"
                        )
                        assert isinstance(r2, mod.SearchAPIErr404Message)
                        assert r2.value == {
                            "message": "Fallback generic message"
                        }

                        r3 = await mod.SearchAPI.async_fetch(
                            atx, mode="404_empty"
                        )
                        assert isinstance(r3, mod.UnknownErr)
                        assert r3.value == {}

                        r4 = await mod.SearchAPI.async_fetch(
                            atx, mode="404_html"
                        )
                        assert isinstance(r4, mod.UnknownErr)
                        assert r4.status == 404

            asyncio.run(_run_async())

        js_err200 = run_js_rest(
            kdl_src, "SearchAPI", "fetch", args=[{"mode": "err200"}]
        )
        assert js_err200["variant"] == "SearchAPIErr200Code"
        assert js_err200["value"] == {
            "code": 1001,
            "message": "Quota exceeded",
        }

        js_ok_empty = run_js_rest(
            kdl_src, "SearchAPI", "fetch", args=[{"mode": "ok_empty_err"}]
        )
        assert js_ok_empty["ok"] is True
        assert js_ok_empty["value"] == {"id": 1, "name": "Alice"}

        js_spec = run_js_rest(
            kdl_src, "SearchAPI", "fetch", args=[{"mode": "404_specific"}]
        )
        assert js_spec["variant"] == "SearchAPIErr404NotFound"

        js_fall = run_js_rest(
            kdl_src,
            "SearchAPI",
            "fetch",
            args=[{"mode": "404_fallthrough"}],
        )
        assert js_fall["variant"] == "SearchAPIErr404Message"
        assert js_fall["value"] == {"message": "Fallback generic message"}

        js_empty = run_js_rest(
            kdl_src, "SearchAPI", "fetch", args=[{"mode": "404_empty"}]
        )
        assert js_empty["kind"] == "unknown"
        assert js_empty["body"] == {}

        js_html = run_js_rest(
            kdl_src, "SearchAPI", "fetch", args=[{"mode": "404_html"}]
        )
        assert js_html["kind"] == "unknown"
        assert js_html["status"] == 404

        js_400 = run_js_rest(
            kdl_src, "SearchAPI", "fetch", args=[{"mode": "400_mismatch"}]
        )
        assert js_400["kind"] == "decode"
        assert js_400["status"] == 400


class TestRestPlaceholdersHeadersCookiesFormAndMultiMethod:
    """Schemas 14..17, 21, 22, 24_rest_form_body, 26, 27, 28 against FakeServer."""

    def test_schema_14_int_placeholder(self, fake_server: FakeServer) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "14_rest_int_placeholder.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            res = mod.API.fetch(mod.via_urllib(), id=7)
            assert isinstance(res, mod.Ok)
            assert res.value == {"id": 7, "name": "Item"}
            assert res.headers["x-echo-path"] == "/items/7"

        js_res = run_js_rest(kdl_src, "API", "fetch", args=[7])
        assert js_res["ok"] is True
        assert js_res["value"] == {"id": 7, "name": "Item"}
        assert js_res["headers"]["x-echo-path"] == "/items/7"

    def test_schema_15_optional_query_param(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "15_rest_query_opt.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            r_none = mod.API.fetch(mod.via_urllib(), q=None)
            assert isinstance(r_none, mod.Ok)
            assert r_none.headers["x-echo-query"] == ""

            r_val = mod.API.fetch(mod.via_urllib(), q="hello")
            assert isinstance(r_val, mod.Ok)
            assert r_val.value == {"id": 0, "name": "hello"}
            assert r_val.headers["x-echo-query"] == "q=hello"

        js_none = run_js_rest(kdl_src, "API", "fetch", args=[{}])
        assert js_none["ok"] is True
        assert js_none["headers"]["x-echo-query"] == ""

        js_val = run_js_rest(
            kdl_src, "API", "fetch", args=[{"q": "hello"}]
        )
        assert js_val["ok"] is True
        assert js_val["value"] == {"id": 0, "name": "hello"}
        assert js_val["headers"]["x-echo-query"] == "q=hello"

    def test_schema_16_header_placeholder_and_per_call_kwargs(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "16_rest_header.kdl")
        with loaded_py_module(kdl_src) as (mod, _):
            with httpx.Client() as client:
                orig_headers = dict(client.headers)
                res = mod.API.fetch(
                    mod.via_httpx(client),
                    token="secret123",
                    headers={"X-Custom": "yes"},
                )
                assert dict(client.headers) == orig_headers

            assert isinstance(res, mod.Ok)
            echo_hdrs = json.loads(res.headers["x-echo-headers"])
            assert echo_hdrs["authorization"] == "Bearer secret123"
            assert echo_hdrs["x-custom"] == "yes"

            async def _run_async() -> None:
                async with httpx.AsyncClient() as aclient:
                    ares = await mod.API.async_fetch(
                        mod.via_httpx_async(aclient),
                        token="secret_async",
                        headers={"X-Custom": "async"},
                    )
                    assert isinstance(ares, mod.Ok)
                    ahdrs = json.loads(ares.headers["x-echo-headers"])
                    assert ahdrs["authorization"] == "Bearer secret_async"
                    assert ahdrs["x-custom"] == "async"

            asyncio.run(_run_async())

        js_res = run_js_rest(
            kdl_src,
            "API",
            "fetch",
            args=[
                {"token": "secret_js"},
                {"headers": {"X-Custom": "from_js"}},
            ],
        )
        assert js_res["ok"] is True
        js_hdrs = json.loads(js_res["headers"]["x-echo-headers"])
        assert js_hdrs["authorization"] == "Bearer secret_js"
        assert js_hdrs["x-custom"] == "from_js"

    def test_schema_17_post_json_and_schema_24_post_form(
        self, fake_server: FakeServer
    ) -> None:
        # 1. 17_rest_post.kdl (JSON POST)
        kdl_json = fake_server.rewrite_schema(SCHEMAS_DIR / "17_rest_post.kdl")
        with loaded_py_module(kdl_json) as (mod_json, _):
            res = mod_json.API.fetch(
                mod_json.via_urllib(), name="NewUser", active=True
            )
            assert isinstance(res, mod_json.Ok)
            assert res.status == 201
            assert res.value == {"id": 99, "name": "NewUser"}
            sent_body = json.loads(json.loads(res.headers["x-echo-body"]))
            assert sent_body == {"name": "NewUser", "active": True}

        js_res = run_js_rest(
            kdl_json,
            "API",
            "fetch",
            args=[{"name": "NewUser", "active": True}],
        )
        assert js_res["ok"] is True
        assert js_res["value"] == {"id": 99, "name": "NewUser"}

        # 2. 24_rest_form_body.kdl (Form POST)
        kdl_form = fake_server.rewrite_schema(
            SCHEMAS_DIR / "24_rest_form_body.kdl"
        )
        with loaded_py_module(kdl_form) as (mod_form, _):
            with requests.Session() as rsession:
                for tx in (
                    mod_form.via_urllib(),
                    mod_form.via_requests(rsession),
                ):
                    f_res = mod_form.API.fetch(tx, name="FormUser", active=True)
                    assert isinstance(f_res, mod_form.Ok)
                    assert f_res.value == {"id": 99, "name": "FormUser"}
                    raw_echo = json.loads(f_res.headers["x-echo-body"])
                    assert "name=FormUser" in raw_echo

        js_form = run_js_rest(
            kdl_form,
            "API",
            "fetch",
            args=[{"name": "FormUser", "active": True}],
        )
        assert js_form["ok"] is True
        assert js_form["value"] == {"id": 99, "name": "FormUser"}

    def test_schema_21_multi_method_and_shared_error(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "21_rest_multi_method.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            tx = mod.via_urllib()
            r_get = mod.API.get_user(tx, id="7")
            assert isinstance(r_get, mod.Ok)
            assert r_get.value == {"id": 7, "name": "Alice"}

            r_list = mod.API.list_users(tx)
            assert isinstance(r_list, mod.Ok)
            assert r_list.value == {"id": 0, "name": ""}

            e1 = mod.API.get_user(tx, id="99")
            e2 = mod.API.list_users(tx, params={"status": "404"})
            assert isinstance(e1, mod.APIErr404)
            assert isinstance(e2, mod.APIErr404)

        js_get = run_js_rest(kdl_src, "API", "getUser", args=["7"])
        assert js_get["ok"] is True
        assert js_get["value"] == {"id": 7, "name": "Alice"}

        js_list = run_js_rest(kdl_src, "API", "listUsers")
        assert js_list["ok"] is True
        assert js_list["value"] == {"id": 0, "name": ""}

    def test_schema_22_response_path_and_non_json_content_types(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "22_rest_response_path.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            # 1. Standard 200 OK extracts data.user
            r_ok = mod.API.fetch(mod.via_urllib())
            assert isinstance(r_ok, mod.Ok)
            assert r_ok.value == {"id": 1, "name": "TestUser"}

            # 2. Non-JSON Content-Type (text/javascript & application/octet-stream) on aiohttp & httpx
            async def _run_async() -> None:
                async with aiohttp.ClientSession() as asession:
                    atx = mod.via_aiohttp(asession)
                    for ct in ("text/javascript", "application/octet-stream"):
                        r_ct = await mod.API.async_fetch(
                            atx, params={"ct": ct}
                        )
                        assert isinstance(r_ct, mod.Ok)
                        assert r_ct.value == {"id": 1, "name": "TestUser"}

                    # 3. 404 error matcher / UnknownErr sees full envelope, not narrowed response-path
                    r_404 = await mod.API.async_fetch(
                        atx,
                        params={"status": "404", "ct": "text/javascript"},
                    )
                    assert isinstance(r_404, mod.UnknownErr)
                    assert r_404.status == 404
                    assert r_404.value == {
                        "data": {"user": {"id": 9, "name": "TestUser"}},
                        "code": 404,
                    }

            asyncio.run(_run_async())

        js_ok = run_js_rest(kdl_src, "API", "fetch")
        assert js_ok["ok"] is True
        assert js_ok["value"] == {"id": 1, "name": "TestUser"}

        js_404 = run_js_rest(
            kdl_src,
            "API",
            "fetch",
            args=[{"params": {"status": "404", "ct": "text/javascript"}}],
        )
        assert js_404["ok"] is False
        assert js_404["kind"] == "unknown"
        assert js_404["body"] == {
            "data": {"user": {"id": 9, "name": "TestUser"}},
            "code": 404,
        }

    def test_schemas_26_27_28_multi_namespace_query_params_and_cookies(
        self, fake_server: FakeServer
    ) -> None:
        # 26_multi_rest_namespace.kdl
        kdl_26 = fake_server.rewrite_schema(
            SCHEMAS_DIR / "26_multi_rest_namespace.kdl"
        )
        with loaded_py_module(kdl_26) as (mod26, _):
            r_first = mod26.First.fetch(mod26.via_urllib())
            r_second = mod26.Second.fetch(mod26.via_urllib())
            assert isinstance(r_first, mod26.Ok) and r_first.value == {"a": 10}
            assert isinstance(r_second, mod26.Ok) and r_second.value == {
                "b": "hello"
            }

        # 27_rest_query_params.kdl
        kdl_27 = fake_server.rewrite_schema(
            SCHEMAS_DIR / "27_rest_query_params.kdl"
        )
        with loaded_py_module(kdl_27) as (mod27, _):
            r_27 = mod27.API.fetch(mod27.via_urllib(), page=2, limit=5)
            assert isinstance(r_27, mod27.Ok)
            assert r_27.value == {"x": 25}
            assert r_27.headers["x-echo-query"] == "page=2&limit=5"

        # 28_rest_cookies.kdl
        kdl_28 = fake_server.rewrite_schema(
            SCHEMAS_DIR / "28_rest_cookies.kdl"
        )
        with loaded_py_module(kdl_28) as (mod28, _):
            r_28 = mod28.API.fetch(mod28.via_urllib())
            assert isinstance(r_28, mod28.Ok)
            assert r_28.value == {"x": 42}
            cookies_echo = json.loads(r_28.headers["x-echo-cookies"])
            assert cookies_echo == {"session": "abc"}


class TestRestWireFormattingAndDelegation:
    """Verify None param/header/cookie omission, JSON null preservation, array styles, and kwargs merge."""

    _DELEGATION_SCHEMA = '''
json User {
    id int
    name str
    nickname str?
}

(rest)struct UserAPI {
    @request name=update-user response=User """
    POST /users?tag={{tag:str}}&filter={{filter:str?}}&csv_tags={{csv_tags:str[]?|csv}}&pipe_ids={{pipe_ids:int[]?|pipe}}&space_words={{space_words:str[]?|space}}&bracket_nums={{bracket_nums:int[]?|bracket}}&repeat_vals={{repeat_vals:str[]?}} HTTP/1.1
    Host: api.example.com
    Content-Type: application/json
    X-App: ssc-client
    X-Trace: {{trace:str?}}
    Cookie: sid={{sid:str}}; pref={{pref:str?}}

    {"name": "{{name:str}}", "nickname": {{nickname:str?}}}
    """
}
'''

    def test_json_null_preserved_while_none_omitted_and_array_styles_encoded(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(self._DELEGATION_SCHEMA)

        with loaded_py_module(kdl_src) as (mod, _):
            res = mod.UserAPI.update_user(
                mod.via_urllib(),
                tag="v1",
                sid="s123",
                name="Alice",
                csv_tags=["a", "b"],
                pipe_ids=[1, 2],
                space_words=["hello", "world"],
                bracket_nums=[10, 20],
                repeat_vals=["r1", "r2"],
                headers={"X-Custom": "from-caller"},
                params={"extra": "yes"},
                cookies={"c_extra": "cval"},
            )
            assert isinstance(res, mod.Ok)
            assert res.value == {"id": 99, "name": "Alice", "nickname": None}

            echo_params = json.loads(res.headers["x-echo-params"])
            assert echo_params == {
                "tag": ["v1"],
                "csv_tags": ["a,b"],
                "pipe_ids": ["1|2"],
                "space_words": ["hello world"],
                "bracket_nums[]": ["10", "20"],
                "repeat_vals": ["r1", "r2"],
                "extra": ["yes"],
            }
            assert "filter" not in echo_params

            echo_headers = json.loads(res.headers["x-echo-headers"])
            assert echo_headers["x-app"] == "ssc-client"
            assert echo_headers["x-custom"] == "from-caller"
            assert "x-trace" not in echo_headers

            echo_cookies = json.loads(res.headers["x-echo-cookies"])
            assert echo_cookies == {"sid": "s123", "c_extra": "cval"}
            assert "pref" not in echo_cookies

            sent_body = json.loads(json.loads(res.headers["x-echo-body"]))
            assert sent_body == {"name": "Alice", "nickname": None}

        js_res = run_js_rest(
            kdl_src,
            "UserAPI",
            "updateUser",
            args=[
                {
                    "tag": "v1",
                    "sid": "s123",
                    "name": "Alice",
                    "csvTags": ["a", "b"],
                    "pipeIds": [1, 2],
                    "spaceWords": ["hello", "world"],
                    "bracketNums": [10, 20],
                    "repeatVals": ["r1", "r2"],
                },
                {
                    "headers": {"X-Custom": "from-caller"},
                    "params": {"extra": "yes"},
                    "cookies": {"c_extra": "cval"},
                },
            ],
        )
        assert js_res["ok"] is True
        assert js_res["value"] == {
            "id": 99,
            "name": "Alice",
            "nickname": None,
        }
        js_params = json.loads(js_res["headers"]["x-echo-params"])
        assert js_params["csv_tags"] == ["a,b"]
        assert js_params["pipe_ids"] == ["1|2"]
        assert js_params["space_words"] == ["hello world"]
        assert js_params["bracket_nums[]"] == ["10", "20"]
        assert js_params["repeat_vals"] == ["r1", "r2"]
        assert js_params["extra"] == ["yes"]
        assert "filter" not in js_params

        js_body = json.loads(json.loads(js_res["headers"]["x-echo-body"]))
        assert js_body == {"name": "Alice", "nickname": None}


class TestRestAliasedRemappingNestedDictsAndModes:
    """JSON key alias remapping, nested (dict)json @value blocks, Aniliberty, dataclass, separate_runtime, http_io."""

    _ALIASED_REST_SCHEMA = '''
json AuthorInfo {
    author_name str from="name"
    author_slug str from="slug"
}

(array)json ArticleList {
    article_id int from="id"
    headline str from="title"
    author AuthorInfo
    tags (array)str from="category_tags"
}

json ValidationErr {
    error_code str from="code"
    error_message str from="detail"
}

(rest)struct ArticleAPI {
    @error 400 ValidationErr
    @request response=ArticleList """
    GET /articles HTTP/1.1
    Host: api.example.com
    """
}
'''

    _NESTED_DICT_REST_SCHEMA = '''
json AnimeResponse {
    translations (dict) {
        @key str
        @value {
            is_active bool
            episodes (dict)EpisodeMap {
                @key int
                @value EpisodeValue {
                    link str from="stream_url"
                    bitrate int?
                    secret_token @skip
                }
            }
        }
    }
}

json Err {
    code int
    message str
}

(rest)struct AnimeAPI {
    @request response=AnimeResponse """
    GET /anime/{{id:int}} HTTP/1.1
    Host: api.example.com
    """
    @error 404 Err
}
'''

    def test_aliased_rest_across_transports_and_modes(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(self._ALIASED_REST_SCHEMA)
        expected_ok = [
            {
                "article_id": 101,
                "headline": "Breaking News",
                "author": {
                    "author_name": "Jane Doe",
                    "author_slug": "jane-doe",
                },
                "tags": ["news", "tech"],
            }
        ]

        # 1. TypedDict mode across all 5 Python transports
        with loaded_py_module(kdl_src) as (mod, _):
            with (
                httpx.Client() as hclient,
                requests.Session() as rsession,
            ):
                for tx in (
                    mod.via_urllib(),
                    mod.via_httpx(hclient),
                    mod.via_requests(rsession),
                ):
                    r_ok = mod.ArticleAPI.fetch(tx)
                    assert isinstance(r_ok, mod.Ok)
                    assert r_ok.value == expected_ok

                    r_400 = mod.ArticleAPI.fetch(tx, params={"err": "400"})
                    assert isinstance(r_400, mod.ArticleAPIErr400)
                    assert r_400.value == {
                        "error_code": "INVALID_QUERY",
                        "error_message": "Limit out of bounds",
                    }

                    r_500 = mod.ArticleAPI.fetch(tx, params={"err": "500"})
                    assert isinstance(r_500, mod.UnknownErr)
                    assert r_500.status == 500
                    assert r_500.value == {
                        "raw_error": "Internal server crash"
                    }

            async def _run_async() -> None:
                async with (
                    httpx.AsyncClient() as aclient,
                    aiohttp.ClientSession() as asession,
                ):
                    for atx in (
                        mod.via_httpx_async(aclient),
                        mod.via_aiohttp(asession),
                    ):
                        r_ok = await mod.ArticleAPI.async_fetch(atx)
                        assert isinstance(r_ok, mod.Ok)
                        assert r_ok.value == expected_ok

            asyncio.run(_run_async())

        # 2. Dataclass + separate_runtime=True
        with loaded_py_module(
            kdl_src, models="dataclass", separate_runtime=True
        ) as (mod_dc, rt_mod):
            assert rt_mod is not None
            r_dc = mod_dc.ArticleAPI.fetch(rt_mod.via_urllib())
            assert isinstance(r_dc, rt_mod.Ok)
            assert normalize_for_parity(r_dc.value) == expected_ok

        # 3. http_io="sync" and http_io="async"
        with loaded_py_module(kdl_src, http_io="sync") as (mod_sync, _):
            assert hasattr(mod_sync.ArticleAPI, "fetch")
            assert not hasattr(mod_sync.ArticleAPI, "async_fetch")

        with loaded_py_module(kdl_src, http_io="async") as (mod_async, _):
            assert not hasattr(mod_async.ArticleAPI, "fetch")
            assert hasattr(mod_async.ArticleAPI, "async_fetch")

        # 4. Node.js viaFetch
        js_ok = run_js_rest(kdl_src, "ArticleAPI", "fetch")
        assert js_ok["ok"] is True
        assert js_ok["value"] == expected_ok

        js_400 = run_js_rest(
            kdl_src,
            "ArticleAPI",
            "fetch",
            args=[{"params": {"err": "400"}}],
        )
        assert js_400["variant"] == "ArticleAPIErr400"
        assert js_400["value"] == {
            "error_code": "INVALID_QUERY",
            "error_message": "Limit out of bounds",
        }

    def test_nested_dict_value_blocks_in_rest(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(self._NESTED_DICT_REST_SCHEMA)
        expected = {
            "translations": {
                "sub_en": {
                    "is_active": True,
                    "episodes": {
                        "1": {
                            "link": "https://stream.example/1",
                            "bitrate": 1080,
                        },
                        "2": {
                            "link": "https://stream.example/2",
                            "bitrate": None,
                        },
                    },
                }
            }
        }

        with loaded_py_module(kdl_src) as (mod, _):
            res = mod.AnimeAPI.fetch(mod.via_urllib(), id=42)
            assert isinstance(res, mod.Ok)
            assert res.value == expected

            err_res = mod.AnimeAPI.fetch(mod.via_urllib(), id=404)
            assert isinstance(err_res, mod.AnimeAPIErr404)
            assert err_res.value == {"code": 404, "message": "Anime not found"}

        with loaded_py_module(kdl_src, models="dataclass") as (mod_dc, _):
            res_dc = mod_dc.AnimeAPI.fetch(mod_dc.via_urllib(), id=42)
            assert isinstance(res_dc, mod_dc.Ok)
            assert normalize_for_parity(res_dc.value) == expected

        js_res = run_js_rest(kdl_src, "AnimeAPI", "fetch", args=[42])
        assert js_res["ok"] is True
        assert js_res["value"] == expected


class TestDummyJsonAndPetStoreE2E:
    """Full end-to-end execution of rest_dummyjson.kdl and rest_petstore.kdl against FakeServer."""

    def test_dummyjson_api_all_endpoints_and_errors(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "rest_dummyjson.kdl")

        with loaded_py_module(kdl_src) as (mod, _):
            tx = mod.via_urllib()

            # 1. get_product (200 OK & 404 Error)
            r_prod = mod.DummyJsonApi.get_product(tx, id=1)
            assert isinstance(r_prod, mod.Ok)
            assert r_prod.value["id"] == 1
            assert r_prod.value["title"] == "Essence Mascara Lash Princess"
            assert r_prod.value["discount_percentage"] == 10.48
            assert r_prod.value["dimensions"] == {
                "width": 15.14,
                "height": 13.08,
                "depth": 22.99,
            }
            assert r_prod.value["reviews"][0]["reviewer_name"] == "Alice"
            assert "undeclaredNoise" not in r_prod.value

            r_404 = mod.DummyJsonApi.get_product(tx, id=999)
            assert isinstance(r_404, mod.DummyJsonApiErr404)
            assert r_404.status == 404
            assert (
                r_404.value["message"] == "Product with id '999' not found"
            )

            # 2. list_products with csv array placeholder
            r_list = mod.DummyJsonApi.list_products(
                tx, limit=10, skip=5, fields=["title", "price"]
            )
            assert isinstance(r_list, mod.Ok)
            assert r_list.value["total"] == 2
            assert r_list.value["limit"] == 10
            assert r_list.value["skip"] == 5
            echo_params = json.loads(r_list.headers["x-echo-params"])
            assert echo_params["select"] == ["title,price"]

            # 3. search_products (200 OK, 400 Err, 500 Err)
            r_search = mod.DummyJsonApi.search_products(
                tx, query="mascara", limit=5
            )
            assert isinstance(r_search, mod.Ok)
            assert (
                r_search.value["products"][0]["title"]
                == "Result for mascara"
            )

            r_s400 = mod.DummyJsonApi.search_products(tx, query="error400")
            assert isinstance(r_s400, mod.DummyJsonApiErr400)
            assert r_s400.value == {"message": "Invalid search query"}

            r_s500 = mod.DummyJsonApi.search_products(tx, query="error500")
            assert isinstance(r_s500, mod.DummyJsonApiErr500)
            assert r_s500.value == {"message": "Search backend failure"}

            # 4. products_by_category
            r_cat = mod.DummyJsonApi.products_by_category(
                tx, category="laptops"
            )
            assert isinstance(r_cat, mod.Ok)
            assert r_cat.value["products"][0]["category"] == "laptops"

            r_cat_404 = mod.DummyJsonApi.products_by_category(
                tx, category="unknown"
            )
            assert isinstance(r_cat_404, mod.DummyJsonApiErr404)

            # 5. list_categories
            r_cats = mod.DummyJsonApi.list_categories(tx)
            assert isinstance(r_cats, mod.Ok)
            assert len(r_cats.value) == 2
            assert r_cats.value[0]["slug"] == "beauty"

            # 6. create_product (POST JSON)
            r_create = mod.DummyJsonApi.create_product(
                tx, title="Luxury Watch", price=199.99, stock=5
            )
            assert isinstance(r_create, mod.Ok)
            assert r_create.status == 201
            assert r_create.value["id"] == 195
            assert r_create.value["title"] == "Luxury Watch"
            assert r_create.value["price"] == 199.99
            assert r_create.value["stock"] == 5

        js_prod = run_js_rest(
            kdl_src, "DummyJsonApi", "getProduct", args=[1]
        )
        assert js_prod["ok"] is True
        assert js_prod["value"]["discount_percentage"] == 10.48

        js_404 = run_js_rest(
            kdl_src, "DummyJsonApi", "getProduct", args=[999]
        )
        assert js_404["ok"] is False
        assert js_404["variant"] == "DummyJsonApiErr404"

        js_create = run_js_rest(
            kdl_src,
            "DummyJsonApi",
            "createProduct",
            args=[
                {"title": "JS Product", "price": 29.5, "stock": 15}
            ],
        )
        assert js_create["ok"] is True
        assert js_create["value"]["title"] == "JS Product"

    def test_petstore_api_all_endpoints_and_error_variants(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "rest_petstore.kdl")

        with loaded_py_module(kdl_src) as (mod, _):
            tx = mod.via_urllib()

            # 1. list_pets
            r_list = mod.PetStoreApi.list_pets(tx, page=2, per_page=10)
            assert isinstance(r_list, mod.Ok)
            assert len(r_list.value["pets"]) == 2
            assert r_list.value["pets"][0]["name"] == "Bella"
            assert r_list.value["pets"][1]["breed"] is None
            assert r_list.value["pagination"] == {
                "total": 2,
                "page": 2,
                "per_page": 10,
                "total_pages": 1,
            }

            # 2. search_pets (200 OK & 422 Err)
            r_dogs = mod.PetStoreApi.search_pets(tx, species="dog")
            assert isinstance(r_dogs, mod.Ok)
            assert len(r_dogs.value["pets"]) == 1
            assert r_dogs.value["pets"][0]["name"] == "Bella"

            r_422 = mod.PetStoreApi.search_pets(tx, species="invalid")
            assert isinstance(r_422, mod.PetStoreApiErr422)
            assert r_422.status == 422
            assert r_422.value == {
                "message": "Unsupported species filter",
                "code": "UNPROCESSABLE_SPECIES",
            }

            # 3. get_pet (200 OK, 400, 404, 422, 500, and UnknownErr on 429/503)
            r_pet = mod.PetStoreApi.get_pet(tx, id=1)
            assert isinstance(r_pet, mod.Ok)
            assert r_pet.value["name"] == "Bella"
            assert r_pet.value["owner"]["full_name"] == "Sarah Connor"
            assert len(r_pet.value["vet_records"]) == 1

            assert isinstance(
                mod.PetStoreApi.get_pet(tx, id=400), mod.PetStoreApiErr400
            )
            assert isinstance(
                mod.PetStoreApi.get_pet(tx, id=404), mod.PetStoreApiErr404
            )
            assert isinstance(
                mod.PetStoreApi.get_pet(tx, id=422), mod.PetStoreApiErr422
            )
            assert isinstance(
                mod.PetStoreApi.get_pet(tx, id=500), mod.PetStoreApiErr500
            )

            r_429 = mod.PetStoreApi.get_pet(tx, id=429)
            assert isinstance(r_429, mod.UnknownErr)
            assert r_429.status == 429

            r_503 = mod.PetStoreApi.get_pet(tx, id=503)
            assert isinstance(r_503, mod.UnknownErr)
            assert r_503.status == 503
            assert r_503.value == {"reason": "Service unavailable"}

        # Verify Dataclass mode on PetStore
        with loaded_py_module(kdl_src, models="dataclass") as (mod_dc, _):
            r_dc = mod_dc.PetStoreApi.get_pet(mod_dc.via_urllib(), id=1)
            assert isinstance(r_dc, mod_dc.Ok)
            assert isinstance(r_dc.value, mod_dc.PetDetailJson)
            assert r_dc.value.name == "Bella"
            assert r_dc.value.owner is not None
            assert r_dc.value.owner.full_name == "Sarah Connor"

        js_pet = run_js_rest(kdl_src, "PetStoreApi", "getPet", args=[1])
        assert js_pet["ok"] is True
        assert js_pet["value"]["name"] == "Bella"
        assert js_pet["value"]["owner"]["full_name"] == "Sarah Connor"

        js_422 = run_js_rest(
            kdl_src,
            "PetStoreApi",
            "searchPets",
            args=[{"species": "invalid"}],
        )
        assert js_422["ok"] is False
        assert js_422["variant"] == "PetStoreApiErr422"

        js_503 = run_js_rest(
            kdl_src, "PetStoreApi", "getPet", args=[503]
        )
        assert js_503["ok"] is False
        assert js_503["kind"] == "unknown"
        assert js_503["status"] == 503


class TestRestTransportErr:
    """Network failure against a closed loopback port (127.0.0.1:1) returns TransportErr."""

    def test_transport_err_across_all_sync_async_and_js_transports(
        self,
    ) -> None:
        unreachable_kdl = rewrite_schema_host(
            (SCHEMAS_DIR / "08_rest_basic.kdl").read_text(encoding="utf-8"),
            "http://127.0.0.1:1",
        )
        with loaded_py_module(unreachable_kdl) as (mod, _):
            # 1. via_urllib
            r_url = mod.API.fetch(mod.via_urllib(timeout=0.2), id="1")
            assert isinstance(r_url, mod.TransportErr)
            assert r_url.is_ok is False
            assert r_url.status == 0
            assert isinstance(r_url.exception, urllib.error.URLError)

            # 2. via_httpx (httpx & httpx2)
            with httpx.Client(timeout=0.2) as hclient:
                r_hx = mod.API.fetch(mod.via_httpx(hclient), id="1")
                assert isinstance(r_hx, mod.TransportErr)
                assert r_hx.status == 0
                assert isinstance(r_hx.exception, httpx.HTTPError)

            with httpx2.Client(timeout=0.2) as h2client:
                r_hx2 = mod.API.fetch(mod.via_httpx(h2client), id="1")
                assert isinstance(r_hx2, mod.TransportErr)
                assert r_hx2.status == 0
                assert isinstance(r_hx2.exception, httpx2.HTTPError)

            # 3. via_requests
            with requests.Session() as rsession:
                r_req = mod.API.fetch(
                    mod.via_requests(rsession), id="1", timeout=0.2
                )
                assert isinstance(r_req, mod.TransportErr)
                assert r_req.status == 0
                assert isinstance(r_req.exception, requests.RequestException)

            # 4. via_httpx_async (httpx & httpx2) & via_aiohttp
            async def _run_async() -> None:
                async with httpx.AsyncClient(timeout=0.2) as aclient:
                    r_ahx = await mod.API.async_fetch(
                        mod.via_httpx_async(aclient), id="1"
                    )
                    assert isinstance(r_ahx, mod.TransportErr)
                    assert r_ahx.status == 0
                    assert isinstance(r_ahx.exception, httpx.HTTPError)

                async with httpx2.AsyncClient(timeout=0.2) as aclient2:
                    r_ahx2 = await mod.API.async_fetch(
                        mod.via_httpx_async(aclient2), id="1"
                    )
                    assert isinstance(r_ahx2, mod.TransportErr)
                    assert r_ahx2.status == 0
                    assert isinstance(r_ahx2.exception, httpx2.HTTPError)

                timeout = aiohttp.ClientTimeout(sock_connect=0.2)
                async with aiohttp.ClientSession(timeout=timeout) as asession:
                    r_aio = await mod.API.async_fetch(
                        mod.via_aiohttp(asession), id="1"
                    )
                    assert isinstance(r_aio, mod.TransportErr)
                    assert r_aio.status == 0
                    assert isinstance(r_aio.exception, aiohttp.ClientError)

            asyncio.run(_run_async())

        js_trans = run_js_rest(
            unreachable_kdl, "API", "fetch", args=["1"]
        )
        assert js_trans["ok"] is False
        assert js_trans["kind"] == "transport"
        assert js_trans["status"] == 0
        assert js_trans["cause"] is not None


class TestGoAndRustRestE2E:
    """Compile and execute generated Go (net/http) and Rust (reqwest + tokio) REST clients against FakeServer."""

    _VOID_API_SCHEMA = '''
(rest)struct VoidAPI {
    @request name=ping """
    GET /ping HTTP/1.1
    Host: api.example.com
    """
    @request name=delete-user """
    DELETE /users/{{id:int}} HTTP/1.1
    Host: api.example.com
    """
}
'''

    def test_go_rest_clients_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "pkg_08": fake_server.rewrite_schema(
                SCHEMAS_DIR / "08_rest_basic.kdl"
            ),
            "pkg_09": fake_server.rewrite_schema(
                SCHEMAS_DIR / "09_rest_void.kdl"
            ),
            "pkg_void": fake_server.rewrite_schema(self._VOID_API_SCHEMA),
            "pkg_10": fake_server.rewrite_schema(
                SCHEMAS_DIR / "10_rest_err_404.kdl"
            ),
            "pkg_12": fake_server.rewrite_schema(
                SCHEMAS_DIR / "12_rest_err_404_keys.kdl"
            ),
            "pkg_13": fake_server.rewrite_schema(
                SCHEMAS_DIR / "13_rest_err_200_field.kdl"
            ),
            "pkg_search": fake_server.rewrite_schema(
                TestRestDotPathAndOrderedFallthrough._SEARCH_FALLTHROUGH_SCHEMA
            ),
            "pkg_deleg": fake_server.rewrite_schema(
                TestRestWireFormattingAndDelegation._DELEGATION_SCHEMA
            ),
            "pkg_dummy": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_dummyjson.kdl"
            ),
            "pkg_pet": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_petstore.kdl"
            ),
            "pkg_closed": rewrite_schema_host(
                (SCHEMAS_DIR / "08_rest_basic.kdl").read_text(encoding="utf-8"),
                "http://127.0.0.1:1",
            ),
        }

        files: dict[str, str] = {}
        for pkg_name, kdl_src in schemas.items():
            res = parsmith.generate(
                kdl_src, "go", package=pkg_name, module_stem="client"
            )
            assert res.ok, f"Go generate failed for {pkg_name}:\n{res.format_text()}"
            files[f"{pkg_name}/client.go"] = res["client.go"]

        files["main.go"] = r"""package main

import (
	"errors"
	"fmt"
	"net/http"
	"os"
	"time"

	"parsmith_e2e_runner/pkg_08"
	"parsmith_e2e_runner/pkg_09"
	"parsmith_e2e_runner/pkg_10"
	"parsmith_e2e_runner/pkg_12"
	"parsmith_e2e_runner/pkg_13"
	"parsmith_e2e_runner/pkg_closed"
	"parsmith_e2e_runner/pkg_deleg"
	"parsmith_e2e_runner/pkg_dummy"
	"parsmith_e2e_runner/pkg_pet"
	"parsmith_e2e_runner/pkg_search"
	"parsmith_e2e_runner/pkg_void"
)

func assert(cond bool, msg string) {
	if !cond {
		fmt.Fprintln(os.Stderr, "ASSERTION FAILED:", msg)
		os.Exit(1)
	}
}

func main() {
	client := &http.Client{Timeout: 5 * time.Second}

	// 1. 08_rest_basic.kdl: 200 OK and *DecodeErr on null, prim, bad (HTML), mismatch
	api08 := pkg_08.NewAPI()
	u42, err := api08.Fetch(client, "42")
	assert(err == nil && u42 != nil && u42.Id == 42 && u42.Name == "Alice", fmt.Sprintf("08 ok: %v %+v", err, u42))

	for _, badID := range []string{"null", "prim", "bad", "mismatch"} {
		_, err := api08.Fetch(client, badID)
		var decErr *pkg_08.DecodeErr
		assert(errors.As(err, &decErr) && decErr.Status == 200, fmt.Sprintf("08 decode err (%s): %T %v", badID, err, err))
	}

	// 2. 09_rest_void.kdl & VoidAPI (ping & delete-user)
	api09 := pkg_09.NewAPI()
	_, err = api09.Fetch(client, "x")
	assert(err == nil, fmt.Sprintf("09 void: %v", err))

	vapi := pkg_void.NewVoidAPI()
	_, err = vapi.Ping(client)
	assert(err == nil, fmt.Sprintf("VoidAPI.Ping: %v", err))
	_, err = vapi.DeleteUser(client, 42)
	assert(err == nil, fmt.Sprintf("VoidAPI.DeleteUser: %v", err))

	// 3. 10_rest_err_404.kdl, 12_rest_err_404_keys.kdl, 13_rest_err_200_field.kdl
	api10 := pkg_10.NewAPI()
	_, err = api10.Fetch(client, "99")
	var e10_404 *pkg_10.APIErr404
	assert(errors.As(err, &e10_404) && e10_404.Status == 404 && e10_404.Body.Code == 404 && e10_404.Body.Message == "not found", fmt.Sprintf("10 404: %T %v", err, err))

	_, err = api10.Fetch(client, "429")
	var e10_unk *pkg_10.UnknownErr
	assert(errors.As(err, &e10_unk) && e10_unk.Status == 429, fmt.Sprintf("10 429: %T %v", err, err))

	api12 := pkg_12.NewAPI()
	_, err = api12.Fetch(client, "99")
	var e12_match *pkg_12.APIErr404ErrorDetail
	assert(errors.As(err, &e12_match) && e12_match.Body.Error == "x" && e12_match.Body.Detail == "y", fmt.Sprintf("12 match: %T %v", err, err))

	_, err = api12.Fetch(client, "404_nokeys")
	var e12_unk *pkg_12.UnknownErr
	assert(errors.As(err, &e12_unk) && e12_unk.Status == 404, fmt.Sprintf("12 nokeys: %T %v", err, err))

	api13 := pkg_13.NewAPI()
	_, err = api13.Fetch(client, "err200")
	var e13_200 *pkg_13.APIErr200Detail
	assert(errors.As(err, &e13_200) && e13_200.Status == 200 && e13_200.Body.Detail == "msg", fmt.Sprintf("13 err200: %T %v", err, err))
	u13, err := api13.Fetch(client, "1")
	assert(err == nil && u13.Id == 1 && u13.Name == "Alice", fmt.Sprintf("13 ok: %v", err))

	// 4. _SEARCH_FALLTHROUGH_SCHEMA
	sapi := pkg_search.NewSearchAPI()
	_, err = sapi.Fetch(client, "err200")
	var s_200 *pkg_search.SearchAPIErr200Code
	assert(errors.As(err, &s_200) && s_200.Body.Code == 1001 && s_200.Body.Message == "Quota exceeded", fmt.Sprintf("search err200: %T %v", err, err))

	u_ok_empty, err := sapi.Fetch(client, "ok_empty_err")
	assert(err == nil && u_ok_empty.Id == 1 && u_ok_empty.Name == "Alice", fmt.Sprintf("search ok_empty_err: %v", err))

	_, err = sapi.Fetch(client, "404_specific")
	var s_spec *pkg_search.SearchAPIErr404NotFound
	assert(errors.As(err, &s_spec) && s_spec.Body.Code == 404 && s_spec.Body.Error == "Ничего не найдено", fmt.Sprintf("search 404_specific: %T %v", err, err))

	_, err = sapi.Fetch(client, "404_fallthrough")
	var s_fall *pkg_search.SearchAPIErr404Message
	assert(errors.As(err, &s_fall) && s_fall.Body.Message == "Fallback generic message", fmt.Sprintf("search 404_fallthrough: %T %v", err, err))

	for _, unkMode := range []string{"404_empty", "404_html"} {
		_, err := sapi.Fetch(client, unkMode)
		var s_unk *pkg_search.UnknownErr
		assert(errors.As(err, &s_unk) && s_unk.Status == 404, fmt.Sprintf("search %s: %T %v", unkMode, err, err))
	}

	_, err = sapi.Fetch(client, "400_mismatch")
	var s_dec *pkg_search.DecodeErr
	assert(errors.As(err, &s_dec) && s_dec.Status == 400, fmt.Sprintf("search 400_mismatch: %T %v", err, err))

	// 5. _DELEGATION_SCHEMA
	dapi := pkg_deleg.NewUserAPI()
	du, err := dapi.UpdateUser(
		client,
		"s123",
		"v1",
		"Alice",
		"",
		"",
		"",
		[]string{"a", "b"},
		[]int64{1, 2},
		[]string{"hello", "world"},
		[]int64{10, 20},
		[]string{"r1", "r2"},
		"",
		pkg_deleg.WithHeader("X-Custom", "from-caller"),
		pkg_deleg.WithParam("extra", "yes"),
		pkg_deleg.WithCookie("c_extra", "cval"),
	)
	assert(err == nil && du.Id == 99 && du.Name == "Alice", fmt.Sprintf("delegation: %v %+v", err, du))

	// 6. rest_dummyjson.kdl
	dummy := pkg_dummy.NewDummyJsonApi()
	prod, err := dummy.GetProduct(client, 1)
	assert(err == nil && prod.Id == 1 && prod.Title == "Essence Mascara Lash Princess" && prod.DiscountPercentage == 10.48 && prod.Reviews[0].ReviewerName == "Alice", fmt.Sprintf("dummy get_product: %v", err))

	_, err = dummy.GetProduct(client, 999)
	var d_404 *pkg_dummy.DummyJsonApiErr404
	assert(errors.As(err, &d_404) && d_404.Status == 404, fmt.Sprintf("dummy 404: %T %v", err, err))

	plist, err := dummy.ListProducts(client, 10, 5, []string{"title", "price"})
	assert(err == nil && plist.Total == 2 && plist.Limit == 10 && plist.Skip == 5, fmt.Sprintf("dummy list: %v", err))

	sres, err := dummy.SearchProducts(client, "mascara", 5)
	assert(err == nil && sres.Products[0].Title == "Result for mascara", fmt.Sprintf("dummy search: %v", err))

	_, err = dummy.SearchProducts(client, "error400", 5)
	var d_400 *pkg_dummy.DummyJsonApiErr400
	assert(errors.As(err, &d_400) && d_400.Body.Message == "Invalid search query", fmt.Sprintf("dummy 400: %T %v", err, err))

	_, err = dummy.SearchProducts(client, "error500", 5)
	var d_500 *pkg_dummy.DummyJsonApiErr500
	assert(errors.As(err, &d_500) && d_500.Body.Message == "Search backend failure", fmt.Sprintf("dummy 500: %T %v", err, err))

	cats, err := dummy.ListCategories(client)
	assert(err == nil && len(cats) == 2 && cats[0].Slug == "beauty", fmt.Sprintf("dummy categories: %v", err))

	created, err := dummy.CreateProduct(client, "Luxury Watch", 199.99, 5)
	assert(err == nil && created.Id == 195 && created.Title == "Luxury Watch" && created.Price == 199.99 && created.Stock == 5, fmt.Sprintf("dummy create: %v", err))

	// 7. rest_petstore.kdl
	petApi := pkg_pet.NewPetStoreApi()
	pets, err := petApi.ListPets(client, 2, 10)
	assert(err == nil && len(pets.Pets) == 2 && pets.Pets[0].Name == "Bella" && pets.Pets[1].Breed == nil && pets.Pagination.Page == 2, fmt.Sprintf("pet list: %v", err))

	dogs, err := petApi.SearchPets(client, "dog", "")
	assert(err == nil && len(dogs.Pets) == 1 && dogs.Pets[0].Name == "Bella", fmt.Sprintf("pet search: %v", err))

	_, err = petApi.SearchPets(client, "invalid", "")
	var p_422 *pkg_pet.PetStoreApiErr422
	assert(errors.As(err, &p_422) && p_422.Status == 422 && p_422.Body.Code != nil && *p_422.Body.Code == "UNPROCESSABLE_SPECIES", fmt.Sprintf("pet 422: %T %v", err, err))

	bella, err := petApi.GetPet(client, 1)
	assert(err == nil && bella.Name == "Bella" && bella.Owner != nil && bella.Owner.FullName == "Sarah Connor", fmt.Sprintf("pet get 1: %v", err))

	_, err = petApi.GetPet(client, 503)
	var p_unk *pkg_pet.UnknownErr
	assert(errors.As(err, &p_unk) && p_unk.Status == 503, fmt.Sprintf("pet 503: %T %v", err, err))

	// 8. Closed loopback port -> *TransportErr
	closedApi := pkg_closed.NewAPI()
	_, err = closedApi.Fetch(&http.Client{Timeout: 200 * time.Millisecond}, "1")
	var tErr *pkg_closed.TransportErr
	assert(errors.As(err, &tErr) && tErr.Cause != "", fmt.Sprintf("closed port: %T %v", err, err))

	fmt.Print("GO_REST_E2E_OK")
}
"""
        out = run_go_custom_program(files)
        assert out == "GO_REST_E2E_OK"

    def test_rust_rest_clients_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "mod_08": fake_server.rewrite_schema(
                SCHEMAS_DIR / "08_rest_basic.kdl"
            ),
            "mod_09": fake_server.rewrite_schema(
                SCHEMAS_DIR / "09_rest_void.kdl"
            ),
            "mod_void": fake_server.rewrite_schema(self._VOID_API_SCHEMA),
            "mod_10": fake_server.rewrite_schema(
                SCHEMAS_DIR / "10_rest_err_404.kdl"
            ),
            "mod_12": fake_server.rewrite_schema(
                SCHEMAS_DIR / "12_rest_err_404_keys.kdl"
            ),
            "mod_13": fake_server.rewrite_schema(
                SCHEMAS_DIR / "13_rest_err_200_field.kdl"
            ),
            "mod_search": fake_server.rewrite_schema(
                TestRestDotPathAndOrderedFallthrough._SEARCH_FALLTHROUGH_SCHEMA
            ),
            "mod_deleg": fake_server.rewrite_schema(
                TestRestWireFormattingAndDelegation._DELEGATION_SCHEMA
            ),
            "mod_dummy": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_dummyjson.kdl"
            ),
            "mod_pet": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_petstore.kdl"
            ),
            "mod_closed": rewrite_schema_host(
                (SCHEMAS_DIR / "08_rest_basic.kdl").read_text(encoding="utf-8"),
                "http://127.0.0.1:1",
            ),
        }

        files: dict[str, str] = {}
        for mod_name, kdl_src in schemas.items():
            res = parsmith.generate(
                kdl_src,
                "rust",
                separate_runtime=True,
                runtime_name="parsmith_runtime",
                module_stem=mod_name,
            )
            assert res.ok, f"Rust generate failed for {mod_name}:\n{res.format_text()}"
            files[f"{mod_name}.rs"] = res[f"{mod_name}.rs"]
            files["parsmith_runtime.rs"] = res["parsmith_runtime.rs"]

        files["main.rs"] = r"""#![allow(dead_code, unused_imports)]
mod parsmith_runtime;
mod mod_08;
mod mod_09;
mod mod_void;
mod mod_10;
mod mod_12;
mod mod_13;
mod mod_search;
mod mod_deleg;
mod mod_dummy;
mod mod_pet;
mod mod_closed;

#[tokio::main]
async fn main() {
    let client = reqwest::Client::new();

    // 1. 08_rest_basic.kdl: 200 OK and APIError::Decode(200, _) on null, prim, bad, mismatch
    let u42 = mod_08::API::fetch(&client, "42").await.unwrap();
    assert_eq!(u42.id, 42);
    assert_eq!(u42.name, "Alice");

    for bad_id in ["null", "prim", "bad", "mismatch"] {
        match mod_08::API::fetch(&client, bad_id).await {
            Err(err @ mod_08::APIError::Decode(200, _)) => {
                assert_eq!(err.status(), Some(200));
            }
            other => panic!("expected Decode(200) for {bad_id}, got {other:?}"),
        }
    }

    // 2. 09_rest_void.kdl & VoidAPI (ping & delete_user)
    mod_09::API::fetch(&client, "x").await.unwrap();
    mod_void::VoidAPI::ping(&client).await.unwrap();
    mod_void::VoidAPI::delete_user(&client, 42).await.unwrap();

    // 3. 10_rest_err_404.kdl, 12_rest_err_404_keys.kdl, 13_rest_err_200_field.kdl
    match mod_10::API::fetch(&client, "99").await {
        Err(mod_10::APIError::Err404(e)) => {
            assert_eq!(e.code, 404);
            assert_eq!(e.message, "not found");
        }
        other => panic!("expected 10 Err404, got {other:?}"),
    }
    match mod_10::API::fetch(&client, "429").await {
        Err(mod_10::APIError::Unknown(429, val)) => {
            assert_eq!(val["error"], "rate limited");
        }
        other => panic!("expected 10 Unknown(429), got {other:?}"),
    }

    match mod_12::API::fetch(&client, "99").await {
        Err(mod_12::APIError::Err404ErrorDetail(e)) => {
            assert_eq!(e.error, "x");
            assert_eq!(e.detail, "y");
        }
        other => panic!("expected 12 Err404ErrorDetail, got {other:?}"),
    }
    match mod_12::API::fetch(&client, "404_nokeys").await {
        Err(mod_12::APIError::Unknown(404, val)) => {
            assert_eq!(val["message"], "not found");
        }
        other => panic!("expected 12 Unknown(404), got {other:?}"),
    }

    match mod_13::API::fetch(&client, "err200").await {
        Err(err @ mod_13::APIError::Err200Detail(_)) => {
            assert_eq!(err.status(), Some(200));
            if let mod_13::APIError::Err200Detail(b) = err {
                assert_eq!(b.detail, "msg");
            }
        }
        other => panic!("expected 13 Err200Detail, got {other:?}"),
    }
    let u13 = mod_13::API::fetch(&client, "1").await.unwrap();
    assert_eq!(u13.id, 1);
    assert_eq!(u13.name, "Alice");

    // 4. _SEARCH_FALLTHROUGH_SCHEMA
    match mod_search::SearchAPI::fetch(&client, Some("err200")).await {
        Err(mod_search::SearchAPIError::Err200Code(e)) => {
            assert_eq!(e.code, 1001);
            assert_eq!(e.message, "Quota exceeded");
        }
        other => panic!("expected SearchAPI Err200Code, got {other:?}"),
    }

    let u_ok = mod_search::SearchAPI::fetch(&client, Some("ok_empty_err")).await.unwrap();
    assert_eq!(u_ok.id, 1);
    assert_eq!(u_ok.name, "Alice");

    match mod_search::SearchAPI::fetch(&client, Some("404_specific")).await {
        Err(mod_search::SearchAPIError::Err404NotFound(e)) => {
            assert_eq!(e.error, "Ничего не найдено");
            assert_eq!(e.code, 404);
        }
        other => panic!("expected SearchAPI Err404NotFound, got {other:?}"),
    }

    match mod_search::SearchAPI::fetch(&client, Some("404_fallthrough")).await {
        Err(mod_search::SearchAPIError::Err404Message(e)) => {
            assert_eq!(e.message, "Fallback generic message");
        }
        other => panic!("expected SearchAPI Err404Message fallthrough, got {other:?}"),
    }

    match mod_search::SearchAPI::fetch(&client, Some("404_empty")).await {
        Err(mod_search::SearchAPIError::Unknown(404, val)) => {
            assert!(val.is_object() && val.as_object().unwrap().is_empty());
        }
        other => panic!("expected SearchAPI Unknown(404) on empty JSON, got {other:?}"),
    }

    match mod_search::SearchAPI::fetch(&client, Some("404_html")).await {
        Err(mod_search::SearchAPIError::Unknown(404, val)) => {
            assert_eq!(val.as_str(), Some("<html><body>404 Not Found</body></html>"));
        }
        other => panic!("expected SearchAPI Unknown(404) on HTML, got {other:?}"),
    }

    match mod_search::SearchAPI::fetch(&client, Some("400_mismatch")).await {
        Err(err @ mod_search::SearchAPIError::Decode(400, _)) => {
            assert_eq!(err.status(), Some(400));
        }
        other => panic!("expected SearchAPI Decode(400) on 400_mismatch, got {other:?}"),
    }

    // 5. _DELEGATION_SCHEMA
    let du = mod_deleg::UserAPI::update_user(
        &client,
        "s123",
        "v1",
        "Alice",
        None,
        None,
        None,
        Some(&["a", "b"]),
        Some(&[1, 2]),
        Some(&["hello", "world"]),
        Some(&[10, 20]),
        Some(&["r1", "r2"]),
        None,
    )
    .await
    .unwrap();
    assert_eq!(du.id, 99);
    assert_eq!(du.name, "Alice");
    assert_eq!(du.nickname, None);

    // 6. rest_dummyjson.kdl
    let prod = mod_dummy::DummyJsonApi::get_product(&client, 1).await.unwrap();
    assert_eq!(prod.id, 1);
    assert_eq!(prod.title, "Essence Mascara Lash Princess");
    assert_eq!(prod.discount_percentage, 10.48);
    assert_eq!(prod.reviews[0].reviewer_name, "Alice");

    match mod_dummy::DummyJsonApi::get_product(&client, 999).await {
        Err(mod_dummy::DummyJsonApiError::Err404(e)) => {
            assert_eq!(e.message, "Product with id '999' not found");
        }
        other => panic!("expected DummyJsonApi Err404, got {other:?}"),
    }

    let plist = mod_dummy::DummyJsonApi::list_products(
        &client,
        Some(10),
        Some(5),
        Some(&["title", "price"]),
    )
    .await
    .unwrap();
    assert_eq!(plist.total, 2);
    assert_eq!(plist.limit, 10);
    assert_eq!(plist.skip, 5);

    let sres = mod_dummy::DummyJsonApi::search_products(&client, "mascara", Some(5))
        .await
        .unwrap();
    assert_eq!(sres.products[0].title, "Result for mascara");

    match mod_dummy::DummyJsonApi::search_products(&client, "error400", None).await {
        Err(mod_dummy::DummyJsonApiError::Err400(e)) => {
            assert_eq!(e.message, "Invalid search query");
        }
        other => panic!("expected DummyJsonApi Err400, got {other:?}"),
    }

    match mod_dummy::DummyJsonApi::search_products(&client, "error500", None).await {
        Err(mod_dummy::DummyJsonApiError::Err500(e)) => {
            assert_eq!(e.message, "Search backend failure");
        }
        other => panic!("expected DummyJsonApi Err500, got {other:?}"),
    }

    let cats = mod_dummy::DummyJsonApi::list_categories(&client).await.unwrap();
    assert_eq!(cats.len(), 2);
    assert_eq!(cats[0].slug, "beauty");

    let created = mod_dummy::DummyJsonApi::create_product(&client, "Luxury Watch", 199.99, 5)
        .await
        .unwrap();
    assert_eq!(created.id, 195);
    assert_eq!(created.title, "Luxury Watch");
    assert_eq!(created.price, 199.99);
    assert_eq!(created.stock, 5);

    // 7. rest_petstore.kdl
    let pets = mod_pet::PetStoreApi::list_pets(&client, Some(2), Some(10))
        .await
        .unwrap();
    assert_eq!(pets.pets.len(), 2);
    assert_eq!(pets.pets[0].name, "Bella");
    assert_eq!(pets.pets[1].breed, None);
    assert_eq!(pets.pagination.page, 2);

    let dogs = mod_pet::PetStoreApi::search_pets(&client, Some("dog"), None)
        .await
        .unwrap();
    assert_eq!(dogs.pets.len(), 1);
    assert_eq!(dogs.pets[0].name, "Bella");

    match mod_pet::PetStoreApi::search_pets(&client, Some("invalid"), None).await {
        Err(mod_pet::PetStoreApiError::Err422(e)) => {
            assert_eq!(e.code.as_deref(), Some("UNPROCESSABLE_SPECIES"));
        }
        other => panic!("expected PetStoreApi Err422, got {other:?}"),
    }

    let bella = mod_pet::PetStoreApi::get_pet(&client, 1).await.unwrap();
    assert_eq!(bella.name, "Bella");
    assert_eq!(bella.owner.unwrap().full_name, "Sarah Connor");

    match mod_pet::PetStoreApi::get_pet(&client, 503).await {
        Err(err @ mod_pet::PetStoreApiError::Unknown(503, _)) => {
            assert_eq!(err.status(), Some(503));
        }
        other => panic!("expected PetStoreApi Unknown(503), got {other:?}"),
    }

    // 8. Closed loopback port -> APIError::Transport
    let closed_err = mod_closed::API::fetch(&client, "1").await.unwrap_err();
    assert_eq!(closed_err.status(), None);
    assert!(matches!(closed_err, mod_closed::APIError::Transport(_)));

    print!("RUST_REST_E2E_OK");
}
"""
        out = run_rust_custom_program(files)
        assert out == "RUST_REST_E2E_OK"

    def test_elixir_rest_clients_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "Pkg08": fake_server.rewrite_schema(
                SCHEMAS_DIR / "08_rest_basic.kdl"
            ),
            "Pkg09": fake_server.rewrite_schema(
                SCHEMAS_DIR / "09_rest_void.kdl"
            ),
            "PkgVoid": fake_server.rewrite_schema(self._VOID_API_SCHEMA),
            "Pkg10": fake_server.rewrite_schema(
                SCHEMAS_DIR / "10_rest_err_404.kdl"
            ),
            "Pkg12": fake_server.rewrite_schema(
                SCHEMAS_DIR / "12_rest_err_404_keys.kdl"
            ),
            "Pkg13": fake_server.rewrite_schema(
                SCHEMAS_DIR / "13_rest_err_200_field.kdl"
            ),
            "PkgSearch": fake_server.rewrite_schema(
                TestRestDotPathAndOrderedFallthrough._SEARCH_FALLTHROUGH_SCHEMA
            ),
            "PkgDeleg": fake_server.rewrite_schema(
                TestRestWireFormattingAndDelegation._DELEGATION_SCHEMA
            ),
            "PkgDummy": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_dummyjson.kdl"
            ),
            "PkgPet": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_petstore.kdl"
            ),
            "PkgClosed": rewrite_schema_host(
                (SCHEMAS_DIR / "08_rest_basic.kdl").read_text(encoding="utf-8"),
                "http://127.0.0.1:1",
            ),
        }

        files: dict[str, str] = {}
        for pkg_name, kdl_src in schemas.items():
            stem = pkg_name.lower()
            res = parsmith.generate(
                kdl_src, "elixir", package=pkg_name, module_stem=stem
            )
            assert res.ok, f"Elixir generate failed for {pkg_name}:\n{res.format_text()}"
            files[f"{stem}.ex"] = res[f"{stem}.ex"]

        eval_code = """
client = Pkg08.Rt.via_httpc()

# 1. 08_rest_basic.kdl
{:ok, u42} = Pkg08.API.fetch(client, "42")
42 = u42.id
"Alice" = u42.name

for bad_id <- ["null", "prim", "bad", "mismatch"] do
  {:error, %Pkg08.Rt.DecodeErr{status: 200}} = Pkg08.API.fetch(client, bad_id)
end

# 2. 09_rest_void.kdl & VoidAPI
{:ok, nil} = Pkg09.API.fetch(client, "x")
{:ok, nil} = PkgVoid.VoidAPI.ping(client)
{:ok, nil} = PkgVoid.VoidAPI.delete_user(client, 42)

# 3. 10_rest_err_404.kdl, 12_rest_err_404_keys.kdl, 13_rest_err_200_field.kdl
{:error, %Pkg10.APIErr404{status: 404, body: e10}} = Pkg10.API.fetch(client, "99")
404 = e10.code
"not found" = e10.message

{:error, %Pkg10.Rt.UnknownErr{status: 429}} = Pkg10.API.fetch(client, "429")

{:error, %Pkg12.APIErr404ErrorDetail{status: 404, body: e12}} = Pkg12.API.fetch(client, "99")
"x" = e12.error
"y" = e12.detail

{:error, %Pkg12.Rt.UnknownErr{status: 404}} = Pkg12.API.fetch(client, "404_nokeys")

{:error, %Pkg13.APIErr200Detail{status: 200, body: e13}} = Pkg13.API.fetch(client, "err200")
"msg" = e13.detail
{:ok, u13} = Pkg13.API.fetch(client, "1")
1 = u13.id
"Alice" = u13.name

# 4. _SEARCH_FALLTHROUGH_SCHEMA
{:error, %PkgSearch.SearchAPIErr200Code{status: 200, body: s200}} = PkgSearch.SearchAPI.fetch(client, "err200")
1001 = s200.code
"Quota exceeded" = s200.message

{:ok, u_ok} = PkgSearch.SearchAPI.fetch(client, "ok_empty_err")
1 = u_ok.id
"Alice" = u_ok.name

{:error, %PkgSearch.SearchAPIErr404NotFound{status: 404, body: s_spec}} = PkgSearch.SearchAPI.fetch(client, "404_specific")
404 = s_spec.code

{:error, %PkgSearch.SearchAPIErr404Message{status: 404, body: s_fall}} = PkgSearch.SearchAPI.fetch(client, "404_fallthrough")
"Fallback generic message" = s_fall.message

{:error, %PkgSearch.Rt.UnknownErr{status: 404}} = PkgSearch.SearchAPI.fetch(client, "404_empty")
{:error, %PkgSearch.Rt.UnknownErr{status: 404}} = PkgSearch.SearchAPI.fetch(client, "404_html")
{:error, %PkgSearch.Rt.DecodeErr{status: 400}} = PkgSearch.SearchAPI.fetch(client, "400_mismatch")

# 5. _DELEGATION_SCHEMA
{:ok, du} = PkgDeleg.UserAPI.update_user(
  client,
  "s123",
  "v1",
  "Alice",
  nil,
  nil,
  nil,
  ["a", "b"],
  [1, 2],
  ["hello", "world"],
  [10, 20],
  ["r1", "r2"],
  nil
)
99 = du.id
"Alice" = du.name
nil = du.nickname

# 6. rest_dummyjson.kdl
{:ok, prod} = PkgDummy.DummyJsonApi.get_product(client, 1)
1 = prod.id
"Essence Mascara Lash Princess" = prod.title
10.48 = prod.discount_percentage

{:error, %PkgDummy.DummyJsonApiErr404{status: 404}} = PkgDummy.DummyJsonApi.get_product(client, 999)

{:ok, plist} = PkgDummy.DummyJsonApi.list_products(client, 10, 5, ["title", "price"])
2 = plist.total
10 = plist.limit
5 = plist.skip

{:ok, sres} = PkgDummy.DummyJsonApi.search_products(client, "mascara", 5)
"Result for mascara" = hd(sres.products).title

{:error, %PkgDummy.DummyJsonApiErr400{body: d400}} = PkgDummy.DummyJsonApi.search_products(client, "error400")
"Invalid search query" = d400.message

{:error, %PkgDummy.DummyJsonApiErr500{body: d500}} = PkgDummy.DummyJsonApi.search_products(client, "error500")
"Search backend failure" = d500.message

{:ok, cats} = PkgDummy.DummyJsonApi.list_categories(client)
2 = length(cats)
"beauty" = hd(cats).slug

{:ok, created} = PkgDummy.DummyJsonApi.create_product(client, "Luxury Watch", 199.99, 5)
195 = created.id
"Luxury Watch" = created.title

# 7. rest_petstore.kdl
{:ok, pets} = PkgPet.PetStoreApi.list_pets(client, 2, 10)
2 = length(pets.pets)
"Bella" = hd(pets.pets).name

{:ok, dogs} = PkgPet.PetStoreApi.search_pets(client, "dog")
1 = length(dogs.pets)

{:error, %PkgPet.PetStoreApiErr422{status: 422, body: p422}} = PkgPet.PetStoreApi.search_pets(client, "invalid")
"UNPROCESSABLE_SPECIES" = p422.code

{:ok, bella} = PkgPet.PetStoreApi.get_pet(client, 1)
"Bella" = bella.name
"Sarah Connor" = bella.owner.full_name

{:error, %PkgPet.Rt.UnknownErr{status: 503}} = PkgPet.PetStoreApi.get_pet(client, 503)

# 8. Closed loopback port -> TransportErr
{:error, %PkgClosed.Rt.TransportErr{status: 0}} = PkgClosed.API.fetch(client, "1", timeout: 200)

IO.write("ELIXIR_REST_E2E_OK")
"""
        out = run_elixir_custom_program(files, eval_code)
        assert out == "ELIXIR_REST_E2E_OK"

    def test_php_rest_clients_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "Ns08": fake_server.rewrite_schema(
                SCHEMAS_DIR / "08_rest_basic.kdl"
            ),
            "Ns09": fake_server.rewrite_schema(
                SCHEMAS_DIR / "09_rest_void.kdl"
            ),
            "NsVoid": fake_server.rewrite_schema(self._VOID_API_SCHEMA),
            "Ns10": fake_server.rewrite_schema(
                SCHEMAS_DIR / "10_rest_err_404.kdl"
            ),
            "Ns12": fake_server.rewrite_schema(
                SCHEMAS_DIR / "12_rest_err_404_keys.kdl"
            ),
            "Ns13": fake_server.rewrite_schema(
                SCHEMAS_DIR / "13_rest_err_200_field.kdl"
            ),
            "NsSearch": fake_server.rewrite_schema(
                TestRestDotPathAndOrderedFallthrough._SEARCH_FALLTHROUGH_SCHEMA
            ),
            "NsDeleg": fake_server.rewrite_schema(
                TestRestWireFormattingAndDelegation._DELEGATION_SCHEMA
            ),
            "NsDummy": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_dummyjson.kdl"
            ),
            "NsPet": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_petstore.kdl"
            ),
            "NsClosed": rewrite_schema_host(
                (SCHEMAS_DIR / "08_rest_basic.kdl").read_text(encoding="utf-8"),
                "http://127.0.0.1:1",
            ),
        }

        files: dict[str, str] = {}
        for ns_name, kdl_src in schemas.items():
            stem = ns_name.lower()
            res = parsmith.generate(
                kdl_src, "php", package=ns_name, module_stem=stem
            )
            assert res.ok, f"PHP generate failed for {ns_name}:\n{res.format_text()}"
            files[f"{stem}.php"] = res[f"{stem}.php"]

        files["main.php"] = r"""<?php
declare(strict_types=1);

require_once __DIR__ . '/ns08.php';
require_once __DIR__ . '/ns09.php';
require_once __DIR__ . '/nsvoid.php';
require_once __DIR__ . '/ns10.php';
require_once __DIR__ . '/ns12.php';
require_once __DIR__ . '/ns13.php';
require_once __DIR__ . '/nssearch.php';
require_once __DIR__ . '/nsdeleg.php';
require_once __DIR__ . '/nsdummy.php';
require_once __DIR__ . '/nspet.php';
require_once __DIR__ . '/nsclosed.php';

function assert_true(bool $cond, string $msg): void {
    if (!$cond) {
        fwrite(STDERR, "ASSERTION FAILED: $msg\n");
        exit(1);
    }
}

// 1. 08_rest_basic.kdl
$r08 = \Ns08\API::fetch(\Ns08\Rt::viaStream(5.0), '42');
assert_true($r08 instanceof \Ns08\Ok && $r08->value->id === 42 && $r08->value->name === 'Alice', '08 ok');
$r08Curl = \Ns08\API::fetch(\Ns08\Rt::viaCurl(5.0), '42');
assert_true($r08Curl instanceof \Ns08\Ok && $r08Curl->value->id === 42, '08 ok curl');

foreach (['null', 'prim', 'bad', 'mismatch'] as $badId) {
    $badRes = \Ns08\API::fetch(\Ns08\Rt::viaStream(5.0), $badId);
    assert_true($badRes instanceof \Ns08\DecodeErr && $badRes->status === 200, "08 decode err $badId");
}

// 2. 09_rest_void.kdl & VoidAPI
$r09 = \Ns09\API::fetch(\Ns09\Rt::viaStream(5.0), 'x');
assert_true($r09 instanceof \Ns09\Ok && $r09->value === null, '09 void');

$rPing = \NsVoid\VoidAPI::ping(\NsVoid\Rt::viaStream(5.0));
assert_true($rPing instanceof \NsVoid\Ok && $rPing->value === null, 'VoidAPI ping');
$rDel = \NsVoid\VoidAPI::delete_user(\NsVoid\Rt::viaStream(5.0), 42);
assert_true($rDel instanceof \NsVoid\Ok && $rDel->value === null, 'VoidAPI delete_user');

// 3. 10, 12, 13
$r10 = \Ns10\API::fetch(\Ns10\Rt::viaStream(5.0), '99');
assert_true($r10 instanceof \Ns10\APIErr404 && $r10->value->code === 404 && $r10->value->message === 'not found', '10 404');
$r10Unk = \Ns10\API::fetch(\Ns10\Rt::viaStream(5.0), '429');
assert_true($r10Unk instanceof \Ns10\UnknownErr && $r10Unk->status === 429, '10 429');

$r12 = \Ns12\API::fetch(\Ns12\Rt::viaStream(5.0), '99');
assert_true($r12 instanceof \Ns12\APIErr404ErrorDetail && $r12->value->error === 'x' && $r12->value->detail === 'y', '12 match');
$r12Unk = \Ns12\API::fetch(\Ns12\Rt::viaStream(5.0), '404_nokeys');
assert_true($r12Unk instanceof \Ns12\UnknownErr && $r12Unk->status === 404, '12 nokeys');

$r13Err = \Ns13\API::fetch(\Ns13\Rt::viaStream(5.0), 'err200');
assert_true($r13Err instanceof \Ns13\APIErr200Detail && $r13Err->value->detail === 'msg', '13 err200');
$r13Ok = \Ns13\API::fetch(\Ns13\Rt::viaStream(5.0), '1');
assert_true($r13Ok instanceof \Ns13\Ok && $r13Ok->value->name === 'Alice', '13 ok');

// 4. _SEARCH_FALLTHROUGH_SCHEMA
$sTx = \NsSearch\Rt::viaStream(5.0);
$s200 = \NsSearch\SearchAPI::fetch($sTx, 'err200');
assert_true($s200 instanceof \NsSearch\SearchAPIErr200Code && $s200->value->code === 1001, 'search err200');
$sOkEmpty = \NsSearch\SearchAPI::fetch($sTx, 'ok_empty_err');
assert_true($sOkEmpty instanceof \NsSearch\Ok && $sOkEmpty->value->id === 1, 'search ok_empty_err');
$sSpec = \NsSearch\SearchAPI::fetch($sTx, '404_specific');
assert_true($sSpec instanceof \NsSearch\SearchAPIErr404NotFound && $sSpec->value->code === 404, 'search 404_specific');
$sFall = \NsSearch\SearchAPI::fetch($sTx, '404_fallthrough');
assert_true($sFall instanceof \NsSearch\SearchAPIErr404Message && $sFall->value->message === 'Fallback generic message', 'search 404_fallthrough');
$sEmpty = \NsSearch\SearchAPI::fetch($sTx, '404_empty');
assert_true($sEmpty instanceof \NsSearch\UnknownErr && $sEmpty->status === 404, 'search 404_empty');
$sHtml = \NsSearch\SearchAPI::fetch($sTx, '404_html');
assert_true($sHtml instanceof \NsSearch\UnknownErr && $sHtml->status === 404, 'search 404_html');
$s400 = \NsSearch\SearchAPI::fetch($sTx, '400_mismatch');
assert_true($s400 instanceof \NsSearch\DecodeErr && $s400->status === 400, 'search 400_mismatch');

// 5. _DELEGATION_SCHEMA
$du = \NsDeleg\UserAPI::update_user(
    \NsDeleg\Rt::viaStream(5.0),
    's123',
    'v1',
    'Alice',
    null,
    null,
    null,
    ['a', 'b'],
    [1, 2],
    ['hello', 'world'],
    [10, 20],
    ['r1', 'r2'],
    null,
);
assert_true($du instanceof \NsDeleg\Ok && $du->value->id === 99 && $du->value->name === 'Alice' && $du->value->nickname === null, 'delegation');

// 6. rest_dummyjson.kdl
$dTx = \NsDummy\Rt::viaStream(5.0);
$prod = \NsDummy\DummyJsonApi::get_product($dTx, 1);
assert_true($prod instanceof \NsDummy\Ok && $prod->value->id === 1 && $prod->value->discount_percentage === 10.48, 'dummy get_product');
$d404 = \NsDummy\DummyJsonApi::get_product($dTx, 999);
assert_true($d404 instanceof \NsDummy\DummyJsonApiErr404, 'dummy 404');
$plist = \NsDummy\DummyJsonApi::list_products($dTx, 10, 5, ['title', 'price']);
assert_true($plist instanceof \NsDummy\Ok && $plist->value->total === 2, 'dummy list');
$sres = \NsDummy\DummyJsonApi::search_products($dTx, 'mascara', 5);
assert_true($sres instanceof \NsDummy\Ok && $sres->value->products[0]->title === 'Result for mascara', 'dummy search');
$d400 = \NsDummy\DummyJsonApi::search_products($dTx, 'error400');
assert_true($d400 instanceof \NsDummy\DummyJsonApiErr400 && $d400->value->message === 'Invalid search query', 'dummy 400');
$d500 = \NsDummy\DummyJsonApi::search_products($dTx, 'error500');
assert_true($d500 instanceof \NsDummy\DummyJsonApiErr500 && $d500->value->message === 'Search backend failure', 'dummy 500');
$cats = \NsDummy\DummyJsonApi::list_categories($dTx);
assert_true($cats instanceof \NsDummy\Ok && count($cats->value) === 2 && $cats->value[0]->slug === 'beauty', 'dummy categories');
$created = \NsDummy\DummyJsonApi::create_product($dTx, 'Luxury Watch', 199.99, 5);
assert_true($created instanceof \NsDummy\Ok && $created->value->id === 195 && $created->value->title === 'Luxury Watch', 'dummy create');

// 7. rest_petstore.kdl
$pTx = \NsPet\Rt::viaStream(5.0);
$pets = \NsPet\PetStoreApi::list_pets($pTx, 2, 10);
assert_true($pets instanceof \NsPet\Ok && count($pets->value->pets) === 2 && $pets->value->pets[0]->name === 'Bella', 'pet list');
$dogs = \NsPet\PetStoreApi::search_pets($pTx, 'dog');
assert_true($dogs instanceof \NsPet\Ok && count($dogs->value->pets) === 1, 'pet search');
$p422 = \NsPet\PetStoreApi::search_pets($pTx, 'invalid');
assert_true($p422 instanceof \NsPet\PetStoreApiErr422 && $p422->value->code === 'UNPROCESSABLE_SPECIES', 'pet 422');
$bella = \NsPet\PetStoreApi::get_pet($pTx, 1);
assert_true($bella instanceof \NsPet\Ok && $bella->value->name === 'Bella' && $bella->value->owner->full_name === 'Sarah Connor', 'pet get 1');
$p503 = \NsPet\PetStoreApi::get_pet($pTx, 503);
assert_true($p503 instanceof \NsPet\UnknownErr && $p503->status === 503, 'pet 503');

// 8. Closed loopback port -> TransportErr
$closedRes = \NsClosed\API::fetch(\NsClosed\Rt::viaStream(0.2), '1');
assert_true($closedRes instanceof \NsClosed\TransportErr && $closedRes->status === 0, 'closed port TransportErr');

echo 'PHP_REST_E2E_OK';
"""
        out = run_php_custom_program(files)
        assert out == "PHP_REST_E2E_OK"

    def test_csharp_rest_clients_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "Ns08": fake_server.rewrite_schema(
                SCHEMAS_DIR / "08_rest_basic.kdl"
            ),
            "Ns09": fake_server.rewrite_schema(
                SCHEMAS_DIR / "09_rest_void.kdl"
            ),
            "NsVoid": fake_server.rewrite_schema(self._VOID_API_SCHEMA),
            "Ns10": fake_server.rewrite_schema(
                SCHEMAS_DIR / "10_rest_err_404.kdl"
            ),
            "Ns12": fake_server.rewrite_schema(
                SCHEMAS_DIR / "12_rest_err_404_keys.kdl"
            ),
            "Ns13": fake_server.rewrite_schema(
                SCHEMAS_DIR / "13_rest_err_200_field.kdl"
            ),
            "NsSearch": fake_server.rewrite_schema(
                TestRestDotPathAndOrderedFallthrough._SEARCH_FALLTHROUGH_SCHEMA
            ),
            "NsDeleg": fake_server.rewrite_schema(
                TestRestWireFormattingAndDelegation._DELEGATION_SCHEMA
            ),
            "NsDummy": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_dummyjson.kdl"
            ),
            "NsPet": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_petstore.kdl"
            ),
            "NsClosed": rewrite_schema_host(
                (SCHEMAS_DIR / "08_rest_basic.kdl").read_text(encoding="utf-8"),
                "http://127.0.0.1:1",
            ),
        }

        files: dict[str, str] = {}
        for ns_name, kdl_src in schemas.items():
            stem = ns_name.lower()
            res = parsmith.generate(
                kdl_src, "csharp", package=ns_name, module_stem=stem
            )
            assert res.ok, f"C# generate failed for {ns_name}:\n{res.format_text()}"
            files[f"{stem}.cs"] = res[f"{stem}.cs"]

        files["Program.cs"] = r"""#nullable enable
using System;
using System.Net.Http;
using System.Threading.Tasks;

public static class Program
{
    private static void Assert(bool cond, string msg)
    {
        if (!cond) throw new Exception("ASSERTION FAILED: " + msg);
    }

    public static async Task Main()
    {
        using var client = new HttpClient { Timeout = TimeSpan.FromSeconds(5) };

        // 1. 08_rest_basic.kdl
        var r08 = await Ns08.API.FetchAsync(client, "42");
        Assert(r08 is Ns08.FetchResult.Ok { Value: { Id: 42L, Name: "Alice" } }, "08 ok");

        foreach (var badId in new[] { "null", "prim", "bad", "mismatch" })
        {
            var badRes = Ns08.API.Fetch(client, badId);
            Assert(badRes is Ns08.FetchResult.DecodeErr { Status: 200 }, "08 decode " + badId);
        }

        // 2. 09_rest_void.kdl & VoidAPI
        Assert(Ns09.API.Fetch(client, "x") is Ns09.FetchResult.Ok, "09 void");
        Assert(NsVoid.VoidAPI.Ping(client) is NsVoid.PingResult.Ok, "VoidAPI Ping");
        Assert(NsVoid.VoidAPI.DeleteUser(client, 42L) is NsVoid.DeleteUserResult.Ok, "VoidAPI DeleteUser");

        // 3. 10, 12, 13
        Assert(Ns10.API.Fetch(client, "99") is Ns10.FetchResult.Err404 { Status: 404, Body: { Code: 404L, Message: "not found" } }, "10 404");
        Assert(Ns10.API.Fetch(client, "429") is Ns10.FetchResult.UnknownErr { Status: 429 }, "10 429");

        Assert(Ns12.API.Fetch(client, "99") is Ns12.FetchResult.Err404ErrorDetail { Status: 404, Body: { Error: "x", Detail: "y" } }, "12 match");
        Assert(Ns12.API.Fetch(client, "404_nokeys") is Ns12.FetchResult.UnknownErr { Status: 404 }, "12 nokeys");

        Assert(Ns13.API.Fetch(client, "err200") is Ns13.FetchResult.Err200Detail { Status: 200, Body.Detail: "msg" }, "13 err200");
        Assert(Ns13.API.Fetch(client, "1") is Ns13.FetchResult.Ok { Value: { Id: 1L, Name: "Alice" } }, "13 ok");

        // 4. _SEARCH_FALLTHROUGH_SCHEMA
        Assert(NsSearch.SearchAPI.Fetch(client, "err200") is NsSearch.FetchResult.Err200Code { Status: 200, Body: { Code: 1001L, Message: "Quota exceeded" } }, "search err200");
        Assert(NsSearch.SearchAPI.Fetch(client, "ok_empty_err") is NsSearch.FetchResult.Ok { Value: { Id: 1L, Name: "Alice" } }, "search ok_empty_err");
        Assert(NsSearch.SearchAPI.Fetch(client, "404_specific") is NsSearch.FetchResult.Err404NotFound { Status: 404, Body.Code: 404L }, "search 404_specific");
        Assert(NsSearch.SearchAPI.Fetch(client, "404_fallthrough") is NsSearch.FetchResult.Err404Message { Status: 404, Body.Message: "Fallback generic message" }, "search 404_fallthrough");
        Assert(NsSearch.SearchAPI.Fetch(client, "404_empty") is NsSearch.FetchResult.UnknownErr { Status: 404 }, "search 404_empty");
        Assert(NsSearch.SearchAPI.Fetch(client, "404_html") is NsSearch.FetchResult.UnknownErr { Status: 404 }, "search 404_html");
        Assert(NsSearch.SearchAPI.Fetch(client, "400_mismatch") is NsSearch.FetchResult.DecodeErr { Status: 400 }, "search 400_mismatch");

        // 5. _DELEGATION_SCHEMA
        var du = NsDeleg.UserAPI.UpdateUser(
            client,
            "s123",
            "v1",
            "Alice",
            null,
            null,
            null,
            new[] { "a", "b" },
            new[] { 1L, 2L },
            new[] { "hello", "world" },
            new[] { 10L, 20L },
            new[] { "r1", "r2" },
            null,
            NsDeleg.Rt.WithHeader("X-Custom", "from-caller"),
            NsDeleg.Rt.WithParam("extra", "yes"),
            NsDeleg.Rt.WithCookie("c_extra", "cval"));
        Assert(du is NsDeleg.UpdateUserResult.Ok { Value: { Id: 99L, Name: "Alice", Nickname: null } }, "delegation");

        // 6. rest_dummyjson.kdl
        Assert(NsDummy.DummyJsonApi.GetProduct(client, 1L) is NsDummy.GetProductResult.Ok { Value: { Id: 1L, Title: "Essence Mascara Lash Princess", DiscountPercentage: 10.48 } }, "dummy get_product");
        Assert(NsDummy.DummyJsonApi.GetProduct(client, 999L) is NsDummy.GetProductResult.Err404 { Status: 404 }, "dummy 404");
        Assert(NsDummy.DummyJsonApi.ListProducts(client, 10L, 5L, new[] { "title", "price" }) is NsDummy.ListProductsResult.Ok { Value: { Total: 2L, Limit: 10L, Skip: 5L } }, "dummy list");
        Assert(NsDummy.DummyJsonApi.SearchProducts(client, "mascara", 5L) is NsDummy.SearchProductsResult.Ok sOk && sOk.Value.Products[0].Title == "Result for mascara", "dummy search");
        Assert(NsDummy.DummyJsonApi.SearchProducts(client, "error400") is NsDummy.SearchProductsResult.Err400 { Body.Message: "Invalid search query" }, "dummy 400");
        Assert(NsDummy.DummyJsonApi.SearchProducts(client, "error500") is NsDummy.SearchProductsResult.Err500 { Body.Message: "Search backend failure" }, "dummy 500");
        Assert(NsDummy.DummyJsonApi.ListCategories(client) is NsDummy.ListCategoriesResult.Ok { Value.Count: 2 } cOk && cOk.Value[0].Slug == "beauty", "dummy categories");
        Assert(NsDummy.DummyJsonApi.CreateProduct(client, "Luxury Watch", 199.99, 5L) is NsDummy.CreateProductResult.Ok { Value: { Id: 195L, Title: "Luxury Watch" } }, "dummy create");

        // 7. rest_petstore.kdl
        Assert(NsPet.PetStoreApi.ListPets(client, 2L, 10L) is NsPet.ListPetsResult.Ok { Value.Pets.Count: 2 } pOk && pOk.Value.Pets[0].Name == "Bella", "pet list");
        Assert(NsPet.PetStoreApi.SearchPets(client, "dog") is NsPet.SearchPetsResult.Ok { Value.Pets.Count: 1 }, "pet search");
        Assert(NsPet.PetStoreApi.SearchPets(client, "invalid") is NsPet.SearchPetsResult.Err422 { Status: 422, Body.Code: "UNPROCESSABLE_SPECIES" }, "pet 422");
        Assert(NsPet.PetStoreApi.GetPet(client, 1L) is NsPet.GetPetResult.Ok { Value: { Name: "Bella", Owner.FullName: "Sarah Connor" } }, "pet get 1");
        Assert(NsPet.PetStoreApi.GetPet(client, 503L) is NsPet.GetPetResult.UnknownErr { Status: 503 }, "pet 503");

        // 8. Closed loopback port -> TransportErr
        var closedRes = NsClosed.API.Fetch(client, "1", NsClosed.Rt.WithTimeout(TimeSpan.FromMilliseconds(200)));
        Assert(closedRes is NsClosed.FetchResult.TransportErr, "closed port TransportErr");

        Console.Write("CSHARP_REST_E2E_OK");
    }
}
"""
        out = run_csharp_custom_program(files)
        assert out == "CSHARP_REST_E2E_OK"

    def test_kotlin_rest_clients_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "pkg_08": fake_server.rewrite_schema(
                SCHEMAS_DIR / "08_rest_basic.kdl"
            ),
            "pkg_09": fake_server.rewrite_schema(
                SCHEMAS_DIR / "09_rest_void.kdl"
            ),
            "pkg_void": fake_server.rewrite_schema(self._VOID_API_SCHEMA),
            "pkg_10": fake_server.rewrite_schema(
                SCHEMAS_DIR / "10_rest_err_404.kdl"
            ),
            "pkg_12": fake_server.rewrite_schema(
                SCHEMAS_DIR / "12_rest_err_404_keys.kdl"
            ),
            "pkg_13": fake_server.rewrite_schema(
                SCHEMAS_DIR / "13_rest_err_200_field.kdl"
            ),
            "pkg_search": fake_server.rewrite_schema(
                TestRestDotPathAndOrderedFallthrough._SEARCH_FALLTHROUGH_SCHEMA
            ),
            "pkg_deleg": fake_server.rewrite_schema(
                TestRestWireFormattingAndDelegation._DELEGATION_SCHEMA
            ),
            "pkg_dummy": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_dummyjson.kdl"
            ),
            "pkg_pet": fake_server.rewrite_schema(
                SCHEMAS_DIR / "rest_petstore.kdl"
            ),
            "pkg_closed": rewrite_schema_host(
                (SCHEMAS_DIR / "08_rest_basic.kdl").read_text(encoding="utf-8"),
                "http://127.0.0.1:1",
            ),
        }

        files: dict[str, str] = {}
        for pkg_name, kdl_src in schemas.items():
            res = parsmith.generate(
                kdl_src, "kotlin", package=pkg_name, module_stem=pkg_name
            )
            assert res.ok, f"Kotlin generate failed for {pkg_name}:\n{res.format_text()}"
            files[f"{pkg_name}.kt"] = res[f"{pkg_name}.kt"]

        files["Main.kt"] = r"""import java.net.http.HttpClient

fun checkCond(cond: Boolean, msg: String) {
    if (!cond) throw RuntimeException("ASSERTION FAILED: $msg")
}

fun main() {
    val client = HttpClient.newHttpClient()

    // 1. 08_rest_basic.kdl
    val r08 = pkg_08.API.fetch(client, "42")
    checkCond(r08 is pkg_08.FetchResult.Ok && r08.value.id == 42L && r08.value.name == "Alice", "08 ok")

    for (badId in listOf("null", "prim", "bad", "mismatch")) {
        val badRes = pkg_08.API.fetch(client, badId)
        checkCond(badRes is pkg_08.FetchResult.DecodeErr && badRes.status == 200, "08 decode $badId")
    }

    // 2. 09_rest_void.kdl & VoidAPI
    checkCond(pkg_09.API.fetch(client, "x") is pkg_09.FetchResult.Ok, "09 void")
    checkCond(pkg_void.VoidAPI.ping(client) is pkg_void.PingResult.Ok, "VoidAPI ping")
    checkCond(pkg_void.VoidAPI.deleteUser(client, 42L) is pkg_void.DeleteUserResult.Ok, "VoidAPI deleteUser")

    // 3. 10, 12, 13
    val r10 = pkg_10.API.fetch(client, "99")
    checkCond(r10 is pkg_10.FetchResult.Err404 && r10.status == 404 && r10.body.code == 404L && r10.body.message == "not found", "10 404")
    val r10Unk = pkg_10.API.fetch(client, "429")
    checkCond(r10Unk is pkg_10.FetchResult.UnknownErr && r10Unk.status == 429, "10 429")

    val r12 = pkg_12.API.fetch(client, "99")
    checkCond(r12 is pkg_12.FetchResult.Err404ErrorDetail && r12.body.error == "x" && r12.body.detail == "y", "12 match")
    val r12Unk = pkg_12.API.fetch(client, "404_nokeys")
    checkCond(r12Unk is pkg_12.FetchResult.UnknownErr && r12Unk.status == 404, "12 nokeys")

    val r13Err = pkg_13.API.fetch(client, "err200")
    checkCond(r13Err is pkg_13.FetchResult.Err200Detail && r13Err.body.detail == "msg", "13 err200")
    val r13Ok = pkg_13.API.fetch(client, "1")
    checkCond(r13Ok is pkg_13.FetchResult.Ok && r13Ok.value.name == "Alice", "13 ok")

    // 4. _SEARCH_FALLTHROUGH_SCHEMA
    val s200 = pkg_search.SearchAPI.fetch(client, "err200")
    checkCond(s200 is pkg_search.FetchResult.Err200Code && s200.body.code == 1001L, "search err200")
    val sOk = pkg_search.SearchAPI.fetch(client, "ok_empty_err")
    checkCond(sOk is pkg_search.FetchResult.Ok && sOk.value.id == 1L, "search ok_empty_err")
    val sSpec = pkg_search.SearchAPI.fetch(client, "404_specific")
    checkCond(sSpec is pkg_search.FetchResult.Err404NotFound && sSpec.body.code == 404L, "search 404_specific")
    val sFall = pkg_search.SearchAPI.fetch(client, "404_fallthrough")
    checkCond(sFall is pkg_search.FetchResult.Err404Message && sFall.body.message == "Fallback generic message", "search 404_fallthrough")
    val sEmpty = pkg_search.SearchAPI.fetch(client, "404_empty")
    checkCond(sEmpty is pkg_search.FetchResult.UnknownErr && sEmpty.status == 404, "search 404_empty")
    val sHtml = pkg_search.SearchAPI.fetch(client, "404_html")
    checkCond(sHtml is pkg_search.FetchResult.UnknownErr && sHtml.status == 404, "search 404_html")
    val s400 = pkg_search.SearchAPI.fetch(client, "400_mismatch")
    checkCond(s400 is pkg_search.FetchResult.DecodeErr && s400.status == 400, "search 400_mismatch")

    // 5. _DELEGATION_SCHEMA
    val du = pkg_deleg.UserAPI.updateUser(
        client,
        "s123",
        "v1",
        "Alice",
        null,
        null,
        null,
        listOf("a", "b"),
        listOf(1L, 2L),
        listOf("hello", "world"),
        listOf(10L, 20L),
        listOf("r1", "r2"),
        null,
        pkg_deleg.Rt.withHeader("X-Custom", "from-caller"),
        pkg_deleg.Rt.withParam("extra", "yes"),
        pkg_deleg.Rt.withCookie("c_extra", "cval"),
    )
    checkCond(du is pkg_deleg.UpdateUserResult.Ok && du.value.id == 99L && du.value.name == "Alice" && du.value.nickname == null, "delegation")

    // 6. rest_dummyjson.kdl
    val prod = pkg_dummy.DummyJsonApi.getProduct(client, 1L)
    checkCond(prod is pkg_dummy.GetProductResult.Ok && prod.value.id == 1L && prod.value.discountPercentage == 10.48, "dummy getProduct")
    checkCond(pkg_dummy.DummyJsonApi.getProduct(client, 999L) is pkg_dummy.GetProductResult.Err404, "dummy 404")
    val plist = pkg_dummy.DummyJsonApi.listProducts(client, 10L, 5L, listOf("title", "price"))
    checkCond(plist is pkg_dummy.ListProductsResult.Ok && plist.value.total == 2L, "dummy list")
    val sres = pkg_dummy.DummyJsonApi.searchProducts(client, "mascara", 5L)
    checkCond(sres is pkg_dummy.SearchProductsResult.Ok && sres.value.products[0].title == "Result for mascara", "dummy search")
    val d400 = pkg_dummy.DummyJsonApi.searchProducts(client, "error400")
    checkCond(d400 is pkg_dummy.SearchProductsResult.Err400 && d400.body.message == "Invalid search query", "dummy 400")
    val d500 = pkg_dummy.DummyJsonApi.searchProducts(client, "error500")
    checkCond(d500 is pkg_dummy.SearchProductsResult.Err500 && d500.body.message == "Search backend failure", "dummy 500")
    val cats = pkg_dummy.DummyJsonApi.listCategories(client)
    checkCond(cats is pkg_dummy.ListCategoriesResult.Ok && cats.value.size == 2 && cats.value[0].slug == "beauty", "dummy categories")
    val created = pkg_dummy.DummyJsonApi.createProduct(client, "Luxury Watch", 199.99, 5L)
    checkCond(created is pkg_dummy.CreateProductResult.Ok && created.value.id == 195L && created.value.title == "Luxury Watch", "dummy create")

    // 7. rest_petstore.kdl
    val pets = pkg_pet.PetStoreApi.listPets(client, 2L, 10L)
    checkCond(pets is pkg_pet.ListPetsResult.Ok && pets.value.pets.size == 2 && pets.value.pets[0].name == "Bella", "pet list")
    val dogs = pkg_pet.PetStoreApi.searchPets(client, "dog")
    checkCond(dogs is pkg_pet.SearchPetsResult.Ok && dogs.value.pets.size == 1, "pet search")
    val p422 = pkg_pet.PetStoreApi.searchPets(client, "invalid")
    checkCond(p422 is pkg_pet.SearchPetsResult.Err422 && p422.body.code == "UNPROCESSABLE_SPECIES", "pet 422")
    val bella = pkg_pet.PetStoreApi.getPet(client, 1L)
    checkCond(bella is pkg_pet.GetPetResult.Ok && bella.value.name == "Bella" && bella.value.owner?.fullName == "Sarah Connor", "pet get 1")
    val p503 = pkg_pet.PetStoreApi.getPet(client, 503L)
    checkCond(p503 is pkg_pet.GetPetResult.UnknownErr && p503.status == 503, "pet 503")

    // 8. Closed loopback port -> TransportErr
    val closedRes = pkg_closed.API.fetch(client, "1", pkg_closed.Rt.withTimeout(200L))
    checkCond(closedRes is pkg_closed.FetchResult.TransportErr, "closed port TransportErr")

    print("KOTLIN_REST_E2E_OK")
}
"""
        out = run_kotlin_custom_program(files)
        assert out == "KOTLIN_REST_E2E_OK"
