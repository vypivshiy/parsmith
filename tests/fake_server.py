"""Deterministic local loopback HTTP server for E2E REST and @request tests.

Can be used as a session-scoped pytest fixture on ``127.0.0.1:0`` via
:class:`FakeServer`, or run standalone from the command line::

    uv run python tests/fake_server.py --host 127.0.0.1 --port 8080
"""

from __future__ import annotations

import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import re
import threading
from types import TracebackType
from typing import Any
import urllib.parse

_KNOWN_BASE_URLS: tuple[str, ...] = (
    "https://api.example.com",
    "http://api.example.com",
    "https://h.example.com",
    "http://h.example.com",
    "https://petstore.example.com",
    "http://petstore.example.com",
    "https://example.com",
    "http://example.com",
    "https://dummyjson.com",
    "http://dummyjson.com",
    "https://databay.com",
    "http://databay.com",
)


def rewrite_schema_host(kdl_src: str, base_url: str) -> str:
    """Rewrite target domains and raw HTTP request lines in a KDL schema to ``base_url``.

    Args:
        kdl_src: Raw KDL schema source text.
        base_url: Loopback base URL such as ``http://127.0.0.1:54321``.

    Returns:
        Modified KDL schema source targeting ``base_url`` over plain HTTP.
    """
    clean_base = base_url.rstrip("/")
    host_port = clean_base.split("://", 1)[-1]
    out = kdl_src
    for prefix in _KNOWN_BASE_URLS:
        out = out.replace(prefix, clean_base)
    out = re.sub(
        r"(?m)^(\s*(?:GET|POST|PUT|DELETE|PATCH|HEAD|OPTIONS)\s+)(/\S*)(\s+HTTP/[0-9.]+)",
        rf"\g<1>{clean_base}\g<2>\g<3>",
        out,
    )
    out = re.sub(
        r"(?m)^(\s*Host:\s*)\S+",
        rf"\g<1>{host_port}",
        out,
    )
    return out


def _make_dummy_product(
    prod_id: int = 1,
    *,
    title: str = "Essence Mascara Lash Princess",
    category: str = "beauty",
    price: float = 9.99,
    stock: int = 50,
) -> dict[str, Any]:
    return {
        "id": prod_id,
        "title": title,
        "description": "Popular mascara known for volumizing and lengthening effects.",
        "category": category,
        "price": price,
        "discountPercentage": 10.48,
        "rating": 4.94,
        "stock": stock,
        "tags": ["beauty", "mascara"],
        "brand": "Essence",
        "sku": f"BEA-ESS-{prod_id:03d}",
        "weight": 4.0,
        "dimensions": {"width": 15.14, "height": 13.08, "depth": 22.99},
        "warrantyInformation": "1 week warranty",
        "shippingInformation": "Ships in 3-5 business days",
        "availabilityStatus": "In Stock",
        "reviews": [
            {
                "rating": 5,
                "comment": "Very satisfied!",
                "date": "2025-01-01T00:00:00.000Z",
                "reviewerName": "Alice",
                "reviewerEmail": "alice@example.com",
            }
        ],
        "returnPolicy": "No return policy",
        "minimumOrderQuantity": 24,
        "meta": {
            "createdAt": "2025-01-01T00:00:00.000Z",
            "updatedAt": "2025-01-02T00:00:00.000Z",
            "barcode": "5784719087687",
            "qrCode": "https://cdn.dummyjson.com/public/qr-code.png",
        },
        "images": [f"https://cdn.dummyjson.com/product-images/{prod_id}/1.webp"],
        "thumbnail": f"https://cdn.dummyjson.com/product-images/{prod_id}/thumbnail.webp",
        "undeclaredNoise": "ignored",
    }


_PET_SUMMARIES: list[dict[str, Any]] = [
    {
        "id": 1,
        "name": "Bella",
        "species": "dog",
        "breed": "Golden Retriever",
        "age": 3,
        "weight": 28.5,
        "vaccinated": True,
        "photo_url": "https://petstore.example.com/photos/bella.jpg",
        "extra_field": "ignored",
    },
    {
        "id": 2,
        "name": "Milo",
        "species": "cat",
        "breed": None,
        "age": 2,
        "weight": 4.2,
        "vaccinated": True,
        "photo_url": None,
        "extra_field": "ignored",
    },
]


def _parse_cookie_header(raw_cookie: str) -> dict[str, str]:
    cookies: dict[str, str] = {}
    if not raw_cookie:
        return cookies
    for part in raw_cookie.split(";"):
        stripped = part.strip()
        if "=" in stripped:
            k, v = stripped.split("=", 1)
            cookies[k.strip()] = v.strip()
    return cookies


class FakeRestRequestHandler(BaseHTTPRequestHandler):
    """Deterministic HTTP request handler for E2E tests."""

    verbose: bool = False

    def log_message(self, format: str, *args: Any) -> None:
        """Suppress noisy stderr access logs unless verbose mode is enabled."""
        if self.verbose:
            super().log_message(format, *args)

    def _read_body(self) -> str:
        length_str = self.headers.get("Content-Length")
        if not length_str:
            return ""
        try:
            length = int(length_str)
        except ValueError:
            return ""
        if length <= 0:
            return ""
        return self.rfile.read(length).decode("utf-8", errors="replace")

    def _collect_request_meta(
        self,
    ) -> tuple[str, str, dict[str, list[str]], dict[str, str], dict[str, str]]:
        parsed_url = urllib.parse.urlsplit(self.path)
        path = parsed_url.path or "/"
        query = parsed_url.query
        params = urllib.parse.parse_qs(query, keep_blank_values=True)
        headers_dict = {
            str(k).lower(): str(v) for k, v in self.headers.items()
        }
        cookies_dict = _parse_cookie_header(self.headers.get("Cookie", ""))
        return path, query, params, headers_dict, cookies_dict

    def _send_payload(
        self,
        status: int,
        body: bytes,
        *,
        content_type: str = "application/json; charset=utf-8",
        extra_headers: dict[str, str] | None = None,
        raw_body: str = "",
    ) -> None:
        path, query, params, req_headers, req_cookies = (
            self._collect_request_meta()
        )
        self.send_response(status)
        if content_type:
            self.send_header("Content-Type", content_type)
        self.send_header("X-Request-Id", "abc123")
        self.send_header("X-Echo-Method", self.command)
        self.send_header("X-Echo-Path", path)
        self.send_header("X-Echo-Query", query)
        self.send_header(
            "X-Echo-Params", json.dumps(params, ensure_ascii=True)
        )
        self.send_header(
            "X-Echo-Headers", json.dumps(req_headers, ensure_ascii=True)
        )
        self.send_header(
            "X-Echo-Cookies", json.dumps(req_cookies, ensure_ascii=True)
        )
        self.send_header(
            "X-Echo-Body", json.dumps(raw_body, ensure_ascii=True)
        )
        if extra_headers:
            for hk, hv in extra_headers.items():
                self.send_header(hk, hv)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        if body:
            self.wfile.write(body)

    def _send_json(
        self,
        status: int,
        payload: Any,
        *,
        content_type: str = "application/json; charset=utf-8",
        extra_headers: dict[str, str] | None = None,
        raw_body: str = "",
    ) -> None:
        data = json.dumps(payload).encode("utf-8")
        self._send_payload(
            status,
            data,
            content_type=content_type,
            extra_headers=extra_headers,
            raw_body=raw_body,
        )

    def _send_text(
        self,
        status: int,
        text: str,
        *,
        content_type: str = "text/html; charset=utf-8",
        extra_headers: dict[str, str] | None = None,
        raw_body: str = "",
    ) -> None:
        self._send_payload(
            status,
            text.encode("utf-8"),
            content_type=content_type,
            extra_headers=extra_headers,
            raw_body=raw_body,
        )

    def do_GET(self) -> None:
        """Handle GET requests across REST, DummyJSON, PetStore, and HTML/@request routes."""
        raw_body = self._read_body()
        path, query, params, req_headers, req_cookies = (
            self._collect_request_meta()
        )

        def q_first(name: str, default: str = "") -> str:
            vals = params.get(name)
            return vals[0] if vals else default

        # 1. Bare query placeholder on root: GET /?32-<username>
        if path == "/" and query.startswith("32-"):
            username = query.split("32-", 1)[1].split("&", 1)[0]
            if username == "nobody":
                self._send_text(
                    200,
                    "<html><body><p>No avatar</p></body></html>",
                    raw_body=raw_body,
                )
            else:
                self._send_text(
                    200,
                    f"<html><body><img src='x'><span class='user'>{username}</span></body></html>",
                    raw_body=raw_body,
                )
            return

        # 2. Void ping endpoint
        if path == "/ping":
            self._send_payload(204, b"", content_type="", raw_body=raw_body)
            return

        # 3. Multi-REST namespace routes (/a and /b)
        if path == "/a":
            self._send_json(200, {"a": 10}, raw_body=raw_body)
            return
        if path == "/b":
            self._send_json(200, {"b": "hello"}, raw_body=raw_body)
            return

        # 4. Users endpoints (/users and /users/<id>)
        if path == "/users":
            if q_first("status") == "404":
                self._send_json(
                    404,
                    {"code": 404, "message": "not found"},
                    raw_body=raw_body,
                )
            else:
                self._send_json(
                    200,
                    {"id": 0, "name": "", "nickname": None},
                    raw_body=raw_body,
                )
            return

        if path.startswith("/users/"):
            user_id = urllib.parse.unquote(path.removeprefix("/users/"))
            if user_id in ("99", "999", "404"):
                self._send_json(
                    404,
                    {
                        "code": 404,
                        "message": "not found",
                        "error": "x",
                        "detail": "y",
                    },
                    raw_body=raw_body,
                )
                return
            if user_id == "404_nokeys":
                self._send_json(
                    404,
                    {"message": "not found"},
                    raw_body=raw_body,
                )
                return
            if user_id == "500":
                self._send_json(
                    500,
                    {"code": 500, "message": "internal"},
                    raw_body=raw_body,
                )
                return
            if user_id == "429":
                self._send_json(
                    429,
                    {"error": "rate limited"},
                    raw_body=raw_body,
                )
                return
            if user_id == "err200":
                self._send_json(
                    200,
                    {
                        "detail": "msg",
                        "extra": 1,
                        "error": {"code": 1001, "message": "Quota exceeded"},
                    },
                    raw_body=raw_body,
                )
                return
            if user_id == "null":
                self._send_payload(
                    200,
                    b"null",
                    content_type="application/json",
                    raw_body=raw_body,
                )
                return
            if user_id == "prim":
                self._send_payload(
                    200,
                    b"12345",
                    content_type="application/json",
                    raw_body=raw_body,
                )
                return
            if user_id == "bad":
                self._send_text(
                    200,
                    "<html>not json</html>",
                    content_type="text/html; charset=utf-8",
                    raw_body=raw_body,
                )
                return
            if user_id == "mismatch":
                self._send_json(
                    200,
                    {"id": 10},
                    raw_body=raw_body,
                )
                return

            try:
                uid = int(user_id)
            except ValueError:
                uid = 0
            self._send_json(
                200,
                {
                    "id": uid,
                    "name": "Alice",
                    "nickname": None,
                    "extra_ignored": 1,
                },
                raw_body=raw_body,
            )
            return

        # 6. Items endpoints (/items and /items/<id>)
        if path == "/items":
            page = int(q_first("page", "0") or "0")
            limit = int(q_first("limit", "1") or "1")
            x_val = page * 10 + limit if page or limit else 1
            self._send_json(
                200,
                {"x": x_val, "page": page, "limit": limit},
                raw_body=raw_body,
            )
            return

        if path.startswith("/items/"):
            item_id = urllib.parse.unquote(path.removeprefix("/items/"))
            try:
                iid = int(item_id)
            except ValueError:
                iid = 7
            self._send_json(
                200,
                {"id": iid, "name": "Item"},
                raw_body=raw_body,
            )
            return

        # 7. Search endpoint (/search)
        if path == "/search":
            mode = q_first("mode")
            if mode == "err200":
                self._send_json(
                    200,
                    {"error": {"code": 1001, "message": "Quota exceeded"}},
                    raw_body=raw_body,
                )
                return
            if mode == "ok_empty_err":
                self._send_json(
                    200,
                    {"id": 1, "name": "Alice", "error": {}},
                    raw_body=raw_body,
                )
                return
            if mode == "404_specific":
                self._send_json(
                    404,
                    {"error": "Ничего не найдено", "code": 404},
                    raw_body=raw_body,
                )
                return
            if mode == "404_fallthrough":
                self._send_json(
                    404,
                    {
                        "error": "Ничего не найдено",
                        "message": "Fallback generic message",
                    },
                    raw_body=raw_body,
                )
                return
            if mode == "404_empty":
                self._send_json(404, {}, raw_body=raw_body)
                return
            if mode == "404_html":
                self._send_text(
                    404,
                    "<html><body>404 Not Found</body></html>",
                    content_type="text/html; charset=utf-8",
                    raw_body=raw_body,
                )
                return
            if mode == "400_mismatch":
                self._send_json(
                    400,
                    {"unexpected": "shape"},
                    raw_body=raw_body,
                )
                return

            q_val = q_first("q")
            self._send_json(
                200,
                {"id": 0, "name": q_val},
                raw_body=raw_body,
            )
            return

        # 8. /me (supports non-JSON Content-Type & error envelope)
        if path == "/me":
            ct = q_first("ct", "application/json; charset=utf-8")
            status_code = int(q_first("status", "200") or "200")
            if status_code == 404:
                self._send_json(
                    404,
                    {
                        "data": {"user": {"id": 9, "name": "TestUser"}},
                        "code": 404,
                    },
                    content_type=ct,
                    raw_body=raw_body,
                )
                return
            self._send_json(
                status_code,
                {
                    "id": 1,
                    "name": "TestUser",
                    "nickname": "TU",
                    "x": 42,
                    "data": {
                        "user": {
                            "id": 1,
                            "name": "TestUser",
                            "nickname": "TU",
                        }
                    },
                },
                content_type=ct,
                raw_body=raw_body,
            )
            return

        # 9. /articles and /articles/<id>
        if path == "/articles":
            err = q_first("err")
            if err == "400":
                self._send_json(
                    400,
                    {
                        "code": "INVALID_QUERY",
                        "detail": "Limit out of bounds",
                        "extra": 123,
                    },
                    raw_body=raw_body,
                )
                return
            if err == "500":
                self._send_json(
                    500,
                    {"raw_error": "Internal server crash"},
                    raw_body=raw_body,
                )
                return
            self._send_json(
                200,
                [
                    {
                        "id": 101,
                        "title": "Breaking News",
                        "author": {"name": "Jane Doe", "slug": "jane-doe"},
                        "category_tags": ["news", "tech"],
                        "unmapped_extra": True,
                    }
                ],
                raw_body=raw_body,
            )
            return

        if path.startswith("/articles/"):
            art_id = urllib.parse.unquote(path.removeprefix("/articles/"))
            self._send_text(
                200,
                f"<html><body><h1>Article {art_id}</h1><p class='summary'>Summary {art_id}</p></body></html>",
                raw_body=raw_body,
            )
            return

        # 10. /anime/<id> (nested (dict)json @value blocks)
        if path.startswith("/anime/"):
            anime_id = urllib.parse.unquote(path.removeprefix("/anime/"))
            if anime_id == "404":
                self._send_json(
                    404,
                    {"code": 404, "message": "Anime not found"},
                    raw_body=raw_body,
                )
                return
            self._send_json(
                200,
                {
                    "translations": {
                        "sub_en": {
                            "is_active": True,
                            "unmodeled_noise": "drop",
                            "episodes": {
                                "1": {
                                    "stream_url": "https://stream.example/1",
                                    "bitrate": 1080,
                                    "secret_token": "hidden",
                                    "fps": 60,
                                },
                                "2": {
                                    "stream_url": "https://stream.example/2",
                                    "bitrate": None,
                                    "secret_token": "hidden2",
                                },
                            },
                        }
                    }
                },
                raw_body=raw_body,
            )
            return

        # 11. DummyJSON routes (/products, /products/categories, /products/search, /products/category/<cat>, /products/<id>)
        if path == "/products/categories":
            self._send_json(
                200,
                [
                    {
                        "slug": "beauty",
                        "name": "Beauty",
                        "url": "https://dummyjson.com/products/category/beauty",
                    },
                    {
                        "slug": "laptops",
                        "name": "Laptops",
                        "url": "https://dummyjson.com/products/category/laptops",
                    },
                ],
                raw_body=raw_body,
            )
            return

        if path == "/products/search":
            q_val = q_first("q")
            if q_val == "error400":
                self._send_json(
                    400,
                    {"message": "Invalid search query"},
                    raw_body=raw_body,
                )
                return
            if q_val == "error500":
                self._send_json(
                    500,
                    {"message": "Search backend failure"},
                    raw_body=raw_body,
                )
                return
            limit = int(q_first("limit", "10") or "10")
            self._send_json(
                200,
                {
                    "products": [
                        _make_dummy_product(1, title=f"Result for {q_val}")
                    ],
                    "total": 1,
                    "skip": 0,
                    "limit": limit,
                },
                raw_body=raw_body,
            )
            return

        if path.startswith("/products/category/"):
            cat = urllib.parse.unquote(path.removeprefix("/products/category/"))
            if cat in ("unknown", "404"):
                self._send_json(
                    404,
                    {"message": f"Category '{cat}' not found"},
                    raw_body=raw_body,
                )
                return
            self._send_json(
                200,
                {
                    "products": [
                        _make_dummy_product(
                            2, title=f"{cat.title()} Item", category=cat
                        )
                    ],
                    "total": 1,
                    "skip": 0,
                    "limit": 30,
                },
                raw_body=raw_body,
            )
            return

        if path == "/products":
            limit = int(q_first("limit", "30") or "30")
            skip = int(q_first("skip", "0") or "0")
            self._send_json(
                200,
                {
                    "products": [
                        _make_dummy_product(1),
                        _make_dummy_product(
                            2, title="Eyeshadow Palette with Mirror"
                        ),
                    ],
                    "total": 2,
                    "skip": skip,
                    "limit": limit,
                },
                raw_body=raw_body,
            )
            return

        if path.startswith("/products/"):
            prod_str = urllib.parse.unquote(path.removeprefix("/products/"))
            if prod_str in ("999", "99999", "404"):
                self._send_json(
                    404,
                    {"message": f"Product with id '{prod_str}' not found"},
                    raw_body=raw_body,
                )
                return
            try:
                pid = int(prod_str)
            except ValueError:
                pid = 1
            self._send_json(
                200,
                _make_dummy_product(pid),
                raw_body=raw_body,
            )
            return

        # 13. PetStore routes (/api/v1/pets, /api/v1/pets/search, /api/v1/pets/<id>)
        if path == "/api/v1/pets":
            page = int(q_first("page", "1") or "1")
            per_page = int(q_first("per_page", "20") or "20")
            self._send_json(
                200,
                {
                    "pets": _PET_SUMMARIES,
                    "pagination": {
                        "total": len(_PET_SUMMARIES),
                        "page": page,
                        "per_page": per_page,
                        "total_pages": 1,
                    },
                },
                raw_body=raw_body,
            )
            return

        if path == "/api/v1/pets/search":
            species = q_first("species")
            breed = q_first("breed")
            if species == "invalid":
                self._send_json(
                    422,
                    {
                        "message": "Unsupported species filter",
                        "code": "UNPROCESSABLE_SPECIES",
                    },
                    raw_body=raw_body,
                )
                return
            filtered = [
                p
                for p in _PET_SUMMARIES
                if (not species or p["species"] == species)
                and (not breed or p["breed"] == breed)
            ]
            self._send_json(
                200,
                {
                    "pets": filtered,
                    "pagination": {
                        "total": len(filtered),
                        "page": 1,
                        "per_page": 20,
                        "total_pages": 1,
                    },
                },
                raw_body=raw_body,
            )
            return

        if path.startswith("/api/v1/pets/"):
            pet_str = urllib.parse.unquote(path.removeprefix("/api/v1/pets/"))
            if pet_str == "400":
                self._send_json(
                    400,
                    {"message": "Bad pet ID", "code": "BAD_ID"},
                    raw_body=raw_body,
                )
                return
            if pet_str in ("404", "999"):
                self._send_json(
                    404,
                    {"message": "Pet not found", "code": "PET_NOT_FOUND"},
                    raw_body=raw_body,
                )
                return
            if pet_str == "422":
                self._send_json(
                    422,
                    {
                        "message": "Unprocessable pet ID",
                        "code": "UNPROCESSABLE",
                    },
                    raw_body=raw_body,
                )
                return
            if pet_str == "429":
                self._send_json(
                    429,
                    {"message": "Too many requests", "code": "RATE_LIMIT"},
                    raw_body=raw_body,
                )
                return
            if pet_str == "500":
                self._send_json(
                    500,
                    {"message": "Vet database offline", "code": "DB_DOWN"},
                    raw_body=raw_body,
                )
                return
            if pet_str == "503":
                self._send_json(
                    503,
                    {"reason": "Service unavailable"},
                    raw_body=raw_body,
                )
                return
            try:
                pet_id = int(pet_str)
            except ValueError:
                pet_id = 1
            self._send_json(
                200,
                {
                    "id": pet_id,
                    "name": "Bella",
                    "species": "dog",
                    "breed": "Golden Retriever",
                    "age": 3,
                    "weight": 28.5,
                    "vaccinated": True,
                    "photo_url": "https://petstore.example.com/photos/bella.jpg",
                    "owner": {
                        "id": 10,
                        "full_name": "Sarah Connor",
                        "phone": "+1-555-0199",
                        "email": "sarah@example.com",
                    },
                    "vet_records": [
                        {
                            "date": "2025-03-15",
                            "clinic": "Happy Paws Clinic",
                            "diagnosis": "Annual checkup",
                            "treatment": "Rabies booster",
                        }
                    ],
                    "notes": "Friendly with children",
                },
                raw_body=raw_body,
            )
            return

        # 14. HTML & Raw @request routes (/posts/<id>, /p, /html/search, /logs/<stream_id>, /free-proxy-list.txt, /playlist.txt)
        if path.startswith("/posts/"):
            post_id = urllib.parse.unquote(path.removeprefix("/posts/"))
            title_text = "Hello World" if post_id == "1" else f"Post {post_id}"
            self._send_text(
                200,
                f"<html><body><h1>{title_text}</h1></body></html>",
                raw_body=raw_body,
            )
            return

        if path == "/p":
            id_val = q_first("id")
            tag_val = q_first("tag")
            sess_val = req_cookies.get("session", "")
            track_val = req_cookies.get("tracking", "")
            self._send_text(
                200,
                f"<html><body><h1>id={id_val} tag={tag_val} session={sess_val} tracking={track_val}</h1></body></html>",
                raw_body=raw_body,
            )
            return

        if path == "/html/search":
            q_val = q_first("q")
            lang_val = q_first("lang")
            page_val = q_first("page")
            x_static = req_headers.get("x-static", "")
            x_opt = req_headers.get("x-opt", "")
            x_caller = req_headers.get("x-caller", "")
            self._send_text(
                200,
                (
                    f"<html><body>"
                    f"<h1>Search: {q_val}</h1>"
                    f"<p class='lang'>{lang_val}</p>"
                    f"<p class='hdr'>{x_static}|{x_opt}|{x_caller}</p>"
                    f"<p class='page'>{page_val}</p>"
                    f"</body></html>"
                ),
                raw_body=raw_body,
            )
            return

        if path.startswith("/logs/"):
            stream_id = urllib.parse.unquote(path.removeprefix("/logs/"))
            self._send_json(
                200,
                {
                    "payload": {
                        "lines": [
                            f"{stream_id}:line-1",
                            f"{stream_id}:line-2",
                            f"{stream_id}:line-3",
                        ]
                    }
                },
                raw_body=raw_body,
            )
            return

        if path == "/free-proxy-list.txt":
            self._send_text(
                200,
                "10.0.0.1:8080\n10.0.0.2:3128\n10.0.0.3:9090",
                content_type="text/plain; charset=utf-8",
                raw_body=raw_body,
            )
            return

        if path == "/playlist.txt":
            self._send_text(
                200,
                "[480p]/v/01_480.m3u8\n[720p]/v/01_720.m3u8",
                content_type="text/plain; charset=utf-8",
                raw_body=raw_body,
            )
            return

        self._send_json(
            404,
            {"code": 404, "message": f"Unknown route: {path}"},
            raw_body=raw_body,
        )

    def do_POST(self) -> None:
        """Handle POST requests for JSON bodies, form bodies, and HTML slug previews."""
        raw_body = self._read_body()
        path, _query, _params, req_headers, _req_cookies = (
            self._collect_request_meta()
        )
        ct = req_headers.get("content-type", "")

        if path == "/users":
            name = "NewUser"
            nickname: str | None = None
            if "application/json" in ct and raw_body:
                try:
                    parsed = json.loads(raw_body)
                    if isinstance(parsed, dict):
                        name = str(parsed.get("name", "NewUser"))
                        nick_val = parsed.get("nickname")
                        nickname = (
                            str(nick_val) if nick_val is not None else None
                        )
                except Exception:
                    pass
            elif "application/x-www-form-urlencoded" in ct and raw_body:
                form = urllib.parse.parse_qs(raw_body, keep_blank_values=True)
                if "name" in form and form["name"]:
                    name = form["name"][0]
            self._send_json(
                201,
                {
                    "id": 99,
                    "name": name,
                    "nickname": nickname,
                    "extra_ignored": True,
                },
                raw_body=raw_body,
            )
            return

        if path == "/products/add":
            title = "Created Product"
            price = 19.99
            stock = 10
            if raw_body:
                try:
                    parsed = json.loads(raw_body)
                    if isinstance(parsed, dict):
                        title = str(parsed.get("title", title))
                        price = float(parsed.get("price", price))
                        stock = int(parsed.get("stock", stock))
                except Exception:
                    pass
            self._send_json(
                201,
                _make_dummy_product(
                    195, title=title, price=price, stock=stock
                ),
                raw_body=raw_body,
            )
            return

        if path.startswith("/articles/slug/"):
            slug = urllib.parse.unquote(path.removeprefix("/articles/slug/"))
            form = urllib.parse.parse_qs(raw_body, keep_blank_values=True)
            mode = form.get("mode", [""])[0]
            token = form.get("token", [""])[0]
            summary = f"From POST mode={mode}" + (
                f" token={token}" if token else ""
            )
            self._send_text(
                200,
                f"<html><body><h1>Slug Article: {slug}</h1><p class='summary'>{summary}</p></body></html>",
                raw_body=raw_body,
            )
            return

        self._send_json(
            404,
            {"code": 404, "message": f"Unknown POST route: {path}"},
            raw_body=raw_body,
        )

    def do_DELETE(self) -> None:
        """Handle DELETE requests."""
        raw_body = self._read_body()
        self._send_payload(204, b"", content_type="", raw_body=raw_body)


class FakeServer:
    """In-process threaded loopback HTTP server for REST and @request E2E testing."""

    def __init__(self, host: str = "127.0.0.1", port: int = 0) -> None:
        self._requested_host = host
        self._requested_port = port
        self._httpd: ThreadingHTTPServer | None = None
        self._thread: threading.Thread | None = None

    @property
    def host(self) -> str:
        """Bound server host address."""
        if self._httpd is not None:
            addr = self._httpd.server_address[0]
            return addr.decode("utf-8") if isinstance(addr, bytes) else str(addr)
        return self._requested_host

    @property
    def port(self) -> int:
        """Bound TCP port number."""
        if self._httpd is not None:
            return int(self._httpd.server_address[1])
        return self._requested_port

    @property
    def base_url(self) -> str:
        """Loopback base URL (``http://127.0.0.1:<port>``)."""
        return f"http://{self.host}:{self.port}"

    def start(self) -> FakeServer:
        """Bind the server socket and start the background serving thread."""
        if self._httpd is not None:
            return self
        self._httpd = ThreadingHTTPServer(
            (self._requested_host, self._requested_port),
            FakeRestRequestHandler,
        )
        self._thread = threading.Thread(
            target=self._httpd.serve_forever,
            name="parsmith-fake-rest-server",
            daemon=True,
        )
        self._thread.start()
        return self

    def stop(self) -> None:
        """Shut down the server and close the listening socket."""
        if self._httpd is not None:
            self._httpd.shutdown()
            self._httpd.server_close()
            self._httpd = None
        if self._thread is not None:
            self._thread.join(timeout=2.0)
            self._thread = None

    def rewrite_schema(self, kdl_src_or_path: str | Path) -> str:
        """Read a KDL schema (source string or file path) and rewrite its target host to ``self.base_url``."""
        if isinstance(kdl_src_or_path, Path):
            raw = kdl_src_or_path.read_text(encoding="utf-8")
        else:
            raw = kdl_src_or_path
        return rewrite_schema_host(raw, self.base_url)

    def __enter__(self) -> FakeServer:
        return self.start()

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc_val: BaseException | None,
        exc_tb: TracebackType | None,
    ) -> None:
        self.stop()


def main() -> None:
    """Run the fake REST server as a standalone foreground process."""
    parser = argparse.ArgumentParser(
        description="Standalone local loopback Fake REST Server for parsmith E2E testing."
    )
    parser.add_argument(
        "--host",
        default="127.0.0.1",
        help="Host interface to bind (default: 127.0.0.1)",
    )
    parser.add_argument(
        "--port",
        type=int,
        default=8080,
        help="TCP port to bind (default: 8080)",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Enable HTTP request access logging to stderr",
    )
    args = parser.parse_args()

    FakeRestRequestHandler.verbose = bool(args.verbose)
    httpd = ThreadingHTTPServer(
        (str(args.host), int(args.port)), FakeRestRequestHandler
    )
    bound_host, bound_port = httpd.server_address[0], httpd.server_address[1]
    host_str = (
        bound_host.decode("utf-8")
        if isinstance(bound_host, bytes)
        else str(bound_host)
    )
    print(f"FakeServer listening on http://{host_str}:{bound_port}")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        httpd.server_close()


if __name__ == "__main__":
    main()
