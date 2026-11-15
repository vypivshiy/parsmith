//! Modular Kotlin runtime (`object Rt`) for `KotlinEmitter`.
//!
//! Splits runtime definitions into:
//! 1. Core / string / Unicode codepoints / regex / collection / JSON helpers (always emitted).
//! 2. HTML DOM & XPath helpers (`org.jsoup`, emitted when `module_has_html(module)` is `true`).
//! 3. HTTP / REST helpers (`java.net.http.HttpClient`, emitted when `module_uses_http(module)` is `true`).

use indexmap::IndexSet;

use crate::ast::Module;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::kotlin::render_kotlin_package;
use crate::codegen::writer::CodeWriter;
use crate::codegen::{CodegenOptions, module_has_html, module_uses_http};

/// Core error, string, Unicode codepoint, regex, collection, and JSON projection runtime helpers.
pub const KOTLIN_CORE_RUNTIME: &str = r##"open class RtError(val context: String, val msg: String) :
    RuntimeException(if (context.isEmpty()) msg else "$context: $msg")

class RtMissingPathError(context: String, msg: String) : RtError(context, msg)

val RT_UNMATCHED_TABLE_ROW: Any = Any()

interface JsonEncodable {
    fun toJsonElement(): JsonElement
    fun toJsonString(): String = toJsonElement().toString()
}

fun toJsonElement(value: Any?): JsonElement = when (value) {
    null -> JsonNull
    is JsonElement -> value
    is JsonEncodable -> value.toJsonElement()
    is String -> JsonPrimitive(value)
    is Boolean -> JsonPrimitive(value)
    is Int -> JsonPrimitive(value)
    is Long -> JsonPrimitive(value)
    is Float -> JsonPrimitive(value)
    is Double -> JsonPrimitive(value)
    is Number -> JsonPrimitive(value)
    is Map<*, *> -> {
        val out = LinkedHashMap<String, JsonElement>()
        for ((k, v) in value.entries) {
            if (k != null) {
                out[k.toString()] = toJsonElement(v)
            }
        }
        JsonObject(out)
    }
    is Iterable<*> -> JsonArray(value.map { toJsonElement(it) })
    is Array<*> -> JsonArray(value.map { toJsonElement(it) })
    else -> JsonPrimitive(value.toString())
}

fun toJsonString(value: Any?): String = toJsonElement(value).toString()

inline fun <T> fallback(block: () -> T, defaultValue: T): T = try {
    block()
} catch (_: Throwable) {
    defaultValue
}

fun <T> itemAt(values: List<T>, index: Long, context: String): T {
    val pos = if (index < 0L) values.size.toLong() + index else index
    if (pos < 0L || pos >= values.size.toLong()) {
        throw RtError(context, "index out of bounds")
    }
    return values[pos.toInt()]
}

fun stringAt(values: List<String>, index: Long, context: String): String =
    itemAt(values, index, context)

fun normalizeSliceRange(len: Long, start: Long, end: Long): Pair<Int, Int> {
    fun norm(n: Long): Long = if (n < 0L) maxOf(len + n, 0L) else minOf(n, len)
    val s = norm(start)
    val e = norm(end)
    return if (s >= e) Pair(0, 0) else Pair(s.toInt(), e.toInt())
}

fun <T> sliceList(values: List<T>, start: Long, end: Long): List<T> {
    val (s, e) = normalizeSliceRange(values.size.toLong(), start, end)
    return if (s >= e) emptyList() else values.subList(s, e).toList()
}

fun <T> len(values: List<T>): Long = values.size.toLong()

fun strLen(value: String): Long = value.codePoints().count()

fun strIndex(value: String, index: Long, context: String): String {
    val cps = value.codePoints().toArray()
    val pos = if (index < 0L) cps.size.toLong() + index else index
    if (pos < 0L || pos >= cps.size.toLong()) {
        throw RtError(context, "index out of bounds")
    }
    return String(cps, pos.toInt(), 1)
}

fun index(value: String, idx: Long, context: String): String =
    strIndex(value, idx, context)

fun strSlice(value: String, start: Long, end: Long): String {
    val cps = value.codePoints().toArray()
    val (s, e) = normalizeSliceRange(cps.size.toLong(), start, end)
    return if (s >= e) "" else String(cps, s, e - s)
}

fun slice(value: String, start: Long, end: Long): String =
    strSlice(value, start, end)

fun <T> unique(values: List<T>): List<T> = values.distinct()

fun trim(value: String, chars: String? = null): String =
    if (chars.isNullOrEmpty()) value.trim() else value.trim { chars.indexOf(it) >= 0 }

fun ltrim(value: String, chars: String? = null): String =
    if (chars.isNullOrEmpty()) value.trimStart() else value.trimStart { chars.indexOf(it) >= 0 }

fun rtrim(value: String, chars: String? = null): String =
    if (chars.isNullOrEmpty()) value.trimEnd() else value.trimEnd { chars.indexOf(it) >= 0 }

private val WS_REGEX = Regex("\\s+")

fun normalizeSpace(value: String): String =
    value.trim().let { if (it.isEmpty()) "" else it.split(WS_REGEX).joinToString(" ") }

fun rmPrefix(value: String, prefix: String): String = value.removePrefix(prefix)

fun rmSuffix(value: String, suffix: String): String = value.removeSuffix(suffix)

fun rmPrefixSuffix(value: String, part: String): String =
    value.removePrefix(part).removeSuffix(part)

fun fmtValue(template: String, value: String): String =
    if (template.contains("{{}}")) {
        template.replace("{{}}", value)
    } else {
        template.replaceFirst("{}", value)
    }

fun repl(value: String, old: String, newValue: String): String =
    value.replace(old, newValue)

fun replMap(value: String, pairs: List<Pair<String, String>>): String {
    var s = value
    for ((old, newValue) in pairs) {
        s = s.replace(old, newValue)
    }
    return s
}

fun lower(value: String): String = value.lowercase()

fun upper(value: String): String = value.uppercase()

private val RE_HEX_ENTITY = Regex("&#[xX]([0-9a-fA-F]+);")
private val RE_DEC_ENTITY = Regex("&#(\\d+);")
private val RE_UNICODE_ESC = Regex("\\\\u([0-9a-fA-F]{4})")
private val RE_BYTES_ESC = Regex("\\\\x([0-9a-fA-F]{2})")

private fun codePointToString(cp: Int): String? =
    if (cp in 0..0x10FFFF && (cp < 0xD800 || cp > 0xDFFF)) {
        String(intArrayOf(cp), 0, 1)
    } else {
        null
    }

fun unescape(value: String): String {
    var s = value
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", "\u00a0")
        .replace("&copy;", "\u00a9")
        .replace("&reg;", "\u00ae")
        .replace("&trade;", "\u2122")
        .replace("&euro;", "\u20ac")
        .replace("&pound;", "\u00a3")
        .replace("&yen;", "\u00a5")
        .replace("&cent;", "\u00a2")
        .replace("&sect;", "\u00a7")
        .replace("&deg;", "\u00b0")
        .replace("&plusmn;", "\u00b1")
        .replace("&times;", "\u00d7")
        .replace("&divide;", "\u00f7")
        .replace("&ndash;", "\u2013")
        .replace("&mdash;", "\u2014")
        .replace("&lsquo;", "\u2018")
        .replace("&rsquo;", "\u2019")
        .replace("&ldquo;", "\u201c")
        .replace("&rdquo;", "\u201d")
        .replace("&bull;", "\u2022")
        .replace("&hellip;", "\u2026")
        .replace("&laquo;", "\u00ab")
        .replace("&raquo;", "\u00bb")
        .replace("&amp;", "&")

    s = RE_HEX_ENTITY.replace(s) { m ->
        m.groupValues[1].toIntOrNull(16)?.let { codePointToString(it) } ?: m.value
    }
    s = RE_DEC_ENTITY.replace(s) { m ->
        m.groupValues[1].toIntOrNull(10)?.let { codePointToString(it) } ?: m.value
    }
    s = RE_UNICODE_ESC.replace(s) { m ->
        m.groupValues[1].toIntOrNull(16)?.let { codePointToString(it) } ?: m.value
    }
    s = RE_BYTES_ESC.replace(s) { m ->
        m.groupValues[1].toIntOrNull(16)?.let { codePointToString(it) } ?: m.value
    }
    return s
        .replace("\\b", "\b")
        .replace("\\f", "\u000c")
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t")
}

fun toInt(value: String, context: String): Long =
    value.trim().toLongOrNull() ?: throw RtError(context, "invalid integer: $value")

fun toFloat(value: String, context: String): Double =
    value.trim().toDoubleOrNull() ?: throw RtError(context, "invalid float: $value")

fun stringBool(value: String): Boolean = value.isNotEmpty()

fun numBool(value: Long): Boolean = value != 0L

fun floatBool(value: Double): Boolean = value != 0.0 && !value.isNaN()

fun <T> seqBool(value: List<T>): Boolean = value.isNotEmpty()

fun assertTrue(value: Boolean, context: String, message: String) {
    if (!value) {
        throw RtError(context, message)
    }
}

fun reSearch(value: String, pattern: String, context: String): String {
    val re = try {
        Regex(pattern)
    } catch (e: Throwable) {
        throw RtError(context, e.message ?: "invalid regex")
    }
    val match = re.find(value) ?: throw RtError(context, "regex did not match")
    return if (match.groupValues.size > 1) match.groupValues[1] else match.value
}

fun reAll(value: String, pattern: String, context: String): List<String> {
    val re = try {
        Regex(pattern)
    } catch (e: Throwable) {
        throw RtError(context, e.message ?: "invalid regex")
    }
    return re.findAll(value).map { m ->
        if (m.groupValues.size > 1) m.groupValues[1] else m.value
    }.toList()
}

private val RE_BACKREF = Regex("\\\\([1-9]\\d*)")

fun reSub(value: String, pattern: String, replacement: String, context: String): String {
    val re = try {
        Regex(pattern)
    } catch (e: Throwable) {
        throw RtError(context, e.message ?: "invalid regex")
    }
    val normRepl = RE_BACKREF.replace(replacement, "\\$$1")
    return re.replace(value, normRepl)
}

fun reSplit(value: String, pattern: String, context: String): List<String> {
    val re = try {
        Regex(pattern)
    } catch (e: Throwable) {
        throw RtError(context, e.message ?: "invalid regex")
    }
    return re.split(value)
}

fun reMatch(value: String, pattern: String): Boolean =
    try {
        Regex(pattern).containsMatchIn(value)
    } catch (_: Throwable) {
        false
    }

fun reAny(values: List<String>, pattern: String): Boolean =
    values.any { reMatch(it, pattern) }

fun reAllMatch(values: List<String>, pattern: String): Boolean =
    values.all { reMatch(it, pattern) }

fun contains(value: String, needle: String): Boolean = value.contains(needle)

fun starts(value: String, needle: String): Boolean = value.startsWith(needle)

fun ends(value: String, needle: String): Boolean = value.endsWith(needle)

fun expectObject(value: JsonElement, context: String): JsonObject =
    value as? JsonObject ?: throw RtError(context, "expected JSON object")

fun expectArray(value: JsonElement, context: String): JsonArray =
    value as? JsonArray ?: throw RtError(context, "expected JSON array")

fun decodeString(value: JsonElement, context: String): String {
    val prim = value as? JsonPrimitive ?: throw RtError(context, "expected string")
    if (!prim.isString) {
        throw RtError(context, "expected string")
    }
    return prim.content
}

fun decodeLong(value: JsonElement, context: String): Long {
    val prim = value as? JsonPrimitive ?: throw RtError(context, "expected integer")
    if (prim.isString) {
        throw RtError(context, "expected integer")
    }
    return prim.content.toLongOrNull() ?: throw RtError(context, "expected integer")
}

fun decodeDouble(value: JsonElement, context: String): Double {
    val prim = value as? JsonPrimitive ?: throw RtError(context, "expected float")
    if (prim.isString) {
        throw RtError(context, "expected float")
    }
    return prim.content.toDoubleOrNull() ?: throw RtError(context, "expected float")
}

fun decodeBoolean(value: JsonElement, context: String): Boolean {
    val prim = value as? JsonPrimitive ?: throw RtError(context, "expected boolean")
    if (prim.isString) {
        throw RtError(context, "expected boolean")
    }
    return when (prim.content) {
        "true" -> true
        "false" -> false
        else -> throw RtError(context, "expected boolean")
    }
}

fun parseMapKeyString(key: String, _context: String): String = key

fun parseMapKeyLong(key: String, context: String): Long =
    key.toLongOrNull() ?: throw RtError(context, "invalid map key '$key'")

fun parseMapKeyDouble(key: String, context: String): Double =
    key.toDoubleOrNull() ?: throw RtError(context, "invalid map key '$key'")

fun parseMapKeyBoolean(key: String, context: String): Boolean = when (key) {
    "true" -> true
    "false" -> false
    else -> throw RtError(context, "invalid map key '$key'")
}

private data class GjsonSeg(val segment: String, val hasEscape: Boolean, val remainder: String?)

private fun nextGjsonSegment(path: String): GjsonSeg {
    val sb = StringBuilder(path.length)
    var hasEscape = false
    var i = 0
    while (i < path.length) {
        val ch = path[i]
        if (ch == '\\' && i + 1 < path.length) {
            val nextCh = path[i + 1]
            if (nextCh == '.' || nextCh == '\\' || nextCh == '#') {
                hasEscape = true
                sb.append(nextCh)
                i += 2
                continue
            }
            sb.append('\\')
            i += 1
        } else if (ch == '.') {
            return GjsonSeg(sb.toString(), hasEscape, path.substring(i + 1))
        } else {
            sb.append(ch)
            i += 1
        }
    }
    return GjsonSeg(sb.toString(), hasEscape, null)
}

fun jsonOpt(value: JsonElement, path: String): JsonElement? = try {
    jsonPath(value, path, "")
} catch (_: Throwable) {
    null
}

fun jsonPath(value: JsonElement, path: String, context: String): JsonElement {
    if (path.isEmpty()) return value
    var current = value
    var rem: String? = path
    while (rem != null) {
        val (segment, hasEscape, nextRem) = nextGjsonSegment(rem)
        rem = nextRem
        if (!hasEscape && segment == "#") {
            if (current is JsonNull) {
                throw RtMissingPathError(context, "missing JSON path segment '#'")
            }
            val arr = current as? JsonArray
                ?: throw RtError(context, "expected JSON array for '#' operator")
            return if (rem == null) {
                JsonPrimitive(arr.size)
            } else {
                val out = ArrayList<JsonElement>()
                for (item in arr) {
                    val v = jsonOpt(item, rem)
                    if (v != null && v !is JsonNull) {
                        out.add(v)
                    }
                }
                JsonArray(out)
            }
        }
        val nextVal: JsonElement? = if (!hasEscape) {
            val idx = segment.toIntOrNull()
            if (idx != null && current is JsonArray) {
                if (idx in 0 until current.size) current[idx] else null
            } else {
                (current as? JsonObject)?.get(segment)
            }
        } else {
            (current as? JsonObject)?.get(segment)
        }
        current = nextVal ?: throw RtMissingPathError(context, "missing JSON path segment '$segment'")
    }
    return current
}

fun extractJsonValue(value: JsonElement, path: String?, context: String): JsonElement =
    if (!path.isNullOrEmpty()) jsonPath(value, path, context) else value

fun parseJson(raw: String, path: String, context: String): JsonElement {
    val parsed = try {
        Json.parseToJsonElement(raw)
    } catch (e: Throwable) {
        throw RtError(context, e.message ?: "invalid JSON")
    }
    return jsonPath(parsed, path, context)
}"##;

/// HTML DOM and XPath extraction helpers (`org.jsoup`), emitted when `module_has_html(module)` is `true`.
pub const KOTLIN_HTML_RUNTIME: &str = r#"const val RT_FALLBACK_HTML_STR: String = "<html><body></body></html>"

fun parseHtml(html: String): Document =
    Jsoup.parse(if (html.isBlank()) RT_FALLBACK_HTML_STR else html).apply {
        outputSettings().prettyPrint(false)
    }

private val RE_CSS_ATTR_TILDE =
    Regex("\\[\\s*([a-zA-Z0-9_-]+)\\s*~=\\s*(?:'([^']*)'|\"([^\"]*)\"|([^\\]\\s'\"]+))\\s*]")
private val RE_CSS_ATTR_PIPE =
    Regex("\\[\\s*([a-zA-Z0-9_-]+)\\s*\\|=\\s*(?:'([^']*)'|\"([^\"]*)\"|([^\\]\\s'\"]+))\\s*]")

private fun normalizeCssQuery(query: String): String {
    var q = RE_CSS_ATTR_TILDE.replace(query) { m ->
        val attr = m.groupValues[1]
        val raw = m.groups[2]?.value ?: m.groups[3]?.value ?: m.groups[4]?.value ?: ""
        "[$attr~=(?:^|\\s)${Regex.escape(raw)}(?:\\s|$)]"
    }
    q = RE_CSS_ATTR_PIPE.replace(q) { m ->
        val attr = m.groupValues[1]
        val raw = m.groups[2]?.value ?: m.groups[3]?.value ?: m.groups[4]?.value ?: ""
        val esc = Regex.escape(raw)
        "[$attr~=(?:^$esc$|^$esc-.*)]"
    }
    return q
}

private fun selectElements(root: Element, query: String): List<Element> {
    val found = root.select(normalizeCssQuery(query))
    return if (root is Document) found else found.filter { it !== root }
}

private fun selectXpathElements(root: Element, query: String): List<Element> {
    val found = root.selectXpath(query)
    return if (root is Document) found else found.filter { it !== root }
}

fun select(root: Element, queries: List<String>, context: String): Element {
    for (query in queries) {
        val first = selectElements(root, query).firstOrNull()
        if (first != null) return first
    }
    throw RtError(context, "selector matched no elements")
}

fun selectOpt(root: Element, queries: List<String>): Element? {
    for (query in queries) {
        val first = selectElements(root, query).firstOrNull()
        if (first != null) return first
    }
    return null
}

fun selectAll(root: Element, queries: List<String>): List<Element> {
    for (query in queries) {
        val found = selectElements(root, query)
        if (found.isNotEmpty()) return found
    }
    return emptyList()
}

private fun cloneForRemove(root: Element): Element {
    if (root is Document) {
        return root.clone().apply { outputSettings().prettyPrint(false) }
    }
    val doc = Document("").apply { outputSettings().prettyPrint(false) }
    val copy = root.clone()
    doc.appendChild(copy)
    return copy
}

fun cssRemove(root: Element, query: String): Element {
    val copy = cloneForRemove(root)
    copy.select(normalizeCssQuery(query)).forEach { if (it !== copy) it.remove() }
    return copy
}

fun cssRemoveAll(roots: List<Element>, query: String): List<Element> =
    roots.map { cssRemove(it, query) }

fun xpathSelect(root: Element, queries: List<String>, context: String): Element {
    for (query in queries) {
        val first = selectXpathElements(root, query).firstOrNull()
        if (first != null) return first
    }
    throw RtError(context, "xpath matched no elements")
}

fun xpathSelectOpt(root: Element, queries: List<String>): Element? {
    for (query in queries) {
        val first = selectXpathElements(root, query).firstOrNull()
        if (first != null) return first
    }
    return null
}

fun xpathSelectAll(root: Element, queries: List<String>): List<Element> {
    for (query in queries) {
        val found = selectXpathElements(root, query)
        if (found.isNotEmpty()) return found
    }
    return emptyList()
}

fun xpathRemove(root: Element, query: String): Element {
    val copy = cloneForRemove(root)
    copy.selectXpath(query).forEach { if (it !== copy) it.remove() }
    return copy
}

fun xpathRemoveAll(roots: List<Element>, query: String): List<Element> =
    roots.map { xpathRemove(it, query) }

fun nodeAt(nodes: List<Element>, index: Long, context: String): Element =
    itemAt(nodes, index, context)

private fun collectNodeText(node: org.jsoup.nodes.Node, sb: StringBuilder) {
    when (node) {
        is org.jsoup.nodes.TextNode -> sb.append(node.wholeText)
        is org.jsoup.nodes.DataNode -> sb.append(node.wholeData)
        is org.jsoup.nodes.Element -> {
            for (child in node.childNodes()) {
                collectNodeText(child, sb)
            }
        }
    }
}

fun text(el: Element): String = StringBuilder().also { collectNodeText(el, it) }.toString()

fun textAll(els: List<Element>): List<String> = els.map { text(it) }

fun raw(el: Element, inner: Boolean): String =
    if (inner) el.html() else el.outerHtml()

fun rawAll(els: List<Element>, inner: Boolean): List<String> =
    els.map { raw(it, inner) }

fun attr(el: Element, names: List<String>, context: String): String {
    for (name in names) {
        if (el.hasAttr(name)) return el.attr(name)
    }
    throw RtError(context, "attribute not found")
}

fun attrList(els: List<Element>, names: List<String>, context: String): List<String> =
    els.map { attr(it, names, context) }

fun attrMulti(el: Element, names: List<String>): List<String> {
    val out = ArrayList<String>()
    for (name in names) {
        if (el.hasAttr(name)) {
            val v = el.attr(name)
            if (v.isNotEmpty()) out.add(v)
        }
    }
    return out
}

fun attrMulti(els: List<Element>, names: List<String>): List<String> {
    val out = ArrayList<String>()
    for (el in els) {
        for (name in names) {
            if (el.hasAttr(name)) {
                val v = el.attr(name)
                if (v.isNotEmpty()) out.add(v)
            }
        }
    }
    return out
}

fun nodeBool(el: Element?): Boolean = el != null

fun nodeBool(els: List<Element>): Boolean = els.isNotEmpty()

fun predCss(el: Element, query: String): Boolean = selectElements(el, query).isNotEmpty()

fun predXpath(el: Element, query: String): Boolean = selectXpathElements(el, query).isNotEmpty()

fun predHasAttr(el: Element, name: String): Boolean = el.hasAttr(name)

fun predAttr(el: Element, name: String): String = if (el.hasAttr(name)) el.attr(name) else ""

fun predAttrOpt(el: Element, name: String): String? = if (el.hasAttr(name)) el.attr(name) else null

fun predText(el: Element): String = text(el)"#;

/// HTTP & REST transport runtime helpers (`java.net.http.HttpClient`), emitted when `module_uses_http(module)` is `true`.
pub const KOTLIN_HTTP_RUNTIME: &str = r#"val defaultHttpClient: HttpClient by lazy { HttpClient.newHttpClient() }

class ReqOpts {
    val headers: MutableMap<String, MutableList<String>> = LinkedHashMap()
    val cookies: MutableMap<String, MutableList<String>> = LinkedHashMap()
    val params: MutableMap<String, MutableList<String>> = LinkedHashMap()
    var body: String? = null
    var timeoutMillis: Long = 0L

    fun addHeader(key: String, value: String) {
        headers.getOrPut(key) { ArrayList() }.add(value)
    }

    fun ensureHeader(key: String, value: String) {
        if (headers.keys.none { it.equals(key, ignoreCase = true) }) {
            addHeader(key, value)
        }
    }

    fun addCookie(name: String, value: String) {
        cookies.getOrPut(name) { ArrayList() }.add(value)
    }

    fun addParam(key: String, value: String) {
        params.getOrPut(key) { ArrayList() }.add(value)
    }
}

fun interface HttpOption {
    fun apply(opts: ReqOpts)
}

fun withHeader(key: String, value: String): HttpOption = HttpOption { opts ->
    opts.addHeader(key, value)
}

fun withHeaders(headers: Map<String, String>): HttpOption = HttpOption { opts ->
    for ((k, v) in headers) {
        opts.addHeader(k, v)
    }
}

fun withParam(key: String, value: String): HttpOption = HttpOption { opts ->
    opts.addParam(key, value)
}

fun withParams(params: Map<String, String>): HttpOption = HttpOption { opts ->
    for ((k, v) in params) {
        opts.addParam(k, v)
    }
}

fun withCookie(name: String, value: String): HttpOption = HttpOption { opts ->
    opts.addCookie(name, value)
}

fun withCookies(cookies: Map<String, String>): HttpOption = HttpOption { opts ->
    for ((k, v) in cookies) {
        opts.addCookie(k, v)
    }
}

fun withTimeout(millis: Long): HttpOption = HttpOption { opts ->
    opts.timeoutMillis = millis
}

fun withTimeout(duration: Duration): HttpOption = HttpOption { opts ->
    opts.timeoutMillis = duration.toMillis()
}

fun encodeForm(pairs: List<Pair<String, String>>): String =
    pairs.joinToString("&") { (k, v) ->
        "${URLEncoder.encode(k, StandardCharsets.UTF_8)}=${URLEncoder.encode(v, StandardCharsets.UTF_8)}"
    }

private fun resolveOpts(dslOpts: ReqOpts, opts: Array<out HttpOption>): ReqOpts {
    val resolved = ReqOpts()
    resolved.body = dslOpts.body
    resolved.timeoutMillis = dslOpts.timeoutMillis
    for ((k, vs) in dslOpts.headers) {
        resolved.headers[k] = ArrayList(vs)
    }
    for ((k, vs) in dslOpts.cookies) {
        resolved.cookies[k] = ArrayList(vs)
    }
    for ((k, vs) in dslOpts.params) {
        resolved.params[k] = ArrayList(vs)
    }
    for (opt in opts) {
        opt.apply(resolved)
    }
    return resolved
}

private fun buildUrl(reqUrl: String, params: Map<String, List<String>>): String {
    if (params.isEmpty()) return reqUrl
    val pairs = ArrayList<Pair<String, String>>()
    for ((k, vs) in params) {
        for (v in vs) {
            pairs.add(k to v)
        }
    }
    val encoded = encodeForm(pairs)
    if (encoded.isEmpty()) return reqUrl
    val sep = if (reqUrl.contains('?')) "&" else "?"
    return "$reqUrl$sep$encoded"
}

private fun isRestrictedHeader(name: String): Boolean =
    name.equals("Host", ignoreCase = true) ||
        name.equals("Content-Length", ignoreCase = true) ||
        name.equals("Connection", ignoreCase = true) ||
        name.equals("Expect", ignoreCase = true) ||
        name.equals("Upgrade", ignoreCase = true)

fun doRequest(
    client: HttpClient,
    method: String,
    reqUrl: String,
    dslOpts: ReqOpts,
    opts: Array<out HttpOption>,
): Pair<Int, String> {
    val resolved = resolveOpts(dslOpts, opts)
    val finalUrl = buildUrl(reqUrl, resolved.params)
    val builder = HttpRequest.newBuilder(URI.create(finalUrl))
    if (resolved.timeoutMillis > 0L) {
        builder.timeout(Duration.ofMillis(resolved.timeoutMillis))
    }
    for ((k, vs) in resolved.headers) {
        if (isRestrictedHeader(k)) continue
        for (v in vs) {
            builder.header(k, v)
        }
    }
    if (resolved.cookies.isNotEmpty()) {
        val cookieParts = ArrayList<String>()
        for ((name, vs) in resolved.cookies) {
            for (v in vs) {
                cookieParts.add("$name=$v")
            }
        }
        if (cookieParts.isNotEmpty()) {
            builder.header("Cookie", cookieParts.joinToString("; "))
        }
    }
    val bodyPublisher = resolved.body?.let { HttpRequest.BodyPublishers.ofString(it, StandardCharsets.UTF_8) }
        ?: HttpRequest.BodyPublishers.noBody()
    builder.method(method.uppercase(), bodyPublisher)
    val resp = client.send(builder.build(), HttpResponse.BodyHandlers.ofString(StandardCharsets.UTF_8))
    return Pair(resp.statusCode(), resp.body() ?: "")
}

sealed interface ErrorCond {
    data class Null(val path: String) : ErrorCond
    data class Bool(val path: String, val expected: Boolean) : ErrorCond
    data class IntVal(val path: String, val expected: Long) : ErrorCond
    data class FloatVal(val path: String, val expected: Double) : ErrorCond
    data class Str(val path: String, val expected: String) : ErrorCond
}

class ErrorMatcher<E>(
    val status: Int,
    val requiredKeys: List<String>,
    val conditions: List<ErrorCond>,
    val decode: (Int, JsonElement) -> E,
)

fun matchErrorJson(
    body: JsonElement,
    requiredKeys: List<String>,
    conditions: List<ErrorCond>,
): Boolean {
    if (body is JsonNull) return false
    for (key in requiredKeys) {
        val v = jsonOpt(body, key)
        if (v == null || v is JsonNull) return false
    }
    for (cond in conditions) {
        val matched = when (cond) {
            is ErrorCond.Null -> {
                val v = jsonOpt(body, cond.path)
                v == null || v is JsonNull
            }
            is ErrorCond.Bool -> {
                val prim = jsonOpt(body, cond.path) as? JsonPrimitive
                prim != null && !prim.isString && prim.content == cond.expected.toString()
            }
            is ErrorCond.IntVal -> {
                val prim = jsonOpt(body, cond.path) as? JsonPrimitive
                prim != null && !prim.isString && prim.content.toLongOrNull() == cond.expected
            }
            is ErrorCond.FloatVal -> {
                val prim = jsonOpt(body, cond.path) as? JsonPrimitive
                prim != null && !prim.isString && prim.content.toDoubleOrNull() == cond.expected
            }
            is ErrorCond.Str -> {
                val prim = jsonOpt(body, cond.path) as? JsonPrimitive
                prim != null && prim.isString && prim.content == cond.expected
            }
        }
        if (!matched) return false
    }
    return true
}

fun fetchHtml(
    client: HttpClient,
    method: String,
    reqUrl: String,
    dslOpts: ReqOpts,
    opts: Array<out HttpOption>,
    context: String,
    responsePath: String? = null,
    responseJoin: String? = null,
): String {
    val (status, body) = try {
        doRequest(client, method, reqUrl, dslOpts, opts)
    } catch (e: Throwable) {
        throw RtError(context, e.message ?: e.toString())
    }
    if (status >= 400) {
        throw RtError(context, "HTTP $status: $body")
    }
    if (!responsePath.isNullOrEmpty()) {
        val bodyVal = parseJson(body, responsePath, context)
        if (bodyVal is JsonArray && responseJoin != null) {
            return bodyVal.joinToString(responseJoin) { item ->
                if (item is JsonPrimitive && item.isString) item.content else item.toString()
            }
        }
        if (bodyVal is JsonPrimitive && bodyVal.isString) {
            return bodyVal.content
        }
        return bodyVal.toString()
    }
    return body
}

fun <R> restCall(
    client: HttpClient,
    method: String,
    reqUrl: String,
    dslOpts: ReqOpts,
    opts: Array<out HttpOption>,
    responsePath: String?,
    context: String,
    matchers: List<ErrorMatcher<R>>,
    decodeOk: (JsonElement) -> R,
    transportErr: (Throwable) -> R,
    decodeErr: (Int, String, String) -> R,
    unknownErr: (Int, JsonElement?) -> R,
): R {
    val (status, text) = try {
        doRequest(client, method, reqUrl, dslOpts, opts)
    } catch (e: Throwable) {
        return transportErr(e)
    }
    val parsedRes = runCatching { Json.parseToJsonElement(text) }
    var lastDecodeErr: RtError? = null

    for (m in matchers) {
        if (m.status != status) continue
        val jsonVal = parsedRes.getOrNull()
        if (jsonVal != null) {
            if (matchErrorJson(jsonVal, m.requiredKeys, m.conditions)) {
                try {
                    return m.decode(status, jsonVal)
                } catch (e: RtError) {
                    lastDecodeErr = e
                }
            }
        } else if (m.requiredKeys.isEmpty() && m.conditions.isEmpty()) {
            try {
                return m.decode(status, JsonPrimitive(text))
            } catch (_: RtError) {
            }
        }
    }

    if (status in 200..299) {
        val parsed = parsedRes.getOrElse { e ->
            return decodeErr(status, text, e.message ?: "invalid JSON")
        }
        val target = try {
            extractJsonValue(parsed, responsePath, context)
        } catch (e: RtError) {
            return decodeErr(status, text, e.message ?: e.msg)
        }
        return try {
            decodeOk(target)
        } catch (e: RtError) {
            decodeErr(status, text, e.message ?: e.msg)
        }
    }

    if (lastDecodeErr != null) {
        return decodeErr(status, text, lastDecodeErr.message ?: lastDecodeErr.msg)
    }

    val unknownVal = parsedRes.getOrElse { if (text.isEmpty()) null else JsonPrimitive(text) }
    return unknownErr(status, unknownVal)
}

fun <R> restCallVoid(
    client: HttpClient,
    method: String,
    reqUrl: String,
    dslOpts: ReqOpts,
    opts: Array<out HttpOption>,
    matchers: List<ErrorMatcher<R>>,
    okResult: () -> R,
    transportErr: (Throwable) -> R,
    decodeErr: (Int, String, String) -> R,
    unknownErr: (Int, JsonElement?) -> R,
): R {
    val (status, text) = try {
        doRequest(client, method, reqUrl, dslOpts, opts)
    } catch (e: Throwable) {
        return transportErr(e)
    }
    val parsedRes = runCatching { Json.parseToJsonElement(text) }
    var lastDecodeErr: RtError? = null

    for (m in matchers) {
        if (m.status != status) continue
        val jsonVal = parsedRes.getOrNull()
        if (jsonVal != null) {
            if (matchErrorJson(jsonVal, m.requiredKeys, m.conditions)) {
                try {
                    return m.decode(status, jsonVal)
                } catch (e: RtError) {
                    lastDecodeErr = e
                }
            }
        } else if (m.requiredKeys.isEmpty() && m.conditions.isEmpty()) {
            try {
                return m.decode(status, JsonPrimitive(text))
            } catch (_: RtError) {
            }
        }
    }

    if (status in 200..299) {
        return okResult()
    }

    if (lastDecodeErr != null) {
        return decodeErr(status, text, lastDecodeErr.message ?: lastDecodeErr.msg)
    }

    val unknownVal = parsedRes.getOrElse { if (text.isEmpty()) null else JsonPrimitive(text) }
    return unknownErr(status, unknownVal)
}"#;

/// Writes the body of `object Rt` to `writer`.
pub fn write_kotlin_runtime_body(
    module: &Module,
    builder: &ModuleBuilder,
    writer: &mut CodeWriter,
) {
    let include_html = module_has_html(module);
    let include_http = module_uses_http(module);

    writer.write_block(KOTLIN_CORE_RUNTIME.trim_end());

    if include_html {
        writer.blankline();
        writer.write_block(KOTLIN_HTML_RUNTIME.trim_end());
    }

    if include_http {
        writer.blankline();
        writer.write_block(KOTLIN_HTTP_RUNTIME.trim_end());
    }

    for def in builder.runtime_defs().values() {
        writer.blankline();
        writer.write_block(def.code.trim_end());
    }
}

/// Collects the required imports for the runtime module (`<runtime_name>.kt`).
pub fn collect_kotlin_runtime_imports(
    module: &Module,
    builder: &ModuleBuilder,
) -> IndexSet<String> {
    let mut imports = IndexSet::new();
    imports.insert("import kotlinx.serialization.json.Json".to_owned());
    imports.insert("import kotlinx.serialization.json.JsonArray".to_owned());
    imports.insert("import kotlinx.serialization.json.JsonElement".to_owned());
    imports.insert("import kotlinx.serialization.json.JsonNull".to_owned());
    imports.insert("import kotlinx.serialization.json.JsonObject".to_owned());
    imports.insert("import kotlinx.serialization.json.JsonPrimitive".to_owned());

    if module_has_html(module) {
        imports.insert("import org.jsoup.Jsoup".to_owned());
        imports.insert("import org.jsoup.nodes.Document".to_owned());
        imports.insert("import org.jsoup.nodes.Element".to_owned());
    }

    if module_uses_http(module) {
        imports.insert("import java.net.URI".to_owned());
        imports.insert("import java.net.URLEncoder".to_owned());
        imports.insert("import java.net.http.HttpClient".to_owned());
        imports.insert("import java.net.http.HttpRequest".to_owned());
        imports.insert("import java.net.http.HttpResponse".to_owned());
        imports.insert("import java.nio.charset.StandardCharsets".to_owned());
        imports.insert("import java.time.Duration".to_owned());
    }

    for imp in builder.runtime_imports() {
        let trimmed = imp.trim();
        if trimmed.starts_with("import ") {
            imports.insert(trimmed.trim_end_matches(';').to_owned());
        } else if !trimmed.is_empty() {
            imports.insert(format!("import {}", trimmed.trim_end_matches(';')));
        }
    }

    imports
}

/// Generates the standalone Kotlin shared runtime file (`<runtime_name>.kt`) for `-R / --separate-runtime`.
pub fn emit_kotlin_runtime_module(
    module: &Module,
    builder: &ModuleBuilder,
    options: &CodegenOptions,
) -> String {
    let mut writer = CodeWriter::with_indent("    ");
    writer.writeln("// Auto-generated runtime helpers by parsmith. DO NOT EDIT.");
    writer.writeln("@file:Suppress(\"unused\", \"RedundantVisibilityModifier\", \"MemberVisibilityCanBePrivate\", \"FunctionName\", \"PropertyName\", \"ClassName\", \"LocalVariableName\", \"CanBeParameter\")");
    writer.blankline();

    if let Some(pkg_line) = render_kotlin_package(&options.package) {
        writer.writeln(pkg_line);
        writer.blankline();
    }

    let imports = collect_kotlin_runtime_imports(module, builder);
    for imp in &imports {
        writer.write_block(imp);
    }
    writer.blankline();

    writer.writeln("object Rt {");
    writer.indented(|writer| {
        write_kotlin_runtime_body(module, builder, writer);
    });
    writer.writeln("}");
    writer.finish()
}
