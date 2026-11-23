# Self-Contained Recipes

Each recipe below demonstrates a complete extraction or REST API pattern using a three-part structure:

1. **Input** — Sample HTML, plain text, or wire JSON payload.
2. **KDL Schema (`.kdl`)** — The declarative `parsmith` schema.
3. **Extracted Typed Output & Multi-Target Usage** — The resulting data structure and how to invoke the generated parser across Python, TypeScript/JavaScript, Go, and Rust.

---

## 1. Single Item Extraction (`struct`)

Extract a single structured object from a product detail page using block pattern-matching selectors (`css { ... }`), regular expression capture (`re`), numeric conversion (`to-float`), runtime validation (`assert`), and nullable fallback (`fallback #null`).

### 1. Input HTML

```html
<main class="product-detail">
  <h1 class="product-title">  Mechanical Keyboard Pro  </h1>
  <div class="pricing">
    <span class="sale-price">USD $149.99</span>
  </div>
  <p class="stock-status">In stock (12 units available)</p>
  <span class="discount-badge"></span>
</main>
```

### 2. KDL Schema (`product_detail.kdl`)

```kdl
struct ProductDetail {
    title {
        css { "h1.product-title"; "h1" }
        text
        normalize-space
        assert "Title must not be empty" { len-ge 3 }
    }
    price {
        css { ".sale-price"; ".regular-price" }
        text
        re #"(\d+\.\d{2})"#
        to-float
        assert { gt 0 }
    }
    stock-count {
        css ".stock-status"
        text
        re #"(\d+)\s+units"#
        to-int
        fallback 0
    }
    coupon-code {
        css ".discount-badge"
        attr "data-coupon"
        fallback #null
    }
}
```

### 3. Extracted Output

```json
{
  "title": "Mechanical Keyboard Pro",
  "price": 149.99,
  "stock_count": 12,
  "coupon_code": null
}
```

### Calling the Generated Parser

**Python (`parsmith generate python product_detail.kdl -o product_detail.py`):**

```python
from product_detail import ProductDetail, ProductDetailType

result: ProductDetailType = ProductDetail(html).parse()
assert result["title"] == "Mechanical Keyboard Pro"
assert result["price"] == 149.99
assert result["coupon_code"] is None
```

**TypeScript (`parsmith generate ts product_detail.kdl -o product_detail.ts`):**

```ts
import { ProductDetail, type ProductDetailType } from "./product_detail";

const result: ProductDetailType = new ProductDetail(html).parse();
```

**Go (`parsmith generate go product_detail.kdl -o product_detail.go`):**

```go
parser, err := NewProductDetail(html)
if err != nil {
	log.Fatal(err)
}
result := parser.Parse() // ProductDetailType
```

**Rust (`parsmith generate rust product_detail.kdl -o product_detail.rs`):**

```rust
let mut parser = ProductDetailParser::new(html)?;
let result: ProductDetailType = parser.parse()?;
```

---

## 2. Repeating Cards (`(list)struct` + `nested`)

Split a catalog page into repeating item cards with `(list)struct` and `@split-doc`, and compose it into a parent page struct via `nested`.

>[!note]
> Intra-file declaration order is strictly top-down (`E302`): declare `CatalogItem` **above** `CatalogPage` so `nested CatalogItem` resolves cleanly.

### 1. Input HTML

```html
<section id="catalog">
  <h2 class="category-heading">Keyboards</h2>
  <article class="product-card" data-sku="KB-01">
    <h3><a href="/items/kb-01">Tactile 75%</a></h3>
    <span class="price">$119.00</span>
    <ul class="tags">
      <li>Hot-swap</li>
      <li>Wireless</li>
    </ul>
  </article>
  <article class="product-card" data-sku="KB-02">
    <h3><a href="/items/kb-02">Linear 65%</a></h3>
    <span class="price">$95.50</span>
    <ul class="tags">
      <li>Wired</li>
    </ul>
  </article>
</section>
```

### 2. KDL Schema (`catalog.kdl`)

```kdl
(list)struct CatalogItem {
    @split-doc { css-all "article.product-card" }

    sku { attr "data-sku" }
    name { css "h3 a"; text; trim }
    url { css "h3 a"; attr "href" }
    price { css ".price"; text; re #"(\d+\.\d{2})"#; to-float }
    tags { css-all "ul.tags li"; text; trim; fallback {} }
}

struct CatalogPage {
    category { css "h2.category-heading"; text; trim }
    items { nested CatalogItem }
}
```

### 3. Extracted Output (`CatalogPage.parse()`)

```json
{
  "category": "Keyboards",
  "items": [
    {
      "sku": "KB-01",
      "name": "Tactile 75%",
      "url": "/items/kb-01",
      "price": 119.0,
      "tags": ["Hot-swap", "Wireless"]
    },
    {
      "sku": "KB-02",
      "name": "Linear 65%",
      "url": "/items/kb-02",
      "price": 95.5,
      "tags": ["Wired"]
    }
  ]
}
```

---

## 3. Specification Table (`(table)struct`)

Extract structured fields from an HTML key-value `<table>` where rows may appear in any order. `(table)struct` locates the table (`@table`), iterates rows (`@rows`), normalizes each row's header key (`@match`), and runs field pipelines starting with `match { ... }` against the cell extracted by `@value`.

### 1. Input HTML

```html
<table class="specs-table">
  <tbody>
    <tr>
      <th>  UPC  </th>
      <td>a897fe39b1053632</td>
    </tr>
    <tr>
      <th>Price (incl. tax)</th>
      <td>£51.77</td>
    </tr>
    <tr>
      <th>Weight</th>
      <td>450g</td>
    </tr>
  </tbody>
</table>
```

### 2. KDL Schema (`specs_table.kdl`)

```kdl
(table)struct ProductSpecs {
    @table { css "table.specs-table" }
    @rows { css-all "tr" }
    @match { css "th"; text; trim; lower }
    @value { css "td"; text; trim }

    upc {
        match { eq "upc" }
    }
    price-incl-tax {
        match { starts "price" }
        re #"(\d+\.\d{2})"#
        to-float
    }
    weight-grams {
        match { eq "weight" }
        re #"(\d+)g"#
        to-int
        fallback #null
    }
}
```

### 3. Extracted Output

```json
{
  "upc": "a897fe39b1053632",
  "price_incl_tax": 51.77,
  "weight_grams": 450
}
```

---

## 4. Metadata Dictionary (`(dict)struct`)

Extract dynamic key-value pairs into a `dict[str, str]` / `Record<string, string>` / `map[string]string` / `HashMap<String, String>` using `(dict)struct` with `@split-doc`, `@key`, and `@value`.

### 1. Input HTML

```html
<head>
  <meta property="og:title" content="Parsmith Schema Guide" />
  <meta property="og:type" content="article" />
  <meta property="og:url" content="https://example.com/guide" />
</head>
```

### 2. KDL Schema (`meta_og.kdl`)

```kdl
(dict)struct OpenGraphMeta {
    @split-doc { css-all "meta[property^='og:']" }
    @key { attr "property"; rm-prefix "og:" }
    @value { attr "content"; trim; fallback #null }
}
```

### 3. Extracted Output

```json
{
  "title": "Parsmith Schema Guide",
  "type": "article",
  "url": "https://example.com/guide"
}
```

---

## 5. Deduplicated Flat String List (`(flat)struct`)

Collect strings across multiple fields into a single deduplicated `list[str]` / `string[]` / `[]string` / `Vec<String>`. Set `keep-order=#true` to preserve first-seen order.

### 1. Input HTML

```html
<footer>
  <nav class="social">
    <a href="https://github.com/org/repo">GitHub</a>
    <a href="https://x.com/org_handle">X</a>
    <a href="https://github.com/org/repo">GitHub Mirror</a>
    <a href="/privacy">Privacy Policy</a>
  </nav>
</footer>
```

### 2. KDL Schema (`social_links.kdl`)

```kdl
(flat)struct SocialLinks keep-order=#true {
    @init {
        hrefs { css-all "footer a[href]"; attr "href" }
    }

    github {
        @hrefs
        filter { contains "github.com" }
        fallback {}
    }
    twitter {
        @hrefs
        filter { contains "twitter.com" "x.com" }
        fallback {}
    }
}
```

### 3. Extracted Output

```json
[
  "https://github.com/org/repo",
  "https://x.com/org_handle"
]
```

---

## 6. Plain-Text & Obfuscated JS Extraction (`(raw)struct` & `(raw)fn`)

When parsing `<script>` bodies, M3U8 playlists, or configuration text that does not require an HTML DOM parser, use `(raw)struct` and `(raw)fn`. Pipeline input starts directly as `STRING` (`STR`).

>[!tip]
> If a `(raw)struct` includes `@split-doc`, its `.parse()` method returns a list of records (`list[T]`); without `@split-doc`, `.parse()` returns a single record (`T`).

### 1. Input Plain Text

```javascript
window.__PLAYER_CONFIG__ = {
  streamUrl: "https://cdn.example.com/hls/master.m3u8?token=9f8a7b",
  bitrateKbps: "4500",
  mirrors: "eu-west,us-east,ap-south"
};
```

### 2. KDL Schema (`player_raw.kdl`)

```kdl
(raw)struct PlayerConfig {
    stream-url {
        re #"streamUrl:\s*"([^"]+)""#
    }
    token {
        re #"token=([a-zA-Z0-9]+)"#
        fallback #null
    }
    bitrate-kbps {
        re #"bitrateKbps:\s*"(\d+)""#
        to-int
    }
    mirrors {
        re #"mirrors:\s*"([^"]+)""#
        split ","
    }
}

(raw)fn extract-stream-token {
    re #"token=([a-zA-Z0-9]+)"#
    fallback #null
}
```

### 3. Extracted Output

`PlayerConfig(script_text).parse()`:

```json
{
  "stream_url": "https://cdn.example.com/hls/master.m3u8?token=9f8a7b",
  "token": "9f8a7b",
  "bitrate_kbps": 4500,
  "mirrors": ["eu-west", "us-east", "ap-south"]
}
```

`extract_stream_token(script_text)` / `extractStreamToken(script_text)` / `ExtractStreamToken(script_text)`:

```json
"9f8a7b"
```

---

## 7. Embedded JSON State (`@init` + `unescape` + `jsonify` + GJSON Paths)

Modern web pages frequently embed state inside HTML attributes (`data-state`) or `<script id="__NEXT_DATA__">` tags. Combine `attr` / `text`, `unescape` (for HTML-entity-encoded attribute payloads), and `jsonify` with **GJSON path expressions** (`\.` for literal dots, `#` for array length, and `#.subpath` for array element projection).

>[!warning]
> `jsonify` does not automatically decode HTML entities. Always place `unescape` before `jsonify` when extracting JSON from an HTML attribute (`attr "data-state"`).

### 1. Input HTML

```html
<div id="app" data-state="{&quot;meta.version&quot;:&quot;2.4&quot;,&quot;store&quot;:{&quot;currency&quot;:&quot;EUR&quot;,&quot;offers&quot;:[{&quot;sku&quot;:&quot;A1&quot;,&quot;price&quot;:29.5},{&quot;sku&quot;:&quot;B2&quot;,&quot;price&quot;:49.0}]}}">
</div>
```

### 2. KDL Schema (`embedded_state.kdl`)

```kdl
json HydratedStore {
    version str from=#"meta\.version"#
    currency str from="store.currency"
    offer_count int from="store.offers.#"
    skus (array)str from="store.offers.#.sku"
    offers (array)OfferItem from="store.offers" {
        sku str
        price float
    }
}

struct AppPage {
    @init {
        raw-state {
            css "#app"
            attr "data-state"
            unescape
        }
    }

    state {
        @raw-state
        jsonify HydratedStore
    }
}
```

### 3. Extracted Output

```json
{
  "state": {
    "version": "2.4",
    "currency": "EUR",
    "offer_count": 2,
    "skus": ["A1", "B2"],
    "offers": [
      { "sku": "A1", "price": 29.5 },
      { "sku": "B2", "price": 49.0 }
    ]
  }
}
```

---

## 8. Typed REST API Client (`(rest)struct` + `@request` + `@error[:<suffix>]`)

Define a transport-agnostic REST client with **Typed placeholders** (`{{id:int}}`, `{{tags:str[]?|csv}}`), allowlist JSON response schemas (`response=`), and **Error discriminators** (`@error[:<suffix>]`) that handle both HTTP 4xx/5xx errors and HTTP 200 soft-error envelopes (`success=#false`).

### 1. Wire JSON Payloads

**200 OK Success Payload:**
```json
{
  "success": true,
  "data": {
    "id": 42,
    "title": "Rust Compiler Internals",
    "tags": ["rust", "compilers"]
  }
}
```

**404 Not Found Error Payload:**
```json
{
  "error": "not_found",
  "message": "Item 42 does not exist"
}
```

**200 Soft Error Payload (`success: false`):**
```json
{
  "success": false,
  "reason": "rate_limited"
}
```

### 2. KDL Schema (`catalog_api.kdl`)

```kdl
define API-BASE="https://api.example.com/v1"
define GET-ITEM-REQ="curl '{{API-BASE}}/items/{{id:int}}?tags={{tags:str[]?|csv}}'"
define DELETE-ITEM-REQ="curl -X DELETE '{{API-BASE}}/items/{{id:int}}'"

json CatalogItem {
    id int from="data.id"
    title str from="data.title"
    tags (array)str from="data.tags"
}

json ApiErrorBody {
    error str
    message str? @omitempty
}

json SoftErrorBody {
    success bool
    reason str
}

(rest)struct CatalogApi {
    @doc "Typed client for the Catalog v1 REST API"

    @request name="get-item" response="CatalogItem" GET-ITEM-REQ
    @request name="delete-item" response=#null DELETE-ITEM-REQ

    @error:not-found 404 ApiErrorBody error="not_found"
    @error:soft-fail 200 SoftErrorBody success=#false "reason"
}
```

### 3. Multi-Target Caller & Monadic Result Handling

Every `(rest)struct` endpoint returns a discriminated result covering five outcomes without throwing uncaught exceptions on HTTP/schema errors:
- **`Ok`** — `2xx` response (that did not match a `200` `@error` rule) projected through the declared `JsonDef`.
- **`<Struct>Err<Status>[<Suffix>]`** — Matched an `@error` rule and projected its error schema.
- **`DecodeErr`** — Response arrived from the network, but JSON parsing or `JsonDef` allowlist projection failed.
- **`UnknownErr`** — Non-`2xx` HTTP status that did not match any `@error` rule.
- **`TransportErr`** — Network/DNS/socket failure raised by the underlying HTTP transport.

#### Python (`via_httpx(client)` / `via_httpx_async(client)` / `via_requests(session)` / `via_aiohttp(session)` / `via_urllib(timeout=None)`)

Third-party Python transport factories require an explicit client or session instance typed via public `*Like` `typing.Protocol` interfaces (`HttpxLike` for `httpx.Client` / `httpx2.Client`, `AsyncHttpxLike` for `httpx.AsyncClient` / `httpx2.AsyncClient`, `RequestsLike` for `requests.Session` / `curl_cffi` / `niquests`, and `AiohttpLike` for `aiohttp.ClientSession`); `via_urllib(timeout=None)` uses stdlib `urllib.request`. (For HTML and `(raw)struct` `@request` helpers, `fetch` / `async_fetch` raise `HttpStatusError` on HTTP `>= 400`.)

```python
import httpx
from catalog_api import (
    CatalogApi,
    CatalogApiErr200SoftFail,
    CatalogApiErr404NotFound,
    DecodeErr,
    Ok,
    TransportErr,
    UnknownErr,
    via_httpx,
)

with httpx.Client() as client:
    res = CatalogApi.get_item(via_httpx(client), id=42, tags=["rust", "compilers"])

match res:
    case Ok(value=item):
        print(f"Found item #{item['id']}: {item['title']}")
    case CatalogApiErr404NotFound(value=err):
        print(f"404 Not Found: {err['message']}")
    case CatalogApiErr200SoftFail(value=err):
        print(f"Soft failure: {err['reason']}")
    case DecodeErr(status=status, cause=cause):
        print(f"Schema mismatch on HTTP {status}: {cause}")
    case UnknownErr(status=status, body=body):
        print(f"Unmapped HTTP {status}: {body}")
    case TransportErr(cause=cause):
        print(f"Network error: {cause}")
```

#### TypeScript / JavaScript (`viaFetch` / `viaAxios`)

```ts
import { CatalogApi, viaFetch } from "./catalog_api";

const res = await CatalogApi.getItem(viaFetch(), {
  id: 42,
  tags: ["rust", "compilers"],
});

if (res.ok) {
  console.log("Found item:", res.value.title);
} else {
  switch (res.kind) {
    case "error":
      if (res.variant === "CatalogApiErr404NotFound") {
        console.error("Not found:", res.value.message);
      } else if (res.variant === "CatalogApiErr200SoftFail") {
        console.error("Soft fail:", res.value.reason);
      }
      break;
    case "decode":
      console.error(`Decode error (HTTP ${res.status}): ${res.cause}`);
      break;
    case "unknown":
      console.error(`Unexpected HTTP ${res.status}:`, res.body);
      break;
    case "transport":
      console.error(`Transport failure: ${res.cause}`);
      break;
  }
}
```

#### Go (`*http.Client` + `WithHeader` / `WithTimeout`)

```go
api := NewCatalogApi()
tags := []string{"rust", "compilers"}
item, err := api.GetItem(http.DefaultClient, 42, tags, WithHeader("X-Request-ID", "req-1"))
if err != nil {
	switch e := err.(type) {
	case *CatalogApiErr404NotFound:
		log.Printf("404 Not Found: %v", e.Body.Message)
	case *CatalogApiErr200SoftFail:
		log.Printf("Soft failure: %s", e.Body.Reason)
	case *DecodeErr:
		log.Printf("Decode error (HTTP %d): %s", e.Status, e.Cause)
	case *UnknownErr:
		log.Printf("Unknown HTTP %d: %s", e.Status, e.Body)
	case *TransportErr:
		log.Printf("Transport error: %s", e.Cause)
	}
	return
}
fmt.Println(item.Title)
```

#### Rust (`&reqwest::Client` + `<Struct>Error`)

```rust
let client = reqwest::Client::new();
match CatalogApi::get_item(&client, 42, Some(&["rust", "compilers"])).await {
    Ok(item) => println!("Found item #{}: {}", item.id, item.title),
    Err(CatalogApiError::Err404NotFound(err)) => {
        eprintln!("404 Not Found: {:?}", err.message);
    }
    Err(CatalogApiError::Err200SoftFail(err)) => {
        eprintln!("Soft failure: {}", err.reason);
    }
    Err(CatalogApiError::Decode(status, err)) => {
        eprintln!("Decode error on HTTP {status}: {err}");
    }
    Err(CatalogApiError::Unknown(status, body)) => {
        eprintln!("Unexpected HTTP {status}: {body}");
    }
    Err(CatalogApiError::Transport(err)) => {
        eprintln!("Transport error: {err}");
    }
}
```
