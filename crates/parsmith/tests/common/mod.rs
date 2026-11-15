#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEST_TMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

pub struct TempTestDir {
    pub path: PathBuf,
}

impl TempTestDir {
    pub fn new(prefix: &str) -> Self {
        let id = TEST_TMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "parsmith_codegen_{}_{}_{}",
            prefix,
            std::process::id(),
            id
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp test dir");
        Self { path: dir }
    }

    pub fn write(&self, rel: &str, content: &str) -> PathBuf {
        let full = self.path.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(&full, content).expect("write file");
        full
    }
}

impl Drop for TempTestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

pub fn resolve_jsdom_pkg() -> String {
    std::env::var("PARSMITH_JSDOM_PKG").unwrap_or_else(|_| {
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut candidates = vec![
            manifest_dir.join("../../playground/package.json"),
            manifest_dir.join("../../package.json"),
        ];
        if let Ok(out) = std::process::Command::new("git")
            .args(["rev-parse", "--git-common-dir"])
            .current_dir(manifest_dir)
            .output()
            && out.status.success()
        {
            let common_git = manifest_dir.join(String::from_utf8_lossy(&out.stdout).trim());
            if let Some(root) = common_git.parent() {
                candidates.push(root.join("playground/package.json"));
                candidates.push(root.join("package.json"));
            }
        }
        for pkg in &candidates {
            if pkg.with_file_name("node_modules").join("jsdom").exists() {
                return pkg.to_string_lossy().replace('\\', "/");
            }
        }
        candidates[0].to_string_lossy().replace('\\', "/")
    })
}

pub const HTML_FIXTURE: &str =
    include_str!("../../../../tests/integration/fixtures/dsl_coverage.html");

pub const SCHEMA_ITEM_INIT_CHECK: &str = r#"
@doc "Item struct with @init, @pre-validate, @check, and nested struct."

struct SingleCard {
    title {
        css "h1"
        text
        trim
    }
    active {
        css ".is-active"
        text
        to-bool
    }
}

struct PageItem {
    @doc "Main page item parser."

    @init {
        canonical-url {
            css "link[rel='canonical']"
            attr "href"
            fallback ""
        }
    }

    @pre-validate {
        assert "missing single section" {
            css "section.single"
        }
    }

    @check has-cards {
        css ".card"
        to-bool
        fallback #false
    }

    heading {
        css "body > h1"
        text
        trim
    }

    canonical {
        @canonical-url
    }

    single {
        css "section.single"
        nested SingleCard
    }
}
"#;

pub const SCHEMA_01_STRINGS_BASIC: &str = r####"
@doc "Focused coverage: CSS selectors, extraction, raw/html cleanup and scalar string transforms."

define CSS-CARD=".card"
define CSS-LINK="a[href]"
define FMT-ABS="https://example.test{{}}"

define STRIP-URL {
    rm-prefix "/"
    rm-suffix ".html"
}

(list)struct StringsBasic {
    @pre-validate {
        assert {
            css CSS-CARD
        }
    }

    @split-doc {
        css-all CSS-CARD
    }

    clean-html {
        css-remove ".ad, script"
        raw
    }

    inner-html {
        raw inner
    }

    title {
        css ".title"
        text
        ltrim
        rtrim
        trim
        normalize-space
        repl "foo" "bar"
    }

    link {
        css CSS-LINK
        attr "href"
        STRIP-URL
        fmt FMT-ABS
    }

    slug {
        css ".slug"
        text
        rm-prefix-suffix "__"
        fallback ""
    }

    normalized-code {
        css ".code"
        text
        re-sub #"\s+"# "-"
        fallback ""
    }

    active-flag {
        css ".is-active"
        to-bool
        fallback #false
    }
}
"####;

pub const SCHEMA_02_ARRAYS_AND_CONVERSIONS: &str = r####"
@doc "Focused coverage: list/string array operations, regex extraction, conversions and list transforms."

define CSS-CARD=".card"
define CSS-LINK="a[href]"
define RE-DIGITS=#"(\d+)"#
define RE-FLOAT=#"(\d+\.\d+)"#
define RE-WORD=#"^[A-Za-z0-9_-]+$"#
define RE-HAS-DIGIT=#"\d"#

define REPL-STATE {
    repl {
        draft "0"
        published "1"
        archived "2"
    }
}

(list)struct ArraysAndConversions {
    @split-doc {
        css-all CSS-CARD
    }

    flags-joined {
        css-all "[data-flag]"
        attr "data-flag"
        lower
        upper
        unique keep-order=#true
        join ","
        fallback ""
    }

    token-list {
        css ".tokens"
        text
        split ","
        trim
        unescape
        assert {
            re-all RE-WORD
            re-any RE-HAS-DIGIT
        }
        fallback {}
    }

    first-token {
        css ".tokens"
        text
        split ","
        trim
        first
        fallback ""
    }

    second-token {
        css ".tokens"
        text
        split ","
        trim
        index 1
        fallback ""
    }

    last-token {
        css ".tokens"
        text
        split ","
        trim
        last
        fallback ""
    }

    token-window {
        css ".tokens"
        text
        split ","
        trim
        slice 0 2
        join "|"
        fallback ""
    }

    state-code {
        css ".state"
        text
        lower
        REPL-STATE
        to-int
        fallback #null
    }

    score {
        css ".score"
        text
        re RE-DIGITS
        to-int
        fallback 0
    }

    ratio {
        css ".ratio"
        text
        re RE-FLOAT
        to-float
        fallback #null
    }

    raw-number-matches {
        raw
        re-all RE-DIGITS
        fallback {}
    }

    any-link-count {
        css-all CSS-LINK
        len
    }
}
"####;

pub const SCHEMA_03_FILTERS_AND_PREDICATES: &str = r####"
@doc "Focused coverage: filter/assert containers and rich predicate combinations for CSS-selected values."

define CSS-CARD=".card"
define CSS-LINK="a[href]"
define RE-WORD=#"^[A-Za-z0-9_-]+$"#
define RE-HREF=#"https?://|/"#
define RE-CODE=#"^code"#

(list)struct FiltersAndPredicates {
    @split-doc {
        css-all CSS-CARD
    }

    filtered-links {
        css-all CSS-LINK
        filter {
            css ".icon"
            has-attr "href" "data-kind"
            attr-eq "data-kind" "nav" "cta"
            attr-ne "rel" "nofollow" "sponsored"
            attr-starts "href" "/" "https://"
            attr-ends "href" ".html" "/"
            attr-re "href" RE-HREF
            text-starts "Go" "Read"
            text-ends "more" "now"
            text-contains "guide" "docs"
            text-re #"guide|docs"#
        }
        attr "href" "data-alt"
        unique keep-order=#true
        fallback {}
    }

    logic-check {
        css ".logic"
        text
        assert {
            contains "alpha"
            not {
                eq "forbidden"
            }
            or {
                starts "pre"
                ends "suf"
            }
            and {
                re RE-WORD
                contains "a"
            }
        }
        fallback ""
    }

    numeric-check {
        css ".code"
        text
        assert {
            len-gt 3
            len-ge 4
            len-lt 20
            len-le 20
            ne "skip" "ignore"
            eq "code-4" "code-8"
            re RE-CODE
        }
        fallback ""
    }
}
"####;

pub const SCHEMA_05_FLAT: &str = r#"
@doc "Focused coverage: flat structs and @init cached values."

define CSS-LINK="a[href]"

(flat)struct FlatCoverage keep-order=#true {
    @init {
        hrefs {
            css-all CSS-LINK
            attr "href"
        }
    }

    cached-hrefs {
        @hrefs
        fallback {}
    }

    filtered-hrefs {
        css-all CSS-LINK
        attr "href"
        filter {
            contains "https://" "/"
        }
        fallback {}
    }

    first-href {
        css-all CSS-LINK
        attr "href"
        first
        fallback ""
    }

    token-parts {
        css ".tokens"
        text
        split ","
        trim
        fallback {}
    }
}
"#;

pub const SCHEMA_06_DICT: &str = r#"
@doc "Focused coverage: dict structs and key/value extraction semantics."

(dict)struct MetaDict {
    @split-doc {
        css-all "meta[name][content]"
    }

    @key {
        attr "name"
    }

    @value {
        attr "content"
    }
}

(dict)struct MetaAliasDict {
    @split-doc {
        css-all "meta[property][content], meta[name][content]"
    }

    @key {
        attr "property" "name"
        first
    }

    @value {
        attr "content"
    }
}

struct DictRoot {
    named-meta {
        nested MetaDict
    }

    alias-meta {
        nested MetaAliasDict
    }
}
"#;

pub const SCHEMA_07_TABLE: &str = r####"
@doc "Focused coverage: table structs, @rows/@match/@value and table-specific predicates."

define CSS-TABLE="table.coverage"
define RE-FLOAT=#"(\d+\.\d+)"#
define RE-CODE=#"^code"#

(table)struct TableCoverage {
    @pre-validate {
        assert {
            css "table.coverage tr"
        }
    }

    @table {
        css CSS-TABLE
    }

    @rows {
        css-all "tr"
    }

    @match {
        css "th"
        text
        trim
        lower
    }

    @value {
        css "td"
        text
        trim
    }

    identifier {
        match {
            eq "id" "identifier"
        }
    }

    code-value {
        match {
            re RE-CODE
        }
    }

    price {
        match {
            starts "price"
            contains "price" "amount"
            not {
                eq "price-old"
            }
        }
        re RE-FLOAT
        to-float
        fallback 0.0
    }

    tax-or-fee {
        match {
            ends "tax" "fee"
        }
        re RE-FLOAT
        to-float
        fallback 0.0
    }

    state {
        match {
            eq "state" "status"
            ne "skip" "ignore"
        }
        lower
        fallback ""
    }
}
"####;

pub const SCHEMA_23_RAW_STRUCT: &str = r####"
@doc "RAW struct tests: plain-text parsing without HTML backend."

(raw)struct RawPlayerScript {
    playlist_url {
        re #"file:\s*[\"']([^\"']+)[\"']"#
    }
    player_id {
        re #"id:\s*[\"']([^\"']+)[\"']"#
    }
}

(raw)struct RawUrlParams {
    dubbing_code {
        split "?"
        index 1
        split "&"
        filter { starts "dubbing_code=" }
        index 0
        rm-prefix "dubbing_code="
    }
    anime_id {
        split "?"
        index 1
        split "&"
        filter { starts "anime_id=" }
        index 0
        rm-prefix "anime_id="
        to-int
    }
}

(raw)struct RawLineItems {
    @split-doc {
        split "\n"
    }

    quality {
        re #"\[(\d+p)\]"#
    }
    url {
        re #"\](.+)"#
    }
}

(raw)struct RawTextTransform {
    upper_text {
        upper
    }
    word_count {
        re-split #"\s+"#
        len
    }
    first_word {
        re-split #"\s+"#
        index 0
    }
}
"####;

pub const SCHEMA_25_FN: &str = r####"
@doc "fn directive tests: module-level single-value extraction functions."

fn page_title {
    @doc "Extract the <h1> text from an HTML document."
    css "h1"
    text
}

fn all_links {
    css-all "a"
    attr "href"
}

(raw)fn first_line {
    @doc "Return the first line of a raw text document."
    split "\n"
    index 0
}

(raw)fn extract_version {
    re #"version=([0-9]+\.[0-9]+\.[0-9]+)"#
}
"####;

pub const SCHEMA_PYTHON_EXTENSIONS: &str = r####"
extension Utils {
    to-base64 {
        sig str str

        py {
            import "from base64 import b64encode"
            emit #"{{out}} = b64encode({{in}}.encode("utf-8")).decode("ascii")"#
        }
    }

    normalize {
        sig str str

        py {
            helper normalize_value {
                import "import re"
                source #"""
                    def normalize_value(value: str) -> str:
                        return re.sub(r"\s+", " ", value).strip()
                    """#
            }
            emit #"{{out}} = normalize_value({{in}})"#
        }
    }

    truthy {
        sig T bool
        py { emit #"{{out}} = bool({{in}})"# }
    }
}

(raw)struct ExtRawParser {
    @check is-valid {
        !Utils.truthy
    }

    encoded {
        !Utils.normalize
        !Utils.to-base64
    }
}

(raw)fn encode_normalized {
    !Utils.normalize
    !Utils.to-base64
}
"####;

pub const SCHEMA_XPATH_DOM: &str = r#"
struct XpathArticle {
    heading {
        xpath "//section[@class='single']/h1"
        text
        trim
    }

    nested_names {
        xpath-all "//div[@class='nested-card']"
        filter {
            xpath ".//a[@href]"
        }
        xpath-remove ".//a"
        text
        trim
    }
}
"#;

pub const SCHEMA_JSON_PROJECTORS: &str = r####"
@doc "Comprehensive JSON projector coverage: object, array, dict, inline hoisting, GJSON paths, nullable, and omitempty."

json JsonAuthor {
    name str
    slug str
}

(array)json JsonQuote {
    text str
    author JsonAuthor
    tags (array)str
    score int
    rating float
    active bool
    note str?
}

(dict)json Translations {
    @key str
    @value TranslationValue {
        title str from="display_title"
        count int
        badge str @omitempty from="meta.badge"
        notes @skip
    }
}

json ReleaseManifest {
    os_version str from="os\\.version"
    nested_version str from="meta.os\\.version"
    backslash_key str from="meta.path\\\\dir"
    hash_key str from="meta.\\#"
    item_count int from="items.#"
    opt_count int? from="missing.#"
    tag_names (array)str from="tags.#.name"
    deep_ids (array)int from="groups.#.meta.id"
    legacy_id int @omitempty from="meta.legacy.id"
    inline_meta {
        env str
        region str? @omitempty
    }
    channels (dict)? @omitempty {
        @key str
        @value (array)str
    }
}

struct JsonBasic {
    @init {
        raw-json {
            css "script#test-data"
            text
            re #"(\[.*\])"#
        }
    }

    full-payload {
        @raw-json
        jsonify JsonQuote
    }

    first-author {
        @raw-json
        jsonify JsonAuthor path="0.author"
    }

    second-author {
        @raw-json
        jsonify JsonAuthor path="1.author"
    }
}

(raw)struct ManifestParser {
    manifest {
        jsonify ReleaseManifest path="envelope\\.data"
    }
}

(raw)struct TranslationsParser {
    translations {
        jsonify Translations
    }
}
"####;

pub const SCHEMA_HTML_FETCH_REQUESTS: &str = r####"
@doc "HTML and Raw structs with @request, typed placeholders, query array styles, and response-join."

struct ArticlePage {
    @doc "Article HTML page parser with fetch methods."

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

pub const SCHEMA_REST_CLIENT: &str = r####"
@doc "Monadic REST client coverage: Ok, typed @error / @error:<suffix> variants, UnknownErr, TransportErr, and DecodeErr."

json User {
    id int
    name str
    nickname str?
    role str @omitempty
}

(array)json ArticleList {
    article_id int from="id"
    headline str from="title"
}

json ApiError {
    code int from="error.code"
    message str from="error.message"
}

json SpecificNotFoundError {
    error str
    code int
}

json GenericError {
    message str
}

(rest)struct UserApi {
    @doc "User service REST client."

    @error 200 ApiError "error.code"
    @error:not-found 404 SpecificNotFoundError error="Not Found"
    @error 404 GenericError
    @error 400 ApiError

    @request name="get-user" response=User doc="Fetch a user by ID." """
    GET /users/{{id:int}}?include_meta={{include_meta:bool?}} HTTP/1.1
    Host: api.example.com
    X-App: parsmith
    """

    @request name="create-user" response=User """
    POST /users?tag={{tag:str}}&filter={{filter:str?}} HTTP/1.1
    Host: api.example.com
    Content-Type: application/json
    X-Trace: {{trace:str?}}
    Cookie: sid={{sid:str}}; pref={{pref:str?}}

    {"name": "{{name:str}}", "nickname": {{nickname:str?}}, "active": {{active:bool}}}
    """

    @request name="get-profile-enveloped" response=User response-path="data.user" """
    GET /me HTTP/1.1
    Host: api.example.com
    """

    @request name="list-articles" response=ArticleList """
    GET /articles HTTP/1.1
    Host: api.example.com
    """

    @request name="ping" """
    GET /ping HTTP/1.1
    Host: api.example.com
    """
}
"####;

pub const SCHEMA_LIST_STRINGS_ARRAYS: &str = r####"
@doc "List struct covering CSS selectors, multi-key attr, string/regex transforms, slicing, and type conversions."

(list)struct ProductCatalog {
    @split-doc {
        css-all ".product-card"
    }

    clean-html {
        css-remove ".ad, script"
        raw
    }

    inner-html {
        raw inner
    }

    title {
        css {
            "h2.title"
            "h3.fallback-title"
        }
        text
        ltrim
        rtrim
        trim
        normalize-space
        unescape
        repl "foo" "bar"
    }

    slug {
        css "a.link"
        attr "href"
        rm-prefix "/"
        rm-suffix ".html"
        rm-prefix-suffix "-"
        lower
        fmt "https://example.test/{{}}?ref={{}}"
    }

    all-links {
        css-all "a[href]"
        attr "href"
    }

    media-urls {
        css-all "[href], [src], [data-src]"
        attr "href" "src" "data-src"
        unique keep-order=#true
    }

    tags {
        css ".tags"
        text
        split ","
        trim
        upper
        slice 0 3
    }

    first-tag {
        css ".tags"
        text
        split ","
        trim
        first
        fallback "UNTAGGED"
    }

    sku {
        css ".sku"
        text
        re #"SKU-([0-9]+)"#
        to-int
    }

    price {
        css ".price"
        text
        re-sub #"[^0-9.]"# ""
        to-float
        fallback 0.0
    }

    in-stock {
        css ".stock-badge"
        to-bool
    }
}
"####;

pub const SCHEMA_FILTERS_AND_PREDICATES: &str = r####"
@doc "Filter, assert, and boolean combinator conditions (and, or, not)."

struct FilteredPage {
    active-links {
        css-all "a"
        filter {
            has-attr "href"
            not {
                attr-starts "href" "javascript:" "#"
            }
            or {
                attr-eq "data-role" "nav" "cta"
                attr-contains "class" "primary"
                text-re #"(?i)^read\s+more"#
            }
        }
        attr "href"
    }

    valid-codes {
        css-all "span.code"
        text
        trim
        filter {
            starts "PRD-"
            ends "-OK"
            not {
                contains "TEST"
            }
        }
        assert "expected at least one valid code" {
            len-ge 1
            re-all #"^PRD-[A-Z0-9]+-OK$"#
        }
    }
}
"####;

pub const SCHEMA_FLAT_DICT_TABLE: &str = r#"
@doc "Flat, Dict, and Table struct kinds."

(flat)struct FlatTags {
    primary-tags {
        css-all ".tag-primary"
        text
        trim
    }
    extra-tag {
        css ".tag-extra"
        text
        trim
        fallback #null
    }
}

(dict)struct MetaProps {
    @split-doc {
        css-all "meta[name][content]"
    }
    @key {
        attr "name"
        lower
    }
    @value {
        attr "content"
        trim
    }
}

(table)struct ProductSpecs {
    @table {
        css "table.specs"
    }
    @rows {
        css-all "tr"
    }
    @match {
        css "th"
        text
        trim
        lower
    }
    @value {
        css "td"
        text
        trim
    }

    product-id {
        match {
            eq "id" "product id"
        }
    }

    total-qty {
        match {
            eq "quantity"
        }
        to-int
        fallback 0
    }

    unit-price {
        match {
            eq "price"
        }
        to-float
        fallback 0.0
    }
}
"#;

pub const SCHEMA_HTML_FN: &str = r#"
struct CardInfo {
    title {
        css "h2"
        text
        trim
    }
}

fn page_title {
    @doc "Extract the <h1> text from an HTML document."
    css "h1"
    text
    trim
}

fn parse_featured_card {
    css ".card.featured"
    nested CardInfo
}
"#;

pub const SCHEMA_CATALOG_JSON_PROJECTORS: &str = r####"
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
}
"####;
