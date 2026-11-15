"""E2E integration tests for core DSL schemas (00..32) across Python and Node.js targets."""

from __future__ import annotations

import json
from typing import Any
import pytest

import parsmith
from tests.integration.helpers import (
    PY_HTML_TARGETS,
    SCHEMAS_DIR,
    assert_cross_target_parity,
    assert_fn_cross_target_parity,
    assert_schema_lint_clean,
    loaded_py_module,
    normalize_for_parity,
    run_csharp_schema,
    run_elixir_schema,
    run_go_schema,
    run_js_schema,
    run_kotlin_schema,
    run_php_schema,
    run_py_schema,
    run_rust_schema,
)

CORE_SCHEMA_FILES: tuple[str, ...] = (
    "00_full.kdl",
    "01_strings_basic.kdl",
    "02_arrays_and_conversions.kdl",
    "03_filters_and_predicates.kdl",
    "04_json_and_nested.kdl",
    "05_flat.kdl",
    "06_dict.kdl",
    "07_table.kdl",
    "18_json_basic.kdl",
    "19_json_mixed.kdl",
    "23_raw_struct.kdl",
    "25_fn.kdl",
    "31_css_all_indexing.kdl",
    "32_json_aliased_remapping.kdl",
)


@pytest.mark.parametrize("schema_file", CORE_SCHEMA_FILES)
def test_core_schemas_are_lint_clean(schema_file: str) -> None:
    """Every core schema in tests/integration/schemas/ passes parsmith.check() with zero errors/warnings."""
    assert_schema_lint_clean(schema_file)


class TestStringsBasic:
    """01_strings_basic.kdl across all 4 Python HTML backends, dataclass, separate_runtime, and JS."""

    _EXPECTED_NON_HTML: list[dict[str, Any]] = [
        {
            "title": "bar Title One",
            "link": "https://example.testguide",
            "slug": "slug-one",
            "normalized_code": "foo-bar",
            "active_flag": True,
        },
        {
            "title": "bar Title Two",
            "link": "https://example.testreadme/",
            "slug": "slug-two",
            "normalized_code": "code-4",
            "active_flag": True,
        },
    ]

    def _assert_strings_basic_result(self, result: list[dict[str, Any]]) -> None:
        assert isinstance(result, list)
        assert len(result) == 2

        non_html = [
            {k: v for k, v in item.items() if "html" not in k}
            for item in result
        ]
        assert non_html == self._EXPECTED_NON_HTML

        for item in result:
            inner = item["inner_html"]
            outer = item["clean_html"]
            assert isinstance(inner, str)
            assert isinstance(outer, str)
            assert not inner.lstrip().startswith("<article")
            assert outer.lstrip().startswith("<article")
            assert '<h2 class="title">' in inner
            assert "Advertisement" not in outer
            assert "var ignored" not in outer
            if "Advertisement" not in inner:
                assert len(inner) < len(outer)

    @pytest.mark.parametrize("target", PY_HTML_TARGETS)
    def test_strings_basic_py_targets(self, html: str, target: str) -> None:
        result = run_py_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
            target=target,
        )
        self._assert_strings_basic_result(result)

    def test_strings_basic_dataclass_and_separate_runtime(
        self, html: str
    ) -> None:
        dc_raw = run_py_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
            models="dataclass",
        )
        self._assert_strings_basic_result(normalize_for_parity(dc_raw))

        sep_res = run_py_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
            separate_runtime=True,
        )
        self._assert_strings_basic_result(sep_res)

    def test_strings_basic_js_target(self, html: str) -> None:
        js_raw = run_js_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
        )
        normalized = normalize_for_parity(js_raw, camel_to_snake=True)
        self._assert_strings_basic_result(normalized)

    def test_strings_basic_go_target(self, html: str) -> None:
        go_raw = run_go_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
        )
        self._assert_strings_basic_result(go_raw)

    def test_strings_basic_rust_target(self, html: str) -> None:
        rust_raw = run_rust_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
        )
        self._assert_strings_basic_result(rust_raw)

    def test_strings_basic_elixir_target(self, html: str) -> None:
        ex_raw = run_elixir_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
        )
        self._assert_strings_basic_result(ex_raw)

    def test_strings_basic_php_native_and_dom_crawler_targets(
        self, html: str
    ) -> None:
        for html_lib in ("native", "dom-crawler"):
            php_raw = run_php_schema(
                SCHEMAS_DIR / "01_strings_basic.kdl",
                "StringsBasic",
                html,
                html_lib=html_lib,
            )
            self._assert_strings_basic_result(php_raw)

    def test_strings_basic_csharp_target(self, html: str) -> None:
        cs_raw = run_csharp_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
        )
        self._assert_strings_basic_result(cs_raw)

    def test_strings_basic_kotlin_target(self, html: str) -> None:
        kt_raw = run_kotlin_schema(
            SCHEMAS_DIR / "01_strings_basic.kdl",
            "StringsBasic",
            html,
        )
        self._assert_strings_basic_result(kt_raw)


class TestCoreSchemaValues:
    """Exact extracted value assertions for core DSL schemas."""

    def test_coverage_root_full(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "00_full.kdl", "CoverageRoot", html
        )
        assert result["title"] == "DSL Coverage Fixture"
        assert result["canonical"] == "https://example.test/coverage"
        assert result["first_author"] == {
            "name": "Author One",
            "slug": "author-one",
        }
        assert result["nested_item"] == {
            "title": "Single Item Title",
            "active": True,
        }
        assert len(result["cards"]) == 2
        assert result["table_data"]["identifier"] == "ABC-123"
        assert result["body_size"] == 3
        assert result["code_length_ok"] == "foo   bar"

    def test_arrays_and_conversions_values(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "02_arrays_and_conversions.kdl",
            "ArraysAndConversions",
            html,
        )
        assert isinstance(result, list)
        assert len(result) == 2

        item0 = result[0]
        assert item0["token_list"] == [
            "tag-core-alpha1-x",
            "item-beta2-y",
            "ref3",
        ]
        assert item0["first_token"] == "tag-core-alpha1-x"
        assert item0["second_token"] == "item-beta2-y"
        assert item0["last_token"] == "ref3"
        assert item0["token_window"] == "tag-core-alpha1-x|item-beta2-y"
        assert item0["state_code"] == 1
        assert item0["score"] == 41
        assert item0["ratio"] == 0.75
        assert item0["any_link_count"] == 2
        assert item0["flags_joined"] == "ALPHA,BETA"

        item1 = result[1]
        assert item1["token_list"] == [
            "tag-core-alpha1-x",
            "item-beta2-y",
            "ref3",
        ]
        assert item1["state_code"] == 0
        assert item1["score"] == 18
        assert item1["ratio"] == 1.5
        assert item1["any_link_count"] == 1
        assert item1["flags_joined"] == "GAMMA,DELTA"

    def test_filters_and_predicates_values(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "03_filters_and_predicates.kdl",
            "FiltersAndPredicates",
            html,
        )
        assert result == [
            {
                "filtered_links": [],
                "logic_check": "prealpha",
                "numeric_check": "",
            },
            {
                "filtered_links": [],
                "logic_check": "prealpha",
                "numeric_check": "code-4",
            },
        ]

    def test_json_nested_root(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "04_json_and_nested.kdl",
            "JsonNestedRoot",
            html,
        )
        assert result["title"] == "DSL Coverage Fixture"
        assert result["first_author"] == {
            "name": "Author One",
            "slug": "author-one",
        }
        assert result["nested_item"] == {
            "title": "Single Item Title",
            "active": True,
        }
        assert result["nested_list"] == [
            {"href": "/nested/one.html", "name": "Nested One"},
            {"href": "/nested/two.html", "name": "Nested Two"},
        ]
        assert len(result["json_payload"]) == 2
        assert result["json_payload"][0]["text"] == "Quote one"
        assert result["json_payload"][1]["text"] == "Quote two"

    def test_flat_coverage_values(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "05_flat.kdl", "FlatCoverage", html
        )
        assert result == [
            "/nested/one.html",
            "/nested/two.html",
            "/guide.html",
            "https://example.test/docs/",
            "/readme/",
            "tag-core-alpha1-x",
            "item-beta2-y",
            "ref3",
        ]

    def test_dict_schemas_values(self, html: str) -> None:
        meta_dict = assert_cross_target_parity(
            SCHEMAS_DIR / "06_dict.kdl", "MetaDict", html
        )
        assert meta_dict == {
            "description": "Coverage description",
            "keywords": "alpha,beta,core",
        }

        alias_dict = assert_cross_target_parity(
            SCHEMAS_DIR / "06_dict.kdl", "MetaAliasDict", html
        )
        assert alias_dict == {
            "description": "Coverage description",
            "og:title": "Coverage OG Title",
            "keywords": "alpha,beta,core",
        }

        dict_root = assert_cross_target_parity(
            SCHEMAS_DIR / "06_dict.kdl", "DictRoot", html
        )
        assert dict_root == {
            "named_meta": meta_dict,
            "alias_meta": alias_dict,
        }

    def test_table_coverage_values(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "07_table.kdl", "TableCoverage", html
        )
        assert result == {
            "identifier": "ABC-123",
            "code_value": "CODE-1",
            "price": 9.99,
            "tax_or_fee": 1.25,
            "state": "active",
        }

    def test_json_basic_values(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "18_json_basic.kdl", "JsonBasic", html
        )
        assert result == {
            "first_author": {"name": "Author One", "slug": "author-one"},
            "second_author": {"name": "Author Two", "slug": "author-two"},
            "full_payload": [
                {
                    "text": "Quote one",
                    "author": {"name": "Author One", "slug": "author-one"},
                    "tags": ["alpha", "beta"],
                    "score": 7,
                    "rating": 4.5,
                    "active": True,
                    "note": "note text",
                },
                {
                    "text": "Quote two",
                    "author": {"name": "Author Two", "slug": "author-two"},
                    "tags": ["core"],
                    "score": 12,
                    "rating": 3.0,
                    "active": False,
                    "note": None,
                },
            ],
        }

    def test_json_mixed_values(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "19_json_mixed.kdl", "JsonMixed", html
        )
        assert result == {
            "html_title": "DSL Coverage Fixture",
            "nested_item": {"title": "Single Item Title"},
            "json_payload": [
                {
                    "text": "Quote one",
                    "author": {"name": "Author One", "slug": "author-one"},
                    "tags": ["alpha", "beta"],
                    "score": 7,
                },
                {
                    "text": "Quote two",
                    "author": {"name": "Author Two", "slug": "author-two"},
                    "tags": ["core"],
                    "score": 12,
                },
            ],
        }

    def test_css_all_indexing_values(self, html: str) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "31_css_all_indexing.kdl", "CssAllIndexing", html
        )
        assert result == {
            "first_link": "/nested/one.html",
            "last_link": "/readme/",
            "second_link": "/nested/two.html",
            "penultimate_link": "https://example.test/docs/",
        }
        doc_result = assert_cross_target_parity(
            SCHEMAS_DIR / "31_css_all_indexing.kdl", "DocIndexing", html
        )
        assert doc_result == result


# ── RAW struct tests (23_raw_struct.kdl) ─────────────────────────────────────

_JS_TEXT = '<script>var player = new Playerjs({id:"player",file:"/v/list/abc.txt"});</script>'
_URL_TEXT = "https://ru.yummyani.me/iframeCVH.html?dubbing_code=Sanae&anime_id=339&episode=1"
_PLAYLIST_TEXT = (
    "[480p]/v/anime/01_480p.m3u8\n"
    "[720p]/v/anime/01_720p.m3u8\n"
    "[1080p]/v/anime/01_1080p.m3u8"
)
_PLAIN_TEXT = "hello world"


class TestRawStructs:
    """23_raw_struct.kdl across all Python targets, dataclass, separate_runtime, and JS."""

    def test_player_script_extraction(self) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "23_raw_struct.kdl", "RawPlayerScript", _JS_TEXT
        )
        assert result == {
            "playlist_url": "/v/list/abc.txt",
            "player_id": "player",
        }

    def test_url_params_extraction(self) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "23_raw_struct.kdl", "RawUrlParams", _URL_TEXT
        )
        assert result == {
            "dubbing_code": "Sanae",
            "anime_id": 339,
        }

    def test_text_transform(self) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "23_raw_struct.kdl", "RawTextTransform", _PLAIN_TEXT
        )
        assert result == {
            "upper_text": "HELLO WORLD",
            "length": 11,
            "first_word": "hello",
        }

    def test_line_items_list(self) -> None:
        result = assert_cross_target_parity(
            SCHEMAS_DIR / "23_raw_struct.kdl", "RawLineItems", _PLAYLIST_TEXT
        )
        assert result == [
            {"quality": "480p", "url": "/v/anime/01_480p.m3u8"},
            {"quality": "720p", "url": "/v/anime/01_720p.m3u8"},
            {"quality": "1080p", "url": "/v/anime/01_1080p.m3u8"},
        ]


# ── Standalone fn tests (25_fn.kdl) ──────────────────────────────────────────

_FN_HTML = "<html><body><h1>Hello World</h1><a href='/a'>A</a><a href='/b'>B</a></body></html>"
_FN_RAW = "first line\nsecond line\nthird line"
_FN_VERSION = "app version=1.2.3 released"


class TestStandaloneFunctions:
    """25_fn.kdl HTML fn and (raw)fn across all Python targets, separate_runtime, and JS."""

    def test_page_title(self) -> None:
        res = assert_fn_cross_target_parity(
            SCHEMAS_DIR / "25_fn.kdl", "page_title", _FN_HTML
        )
        assert res == "Hello World"

    def test_all_links(self) -> None:
        res = assert_fn_cross_target_parity(
            SCHEMAS_DIR / "25_fn.kdl", "all_links", _FN_HTML
        )
        assert res == ["/a", "/b"]

    def test_first_line(self) -> None:
        res = assert_fn_cross_target_parity(
            SCHEMAS_DIR / "25_fn.kdl", "first_line", _FN_RAW
        )
        assert res == "first line"

    def test_extract_version(self) -> None:
        res = assert_fn_cross_target_parity(
            SCHEMAS_DIR / "25_fn.kdl", "extract_version", _FN_VERSION
        )
        assert res == "1.2.3"


# ── JSON aliased remapping, (dict)json, inline @value, GJSON, & errors ───────


class TestJsonProjectorAndErrors:
    """32_json_aliased_remapping.kdl, inline JSON schemas, GJSON paths, and RtJson*Error parity."""

    def test_schema_32_json_aliased_remapping(self) -> None:
        custom_html = """
        <html><body>
        <script id="test-data" type="application/json">[{"@id": 100, "@context": "https://example.com/ctx", "@type": "Product", "data-version": "2.0", "author": {"dc:creator": "John Doe", "slug": "johndoe", "location": {"lat": 51.5, "lng": -0.12}}, "rating_scores": [5, 4, 5]}]</script>
        </body></html>
        """
        res = assert_cross_target_parity(
            SCHEMAS_DIR / "32_json_aliased_remapping.kdl",
            "JsonAliasedHtmlScraper",
            custom_html,
        )
        assert res == {
            "items": [
                {
                    "item_id": 100,
                    "item_context": "https://example.com/ctx",
                    "item_type": "Product",
                    "data_version": "2.0",
                    "author": {
                        "author_name": "John Doe",
                        "author_slug": "johndoe",
                        "location": {
                            "latitude": 51.5,
                            "longitude": -0.12,
                        },
                    },
                    "scores": [5, 4, 5],
                }
            ]
        }

    def test_json_dot_path_and_omitempty_projection(self) -> None:
        kdl_src = """
        json DotProfile {
            user_id str from="meta.user_id"
            avatar_url str? from="profile.avatar.url"
            top_badge str? from="badges.0.icon"
            legacy_id int @omitempty from="meta.legacy.id"
        }

        (raw)struct DotParser {
            user {
                jsonify DotProfile
            }
        }
        """
        raw_json_str = json.dumps(
            {
                "meta": {"user_id": "u42"},
                "profile": {
                    "avatar": {
                        "url": "https://example.com/icon.png",
                        "width": 64,
                    }
                },
                "badges": [{"icon": "shield", "level": 1}],
                "extra_key": "drop_me",
            }
        )
        res = assert_cross_target_parity(
            kdl_src,
            "DotParser",
            raw_json_str,
            strip_none_dict_values=True,
        )
        assert res["user"] == {
            "user_id": "u42",
            "avatar_url": "https://example.com/icon.png",
            "top_badge": "shield",
        }
        assert "legacy_id" not in res["user"]
        assert "extra_key" not in res["user"]

    def test_top_level_dict_json_and_inline_value_schemas(self) -> None:
        kdl_src = """
        (dict)json Translations {
            @key str
            @value TranslationValue {
                title str from="display_title"
                count int
                notes @skip
            }
        }

        (raw)struct TranslationsParser {
            translations {
                jsonify Translations
            }
        }
        """
        raw_payload = json.dumps(
            {
                "rus": {
                    "display_title": "Русский",
                    "count": 12,
                    "notes": "ignore",
                },
                "eng": {"display_title": "English", "count": 24},
            }
        )
        res = assert_cross_target_parity(
            kdl_src, "TranslationsParser", raw_payload
        )
        assert res == {
            "translations": {
                "rus": {"title": "Русский", "count": 12},
                "eng": {"title": "English", "count": 24},
            }
        }

        with loaded_py_module(kdl_src) as (mod, _):
            with pytest.raises(
                mod.RtJsonSchemaError,
                match=r"Translations: Expected dict, got list",
            ):
                mod.TranslationsParser('["invalid", "array"]').parse()

        with pytest.raises(
            RuntimeError,
            match=r"RtJsonSchemaError: Translations: Expected object, got array",
        ):
            run_js_schema(
                kdl_src, "TranslationsParser", '["invalid", "array"]'
            )

        with pytest.raises(RuntimeError, match=r"unmarshal"):
            run_go_schema(
                kdl_src, "TranslationsParser", '["invalid", "array"]'
            )

        with pytest.raises(
            RuntimeError, match=r"expected JSON object"
        ):
            run_rust_schema(
                kdl_src, "TranslationsParser", '["invalid", "array"]'
            )

        with pytest.raises(
            RuntimeError, match=r"expected JSON object"
        ):
            run_elixir_schema(
                kdl_src, "TranslationsParser", '["invalid", "array"]'
            )

        with pytest.raises(
            RuntimeError, match=r"Expected dict|expected JSON object"
        ):
            run_php_schema(
                kdl_src, "TranslationsParser", '["invalid", "array"]'
            )

        with pytest.raises(
            RuntimeError, match=r"Expected object|expected JSON object"
        ):
            run_csharp_schema(
                kdl_src, "TranslationsParser", '["invalid", "array"]'
            )

        with pytest.raises(
            RuntimeError, match=r"expected JSON object"
        ):
            run_kotlin_schema(
                kdl_src, "TranslationsParser", '["invalid", "array"]'
            )

    def test_inline_dict_nullable_and_omitempty(self) -> None:
        kdl_src = """
        json ConfigResponse {
            id str
            meta (dict)? @omitempty {
                @key str
                @value str
            }
            tags (dict)? {
                @key str
                @value str
            }
        }

        (raw)struct ConfigParser {
            cfg {
                jsonify ConfigResponse
            }
        }
        """
        r1 = run_py_schema(kdl_src, "ConfigParser", '{"id": "c1"}')
        assert r1["cfg"] == {"id": "c1", "tags": None}
        assert "meta" not in r1["cfg"]

        r2 = run_py_schema(
            kdl_src,
            "ConfigParser",
            '{"id": "c1", "meta": null, "tags": null}',
        )
        assert r2["cfg"] == {"id": "c1", "tags": None}
        assert "meta" not in r2["cfg"]

        js1 = run_js_schema(kdl_src, "ConfigParser", '{"id": "c1"}')
        assert js1["cfg"] == {"id": "c1", "tags": None}
        assert "meta" not in js1["cfg"]

        js2 = run_js_schema(
            kdl_src,
            "ConfigParser",
            '{"id": "c1", "meta": null, "tags": null}',
        )
        assert js2["cfg"] == {"id": "c1", "tags": None}
        assert "meta" not in js2["cfg"]

        r3 = assert_cross_target_parity(
            kdl_src,
            "ConfigParser",
            '{"id": "c1", "meta": {"env": "prod"}, "tags": {"team": "core"}}',
        )
        assert r3["cfg"] == {
            "id": "c1",
            "meta": {"env": "prod"},
            "tags": {"team": "core"},
        }

    def test_nested_dict_value_schema_multi_level_and_anonymous_blocks(
        self,
    ) -> None:
        kdl_multi = """
        json AnimeResponse {
            translations (dict)Translation {
                @key str
                @value TranslationValue {
                    episodes (dict)EpisodeMap {
                        @key int
                        @value EpisodeValue {
                            link str from="stream_url"
                            screenshots @skip
                        }
                    }
                    is_active bool
                    kind str from="type"
                }
            }
        }

        (raw)struct AnimeParser {
            anime {
                jsonify AnimeResponse
            }
        }
        """
        payload_multi = json.dumps(
            {
                "translations": {
                    "sub": {
                        "episodes": {
                            "1": {
                                "stream_url": "https://stream/1",
                                "screenshots": ["s1.jpg"],
                            },
                            "2": {"stream_url": "https://stream/2"},
                        },
                        "is_active": True,
                        "type": "tv",
                    }
                }
            }
        )
        res_multi = assert_cross_target_parity(
            kdl_multi, "AnimeParser", payload_multi
        )
        assert res_multi == {
            "anime": {
                "translations": {
                    "sub": {
                        "episodes": {
                            "1": {"link": "https://stream/1"},
                            "2": {"link": "https://stream/2"},
                        },
                        "is_active": True,
                        "kind": "tv",
                    }
                }
            }
        }

        kdl_anon = """
        json Project {
            groups (dict) {
                @key str
                @value (array)GroupItem {
                    id int
                    name str from="group_name"
                }
            }
            settings (dict) {
                @key str
                @value {
                    enabled bool
                    priority int? @omitempty
                }
            }
        }

        (raw)struct ProjectParser {
            proj {
                jsonify Project
            }
        }
        """
        payload_anon = json.dumps(
            {
                "groups": {
                    "backend": [
                        {"id": 1, "group_name": "Core"},
                        {"id": 2, "group_name": "Ops"},
                    ]
                },
                "settings": {
                    "notifications": {"enabled": True, "priority": 5},
                    "telemetry": {"enabled": False},
                },
            }
        )
        res_anon = assert_cross_target_parity(
            kdl_anon,
            "ProjectParser",
            payload_anon,
            strip_none_dict_values=True,
        )
        assert res_anon == {
            "proj": {
                "groups": {
                    "backend": [
                        {"id": 1, "name": "Core"},
                        {"id": 2, "name": "Ops"},
                    ]
                },
                "settings": {
                    "notifications": {"enabled": True, "priority": 5},
                    "telemetry": {"enabled": False},
                },
            }
        }

    def test_inline_combined_nested_hierarchy(self) -> None:
        kdl_src = """
        json AnimeResponse {
            id str
            anime_poster str

            franchise Franchise {
                id str
                shikimori_id str from="shiki_id"

                links (array)Links {
                    id int
                    relation str
                }

                nodes (array)Node {
                    id int
                    name str
                    score float
                }
            }

            material_data {
                anime_title str from="title"
                year int
            }

            self_hosted SelfHosted? from="self_hosted_data" @omitempty {
                available bool
                episodes (array)str
                translations (dict) {
                    @key str
                    @value (array)str
                }
            }
        }

        (raw)struct FullAnimeParser {
            data {
                jsonify AnimeResponse
            }
        }
        """
        raw_payload = json.dumps(
            {
                "id": "anime_full",
                "anime_poster": "https://example.com/poster.jpg",
                "franchise": {
                    "id": "fr_1",
                    "shiki_id": "sh_100",
                    "links": [{"id": 1, "relation": "sequel"}],
                    "nodes": [{"id": 10, "name": "Original", "score": 8.5}],
                },
                "material_data": {
                    "title": "Anime Title",
                    "year": 2024,
                },
                "self_hosted_data": {
                    "available": True,
                    "episodes": ["ep1", "ep2"],
                    "translations": {"1": ["jap", "eng"]},
                },
            }
        )
        res = assert_cross_target_parity(
            kdl_src,
            "FullAnimeParser",
            raw_payload,
            strip_none_dict_values=True,
        )
        assert res == {
            "data": {
                "id": "anime_full",
                "anime_poster": "https://example.com/poster.jpg",
                "franchise": {
                    "id": "fr_1",
                    "shikimori_id": "sh_100",
                    "links": [{"id": 1, "relation": "sequel"}],
                    "nodes": [{"id": 10, "name": "Original", "score": 8.5}],
                },
                "material_data": {
                    "anime_title": "Anime Title",
                    "year": 2024,
                },
                "self_hosted": {
                    "available": True,
                    "episodes": ["ep1", "ep2"],
                    "translations": {"1": ["jap", "eng"]},
                },
            }
        }

    def test_gjson_path_subset_escaped_dots_hash_len_and_projection(
        self,
    ) -> None:
        kdl_src = r"""
        json ReleaseManifest {
            os_version str from="os\\.version"
            nested_version str from="meta.os\\.version"
            backslash_key str from="meta.path\\\\dir"
            hash_key str from="meta.\\#"
            item_count int from="items.#"
            opt_count int? from="missing.#"
            tag_names (array)str from="tags.#.name"
            deep_ids (array)int from="groups.#.meta.id"
        }

        (raw)struct ManifestParser {
            manifest {
                jsonify ReleaseManifest path="envelope\\.data"
            }
        }
        """
        payload = json.dumps(
            {
                "envelope.data": {
                    "os.version": "24.04",
                    "meta": {
                        "os.version": "6.8.0-generic",
                        "path\\dir": "C:\\bin",
                        "#": "pound_val",
                    },
                    "items": ["a", "b", "c"],
                    "tags": [
                        {"name": "action", "id": 1},
                        {"name": "sci-fi", "id": 2},
                        {"other": "skipped"},
                    ],
                    "groups": [
                        {"meta": {"id": 10}},
                        {"meta": {"id": 20}},
                    ],
                }
            }
        )
        res = assert_cross_target_parity(kdl_src, "ManifestParser", payload)
        assert res == {
            "manifest": {
                "os_version": "24.04",
                "nested_version": "6.8.0-generic",
                "backslash_key": "C:\\bin",
                "hash_key": "pound_val",
                "item_count": 3,
                "opt_count": None,
                "tag_names": ["action", "sci-fi"],
                "deep_ids": [10, 20],
            }
        }

        bad_payload = json.dumps(
            {
                "envelope.data": {
                    "os.version": "24.04",
                    "meta": {
                        "os.version": "6.8.0-generic",
                        "path\\dir": "C:\\bin",
                        "#": "pound_val",
                    },
                    "items": "not_an_array",
                    "tags": [],
                    "groups": [],
                }
            }
        )
        with loaded_py_module(kdl_src, separate_runtime=True) as (mod, rt_mod):
            assert rt_mod is not None
            with pytest.raises(
                rt_mod.RtJsonPathError,
                match=r"ReleaseManifest\.item_count: Expected list for '#'",
            ):
                mod.ManifestParser(bad_payload).parse()

        with pytest.raises(
            RuntimeError,
            match=r"RtJsonPathError: ReleaseManifest\.item_count: Expected list for '#'",
        ):
            run_js_schema(kdl_src, "ManifestParser", bad_payload)

        with pytest.raises(
            RuntimeError,
            match=r"ReleaseManifest\.item_count: expected list for '#'",
        ):
            run_go_schema(kdl_src, "ManifestParser", bad_payload)

        with pytest.raises(
            RuntimeError,
            match=r"ReleaseManifest\.item_count: expected JSON array for '#' operator",
        ):
            run_rust_schema(kdl_src, "ManifestParser", bad_payload)

        with pytest.raises(
            RuntimeError,
            match=r"ReleaseManifest\.item_count: expected JSON array for '#' operator",
        ):
            run_elixir_schema(kdl_src, "ManifestParser", bad_payload)

        with pytest.raises(
            RuntimeError,
            match=r"ReleaseManifest\.item_count: Expected list for '#'",
        ):
            run_php_schema(kdl_src, "ManifestParser", bad_payload)

        with pytest.raises(
            RuntimeError,
            match=r"ReleaseManifest\.item_count: Expected list for '#'",
        ):
            run_csharp_schema(kdl_src, "ManifestParser", bad_payload)

        with pytest.raises(
            RuntimeError,
            match=r"ReleaseManifest\.item_count: expected JSON array for '#' operator",
        ):
            run_kotlin_schema(kdl_src, "ManifestParser", bad_payload)

    def test_rt_json_error_classes_and_messages_across_py_and_js(
        self,
    ) -> None:
        kdl_src = """
        json user_profile {
            id int
            username str from="user_name"
            city str from="address.city"
            tags (array)str?
            translations (dict)? {
                @key str
                @value (array)str
            }
        }

        (raw)struct ProfileParser {
            profile {
                jsonify user_profile
            }
        }
        """
        cases: list[tuple[str, str, str, str]] = [
            (
                "null",
                "RtJsonSchemaError",
                r"UserProfile: Expected dict, got NoneType",
                r"RtJsonSchemaError: UserProfile: Expected object, got null",
            ),
            (
                "42",
                "RtJsonSchemaError",
                r"UserProfile: Expected dict, got int",
                r"RtJsonSchemaError: UserProfile: Expected object, got number",
            ),
            (
                "[1, 2]",
                "RtJsonSchemaError",
                r"UserProfile: Expected dict, got list",
                r"RtJsonSchemaError: UserProfile: Expected object, got array",
            ),
            (
                '{"id": 1, "address": {"city": "NY"}}',
                "RtJsonFieldMissingError",
                r"UserProfile\.username: Required JSON field 'user_name' \(mapped to 'username'\) is missing",
                r"RtJsonFieldMissingError: UserProfile\.username: Required JSON field 'user_name' \(mapped to 'username'\) is missing",
            ),
            (
                '{"id": 1, "user_name": null, "address": {"city": "NY"}}',
                "RtJsonFieldMissingError",
                r"UserProfile\.username: Field 'user_name' is null, but 'username' is not nullable",
                r"RtJsonFieldMissingError: UserProfile\.username: Field 'user_name' is null, but 'username' is not nullable",
            ),
            (
                '{"id": 1, "user_name": "alice", "address": {}}',
                "RtJsonPathError",
                r"UserProfile\.city: Missing key 'city' in path 'address\.city'",
                r"RtJsonPathError: UserProfile\.city: Missing key 'city' in path 'address\.city'",
            ),
            (
                '{"id": 1, "user_name": "alice", "address": {"city": "NY"}, "tags": "not_an_array"}',
                "RtJsonSchemaError",
                r"UserProfile\.tags: Expected list, got str",
                r"RtJsonSchemaError: UserProfile\.tags: Expected array, got string",
            ),
            (
                '{"id": 1, "user_name": "alice", "address": {"city": "NY"}, "translations": {"en": "not_an_array"}}',
                "RtJsonSchemaError",
                r"UserProfile\.translations: Expected list, got str",
                r"RtJsonSchemaError: UserProfile\.translations: Expected array, got string",
            ),
        ]

        with loaded_py_module(kdl_src) as (mod, _):
            for payload, exc_cls_name, py_pattern, js_pattern in cases:
                exc_cls = getattr(mod, exc_cls_name)
                with pytest.raises(exc_cls, match=py_pattern):
                    mod.ProfileParser(payload).parse()

                with pytest.raises(RuntimeError, match=js_pattern):
                    run_js_schema(kdl_src, "ProfileParser", payload)

                with pytest.raises(
                    RuntimeError,
                    match=r"UserProfile|parsmith: json unmarshal failed",
                ):
                    run_go_schema(kdl_src, "ProfileParser", payload)

                with pytest.raises(RuntimeError, match=r"UserProfile"):
                    run_rust_schema(kdl_src, "ProfileParser", payload)

                with pytest.raises(RuntimeError, match=r"UserProfile"):
                    run_elixir_schema(kdl_src, "ProfileParser", payload)

                with pytest.raises(RuntimeError, match=r"UserProfile"):
                    run_php_schema(kdl_src, "ProfileParser", payload)

                with pytest.raises(RuntimeError, match=r"UserProfile"):
                    run_csharp_schema(kdl_src, "ProfileParser", payload)

                with pytest.raises(RuntimeError, match=r"UserProfile"):
                    run_kotlin_schema(kdl_src, "ProfileParser", payload)
