"""E2E integration tests for HTML struct and (raw)struct @request fetch methods against FakeServer."""

from __future__ import annotations

import asyncio
from pathlib import Path
import subprocess
import sys
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
    PY_HTML_TARGETS,
    SCHEMAS_DIR,
    assert_schema_lint_clean,
    loaded_py_module,
    run_csharp_custom_program,
    run_elixir_custom_program,
    run_go_custom_program,
    run_js_fetch,
    run_kotlin_custom_program,
    run_php_custom_program,
    run_rust_custom_program,
)

HTML_FETCH_SCHEMA_FILES: tuple[str, ...] = (
    "23_html_fetch.kdl",
    "24_html_fetch_query_placeholder.kdl",
    "29_html_fetch_params_cookies.kdl",
    "30_raw_struct_request.kdl",
)


@pytest.mark.parametrize("schema_file", HTML_FETCH_SCHEMA_FILES)
def test_html_fetch_schemas_are_lint_clean(schema_file: str) -> None:
    """Every HTML/@request schema in tests/integration/schemas/ passes parsmith.check()."""
    assert_schema_lint_clean(schema_file)


class TestSimpleHtmlFetchAllTransports:
    """23_html_fetch.kdl (SimplePage) across all 5 Python transports, 4 DOM backends, and Node.js."""

    @pytest.mark.parametrize("target", PY_HTML_TARGETS)
    def test_via_urllib_across_all_py_dom_targets(
        self, fake_server: FakeServer, target: str
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "23_html_fetch.kdl")
        with loaded_py_module(kdl_src, target=target) as (mod, _):
            page = mod.SimplePage.fetch(mod.via_urllib(), id="1")
            assert page.parse() == {"title": "Hello World"}

            page_42 = mod.SimplePage.fetch(mod.via_urllib(), id="42")
            assert page_42.parse() == {"title": "Post 42"}

    def test_sync_transports_httpx_and_requests(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "23_html_fetch.kdl")
        with loaded_py_module(kdl_src) as (mod, _):
            # 1. via_httpx with explicit httpx.Client
            with httpx.Client() as hclient:
                p_httpx = mod.SimplePage.fetch(
                    mod.via_httpx(hclient), id="1"
                )
                assert p_httpx.parse() == {"title": "Hello World"}

            # 2. via_httpx with explicit httpx2.Client
            with httpx2.Client() as h2client:
                p_httpx2 = mod.SimplePage.fetch(
                    mod.via_httpx(h2client), id="1"
                )
                assert p_httpx2.parse() == {"title": "Hello World"}

            # 3. via_requests with explicit requests.Session
            with requests.Session() as rsession:
                p_req = mod.SimplePage.fetch(
                    mod.via_requests(rsession), id="1"
                )
                assert p_req.parse() == {"title": "Hello World"}

    def test_async_transports_httpx_async_and_aiohttp(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "23_html_fetch.kdl")
        with loaded_py_module(kdl_src) as (mod, _):

            async def _run() -> None:
                async with httpx.AsyncClient() as aclient:
                    p1 = await mod.SimplePage.async_fetch(
                        mod.via_httpx_async(aclient), id="1"
                    )
                    assert p1.parse() == {"title": "Hello World"}

                async with httpx2.AsyncClient() as aclient2:
                    p1_v2 = await mod.SimplePage.async_fetch(
                        mod.via_httpx_async(aclient2), id="1"
                    )
                    assert p1_v2.parse() == {"title": "Hello World"}

                async with aiohttp.ClientSession() as asession:
                    p2 = await mod.SimplePage.async_fetch(
                        mod.via_aiohttp(asession), id="1"
                    )
                    assert p2.parse() == {"title": "Hello World"}

            asyncio.run(_run())

    def test_http_status_error_404_and_500_across_all_transports(
        self, fake_server: FakeServer
    ) -> None:
        status_kdl = fake_server.rewrite_schema(
            '''
struct StatusPage {
    @request """
    GET /users/{{id:str}} HTTP/1.1
    Host: example.com
    """
    title { css "h1"; text; fallback "" }
}
'''
        )
        cases: tuple[tuple[str, int, str], ...] = (
            ("404", 404, "not found"),
            ("500", 500, "internal"),
        )
        with loaded_py_module(status_kdl) as (mod, _):
            with (
                httpx.Client() as hclient,
                httpx2.Client() as h2client,
                requests.Session() as rsession,
            ):
                sync_transports = (
                    mod.via_urllib(),
                    mod.via_httpx(hclient),
                    mod.via_httpx(h2client),
                    mod.via_requests(rsession),
                )
                for tx in sync_transports:
                    for status_id, expected_status, expected_snippet in cases:
                        with pytest.raises(mod.HttpStatusError) as exc_info:
                            mod.StatusPage.fetch(tx, id=status_id)
                        err = exc_info.value
                        assert not isinstance(err, mod.TransportError)
                        assert err.status == expected_status
                        assert err.headers.get("x-request-id") == "abc123"
                        assert expected_snippet in err.text
                        assert isinstance(err.response, mod.RawResponse)
                        assert err.response.status == expected_status
                        assert err.response.headers == err.headers
                        assert err.response.text == err.text

            async def _run_async() -> None:
                async with (
                    httpx.AsyncClient() as aclient,
                    httpx2.AsyncClient() as aclient2,
                    aiohttp.ClientSession() as asession,
                ):
                    async_transports = (
                        mod.via_httpx_async(aclient),
                        mod.via_httpx_async(aclient2),
                        mod.via_aiohttp(asession),
                    )
                    for atx in async_transports:
                        for status_id, expected_status, expected_snippet in cases:
                            with pytest.raises(mod.HttpStatusError) as exc_info:
                                await mod.StatusPage.async_fetch(
                                    atx, id=status_id
                                )
                            err = exc_info.value
                            assert not isinstance(err, mod.TransportError)
                            assert err.status == expected_status
                            assert err.headers.get("x-request-id") == "abc123"
                            assert expected_snippet in err.text
                            assert isinstance(err.response, mod.RawResponse)
                            assert err.response.status == expected_status
                            assert err.response.headers == err.headers
                            assert err.response.text == err.text

            asyncio.run(_run_async())

    def test_mypy_strict_accepts_real_http_clients(
        self, tmp_path: Path
    ) -> None:
        kdl_src = (SCHEMAS_DIR / "23_html_fetch.kdl").read_text(
            encoding="utf-8"
        )
        res = parsmith.generate(
            kdl_src, "python", module_stem="gen_html_clients"
        )
        assert res.ok, res.format_text()

        gen_path = tmp_path / "gen_html_clients.py"
        gen_path.write_text(res["gen_html_clients.py"], encoding="utf-8")

        consumer_path = tmp_path / "check_real_clients.py"
        consumer_path.write_text(
            """from __future__ import annotations

import aiohttp
import httpx
import httpx2
import requests

import gen_html_clients as gen


def check_sync() -> None:
    tx_urllib: gen.SyncTransport = gen.via_urllib()
    tx_httpx: gen.SyncTransport = gen.via_httpx(httpx.Client())
    tx_httpx2: gen.SyncTransport = gen.via_httpx(httpx2.Client())
    tx_requests: gen.SyncTransport = gen.via_requests(requests.Session())
    for tx in (tx_urllib, tx_httpx, tx_httpx2, tx_requests):
        page: gen.SimplePage = gen.SimplePage.fetch(tx, id="1")
        _parsed: gen.SimplePageType = page.parse()


async def check_async() -> None:
    tx_httpx_async: gen.AsyncTransport = gen.via_httpx_async(httpx.AsyncClient())
    tx_httpx2_async: gen.AsyncTransport = gen.via_httpx_async(httpx2.AsyncClient())
    tx_aiohttp: gen.AsyncTransport = gen.via_aiohttp(aiohttp.ClientSession())
    for atx in (tx_httpx_async, tx_httpx2_async, tx_aiohttp):
        page: gen.SimplePage = await gen.SimplePage.async_fetch(atx, id="1")
        _parsed: gen.SimplePageType = page.parse()
""",
            encoding="utf-8",
        )

        proc = subprocess.run(
            [
                sys.executable,
                "-m",
                "mypy",
                "--strict",
                "--no-incremental",
                "--cache-dir",
                str(tmp_path / ".mypy_cache"),
                str(gen_path),
                str(consumer_path),
            ],
            capture_output=True,
            text=True,
            check=False,
        )
        assert proc.returncode == 0, (
            f"mypy --strict failed:\nstdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
        )

    def test_js_via_fetch(self, fake_server: FakeServer) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "23_html_fetch.kdl")
        res_pos = run_js_fetch(kdl_src, "SimplePage", "fetch", args=["1"])
        assert res_pos["parsed"] == {"title": "Hello World"}

        res_named = run_js_fetch(
            kdl_src, "SimplePage", "fetch", args=[{"id": "99"}]
        )
        assert res_named["parsed"] == {"title": "Post 99"}


class TestQueryPlaceholderAndCheckMethods:
    """24_html_fetch_query_placeholder.kdl (?32-{{username}}) and @check exists."""

    def test_query_placeholder_sync_and_async_py(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "24_html_fetch_query_placeholder.kdl"
        )
        with loaded_py_module(kdl_src) as (mod, _):
            page_ok = mod.ForumPage.fetch(mod.via_urllib(), username="alice")
            assert page_ok.exists() is True

            page_miss = mod.ForumPage.fetch(
                mod.via_urllib(), username="nobody"
            )
            assert page_miss.exists() is False

            async def _run_async() -> None:
                async with aiohttp.ClientSession() as asession:
                    p_ok = await mod.ForumPage.async_fetch(
                        mod.via_aiohttp(asession), username="alice"
                    )
                    assert p_ok.exists() is True
                    p_miss = await mod.ForumPage.async_fetch(
                        mod.via_aiohttp(asession), username="nobody"
                    )
                    assert p_miss.exists() is False

            asyncio.run(_run_async())

    def test_query_placeholder_js(self, fake_server: FakeServer) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "24_html_fetch_query_placeholder.kdl"
        )
        res_ok = run_js_fetch(
            kdl_src,
            "ForumPage",
            "fetch",
            args=[{"username": "alice"}],
            check_method="exists",
            skip_parse=True,
        )
        assert res_ok["checkResult"] is True

        res_miss = run_js_fetch(
            kdl_src,
            "ForumPage",
            "fetch",
            args=[{"username": "nobody"}],
            check_method="exists",
            skip_parse=True,
        )
        assert res_miss["checkResult"] is False


class TestParamsCookiesAndRawStructRequests:
    """29_html_fetch_params_cookies.kdl and 30_raw_struct_request.kdl across transports."""

    def test_params_and_cookies_in_same_html_struct(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "29_html_fetch_params_cookies.kdl"
        )
        expected = {"title": "id=7 tag=news session=abc tracking=t-900"}

        with loaded_py_module(kdl_src) as (mod, _):
            assert (
                mod.Page.fetch(
                    mod.via_urllib(), id=7, tag="news", tid="t-900"
                ).parse()
                == expected
            )
            with httpx.Client() as hclient:
                assert (
                    mod.Page.fetch(
                        mod.via_httpx(hclient), id=7, tag="news", tid="t-900"
                    ).parse()
                    == expected
                )
            with requests.Session() as rsession:
                assert (
                    mod.Page.fetch(
                        mod.via_requests(rsession),
                        id=7,
                        tag="news",
                        tid="t-900",
                    ).parse()
                    == expected
                )

            async def _run_async() -> None:
                async with httpx.AsyncClient() as aclient:
                    p_hx = await mod.Page.async_fetch(
                        mod.via_httpx_async(aclient),
                        id=7,
                        tag="news",
                        tid="t-900",
                    )
                    assert p_hx.parse() == expected
                async with aiohttp.ClientSession() as asession:
                    p_aio = await mod.Page.async_fetch(
                        mod.via_aiohttp(asession),
                        id=7,
                        tag="news",
                        tid="t-900",
                    )
                    assert p_aio.parse() == expected

            asyncio.run(_run_async())

        js_res = run_js_fetch(
            kdl_src,
            "Page",
            "fetch",
            args=[{"id": 7, "tag": "news", "tid": "t-900"}],
        )
        assert js_res["parsed"] == expected

    def test_raw_struct_curl_request(self, fake_server: FakeServer) -> None:
        kdl_src = fake_server.rewrite_schema(
            SCHEMAS_DIR / "30_raw_struct_request.kdl"
        )
        expected = {
            "proxies": [
                "10.0.0.1:8080",
                "10.0.0.2:3128",
                "10.0.0.3:9090",
            ]
        }

        with loaded_py_module(kdl_src) as (mod, _):
            assert (
                mod.RawProxyList.fetch(mod.via_urllib()).parse() == expected
            )
            with httpx.Client() as hclient:
                assert (
                    mod.RawProxyList.fetch(mod.via_httpx(hclient)).parse()
                    == expected
                )
            with requests.Session() as rsession:
                assert (
                    mod.RawProxyList.fetch(mod.via_requests(rsession)).parse()
                    == expected
                )

            async def _run_async() -> None:
                async with httpx.AsyncClient() as aclient:
                    r1 = await mod.RawProxyList.async_fetch(
                        mod.via_httpx_async(aclient)
                    )
                    assert r1.parse() == expected
                async with aiohttp.ClientSession() as asession:
                    r2 = await mod.RawProxyList.async_fetch(
                        mod.via_aiohttp(asession)
                    )
                    assert r2.parse() == expected

            asyncio.run(_run_async())

        js_res = run_js_fetch(kdl_src, "RawProxyList", "fetch")
        assert js_res["parsed"] == expected

    def test_raw_playlist_list_struct_fetch(
        self, fake_server: FakeServer
    ) -> None:
        raw_playlist_kdl = r'''
(raw)struct PlaylistStream {
    @request """
    GET /playlist.txt HTTP/1.1
    Host: example.com
    """
    @split-doc { split "\n" }
    quality { re #"\[(\d+p)\]"# }
    url { re #"\](.+)"# }
}
'''
        kdl_src = fake_server.rewrite_schema(raw_playlist_kdl)
        expected = [
            {"quality": "480p", "url": "/v/01_480.m3u8"},
            {"quality": "720p", "url": "/v/01_720.m3u8"},
        ]
        with loaded_py_module(kdl_src) as (mod, _):
            assert (
                mod.PlaylistStream.fetch(mod.via_urllib()).parse() == expected
            )
        js_res = run_js_fetch(kdl_src, "PlaylistStream", "fetch")
        assert js_res["parsed"] == expected


class TestNamedRequestsFormPostResponseJoinAndKwargsMerge:
    """Named @request, form POST body, response-path + response-join, None omission, and modes."""

    _ARTICLE_AND_LOG_SCHEMA = '''
struct ArticlePage {
    @request """
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
    @request response-path="payload.lines" response-join="\\n" """
    GET /logs/{{stream_id:str}} HTTP/1.1
    Host: example.com
    """

    lines {
        split "\\n"
    }
}
'''

    _SEARCH_MERGE_SCHEMA = '''
struct SearchPage {
    @request """
    GET /html/search?q={{q:str}}&lang={{lang:str?}} HTTP/1.1
    Host: example.com
    X-Static: 1
    X-Opt: {{opt_hdr:str?}}
    """
    title { css "h1"; text }
    lang { css "p.lang"; text }
    hdr { css "p.hdr"; text }
    page { css "p.page"; text }
}
'''

    def test_named_form_post_and_response_join_across_py_and_js(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(self._ARTICLE_AND_LOG_SCHEMA)

        with loaded_py_module(kdl_src) as (mod, _):
            # 1. GET /articles/<id> with array query styles
            art = mod.ArticlePage.fetch(
                mod.via_urllib(),
                id=7,
                sid="s1",
                tags=["a", "b"],
                pipes=[1, 2],
                spaces=["x", "y"],
                brackets=[10, 20],
                repeats=["r1", "r2"],
            )
            assert art.parse() == {
                "heading": "Article 7",
                "summary": "Summary 7",
            }

            # 2. POST /articles/slug/<slug> (form body with omitted and provided optional token)
            slug_no_tok = mod.ArticlePage.fetch_by_slug(
                mod.via_urllib(), slug="hello-world"
            )
            assert slug_no_tok.parse() == {
                "heading": "Slug Article: hello-world",
                "summary": "From POST mode=preview",
            }

            with requests.Session() as rsession:
                slug_with_tok = mod.ArticlePage.fetch_by_slug(
                    mod.via_requests(rsession),
                    slug="rust-guide",
                    token="tok99",
                )
                assert slug_with_tok.parse() == {
                    "heading": "Slug Article: rust-guide",
                    "summary": "From POST mode=preview token=tok99",
                }

            # 3. (raw)struct with response-path="payload.lines" + response-join="\n"
            log_res = mod.ChunkedLog.fetch(
                mod.via_urllib(), stream_id="s42"
            ).parse()
            assert log_res == {
                "lines": ["s42:line-1", "s42:line-2", "s42:line-3"]
            }

            async def _run_async() -> None:
                async with aiohttp.ClientSession() as asession:
                    a_slug = await mod.ArticlePage.async_fetch_by_slug(
                        mod.via_aiohttp(asession),
                        slug="async-post",
                        token="tk_async",
                    )
                    assert a_slug.parse() == {
                        "heading": "Slug Article: async-post",
                        "summary": "From POST mode=preview token=tk_async",
                    }
                    a_log = await mod.ChunkedLog.async_fetch(
                        mod.via_aiohttp(asession), stream_id="async9"
                    )
                    assert a_log.parse() == {
                        "lines": [
                            "async9:line-1",
                            "async9:line-2",
                            "async9:line-3",
                        ]
                    }

            asyncio.run(_run_async())

        js_slug1 = run_js_fetch(
            kdl_src,
            "ArticlePage",
            "fetchBySlug",
            args=[{"slug": "js-slug"}],
        )
        assert js_slug1["parsed"] == {
            "heading": "Slug Article: js-slug",
            "summary": "From POST mode=preview",
        }

        js_slug2 = run_js_fetch(
            kdl_src,
            "ArticlePage",
            "fetch_by_slug",
            args=[{"slug": "js-slug-2", "token": "jtok"}],
        )
        assert js_slug2["parsed"] == {
            "heading": "Slug Article: js-slug-2",
            "summary": "From POST mode=preview token=jtok",
        }

        js_log = run_js_fetch(
            kdl_src,
            "ChunkedLog",
            "fetch",
            args=[{"streamId": "js7"}],
        )
        assert js_log["parsed"] == {
            "lines": ["js7:line-1", "js7:line-2", "js7:line-3"]
        }

    def test_none_omission_and_caller_kwargs_merge(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(self._SEARCH_MERGE_SCHEMA)

        with loaded_py_module(kdl_src) as (mod, _):
            # lang and opt_hdr default to None -> omitted; caller headers and params merged
            with httpx.Client() as hclient:
                page = mod.SearchPage.fetch(
                    mod.via_httpx(hclient),
                    q="rust",
                    headers={"X-Caller": "2"},
                    params={"page": "3"},
                )
            assert page.parse() == {
                "title": "Search: rust",
                "lang": "",
                "hdr": "1||2",
                "page": "3",
            }

            # When optional lang and opt_hdr are provided
            page_full = mod.SearchPage.fetch(
                mod.via_urllib(),
                q="kdl",
                lang="en",
                opt_hdr="opt-v",
                headers={"X-Caller": "9"},
                params={"page": "1"},
            )
            assert page_full.parse() == {
                "title": "Search: kdl",
                "lang": "en",
                "hdr": "1|opt-v|9",
                "page": "1",
            }

        js_res = run_js_fetch(
            kdl_src,
            "SearchPage",
            "fetch",
            args=[
                {"q": "rust"},
                {"headers": {"X-Caller": "2"}, "params": {"page": "3"}},
            ],
        )
        assert js_res["parsed"] == {
            "title": "Search: rust",
            "lang": "",
            "hdr": "1||2",
            "page": "3",
        }

    def test_dataclass_separate_runtime_and_http_io_modes(
        self, fake_server: FakeServer
    ) -> None:
        kdl_src = fake_server.rewrite_schema(SCHEMAS_DIR / "23_html_fetch.kdl")

        # 1. models="dataclass" + separate_runtime=True
        with loaded_py_module(
            kdl_src, models="dataclass", separate_runtime=True
        ) as (mod_dc, rt_mod):
            assert rt_mod is not None
            page_dc = mod_dc.SimplePage.fetch(rt_mod.via_urllib(), id="1")
            parsed_dc = page_dc.parse()
            assert isinstance(parsed_dc, mod_dc.SimplePageType)
            assert parsed_dc.title == "Hello World"

        # 2. http_io="sync" emits only fetch
        with loaded_py_module(kdl_src, http_io="sync") as (mod_sync, _):
            assert hasattr(mod_sync.SimplePage, "fetch")
            assert not hasattr(mod_sync.SimplePage, "async_fetch")
            assert mod_sync.SimplePage.fetch(
                mod_sync.via_urllib(), id="1"
            ).parse() == {"title": "Hello World"}

        # 3. http_io="async" emits only async_fetch
        with loaded_py_module(kdl_src, http_io="async") as (mod_async, _):
            assert not hasattr(mod_async.SimplePage, "fetch")
            assert hasattr(mod_async.SimplePage, "async_fetch")

            async def _run() -> Any:
                async with httpx.AsyncClient() as aclient:
                    p = await mod_async.SimplePage.async_fetch(
                        mod_async.via_httpx_async(aclient), id="1"
                    )
                    return p.parse()

            assert asyncio.run(_run()) == {"title": "Hello World"}

    def test_transport_error_on_closed_loopback_port(self) -> None:
        unreachable_kdl = rewrite_schema_host(
            (SCHEMAS_DIR / "23_html_fetch.kdl").read_text(encoding="utf-8"),
            "http://127.0.0.1:1",
        )
        with loaded_py_module(unreachable_kdl) as (mod, _):
            with pytest.raises(mod.TransportError) as exc_urllib:
                mod.SimplePage.fetch(mod.via_urllib(timeout=0.2), id="1")
            assert isinstance(exc_urllib.value.cause, urllib.error.URLError)

            with httpx.Client(timeout=0.2) as hclient:
                with pytest.raises(mod.TransportError) as exc_httpx:
                    mod.SimplePage.fetch(mod.via_httpx(hclient), id="1")
                assert isinstance(exc_httpx.value.cause, httpx.HTTPError)

            with httpx2.Client(timeout=0.2) as h2client:
                with pytest.raises(mod.TransportError) as exc_httpx2:
                    mod.SimplePage.fetch(mod.via_httpx(h2client), id="1")
                assert isinstance(exc_httpx2.value.cause, httpx2.HTTPError)

            with requests.Session() as rsession:
                with pytest.raises(mod.TransportError) as exc_req:
                    mod.SimplePage.fetch(
                        mod.via_requests(rsession), id="1", timeout=0.2
                    )
                assert isinstance(
                    exc_req.value.cause, requests.RequestException
                )

            async def _run_async_errors() -> None:
                async with httpx.AsyncClient(timeout=0.2) as aclient:
                    with pytest.raises(mod.TransportError) as exc_ahx:
                        await mod.SimplePage.async_fetch(
                            mod.via_httpx_async(aclient), id="1"
                        )
                    assert isinstance(exc_ahx.value.cause, httpx.HTTPError)

                async with httpx2.AsyncClient(timeout=0.2) as aclient2:
                    with pytest.raises(mod.TransportError) as exc_ahx2:
                        await mod.SimplePage.async_fetch(
                            mod.via_httpx_async(aclient2), id="1"
                        )
                    assert isinstance(exc_ahx2.value.cause, httpx2.HTTPError)

                timeout = aiohttp.ClientTimeout(sock_connect=0.2)
                async with aiohttp.ClientSession(timeout=timeout) as asession:
                    with pytest.raises(mod.TransportError) as exc_aio:
                        await mod.SimplePage.async_fetch(
                            mod.via_aiohttp(asession), id="1"
                        )
                    assert isinstance(exc_aio.value.cause, aiohttp.ClientError)

            asyncio.run(_run_async_errors())

        with pytest.raises(RuntimeError, match="TransportError"):
            run_js_fetch(unreachable_kdl, "SimplePage", "fetch", args=["1"])


class TestGoAndRustHtmlFetchE2E:
    """Compile and execute generated Go and Rust @request HTML and (raw)struct fetch methods against FakeServer."""

    def test_go_html_and_raw_fetch_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        combined_kdl = "\n\n".join(
            [
                fake_server.rewrite_schema(SCHEMAS_DIR / "23_html_fetch.kdl"),
                fake_server.rewrite_schema(
                    SCHEMAS_DIR / "24_html_fetch_query_placeholder.kdl"
                ),
                fake_server.rewrite_schema(
                    SCHEMAS_DIR / "29_html_fetch_params_cookies.kdl"
                ),
                fake_server.rewrite_schema(
                    SCHEMAS_DIR / "30_raw_struct_request.kdl"
                ),
                fake_server.rewrite_schema(
                    TestNamedRequestsFormPostResponseJoinAndKwargsMerge._ARTICLE_AND_LOG_SCHEMA
                ),
            ]
        )
        res_main = parsmith.generate(
            combined_kdl, "go", package="main", module_stem="fetch_schemas"
        )
        assert res_main.ok, res_main.format_text()

        unreachable_kdl = rewrite_schema_host(
            (SCHEMAS_DIR / "23_html_fetch.kdl").read_text(encoding="utf-8"),
            "http://127.0.0.1:1",
        )
        res_closed = parsmith.generate(
            unreachable_kdl,
            "go",
            package="pkg_closed",
            module_stem="closed",
        )
        assert res_closed.ok, res_closed.format_text()

        files = {
            "fetch_schemas.go": res_main["fetch_schemas.go"],
            "pkg_closed/closed.go": res_closed["closed.go"],
            "main.go": r"""package main

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"os"
	"reflect"
	"time"

	"parsmith_e2e_runner/pkg_closed"
)

func assert(cond bool, msg string) {
	if !cond {
		fmt.Fprintln(os.Stderr, "ASSERTION FAILED:", msg)
		os.Exit(1)
	}
}

func main() {
	ctx := context.Background()
	client := &http.Client{Timeout: 5 * time.Second}

	// 1. 23_html_fetch.kdl (SimplePage)
	p1, err := NewSimplePageFetch(ctx, client, "1")
	assert(err == nil && p1.Parse().Title == "Hello World", fmt.Sprintf("23 id=1: %v", err))
	p42, err := NewSimplePageFetch(ctx, client, "42")
	assert(err == nil && p42.Parse().Title == "Post 42", fmt.Sprintf("23 id=42: %v", err))

	// 2. 24_html_fetch_query_placeholder.kdl (ForumPage + @check exists)
	fOk, err := NewForumPageFetch(ctx, client, "alice")
	assert(err == nil && fOk.exists(), fmt.Sprintf("24 alice: %v", err))
	fMiss, err := NewForumPageFetch(ctx, client, "nobody")
	assert(err == nil && !fMiss.exists(), fmt.Sprintf("24 nobody: %v", err))

	// 3. 29_html_fetch_params_cookies.kdl (Page)
	pg, err := NewPageFetch(ctx, client, "t-900", 7, "news")
	assert(err == nil && pg.Parse().Title == "id=7 tag=news session=abc tracking=t-900", fmt.Sprintf("29 page: %v %+v", err, pg))

	// 4. 30_raw_struct_request.kdl (RawProxyList)
	rawList, err := NewRawProxyListFetch(ctx, client)
	expectedProxies := []string{"10.0.0.1:8080", "10.0.0.2:3128", "10.0.0.3:9090"}
	assert(err == nil && reflect.DeepEqual(rawList.Parse().Proxies, expectedProxies), fmt.Sprintf("30 raw: %v %+v", err, rawList))

	// 5. _ARTICLE_AND_LOG_SCHEMA (ArticlePage GET + form POST & ChunkedLog response-path + response-join)
	art, err := NewArticlePageFetch(
		ctx,
		client,
		7,
		"s1",
		"",
		"",
		"",
		[]string{"a", "b"},
		[]int64{1, 2},
		[]string{"x", "y"},
		[]int64{10, 20},
		[]string{"r1", "r2"},
	)
	assert(err == nil && art.Parse().Heading == "Article 7" && art.Parse().Summary == "Summary 7", fmt.Sprintf("ArticlePage GET: %v", err))

	slugArt, err := NewArticlePageBySlug(ctx, client, "rust-guide", "tok99")
	assert(err == nil && slugArt.Parse().Heading == "Slug Article: rust-guide" && slugArt.Parse().Summary == "From POST mode=preview token=tok99", fmt.Sprintf("ArticlePage POST: %v %+v", err, slugArt))

	logRes, err := NewChunkedLogFetch(ctx, client, "s42")
	expectedLines := []string{"s42:line-1", "s42:line-2", "s42:line-3"}
	assert(err == nil && reflect.DeepEqual(logRes.Parse().Lines, expectedLines), fmt.Sprintf("ChunkedLog: %v %+v", err, logRes))

	// 6. Closed loopback port http://127.0.0.1:1 -> *TransportErr
	_, err = pkg_closed.NewSimplePageFetch(ctx, &http.Client{Timeout: 200 * time.Millisecond}, "1")
	var tErr *pkg_closed.TransportErr
	assert(errors.As(err, &tErr) && tErr.Cause != "", fmt.Sprintf("closed port: %T %v", err, err))

	fmt.Print("GO_HTML_FETCH_E2E_OK")
}
""",
        }
        out = run_go_custom_program(files)
        assert out == "GO_HTML_FETCH_E2E_OK"

    def test_rust_html_and_raw_fetch_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "mod_23": fake_server.rewrite_schema(
                SCHEMAS_DIR / "23_html_fetch.kdl"
            ),
            "mod_24": fake_server.rewrite_schema(
                SCHEMAS_DIR / "24_html_fetch_query_placeholder.kdl"
            ),
            "mod_29": fake_server.rewrite_schema(
                SCHEMAS_DIR / "29_html_fetch_params_cookies.kdl"
            ),
            "mod_30": fake_server.rewrite_schema(
                SCHEMAS_DIR / "30_raw_struct_request.kdl"
            ),
            "mod_art": fake_server.rewrite_schema(
                TestNamedRequestsFormPostResponseJoinAndKwargsMerge._ARTICLE_AND_LOG_SCHEMA
            ),
            "mod_closed": rewrite_schema_host(
                (SCHEMAS_DIR / "23_html_fetch.kdl").read_text(encoding="utf-8"),
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
            if mod_name == "mod_art":
                files["parsmith_runtime.rs"] = res["parsmith_runtime.rs"]

        files["main.rs"] = r"""#![allow(dead_code, unused_imports)]
mod parsmith_runtime;
mod mod_23;
mod mod_24;
mod mod_29;
mod mod_30;
mod mod_art;
mod mod_closed;

#[tokio::main]
async fn main() {
    let client = reqwest::Client::new();

    // 1. 23_html_fetch.kdl (SimplePage)
    let mut p1 = mod_23::SimplePageParser::fetch(&client, "1").await.unwrap();
    assert_eq!(p1.parse().unwrap().title, "Hello World");
    let mut p42 = mod_23::SimplePageParser::fetch(&client, "42").await.unwrap();
    assert_eq!(p42.parse().unwrap().title, "Post 42");

    // 2. 24_html_fetch_query_placeholder.kdl (ForumPage + @check exists)
    let mut f_ok = mod_24::ForumPageParser::fetch(&client, "alice").await.unwrap();
    assert!(f_ok.exists().unwrap());
    let mut f_miss = mod_24::ForumPageParser::fetch(&client, "nobody").await.unwrap();
    assert!(!f_miss.exists().unwrap());

    // 3. 29_html_fetch_params_cookies.kdl (Page)
    let mut pg = mod_29::PageParser::fetch(&client, "t-900", 7, "news").await.unwrap();
    assert_eq!(
        pg.parse().unwrap().title,
        "id=7 tag=news session=abc tracking=t-900"
    );

    // 4. 30_raw_struct_request.kdl (RawProxyList)
    let mut raw_list = mod_30::RawProxyListParser::fetch(&client).await.unwrap();
    assert_eq!(
        raw_list.parse().unwrap().proxies,
        vec![
            "10.0.0.1:8080".to_string(),
            "10.0.0.2:3128".to_string(),
            "10.0.0.3:9090".to_string()
        ]
    );

    // 5. _ARTICLE_AND_LOG_SCHEMA (ArticlePage GET + form POST & ChunkedLog response-path + response-join)
    let mut art = mod_art::ArticlePageParser::fetch(
        &client,
        7,
        "s1",
        None,
        None,
        None,
        Some(&["a", "b"]),
        Some(&[1, 2]),
        Some(&["x", "y"]),
        Some(&[10, 20]),
        Some(&["r1", "r2"]),
    )
    .await
    .unwrap();
    let art_res = art.parse().unwrap();
    assert_eq!(art_res.heading, "Article 7");
    assert_eq!(art_res.summary, "Summary 7");

    let mut slug_no_tok = mod_art::ArticlePageParser::fetch_by_slug(&client, "hello-world", None)
        .await
        .unwrap();
    let slug_no_tok_res = slug_no_tok.parse().unwrap();
    assert_eq!(slug_no_tok_res.heading, "Slug Article: hello-world");
    assert_eq!(slug_no_tok_res.summary, "From POST mode=preview");

    let mut slug_tok = mod_art::ArticlePageParser::fetch_by_slug(&client, "rust-guide", Some("tok99"))
        .await
        .unwrap();
    let slug_tok_res = slug_tok.parse().unwrap();
    assert_eq!(slug_tok_res.heading, "Slug Article: rust-guide");
    assert_eq!(slug_tok_res.summary, "From POST mode=preview token=tok99");

    let mut log_p = mod_art::ChunkedLogParser::fetch(&client, "s42").await.unwrap();
    assert_eq!(
        log_p.parse().unwrap().lines,
        vec![
            "s42:line-1".to_string(),
            "s42:line-2".to_string(),
            "s42:line-3".to_string()
        ]
    );

    // 6. Closed loopback port http://127.0.0.1:1 -> Err(RtError)
    let closed_res = mod_closed::SimplePageParser::fetch(&client, "1").await;
    assert!(closed_res.is_err());

    print!("RUST_HTML_FETCH_E2E_OK");
}
"""
        out = run_rust_custom_program(files)
        assert out == "RUST_HTML_FETCH_E2E_OK"

    def test_elixir_html_and_raw_fetch_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "Mod23": fake_server.rewrite_schema(
                SCHEMAS_DIR / "23_html_fetch.kdl"
            ),
            "Mod24": fake_server.rewrite_schema(
                SCHEMAS_DIR / "24_html_fetch_query_placeholder.kdl"
            ),
            "Mod29": fake_server.rewrite_schema(
                SCHEMAS_DIR / "29_html_fetch_params_cookies.kdl"
            ),
            "Mod30": fake_server.rewrite_schema(
                SCHEMAS_DIR / "30_raw_struct_request.kdl"
            ),
            "ModArt": fake_server.rewrite_schema(
                TestNamedRequestsFormPostResponseJoinAndKwargsMerge._ARTICLE_AND_LOG_SCHEMA
            ),
            "ModClosed": rewrite_schema_host(
                (SCHEMAS_DIR / "23_html_fetch.kdl").read_text(encoding="utf-8"),
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
client = Mod23.Rt.via_httpc()

{:ok, p1} = Mod23.SimplePage.fetch(client, "1")
"Hello World" = Mod23.SimplePage.parse(p1).title

{:ok, p42} = Mod23.SimplePage.fetch(client, "42")
"Post 42" = Mod23.SimplePage.parse(p42).title

{:ok, f_ok} = Mod24.ForumPage.fetch(client, "alice")
true = Mod24.ForumPage.exists(f_ok)

{:ok, f_miss} = Mod24.ForumPage.fetch(client, "nobody")
false = Mod24.ForumPage.exists(f_miss)

{:ok, pg} = Mod29.Page.fetch(client, "t-900", 7, "news")
"id=7 tag=news session=abc tracking=t-900" = Mod29.Page.parse(pg).title

{:ok, raw_list} = Mod30.RawProxyList.fetch(client)
["10.0.0.1:8080", "10.0.0.2:3128", "10.0.0.3:9090"] = Mod30.RawProxyList.parse(raw_list).proxies

{:ok, art} = ModArt.ArticlePage.fetch(
  client,
  7,
  "s1",
  nil,
  nil,
  nil,
  ["a", "b"],
  [1, 2],
  ["x", "y"],
  [10, 20],
  ["r1", "r2"]
)
art_res = ModArt.ArticlePage.parse(art)
"Article 7" = art_res.heading
"Summary 7" = art_res.summary

slug_art = ModArt.ArticlePage.fetch_by_slug!(client, "rust-guide", "tok99")
slug_res = ModArt.ArticlePage.parse(slug_art)
"Slug Article: rust-guide" = slug_res.heading
"From POST mode=preview token=tok99" = slug_res.summary

log_p = ModArt.ChunkedLog.fetch!(client, "s42")
["s42:line-1", "s42:line-2", "s42:line-3"] = ModArt.ChunkedLog.parse(log_p).lines

{:error, %ModClosed.Rt.TransportError{}} = ModClosed.SimplePage.fetch(client, "1", timeout: 200)

IO.write("ELIXIR_HTML_FETCH_E2E_OK")
"""
        out = run_elixir_custom_program(files, eval_code)
        assert out == "ELIXIR_HTML_FETCH_E2E_OK"

    def test_php_html_and_raw_fetch_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "Ns23": fake_server.rewrite_schema(
                SCHEMAS_DIR / "23_html_fetch.kdl"
            ),
            "Ns24": fake_server.rewrite_schema(
                SCHEMAS_DIR / "24_html_fetch_query_placeholder.kdl"
            ),
            "Ns29": fake_server.rewrite_schema(
                SCHEMAS_DIR / "29_html_fetch_params_cookies.kdl"
            ),
            "Ns30": fake_server.rewrite_schema(
                SCHEMAS_DIR / "30_raw_struct_request.kdl"
            ),
            "NsArt": fake_server.rewrite_schema(
                TestNamedRequestsFormPostResponseJoinAndKwargsMerge._ARTICLE_AND_LOG_SCHEMA
            ),
            "NsClosed": rewrite_schema_host(
                (SCHEMAS_DIR / "23_html_fetch.kdl").read_text(encoding="utf-8"),
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

require_once __DIR__ . '/ns23.php';
require_once __DIR__ . '/ns24.php';
require_once __DIR__ . '/ns29.php';
require_once __DIR__ . '/ns30.php';
require_once __DIR__ . '/nsart.php';
require_once __DIR__ . '/nsclosed.php';

function assert_true(bool $cond, string $msg): void {
    if (!$cond) {
        fwrite(STDERR, "ASSERTION FAILED: $msg\n");
        exit(1);
    }
}

$p1 = \Ns23\SimplePage::fetch(\Ns23\Rt::viaStream(5.0), '1');
assert_true($p1->parse()->title === 'Hello World', '23 id=1');

$p42 = \Ns23\SimplePage::fetch(\Ns23\Rt::viaCurl(5.0), '42');
assert_true($p42->parse()->title === 'Post 42', '23 id=42 viaCurl');

$fOk = \Ns24\ForumPage::fetch(\Ns24\Rt::viaStream(5.0), 'alice');
assert_true($fOk->exists() === true, '24 alice');
$fMiss = \Ns24\ForumPage::fetch(\Ns24\Rt::viaStream(5.0), 'nobody');
assert_true($fMiss->exists() === false, '24 nobody');

$pg = \Ns29\Page::fetch(\Ns29\Rt::viaStream(5.0), 't-900', 7, 'news');
assert_true($pg->parse()->title === 'id=7 tag=news session=abc tracking=t-900', '29 page');

$rawList = \Ns30\RawProxyList::fetch(\Ns30\Rt::viaStream(5.0));
assert_true($rawList->parse()->proxies === ['10.0.0.1:8080', '10.0.0.2:3128', '10.0.0.3:9090'], '30 proxies');

$art = \NsArt\ArticlePage::fetch(
    \NsArt\Rt::viaStream(5.0),
    7,
    's1',
    null,
    null,
    null,
    ['a', 'b'],
    [1, 2],
    ['x', 'y'],
    [10, 20],
    ['r1', 'r2'],
);
assert_true($art->parse()->heading === 'Article 7' && $art->parse()->summary === 'Summary 7', 'ArticlePage GET');

$slugArt = \NsArt\ArticlePage::fetch_by_slug(\NsArt\Rt::viaStream(5.0), 'rust-guide', 'tok99');
assert_true($slugArt->parse()->heading === 'Slug Article: rust-guide' && $slugArt->parse()->summary === 'From POST mode=preview token=tok99', 'ArticlePage POST');

$logRes = \NsArt\ChunkedLog::fetch(\NsArt\Rt::viaStream(5.0), 's42');
assert_true($logRes->parse()->lines === ['s42:line-1', 's42:line-2', 's42:line-3'], 'ChunkedLog');

$threwTransport = false;
try {
    \NsClosed\SimplePage::fetch(\NsClosed\Rt::viaStream(0.2), '1');
} catch (\NsClosed\TransportError $e) {
    $threwTransport = true;
}
assert_true($threwTransport, 'Closed port TransportError');

echo 'PHP_HTML_FETCH_E2E_OK';
"""
        out = run_php_custom_program(files)
        assert out == "PHP_HTML_FETCH_E2E_OK"

    def test_csharp_html_and_raw_fetch_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "Ns23": fake_server.rewrite_schema(
                SCHEMAS_DIR / "23_html_fetch.kdl"
            ),
            "Ns24": fake_server.rewrite_schema(
                SCHEMAS_DIR / "24_html_fetch_query_placeholder.kdl"
            ),
            "Ns29": fake_server.rewrite_schema(
                SCHEMAS_DIR / "29_html_fetch_params_cookies.kdl"
            ),
            "Ns30": fake_server.rewrite_schema(
                SCHEMAS_DIR / "30_raw_struct_request.kdl"
            ),
            "NsArt": fake_server.rewrite_schema(
                TestNamedRequestsFormPostResponseJoinAndKwargsMerge._ARTICLE_AND_LOG_SCHEMA
            ),
            "NsClosed": rewrite_schema_host(
                (SCHEMAS_DIR / "23_html_fetch.kdl").read_text(encoding="utf-8"),
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

        var p1 = await Ns23.SimplePage.FetchAsync(client, "1");
        Assert(p1.Parse().Title == "Hello World", "23 id=1");
        var p42 = Ns23.SimplePage.Fetch(client, "42");
        Assert(p42.Parse().Title == "Post 42", "23 id=42");

        var fOk = Ns24.ForumPage.Fetch(client, "alice");
        Assert(fOk.Exists(), "24 alice");
        var fMiss = Ns24.ForumPage.Fetch(client, "nobody");
        Assert(!fMiss.Exists(), "24 nobody");

        var pg = Ns29.Page.Fetch(client, "t-900", 7L, "news");
        Assert(pg.Parse().Title == "id=7 tag=news session=abc tracking=t-900", "29 page");

        var rawList = Ns30.RawProxyList.Fetch(client);
        Assert(string.Join(",", rawList.Parse().Proxies) == "10.0.0.1:8080,10.0.0.2:3128,10.0.0.3:9090", "30 proxies");

        var art = await NsArt.ArticlePage.FetchAsync(
            client,
            7L,
            "s1",
            null,
            null,
            null,
            new[] { "a", "b" },
            new[] { 1L, 2L },
            new[] { "x", "y" },
            new[] { 10L, 20L },
            new[] { "r1", "r2" });
        var artRes = art.Parse();
        Assert(artRes.Heading == "Article 7" && artRes.Summary == "Summary 7", "ArticlePage GET");

        var slugArt = NsArt.ArticlePage.FetchBySlug(client, "rust-guide", "tok99");
        var slugRes = slugArt.Parse();
        Assert(slugRes.Heading == "Slug Article: rust-guide" && slugRes.Summary == "From POST mode=preview token=tok99", "ArticlePage POST");

        var logP = NsArt.ChunkedLog.Fetch(client, "s42");
        Assert(string.Join(",", logP.Parse().Lines) == "s42:line-1,s42:line-2,s42:line-3", "ChunkedLog");

        bool threwClosed = false;
        try
        {
            NsClosed.SimplePage.Fetch(client, "1", NsClosed.Rt.WithTimeout(TimeSpan.FromMilliseconds(200)));
        }
        catch (NsClosed.Rt.TransportError)
        {
            threwClosed = true;
        }
        Assert(threwClosed, "closed port TransportError");

        Console.Write("CSHARP_HTML_FETCH_E2E_OK");
    }
}
"""
        out = run_csharp_custom_program(files)
        assert out == "CSHARP_HTML_FETCH_E2E_OK"

    def test_kotlin_html_and_raw_fetch_against_fake_server(
        self, fake_server: FakeServer
    ) -> None:
        schemas = {
            "pkg_23": fake_server.rewrite_schema(
                SCHEMAS_DIR / "23_html_fetch.kdl"
            ),
            "pkg_24": fake_server.rewrite_schema(
                SCHEMAS_DIR / "24_html_fetch_query_placeholder.kdl"
            ),
            "pkg_29": fake_server.rewrite_schema(
                SCHEMAS_DIR / "29_html_fetch_params_cookies.kdl"
            ),
            "pkg_30": fake_server.rewrite_schema(
                SCHEMAS_DIR / "30_raw_struct_request.kdl"
            ),
            "pkg_art": fake_server.rewrite_schema(
                TestNamedRequestsFormPostResponseJoinAndKwargsMerge._ARTICLE_AND_LOG_SCHEMA
            ),
            "pkg_closed": rewrite_schema_host(
                (SCHEMAS_DIR / "23_html_fetch.kdl").read_text(encoding="utf-8"),
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

    val p1 = pkg_23.SimplePage.fetch(client, "1")
    checkCond(p1.parse().title == "Hello World", "23 id=1")
    val p42 = pkg_23.SimplePage.fetch(client, "42")
    checkCond(p42.parse().title == "Post 42", "23 id=42")

    val fOk = pkg_24.ForumPage.fetch(client, "alice")
    checkCond(fOk.exists(), "24 alice")
    val fMiss = pkg_24.ForumPage.fetch(client, "nobody")
    checkCond(!fMiss.exists(), "24 nobody")

    val pg = pkg_29.Page.fetch(client, "t-900", 7L, "news")
    checkCond(pg.parse().title == "id=7 tag=news session=abc tracking=t-900", "29 page")

    val rawList = pkg_30.RawProxyList.fetch(client)
    checkCond(rawList.parse().proxies == listOf("10.0.0.1:8080", "10.0.0.2:3128", "10.0.0.3:9090"), "30 proxies")

    val art = pkg_art.ArticlePage.fetch(
        client,
        7L,
        "s1",
        null,
        null,
        null,
        listOf("a", "b"),
        listOf(1L, 2L),
        listOf("x", "y"),
        listOf(10L, 20L),
        listOf("r1", "r2"),
    )
    val artRes = art.parse()
    checkCond(artRes.heading == "Article 7" && artRes.summary == "Summary 7", "ArticlePage GET")

    val slugArt = pkg_art.ArticlePage.fetchBySlug(client, "rust-guide", "tok99")
    val slugRes = slugArt.parse()
    checkCond(slugRes.heading == "Slug Article: rust-guide" && slugRes.summary == "From POST mode=preview token=tok99", "ArticlePage POST")

    val logP = pkg_art.ChunkedLog.fetch(client, "s42")
    checkCond(logP.parse().lines == listOf("s42:line-1", "s42:line-2", "s42:line-3"), "ChunkedLog")

    var threwClosed = false
    try {
        pkg_closed.SimplePage.fetch(client, "1", pkg_closed.Rt.withTimeout(200L))
    } catch (_: pkg_closed.Rt.RtError) {
        threwClosed = true
    }
    checkCond(threwClosed, "closed port RtError")

    print("KOTLIN_HTML_FETCH_E2E_OK")
}
"""
        out = run_kotlin_custom_program(files)
        assert out == "KOTLIN_HTML_FETCH_E2E_OK"
