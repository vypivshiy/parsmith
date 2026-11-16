from __future__ import annotations

import importlib
import json
import sys
import types
from pathlib import Path
from typing import Any

import pytest

import parsmith
from parsmith import (
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
)


def test_version_and_exports() -> None:
    assert isinstance(parsmith.__version__, str)
    assert len(parsmith.__version__) > 0
    assert parsmith.version() == parsmith.__version__
    assert TypeInfo.__name__ == "TypeInfo"


def test_check_inline_valid_and_invalid_kdl() -> None:
    valid_schema = """
    struct BookCard {
        title { css "h3 > a"; attr "title"; trim }
        price { css ".price"; text; re #"(\\d+\\.\\d+)"#; to-float; fallback 0.0 }
    }
    """
    res: CheckResult = parsmith.check(valid_schema)
    assert res.ok is True
    assert bool(res) is True
    assert res.has_errors() is False
    assert res.has_warnings() is False
    assert res.is_ok() is True
    assert res.error_count == 0
    assert res.warning_count == 0
    assert res.diagnostics == []
    assert res.errors == []
    assert res.warnings == []
    assert res.format_text() == ""
    assert json.loads(res.format_json()) == {"diagnostics": []}
    assert res.to_dict() == {"ok": True, "diagnostics": []}

    # Invalid KDL: pipeline type mismatch (E100) + warning (W011 positional alias)
    invalid_schema = """
    json Item {
        name str "legacy_name"
    }
    struct Broken {
        bad_field { trim }
    }
    """
    bad_res = parsmith.check(invalid_schema, path="broken.kdl")
    assert bad_res.ok is False
    assert bool(bad_res) is False
    assert bad_res.has_errors() is True
    assert bad_res.has_warnings() is True
    assert bad_res.error_count == 1
    assert bad_res.warning_count == 1
    assert len(bad_res.diagnostics) == 2

    err_diag: Diagnostic = bad_res.errors[0]
    assert err_diag.code == "E100"
    assert err_diag.code_name == "TYPE_MISMATCH"
    assert err_diag.severity == "error"
    assert err_diag.is_error is True
    assert err_diag.is_warning is False
    assert err_diag.path == "broken.kdl"
    assert err_diag.line == 6
    assert err_diag.col > 0
    assert err_diag.end_line >= err_diag.line
    assert err_diag.end_col >= err_diag.col
    assert err_diag.hint is not None
    assert isinstance(err_diag.notes, list)

    rendered = err_diag.render()
    assert "error[E100]" in rendered
    assert "broken.kdl:6:" in rendered
    assert "bad_field { trim }" in rendered
    assert str(err_diag) == rendered

    custom_rendered = err_diag.render(source=invalid_schema)
    assert "bad_field { trim }" in custom_rendered

    diag_dict = err_diag.to_dict()
    assert diag_dict["code"] == "E100"
    assert diag_dict["severity"] == "error"
    assert diag_dict["path"] == "broken.kdl"

    warn_diag: Diagnostic = bad_res.warnings[0]
    assert warn_diag.code == "W011"
    assert warn_diag.severity == "warning"
    assert warn_diag.is_warning is True
    assert warn_diag.is_error is False

    text_out = bad_res.format_text()
    assert "error[E100]" in text_out
    assert "warning[W011]" in text_out

    json_out = json.loads(bad_res.format_json())
    assert len(json_out["diagnostics"]) == 2


def test_check_filesystem_paths_and_target_filtering(tmp_path: Path) -> None:
    # 'self' is valid as a struct field in Python, but cannot be a raw identifier in Rust ('E403')
    schema_text = """
    struct MyParser {
        self { css "span.fn"; text }
    }
    """
    schema_file = tmp_path / "reserved_rust.kdl"
    schema_file.write_text(schema_text, encoding="utf-8")

    # 1. Default 4-ecosystem validation catches Rust keyword collision (E403)
    res_all = parsmith.check(schema_file)
    assert res_all.has_errors() is True
    assert any(d.code == "E403" for d in res_all.errors)

    # Also test passing string path
    res_str_path = parsmith.check(str(schema_file))
    assert res_str_path.has_errors() is True
    assert any(d.code == "E403" for d in res_str_path.errors)

    # 2. Target-filtered check for Python only succeeds with zero errors
    res_py = parsmith.check(schema_file, targets=["python"])
    assert res_py.ok is True
    assert res_py.has_errors() is False

    # 3. Non-existent .kdl path reports E003
    res_missing = parsmith.check(tmp_path / "does_not_exist.kdl")
    assert res_missing.has_errors() is True
    assert res_missing.errors[0].code == "E003"


def test_check_and_generate_vfs_diamond_imports() -> None:
    sources: dict[str, str] = {
        "common/ext.kdl": """
        extension TextExt {
            clean {
                sig str str
                py { emit "{{out}} = {{in}}.strip().upper()" }
                js { emit "const {{out}} = {{in}}.trim().toUpperCase();" }
            }
        }
        define PRICE_CSS=".price-val"
        """,
        "models/author.kdl": """
        import "../common/ext.kdl" {
            (extension)TextExt
        }
        struct AuthorBadge {
            name { css ".author"; text; !TextExt.clean }
        }
        """,
        "models/pricing.kdl": """
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
        """,
        "pages/product.kdl": """
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
        """,
    }

    # Check by VFS entry key
    check_res = parsmith.check("pages/product.kdl", sources=sources)
    assert check_res.ok is True, check_res.format_text()

    # Check inline KDL source importing from VFS
    inline_entry = """
    import "./pages/product.kdl" {
        (struct)ProductPage
    }
    struct StoreFront {
        featured { css ".featured"; nested ProductPage }
    }
    """
    inline_check = parsmith.check(inline_entry, sources=sources)
    assert inline_check.ok is True, inline_check.format_text()

    # Generate Python code from VFS diamond graph and execute it dynamically
    gen_py = parsmith.generate("pages/product.kdl", "python", sources=sources)
    assert gen_py.ok is True, gen_py.format_text()
    assert "product.py" in gen_py
    py_code = gen_py["product.py"]
    assert gen_py.primary_code() == py_code
    assert gen_py.code == py_code

    mod = types.ModuleType("product_vfs_mod")
    sys.modules[mod.__name__] = mod
    try:
        exec(py_code, mod.__dict__)
        html = """
        <html>
          <body>
            <h1 class="title">  rust compiler  </h1>
            <div class="author-box"><span class="author">  alice  </span></div>
            <div class="price-box"><span class="price-val">49.95</span></div>
            <script id="discount">{"percent": 15, "code": "SAVE15"}</script>
          </body>
        </html>
        """
        parser = mod.ProductPage(html)
        parsed = parser.parse()
        assert parsed == {
            "title": "RUST COMPILER",
            "author": {"name": "ALICE"},
            "pricing": {"amount": 49.95},
            "discount": {"percent": 15, "code": "SAVE15"},
        }
    finally:
        sys.modules.pop(mod.__name__, None)


def test_generate_python_dataclass_separate_runtime_and_rest_execution() -> None:
    schema = '''
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
    '''

    # Test CodegenOptions class as well as kwargs override
    opts = CodegenOptions(
        "python",
        models="dataclass",
        bs4_parser="html.parser",
        http_io="sync",
        separate_runtime=True,
        runtime_name="custom_rt",
        module_stem="user_api",
        prologue="# PROLOGUE_MARKER",
        epilogue="# EPILOGUE_MARKER",
    )
    assert opts.target == "python"
    assert opts.models == "dataclass"
    assert opts.bs4_parser == "html.parser"
    assert opts.http_io == "sync"
    assert opts.separate_runtime is True
    assert opts.runtime_name == "custom_rt"
    assert opts.module_stem == "user_api"
    assert opts.prologue == "# PROLOGUE_MARKER"
    assert opts.epilogue == "# EPILOGUE_MARKER"

    res: GenerateResult = parsmith.generate(schema, options=opts)
    assert res.ok is True, res.format_text()
    assert bool(res) is True
    assert set(res.files.keys()) == {"user_api.py", "custom_rt.py"}

    client_code = res["user_api.py"]
    rt_code = res["custom_rt.py"]
    assert "# PROLOGUE_MARKER" in client_code
    assert "# EPILOGUE_MARKER" in client_code
    assert "from custom_rt import" in client_code

    # Dynamically load custom_rt and user_api and test end-to-end REST dispatch
    rt_mod = types.ModuleType("custom_rt")
    api_mod = types.ModuleType("user_api")
    sys.modules["custom_rt"] = rt_mod
    sys.modules["user_api"] = api_mod
    try:
        exec(rt_code, rt_mod.__dict__)
        exec(client_code, api_mod.__dict__)

        class MockSyncTransport:
            def __init__(self, status: int, body: str) -> None:
                self.status = status
                self.body = body
                self.last_url = ""

            def send(self, req: Any) -> Any:
                self.last_url = req.url
                return rt_mod.RawResponse(
                    status=self.status,
                    headers={"content-type": "application/json"},
                    text=self.body,
                )

        # 1. 200 OK response -> Ok[UserProfileJson] dataclass instance
        t_ok = MockSyncTransport(200, '{"id": 7, "user": {"name": "bob"}}')
        out_ok = api_mod.UserApiClient.get_user(t_ok, user_id=7)
        assert isinstance(out_ok, rt_mod.Ok)
        assert out_ok.is_ok is True
        assert out_ok.value.id == 7
        assert out_ok.value.username == "bob"
        assert out_ok.value.role is None
        assert t_ok.last_url == "https://api.example.com/v1/users/7"

        # 2. 404 Error response -> UserApiClientErr404NotFound
        t_404 = MockSyncTransport(404, '{"message": "User missing", "code": 40401}')
        out_404 = api_mod.UserApiClient.get_user(t_404, user_id=999)
        assert isinstance(out_404, api_mod.UserApiClientErr404NotFound)
        assert out_404.is_ok is False
        assert out_404.status == 404
        assert out_404.value.message == "User missing"
        assert out_404.value.code == 40401

        # 3. 200 Malformed JSON -> DecodeErr
        t_bad = MockSyncTransport(200, '{"id": "not-an-int"}')
        out_bad = api_mod.UserApiClient.get_user(t_bad, user_id=7)
        assert isinstance(out_bad, rt_mod.DecodeErr)
        assert out_bad.status == 200
    finally:
        sys.modules.pop("user_api", None)
        sys.modules.pop("custom_rt", None)


def test_generate_ts_and_js_targets() -> None:
    schema = """
    struct Article {
        title { css "h1"; text; trim }
        tags { css-all "ul.tags > li"; text }
    }
    """

    # TypeScript generation with dict options
    res_ts = parsmith.generate(
        schema,
        "ts",
        options={"module_stem": "article_parser", "prologue": "// TS_PROLOGUE"},
    )
    assert res_ts.ok is True
    assert list(res_ts.files.keys()) == ["article_parser.ts"]
    ts_code = res_ts["article_parser.ts"]
    assert "// TS_PROLOGUE" in ts_code
    assert "export interface ArticleType" in ts_code
    assert "export class Article" in ts_code

    # JavaScript generation emits both .js and .d.ts
    res_js = parsmith.generate(schema, "js", module_stem="article_bundle")
    assert res_js.ok is True
    assert set(res_js.files.keys()) == {"article_bundle.js", "article_bundle.d.ts"}
    assert "export class Article" in res_js["article_bundle.js"]
    assert "export declare class Article" in res_js["article_bundle.d.ts"]

    # Invalid option raises ValueError
    with pytest.raises(ValueError, match="Invalid target"):
        parsmith.generate(schema, "cobol")


def test_generate_python_html_lib_options_and_target_shorthands() -> None:
    schema = """
    struct Card {
        title { css "h1"; text; trim }
    }
    """

    opts_default = CodegenOptions()
    assert opts_default.target == "python"
    assert opts_default.html_lib == "bs4"
    assert opts_default.runtime_name == ".parsmith_runtime"
    assert "html_lib=\"bs4\"" in repr(opts_default) or "html_lib='bs4'" in repr(opts_default)
    assert opts_default.to_dict()["html_lib"] == "bs4"
    assert opts_default.to_dict()["runtime_name"] == ".parsmith_runtime"

    opts_go_default = CodegenOptions("go")
    assert opts_go_default.target == "go"
    assert opts_go_default.runtime_name == "parsmith_runtime"
    opts_go_default.target = "python"
    assert opts_go_default.runtime_name == ".parsmith_runtime"
    opts_go_default.target = "rust"
    assert opts_go_default.runtime_name == "parsmith_runtime"

    opts_lxml = CodegenOptions("python", html_lib="lxml")
    assert opts_lxml.html_lib == "lxml"
    opts_lxml.html_lib = "slax"
    assert opts_lxml.html_lib == "selectolax"

    opts_shorthand = CodegenOptions("py-parsel")
    assert opts_shorthand.target == "python"
    assert opts_shorthand.html_lib == "parsel"

    # 1. Explicit html_lib kwarg
    res_lxml = parsmith.generate(schema, html_lib="lxml")
    assert res_lxml.ok is True
    assert "from lxml import html" in res_lxml["schema.py"]

    # 2. options={"lib": ...} and options={"html-lib": ...}
    res_parsel = parsmith.generate(schema, options={"lib": "parsel"})
    assert res_parsel.ok is True
    assert "from parsel import Selector, SelectorList" in res_parsel["schema.py"]

    res_slax_dict = parsmith.generate(schema, options={"html-lib": "slax"})
    assert res_slax_dict.ok is True
    assert "from selectolax.lexbor import LexborHTMLParser, LexborNode" in res_slax_dict["schema.py"]

    # 3. Target shorthands: py-bs4, py-lxml, py-parsel, py-selectolax, py-slax
    for target_shorthand, expected_import in [
        ("py-bs4", "from bs4 import BeautifulSoup, Tag"),
        ("python-bs4", "from bs4 import BeautifulSoup, Tag"),
        ("py-lxml", "from lxml import html"),
        ("python-lxml", "from lxml import html"),
        ("py-parsel", "from parsel import Selector, SelectorList"),
        ("python-parsel", "from parsel import Selector, SelectorList"),
        ("py-selectolax", "from selectolax.lexbor import LexborHTMLParser, LexborNode"),
        ("py-slax", "from selectolax.lexbor import LexborHTMLParser, LexborNode"),
    ]:
        res = parsmith.generate(schema, target_shorthand)
        assert res.ok is True
        assert expected_import in res["schema.py"]

    # 4. Explicit html_lib kwarg overrides target shorthand
    res_override = parsmith.generate(schema, "py-bs4", html_lib="parsel")
    assert res_override.ok is True
    assert "from parsel import Selector, SelectorList" in res_override["schema.py"]

    with pytest.raises(ValueError, match="Invalid --html-lib"):
        parsmith.generate(schema, html_lib="cheerio")


def test_inspect_schema_single_and_multi_file_vfs() -> None:
    vfs: dict[str, str] = {
        "shared.kdl": """
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
        """,
        "catalog.kdl": '''
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
        ''',
    }

    info: ModuleInfo = parsmith.inspect_schema("catalog.kdl", sources=vfs)
    assert info.ok is True
    assert bool(info) is True
    assert info.has_errors() is False
    assert info.doc == "Catalog schema"
    assert info.source_file == "catalog.kdl"
    assert info.struct_kinds == {
        "BookCard": "list",
        "CatalogPage": "item",
        "CatalogApi": "rest",
    }

    # Verify structs & fields
    assert len(info.structs) == 3
    book_card: StructInfo = info.structs[0]
    assert book_card.name == "BookCard"
    assert book_card.kind == "list"
    assert len(book_card.fields) == 3
    price_field: FieldInfo = book_card.fields[2]
    assert price_field.name == "price"
    assert price_field.has_fallback is True
    assert price_field.type_info.base == "float"
    assert price_field.type_info.dsl_name == "FLOAT"
    assert price_field.type_info.is_array is False

    catalog_page: StructInfo = info.structs[1]
    assert catalog_page.name == "CatalogPage"
    assert catalog_page.kind == "item"
    assert catalog_page.doc == "Main catalog page"
    assert catalog_page.has_pre_validate is True
    assert len(catalog_page.init_fields) == 1
    init_f: InitFieldInfo = catalog_page.init_fields[0]
    assert init_f.name == "raw_state"
    assert init_f.type_info.base == "string"

    assert len(catalog_page.checks) == 1
    chk: CheckMethodInfo = catalog_page.checks[0]
    assert chk.name == "has_books"
    assert chk.return_type.base == "bool"

    books_field: FieldInfo = catalog_page.fields[1]
    assert books_field.name == "books"
    assert books_field.type_info.base == "nested"
    assert books_field.type_info.reference == "BookCard"
    assert books_field.type_info.is_array is True

    # Verify @request and PlaceholderInfo on HTML struct
    assert len(catalog_page.requests) == 1
    req: RequestInfo = catalog_page.requests[0]
    assert req.name == "fetch-page"
    assert req.method == "GET"
    assert req.body_kind == "empty"
    assert len(req.placeholders) == 3
    ph_tags: PlaceholderInfo = req.placeholders[1]
    assert ph_tags.name == "tags"
    assert ph_tags.type_name == "str"
    assert ph_tags.is_array is True
    assert ph_tags.is_optional is False
    assert ph_tags.style == "csv"
    assert ph_tags.token == "{{tags[]|csv}}"

    # Verify (rest)struct endpoints and @error variants
    catalog_api: StructInfo = info.structs[2]
    assert catalog_api.name == "CatalogApi"
    assert catalog_api.kind == "rest"
    assert len(catalog_api.requests) == 1
    assert len(catalog_api.rest_endpoints) == 1
    ep: RestEndpointInfo = catalog_api.rest_endpoints[0]
    assert ep.name == "get-translations"
    assert ep.method == "POST"
    assert ep.response_schema == "Translations"
    assert ep.response_path == "data"
    assert ep.result_alias_name == "GetTranslationsResult"
    assert ep.body_kind == "json"
    assert len(ep.placeholders) == 2

    assert len(catalog_api.errors) == 2
    err_404: ErrorVariantInfo = catalog_api.errors[0]
    assert err_404.variant_name == "CatalogApiErr404NotFound"
    assert err_404.name == "CatalogApiErr404NotFound"
    assert err_404.status == 404
    assert err_404.schema_name == "ApiError"
    assert err_404.suffix == "not-found"
    assert err_404.conditions == {"detail": "Not found"}

    err_500: ErrorVariantInfo = catalog_api.errors[1]
    assert err_500.variant_name == "CatalogApiErr500Code"
    assert err_500.status == 500
    assert err_500.required_keys == ["code"]

    # Verify standalone functions
    assert len(info.functions) == 1
    fn_info: FunctionInfo = info.functions[0]
    assert fn_info.name == "extract_heading"
    assert fn_info.is_raw is False
    assert fn_info.doc == "Extracts heading"
    assert fn_info.return_type.base == "string"
    param0: FnParamInfo = fn_info.params[0]
    assert param0.name == "document"
    assert param0.type_info.base == "document"

    # Verify JSON definitions (including hoisted inline value schema)
    json_names = [j.name for j in info.json_defs]
    assert json_names == ["Author", "TranslationsValue", "Translations", "ApiError"]
    hoisted_def: JsonDefInfo = info.json_defs[1]
    assert hoisted_def.name == "TranslationsValue"
    assert hoisted_def.is_hoisted is True
    stream_f: JsonFieldInfo = hoisted_def.fields[0]
    assert stream_f.name == "stream_url"
    assert stream_f.alias == "url"
    token_f: JsonFieldInfo = hoisted_def.fields[2]
    assert token_f.name == "internal_token"
    assert token_f.skip is True

    # Verify extensions
    assert len(info.extensions) == 1
    ext_info: ExtensionInfo = info.extensions[0]
    assert ext_info.namespace == "StrExt"
    op_info: ExtensionOpInfo = ext_info.operations[0]
    assert op_info.qualified_name == "StrExt.slugify"
    assert "py" in op_info.targets
    assert "ts" in op_info.targets

    # Verify serialization methods
    as_dict = info.to_dict()
    assert as_dict["ok"] is True
    assert len(as_dict["structs"]) == 3
    as_json = json.loads(info.to_json())
    assert as_json["source_file"] == "catalog.kdl"

    # Single-file inline inspect_schema and invalid schema inspection
    single_info = parsmith.inspect_schema(
        'struct Simple { title { css "h1"; text } }', path="simple.kdl"
    )
    assert single_info.ok is True
    assert len(single_info.structs) == 1
    assert single_info.structs[0].name == "Simple"

    bad_info = parsmith.inspect_schema('struct Broken { f { trim } }', path="bad.kdl")
    assert bad_info.ok is False
    assert bool(bad_info) is False
    assert bad_info.has_errors() is True
    assert len(bad_info.errors) == 1
    assert bad_info.errors[0].code == "E100"
    assert "error[E100]" in bad_info.format_text()


def test_generate_from_disk_and_error_handling(tmp_path: Path) -> None:
    schema_file = tmp_path / "disk_card.kdl"
    schema_file.write_text(
        'struct DiskCard { heading { css "h2"; text; trim } }\n',
        encoding="utf-8",
    )
    res = parsmith.generate(schema_file, "python")
    assert res.ok is True
    assert "disk_card.py" in res.files

    bad_gen = parsmith.generate('struct Broken { f { trim } }', "python")
    assert bad_gen.ok is False
    assert bool(bad_gen) is False
    assert bad_gen.has_errors() is True
    assert bad_gen.files == {}
    assert len(bad_gen.errors) == 1
    assert bad_gen.errors[0].code == "E100"
    assert "error[E100]" in bad_gen.format_text()
    assert json.loads(bad_gen.format_json())["diagnostics"][0]["code"] == "E100"
    assert bad_gen.to_dict()["ok"] is False


def test_json_to_kdl_roundtrip_into_check_and_generate() -> None:
    sample_payload = json.dumps(
        {
            "@id": 101,
            "user-name": "charlie",
            "tags": ["rust", "python"],
            "metrics": {"score": 98.5, "verified": True},
        }
    )

    kdl_schema = parsmith.json_to_kdl(sample_payload, name="UserResponse")
    assert "json UserResponse" in kdl_schema
    assert 'id int from="@id"' in kdl_schema
    assert 'user_name str from="user-name"' in kdl_schema

    # Pass bootstrapped KDL directly into parsmith.check() and parsmith.generate()
    full_schema = (
        kdl_schema
        + """
    (raw)struct PayloadExtractor {
        payload { jsonify UserResponse }
    }
    """
    )
    chk = parsmith.check(full_schema)
    assert chk.ok is True, chk.format_text()

    gen = parsmith.generate(full_schema, "python")
    assert gen.ok is True, gen.format_text()

    mod = types.ModuleType("bootstrapped_mod")
    sys.modules[mod.__name__] = mod
    try:
        exec(gen["schema.py"], mod.__dict__)
        parsed = mod.PayloadExtractor(sample_payload).parse()
        assert parsed == {
            "payload": {
                "id": 101,
                "user_name": "charlie",
                "tags": ["rust", "python"],
                "metrics": {"score": 98.5, "verified": True},
            }
        }
    finally:
        sys.modules.pop(mod.__name__, None)

    with pytest.raises(ValueError):
        parsmith.json_to_kdl("{broken json")


def test_generate_go_and_rust_targets_and_diagnostics() -> None:
    schema = """
    struct Article {
        title { css "h1"; text; trim }
        tags { css-all "ul.tags > li"; text }
    }
    """

    # 1. CodegenOptions with target="go" and package="pkg"
    opts_go = CodegenOptions(target="go", package="pkg")
    assert opts_go.target == "go"
    assert opts_go.package == "pkg"
    assert "package=\"pkg\"" in repr(opts_go) or "package='pkg'" in repr(opts_go)
    assert opts_go.to_dict()["package"] == "pkg"
    opts_go.package = "otherpkg"
    assert opts_go.package == "otherpkg"

    # 2. parsmith.generate(..., target="go", package="custompkg")
    res_go = parsmith.generate(schema, target="go", package="custompkg", module_stem="article_go")
    assert res_go.ok is True, res_go.format_text()
    assert list(res_go.files.keys()) == ["article_go.go"]
    go_code = res_go["article_go.go"]
    assert "package custompkg" in go_code
    assert "type ArticleType struct" in go_code
    assert "type Article struct" in go_code

    # 3. parsmith.generate(..., target="golang", separate_runtime=True, runtime_name="my_rt")
    res_golang_sep = parsmith.generate(
        schema,
        target="golang",
        separate_runtime=True,
        runtime_name="my_rt",
        options={"package": "sep_pkg"},
    )
    assert res_golang_sep.ok is True, res_golang_sep.format_text()
    assert set(res_golang_sep.files.keys()) == {"schema.go", "my_rt.go"}
    assert "package sep_pkg" in res_golang_sep["schema.go"]
    assert "package sep_pkg" in res_golang_sep["my_rt.go"]
    assert "func rtCssSelect(" not in res_golang_sep["schema.go"]
    assert "func rtCssSelect(" in res_golang_sep["my_rt.go"]

    # 4. parsmith.generate(..., target="rust")
    res_rust = parsmith.generate(schema, target="rust", module_stem="article_rs")
    assert res_rust.ok is True, res_rust.format_text()
    assert list(res_rust.files.keys()) == ["article_rs.rs"]
    rust_code = res_rust["article_rs.rs"]
    assert "pub mod rt {" in rust_code
    assert "pub struct ArticleType {" in rust_code
    assert "pub struct ArticleParser {" in rust_code

    # 5. parsmith.generate(..., target="rs", separate_runtime=True, runtime_name="my_rt")
    res_rs_sep = parsmith.generate(
        schema,
        target="rs",
        separate_runtime=True,
        runtime_name="my_rt",
    )
    assert res_rs_sep.ok is True, res_rs_sep.format_text()
    assert set(res_rs_sep.files.keys()) == {"schema.rs", "my_rt.rs"}
    assert "use super::my_rt as rt;" in res_rs_sep["schema.rs"]
    assert "pub mod rt {" not in res_rs_sep["schema.rs"]
    assert "pub struct RtError" in res_rs_sep["my_rt.rs"]

    # 6. Invalid Go package name -> E002
    bad_pkg_res = parsmith.generate(schema, target="go", package="123invalid")
    assert bad_pkg_res.ok is False
    assert bad_pkg_res.has_errors() is True
    assert bad_pkg_res.files == {}
    assert any(d.code == "E002" for d in bad_pkg_res.errors)

    # 7. XPath operation -> E203 for Go and Rust
    xpath_schema = """
    struct XPathExtractor {
        heading { xpath "//h1"; text }
    }
    """
    xpath_go = parsmith.generate(xpath_schema, target="go")
    assert xpath_go.ok is False
    assert any(d.code == "E203" for d in xpath_go.errors)

    xpath_rust = parsmith.generate(xpath_schema, target="rust")
    assert xpath_rust.ok is False
    assert any(d.code == "E203" for d in xpath_rust.errors)


def test_generate_elixir_php_csharp_kotlin_targets_and_php_html_lib() -> None:
    schema = """
    struct Article {
        title { css "h1"; text; trim }
        tags { css-all "ul.tags > li"; text }
    }
    """

    # 1. Elixir (`elixir` and alias `ex`)
    opts_ex = CodegenOptions("ex", package="MyApp.Parsers")
    assert opts_ex.target == "elixir"
    assert opts_ex.package == "MyApp.Parsers"
    res_ex = parsmith.generate(
        schema,
        target="elixir",
        package="MyApp.Parsers",
        separate_runtime=True,
        runtime_name="rt_ex",
        module_stem="article_ex",
    )
    assert res_ex.ok is True, res_ex.format_text()
    assert set(res_ex.files.keys()) == {"article_ex.ex", "rt_ex.ex"}
    assert "defmodule MyApp.Parsers.Article do" in res_ex["article_ex.ex"]
    assert "defmodule MyApp.Parsers.RtEx do" in res_ex["rt_ex.ex"]

    # 2. PHP (`php`, `php-native`, `php-dom-crawler`, `php_html_lib`, `html_lib`)
    opts_php = CodegenOptions("php")
    assert opts_php.target == "php"
    assert opts_php.php_html_lib == "native"
    assert opts_php.to_dict()["php_html_lib"] == "native"
    opts_php.php_html_lib = "dom-crawler"
    assert opts_php.php_html_lib == "dom-crawler"

    opts_php_shorthand = CodegenOptions("php-dom-crawler")
    assert opts_php_shorthand.target == "php"
    assert opts_php_shorthand.php_html_lib == "dom-crawler"
    opts_php_shorthand.target = "php-native"
    assert opts_php_shorthand.php_html_lib == "native"

    res_php_native = parsmith.generate(schema, "php", package="App\\Parsers")
    assert res_php_native.ok is True, res_php_native.format_text()
    assert "namespace App\\Parsers;" in res_php_native["schema.php"]
    assert "\\DOMDocument" in res_php_native["schema.php"]

    res_php_crawler = parsmith.generate(
        schema,
        "php",
        php_html_lib="dom-crawler",
        separate_runtime=True,
        runtime_name="rt_php",
    )
    assert res_php_crawler.ok is True, res_php_crawler.format_text()
    assert set(res_php_crawler.files.keys()) == {"schema.php", "rt_php.php"}
    assert "Symfony\\Component\\DomCrawler\\Crawler" in res_php_crawler["schema.php"]
    assert "require_once __DIR__ . '/rt_php.php';" in res_php_crawler["schema.php"]

    res_php_shorthand = parsmith.generate(schema, "php-dom-crawler")
    assert res_php_shorthand.ok is True
    assert "Symfony\\Component\\DomCrawler\\Crawler" in res_php_shorthand["schema.php"]

    res_php_hl_kwarg = parsmith.generate(schema, "php", html_lib="dom-crawler")
    assert res_php_hl_kwarg.ok is True
    assert "Symfony\\Component\\DomCrawler\\Crawler" in res_php_hl_kwarg["schema.php"]

    # 3. C# (`csharp` and alias `cs`)
    opts_cs = CodegenOptions("cs", package="MyApp.Parsers")
    assert opts_cs.target == "csharp"
    res_cs = parsmith.generate(
        schema,
        target="csharp",
        package="MyApp.Parsers",
        separate_runtime=True,
        runtime_name="RtModule",
        module_stem="ArticleCs",
    )
    assert res_cs.ok is True, res_cs.format_text()
    assert set(res_cs.files.keys()) == {"ArticleCs.cs", "RtModule.cs"}
    assert "namespace MyApp.Parsers;" in res_cs["ArticleCs.cs"]
    assert "public sealed class Article" in res_cs["ArticleCs.cs"]
    assert "public static partial class Rt" in res_cs["RtModule.cs"]

    # 4. Kotlin (`kotlin` and alias `kt`)
    opts_kt = CodegenOptions("kt", package="com.example.parsers")
    assert opts_kt.target == "kotlin"
    res_kt = parsmith.generate(
        schema,
        target="kotlin",
        package="com.example.parsers",
        separate_runtime=True,
        runtime_name="rt_kt",
        module_stem="ArticleKt",
    )
    assert res_kt.ok is True, res_kt.format_text()
    assert set(res_kt.files.keys()) == {"ArticleKt.kt", "rt_kt.kt"}
    assert "package com.example.parsers" in res_kt["ArticleKt.kt"]
    assert "class Article(" in res_kt["ArticleKt.kt"]
    assert "object Rt" in res_kt["rt_kt.kt"]


def test_directory_and_multi_path_batch_check_and_generate_separate_runtime(
    tmp_path: Path,
) -> None:
    schemas_dir = tmp_path / "schemas"
    schemas_dir.mkdir()

    common_path = schemas_dir / "_common.kdl"
    common_path.write_text(
        '''
        define HOST="https://kodik.example"
        define SEARCH_REQ="curl '{{HOST}}/search?q={{query:str}}'"
        extension NormExt {
            clean {
                sig str str
                py {
                    helper norm_title {
                        source "def norm_title(s: str) -> str:\\n    return s.strip()"
                    }
                    emit "{{out}} = norm_title({{in}})"
                }
            }
        }
        ''',
        encoding="utf-8",
    )

    defines_only_path = schemas_dir / "defines_only.kdl"
    defines_only_path.write_text(
        'define UNUSED_ONLY="1"\n',
        encoding="utf-8",
    )

    kodik_path = schemas_dir / "kodik.kdl"
    kodik_path.write_text(
        '''
        import "./_common.kdl" {
            (define)HOST
            (define)SEARCH_REQ
            (extension)NormExt
        }
        struct KodikPage {
            @request SEARCH_REQ
            title { css "h1.title"; text; !NormExt.clean }
        }
        ''',
        encoding="utf-8",
    )

    yummy_path = schemas_dir / "yummy_anime.kdl"
    yummy_path.write_text(
        '''
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
        ''',
        encoding="utf-8",
    )

    # 1. Directory and multi-path check
    chk_dir = parsmith.check(schemas_dir)
    assert chk_dir.ok is True, chk_dir.format_text()

    chk_multi = parsmith.check([kodik_path, yummy_path])
    assert chk_multi.ok is True, chk_multi.format_text()

    with pytest.raises(TypeError):
        parsmith.check(b"not a str")  # type: ignore[arg-type]

    # 2. Directory and multi-path generate with separate_runtime=True
    res_dir = parsmith.generate(schemas_dir, "python", separate_runtime=True)
    assert res_dir.ok is True, res_dir.format_text()
    assert set(res_dir.files.keys()) == {
        "kodik.py",
        "yummy_anime.py",
        "parsmith_runtime.py",
    }
    assert "_common.py" not in res_dir.files
    assert "defines_only.py" not in res_dir.files
    assert "from .parsmith_runtime import " in res_dir["kodik.py"]
    assert "from .parsmith_runtime import " in res_dir["yummy_anime.py"]

    res_multi = parsmith.generate(
        [kodik_path, yummy_path], "python", separate_runtime=True
    )
    assert res_multi.ok is True, res_multi.format_text()
    assert set(res_multi.files.keys()) == {
        "kodik.py",
        "yummy_anime.py",
        "parsmith_runtime.py",
    }
    assert "from .parsmith_runtime import " in res_multi["kodik.py"]
    assert "from .parsmith_runtime import " in res_multi["yummy_anime.py"]

    # 3. Write generated package to disk and execute both KodikPage.fetch and YummyClient.get_anime end-to-end
    pkg_dir = tmp_path / "my_scrapers"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("", encoding="utf-8")
    for fname, content in res_dir.files.items():
        (pkg_dir / fname).write_text(content, encoding="utf-8")

    sys.path.insert(0, str(tmp_path))
    try:
        rt_mod: Any = importlib.import_module("my_scrapers.parsmith_runtime")
        kodik_mod: Any = importlib.import_module("my_scrapers.kodik")
        yummy_mod: Any = importlib.import_module("my_scrapers.yummy_anime")

        class MockSyncTransport:
            def __init__(self, status: int, body: str, content_type: str) -> None:
                self.status = status
                self.body = body
                self.content_type = content_type
                self.last_url = ""
                self.last_params: Any = []

            def send(self, req: Any) -> Any:
                self.last_url = req.url
                self.last_params = req.params
                return rt_mod.RawResponse(
                    status=self.status,
                    headers={"content-type": self.content_type},
                    text=self.body,
                )

        # Execute HTML @request + user extension helper via KodikPage.fetch
        html_transport = MockSyncTransport(
            200,
            '<html><body><h1 class="title">  Code Geass  </h1></body></html>',
            "text/html",
        )
        kodik_page = kodik_mod.KodikPage.fetch(html_transport, query="geass")
        assert html_transport.last_url == "https://kodik.example/search"
        assert html_transport.last_params == {"q": "geass"}
        assert kodik_page.parse() == {"title": "Code Geass"}

        # Execute REST endpoint + JSON projection via YummyClient.get_anime
        rest_transport = MockSyncTransport(
            200,
            '{"id": 77, "title": "Steins;Gate"}',
            "application/json",
        )
        anime_res = yummy_mod.YummyClient.get_anime(rest_transport, anime_id=77)
        assert rest_transport.last_url == "https://api.yummy.example/v1/anime/77"
        assert isinstance(anime_res, rt_mod.Ok)
        assert anime_res.is_ok is True
        assert anime_res.value == {"id": 77, "title": "Steins;Gate"}
    finally:
        if str(tmp_path) in sys.path:
            sys.path.remove(str(tmp_path))
        for mod_name in [
            "my_scrapers.yummy_anime",
            "my_scrapers.kodik",
            "my_scrapers.parsmith_runtime",
            "my_scrapers",
        ]:
            sys.modules.pop(mod_name, None)



