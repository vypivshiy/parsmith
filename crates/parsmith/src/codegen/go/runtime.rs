//! Go runtime helper definitions (`BASE_RUNTIME`, `GO_RUNTIME`, `BASE_REST_RUNTIME`, `NETHTTP_REST_RUNTIME`).
//!
//! All Go source snippets use literal tab (`\t`) indentation matching `gofmt`.

use std::fmt::Write as _;

use indexmap::IndexMap;

use crate::validators::normalize_regex_pattern;

/// Base panic-recovery helper (`rtFallback`) used by pipelines with a terminal `fallback`.
pub const BASE_RUNTIME: &str = "\
// rtFallback runs fn with panic recovery, returning fallback on panic.
func rtFallback[T any](fn func() T, fallback T) (result T) {
\tdefer func() {
\t\tif r := recover(); r != nil {
\t\t\tresult = fallback
\t\t}
\t}()
\treturn fn()
}";

/// Standard library and third-party imports required by [`BASE_REST_RUNTIME`] and [`NETHTTP_REST_RUNTIME`].
pub const REST_IMPORTS: &[&str] = &[
    "\"context\"",
    "\"encoding/json\"",
    "\"fmt\"",
    "\"github.com/tidwall/gjson\"",
    "\"io\"",
    "\"net/http\"",
    "\"net/url\"",
    "\"strings\"",
    "\"time\"",
];

/// HTTP option types, functional option constructors, error dispatcher, and REST error types
/// (`DecodeErr`, `UnknownErr`, `TransportErr`).
pub const BASE_REST_RUNTIME: &str = include_str!("runtime/rest_base.go");

/// `net/http` execution helpers (`rtResolveOpts`, `rtBuildURL`, `rtDoRequest`, `rtDo`,
/// `rtExtractPath`, `rtExtractJSONSlice`, `rtRestCall`, `rtRestCallValue`, `rtRestCallSlice`,
/// `rtRestCallVoid`, `rtFetchHTML`).
pub const NETHTTP_REST_RUNTIME: &str = include_str!("runtime/nethttp_rest.go");

/// Looks up a selective `rt*` runtime helper definition by `name`, returning `(imports, source_code)`.
pub fn get_go_runtime_helper(name: &str) -> Option<(&'static [&'static str], &'static str)> {
    match name {
        "rtFallback" => Some((&[], BASE_RUNTIME)),
        "rtJSONBody" => Some((
            &["\"encoding/json\""],
            "\
func rtJSONBody(value any) string {
\tbody, err := json.Marshal(value)
\tif err != nil {
\t\tpanic(\"parsmith: encode JSON body: \" + err.Error())
\t}
\treturn string(body)
}",
        )),
        "rtAssert" => Some((
            &[],
            "\
func rtAssert(cond bool, msg string) {
\tif !cond {
\t\tpanic(\"parsmith: \" + msg)
\t}
}",
        )),
        "rtCssSelect" => Some((
            &["\"github.com/PuerkitoBio/goquery\""],
            "\
func rtCssSelect(sel *goquery.Selection, queries ...string) *goquery.Selection {
\tfor _, q := range queries {
\t\tres := sel.Find(q).First()
\t\tif res.Length() > 0 {
\t\t\treturn res
\t\t}
\t}
\tpanic(\"parsmith: css selector matched no elements\")
}",
        )),
        "rtAttr" => Some((
            &["\"github.com/PuerkitoBio/goquery\""],
            "\
func rtAttr(sel *goquery.Selection, key string) string {
\tif sel.Length() == 0 {
\t\tpanic(\"parsmith: empty selection for attr \" + key)
\t}
\tval, ok := sel.Attr(key)
\tif !ok {
\t\tpanic(\"parsmith: missing attribute \" + key)
\t}
\treturn val
}",
        )),
        "rtAttrList" => Some((
            &["\"github.com/PuerkitoBio/goquery\""],
            "\
func rtAttrList(sel *goquery.Selection, key string) []string {
\tout := make([]string, 0, sel.Length())
\tsel.Each(func(_ int, s *goquery.Selection) {
\t\tval, ok := s.Attr(key)
\t\tif !ok {
\t\t\tpanic(\"parsmith: missing attribute \" + key)
\t\t}
\t\tout = append(out, val)
\t})
\treturn out
}",
        )),
        "rtAttrMulti" => Some((
            &["\"github.com/PuerkitoBio/goquery\""],
            "\
func rtAttrMulti(sel *goquery.Selection, keys ...string) []string {
\tout := make([]string, 0)
\tsel.Each(func(_ int, s *goquery.Selection) {
\t\tfor _, k := range keys {
\t\t\tif val, ok := s.Attr(k); ok && val != \"\" {
\t\t\t\tout = append(out, val)
\t\t\t}
\t\t}
\t})
\treturn out
}",
        )),
        "rtHasAttr" => Some((
            &["\"github.com/PuerkitoBio/goquery\""],
            "\
func rtHasAttr(s *goquery.Selection, key string) bool {
\t_, ok := s.Attr(key)
\treturn ok
}",
        )),
        "rtAttrOr" => Some((
            &["\"github.com/PuerkitoBio/goquery\""],
            "\
func rtAttrOr(s *goquery.Selection, key string) string {
\tv, _ := s.Attr(key)
\treturn v
}",
        )),
        "rtTrim" => Some((
            &["\"strings\""],
            "\
func rtTrim(s, cutset string) string {
\tif cutset == \"\" {
\t\treturn strings.TrimSpace(s)
\t}
\treturn strings.Trim(s, cutset)
}",
        )),
        "rtTrimArr" => Some((
            &[],
            "\
func rtTrimArr(arr []string, cutset string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtTrim(s, cutset)
\t}
\treturn r
}",
        )),
        "rtLTrim" => Some((
            &["\"strings\""],
            "\
func rtLTrim(s, cutset string) string {
\tif cutset == \"\" {
\t\treturn strings.TrimLeft(s, \" \\t\\n\\r\\v\\f\")
\t}
\treturn strings.TrimLeft(s, cutset)
}",
        )),
        "rtLTrimArr" => Some((
            &[],
            "\
func rtLTrimArr(arr []string, cutset string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtLTrim(s, cutset)
\t}
\treturn r
}",
        )),
        "rtRTrim" => Some((
            &["\"strings\""],
            "\
func rtRTrim(s, cutset string) string {
\tif cutset == \"\" {
\t\treturn strings.TrimRight(s, \" \\t\\n\\r\\v\\f\")
\t}
\treturn strings.TrimRight(s, cutset)
}",
        )),
        "rtRTrimArr" => Some((
            &[],
            "\
func rtRTrimArr(arr []string, cutset string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtRTrim(s, cutset)
\t}
\treturn r
}",
        )),
        "rtRmPrefix" => Some((
            &["\"strings\""],
            "\
func rtRmPrefix(s, prefix string) string {
\treturn strings.TrimPrefix(s, prefix)
}",
        )),
        "rtRmPrefixArr" => Some((
            &[],
            "\
func rtRmPrefixArr(arr []string, prefix string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtRmPrefix(s, prefix)
\t}
\treturn r
}",
        )),
        "rtRmSuffix" => Some((
            &["\"strings\""],
            "\
func rtRmSuffix(s, suffix string) string {
\treturn strings.TrimSuffix(s, suffix)
}",
        )),
        "rtRmSuffixArr" => Some((
            &[],
            "\
func rtRmSuffixArr(arr []string, suffix string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtRmSuffix(s, suffix)
\t}
\treturn r
}",
        )),
        "rtRmPrefixSuffix" => Some((
            &["\"strings\""],
            "\
func rtRmPrefixSuffix(s, sub string) string {
\treturn strings.TrimSuffix(strings.TrimPrefix(s, sub), sub)
}",
        )),
        "rtRmPrefixSuffixArr" => Some((
            &[],
            "\
func rtRmPrefixSuffixArr(arr []string, sub string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtRmPrefixSuffix(s, sub)
\t}
\treturn r
}",
        )),
        "rtFmt" => Some((
            &["\"strings\""],
            "\
func rtFmt(template, value string) string {
\treturn strings.ReplaceAll(template, \"{{}}\", value)
}",
        )),
        "rtFmtArr" => Some((
            &[],
            "\
func rtFmtArr(arr []string, template string) []string {
\tr := make([]string, len(arr))
\tfor i, v := range arr {
\t\tr[i] = rtFmt(template, v)
\t}
\treturn r
}",
        )),
        "rtRepl" => Some((
            &["\"strings\""],
            "\
func rtRepl(s, old, new string) string {
\treturn strings.ReplaceAll(s, old, new)
}",
        )),
        "rtReplArr" => Some((
            &[],
            "\
func rtReplArr(arr []string, old, new string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtRepl(s, old, new)
\t}
\treturn r
}",
        )),
        "rtReplMap" => Some((
            &["\"strings\""],
            "\
func rtReplMap(s string, m map[string]string) string {
\tfor k, v := range m {
\t\ts = strings.ReplaceAll(s, k, v)
\t}
\treturn s
}",
        )),
        "rtReplMapArr" => Some((
            &[],
            "\
func rtReplMapArr(arr []string, m map[string]string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtReplMap(s, m)
\t}
\treturn r
}",
        )),
        "rtLower" => Some((
            &["\"strings\""],
            "\
func rtLower(s string) string {
\treturn strings.ToLower(s)
}",
        )),
        "rtLowerArr" => Some((
            &[],
            "\
func rtLowerArr(arr []string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtLower(s)
\t}
\treturn r
}",
        )),
        "rtUpper" => Some((
            &["\"strings\""],
            "\
func rtUpper(s string) string {
\treturn strings.ToUpper(s)
}",
        )),
        "rtUpperArr" => Some((
            &[],
            "\
func rtUpperArr(arr []string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtUpper(s)
\t}
\treturn r
}",
        )),
        "rtNormalizeSpace" => Some((
            &["\"strings\""],
            "\
func rtNormalizeSpace(s string) string {
\treturn strings.Join(strings.Fields(s), \" \")
}",
        )),
        "rtNormalizeSpaceArr" => Some((
            &[],
            "\
func rtNormalizeSpaceArr(arr []string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtNormalizeSpace(s)
\t}
\treturn r
}",
        )),
        "rtUnescape" => Some((
            &["\"html\"", "\"regexp\"", "\"strconv\"", "\"strings\""],
            "\
var (
\trtUnicodeEscRe = regexp.MustCompile(`\\\\u([0-9a-fA-F]{4})`)
\trtBytesEscRe   = regexp.MustCompile(`\\\\x([0-9a-fA-F]{2})`)
\trtCharEscRepl  = strings.NewReplacer(
\t\t`\\b`, \"\\b\",
\t\t`\\f`, \"\\f\",
\t\t`\\n`, \"\\n\",
\t\t`\\r`, \"\\r\",
\t\t`\\t`, \"\\t\",
\t)
)

func rtUnescape(s string) string {
\tout := html.UnescapeString(s)
\tout = rtUnicodeEscRe.ReplaceAllStringFunc(out, func(m string) string {
\t\tv, err := strconv.ParseInt(m[2:], 16, 32)
\t\tif err != nil {
\t\t\treturn m
\t\t}
\t\treturn string(rune(v))
\t})
\tout = rtBytesEscRe.ReplaceAllStringFunc(out, func(m string) string {
\t\tv, err := strconv.ParseInt(m[2:], 16, 32)
\t\tif err != nil {
\t\t\treturn m
\t\t}
\t\treturn string(rune(v))
\t})
\treturn rtCharEscRepl.Replace(out)
}",
        )),
        "rtUnescapeArr" => Some((
            &[],
            "\
func rtUnescapeArr(arr []string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtUnescape(s)
\t}
\treturn r
}",
        )),
        "rtReSearch" => Some((
            &["\"regexp\""],
            "\
func rtReSearch(pattern, value, msg string) string {
\tre := regexp.MustCompile(pattern)
\tm := re.FindStringSubmatch(value)
\tif len(m) < 2 {
\t\tpanic(\"parsmith: \" + msg)
\t}
\treturn m[1]
}",
        )),
        "rtReSearchArr" => Some((
            &[],
            "\
func rtReSearchArr(arr []string, pattern, msg string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtReSearch(pattern, s, msg)
\t}
\treturn r
}",
        )),
        "rtReAll" => Some((
            &["\"regexp\""],
            "\
func rtReAll(pattern, value string) []string {
\tre := regexp.MustCompile(pattern)
\tmatches := re.FindAllStringSubmatch(value, -1)
\tout := make([]string, len(matches))
\tfor i, m := range matches {
\t\tif len(m) > 1 {
\t\t\tout[i] = m[1]
\t\t} else {
\t\t\tout[i] = m[0]
\t\t}
\t}
\treturn out
}",
        )),
        "rtReSub" => Some((
            &["\"regexp\""],
            "\
var rtBackrefRe = regexp.MustCompile(`\\\\([1-9])`)

func rtReSub(pattern, repl, s string) string {
\tre := regexp.MustCompile(pattern)
\tgoRepl := rtBackrefRe.ReplaceAllString(repl, \"$${$1}\")
\treturn re.ReplaceAllString(s, goRepl)
}",
        )),
        "rtReSubArr" => Some((
            &[],
            "\
func rtReSubArr(arr []string, pattern, repl string) []string {
\tr := make([]string, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtReSub(pattern, repl, s)
\t}
\treturn r
}",
        )),
        "rtReSplit" => Some((
            &["\"regexp\""],
            "\
func rtReSplit(pattern, value string) []string {
\treturn regexp.MustCompile(pattern).Split(value, -1)
}",
        )),
        "rtReMatch" => Some((
            &["\"regexp\""],
            "\
func rtReMatch(pattern, value string) bool {
\treturn regexp.MustCompile(pattern).MatchString(value)
}",
        )),
        "rtReAllMatch" => Some((
            &["\"regexp\""],
            "\
func rtReAllMatch(pattern string, arr []string) bool {
\tre := regexp.MustCompile(pattern)
\tfor _, s := range arr {
\t\tif !re.MatchString(s) {
\t\t\treturn false
\t\t}
\t}
\treturn true
}",
        )),
        "rtReAnyMatch" => Some((
            &["\"regexp\""],
            "\
func rtReAnyMatch(pattern string, arr []string) bool {
\tre := regexp.MustCompile(pattern)
\tfor _, s := range arr {
\t\tif re.MatchString(s) {
\t\t\treturn true
\t\t}
\t}
\treturn false
}",
        )),
        "rtStrIndex" => Some((
            &[],
            "\
func rtStrIndex(s string, i int) string {
\tr := []rune(s)
\tif i < 0 {
\t\ti += len(r)
\t}
\tif i < 0 || i >= len(r) {
\t\tpanic(\"parsmith: string index out of range\")
\t}
\treturn string(r[i])
}",
        )),
        "rtStrSlice" => Some((
            &[],
            "\
func rtStrSlice(s string, start, end int) string {
\tr := []rune(s)
\tn := len(r)
\tif start < 0 {
\t\tstart += n
\t}
\tif end < 0 {
\t\tend += n
\t}
\tif start < 0 {
\t\tstart = 0
\t} else if start > n {
\t\tstart = n
\t}
\tif end < 0 {
\t\tend = 0
\t} else if end > n {
\t\tend = n
\t}
\tif start >= end {
\t\treturn \"\"
\t}
\treturn string(r[start:end])
}",
        )),
        "rtIndex" => Some((
            &[],
            "\
func rtIndex[T any](arr []T, i int) T {
\tif i < 0 {
\t\ti += len(arr)
\t}
\tif i < 0 || i >= len(arr) {
\t\tpanic(\"parsmith: slice index out of range\")
\t}
\treturn arr[i]
}",
        )),
        "rtSlice" => Some((
            &[],
            "\
func rtSlice[T any](arr []T, start, end int) []T {
\tn := len(arr)
\tif start < 0 {
\t\tstart += n
\t}
\tif end < 0 {
\t\tend += n
\t}
\tif start < 0 {
\t\tstart = 0
\t} else if start > n {
\t\tstart = n
\t}
\tif end < 0 {
\t\tend = 0
\t} else if end > n {
\t\tend = n
\t}
\tif start >= end {
\t\treturn []T{}
\t}
\tout := make([]T, end-start)
\tcopy(out, arr[start:end])
\treturn out
}",
        )),
        "rtNodeIndex" => Some((
            &["\"github.com/PuerkitoBio/goquery\""],
            "\
func rtNodeIndex(sel *goquery.Selection, i int) *goquery.Selection {
\tn := sel.Length()
\tif i < 0 {
\t\ti += n
\t}
\tif i < 0 || i >= n {
\t\tpanic(\"parsmith: selection index out of range\")
\t}
\treturn sel.Eq(i)
}",
        )),
        "rtNodeSlice" => Some((
            &["\"github.com/PuerkitoBio/goquery\""],
            "\
func rtNodeSlice(sel *goquery.Selection, start, end int) *goquery.Selection {
\tn := sel.Length()
\tif start < 0 {
\t\tstart += n
\t}
\tif end < 0 {
\t\tend += n
\t}
\tif start < 0 {
\t\tstart = 0
\t} else if start > n {
\t\tstart = n
\t}
\tif end < 0 {
\t\tend = 0
\t} else if end > n {
\t\tend = n
\t}
\tif start >= end {
\t\treturn sel.Slice(0, 0)
\t}
\treturn sel.Slice(start, end)
}",
        )),
        "rtLen" => Some((
            &["\"github.com/PuerkitoBio/goquery\"", "\"unicode/utf8\""],
            "\
func rtLen(v any) int64 {
\tswitch t := v.(type) {
\tcase *goquery.Selection:
\t\treturn int64(t.Length())
\tcase string:
\t\treturn int64(utf8.RuneCountInString(t))
\tcase []string:
\t\treturn int64(len(t))
\tcase []int64:
\t\treturn int64(len(t))
\tcase []float64:
\t\treturn int64(len(t))
\tdefault:
\t\treturn 0
\t}
}",
        )),
        "rtUnique" => Some((
            &[],
            "\
func rtUnique[T comparable](arr []T) []T {
\tseen := make(map[T]bool)
\tr := make([]T, 0, len(arr))
\tfor _, v := range arr {
\t\tif !seen[v] {
\t\t\tseen[v] = true
\t\t\tr = append(r, v)
\t\t}
\t}
\treturn r
}",
        )),
        "rtToInt" => Some((
            &["\"strconv\"", "\"strings\""],
            "\
func rtToInt(s string) int64 {
\tn, err := strconv.ParseInt(strings.TrimSpace(s), 10, 64)
\tif err != nil {
\t\tpanic(\"parsmith: failed to parse int: \" + s)
\t}
\treturn n
}",
        )),
        "rtToIntArr" => Some((
            &[],
            "\
func rtToIntArr(arr []string) []int64 {
\tr := make([]int64, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtToInt(s)
\t}
\treturn r
}",
        )),
        "rtToFloat" => Some((
            &["\"strconv\"", "\"strings\""],
            "\
func rtToFloat(s string) float64 {
\tf, err := strconv.ParseFloat(strings.TrimSpace(s), 64)
\tif err != nil {
\t\tpanic(\"parsmith: failed to parse float: \" + s)
\t}
\treturn f
}",
        )),
        "rtToFloatArr" => Some((
            &[],
            "\
func rtToFloatArr(arr []string) []float64 {
\tr := make([]float64, len(arr))
\tfor i, s := range arr {
\t\tr[i] = rtToFloat(s)
\t}
\treturn r
}",
        )),
        "rtToBool" => Some((
            &[],
            "\
func rtToBool(v any) bool {
\tif v == nil {
\t\treturn false
\t}
\tswitch t := v.(type) {
\tcase bool:
\t\treturn t
\tcase string:
\t\treturn t != \"\"
\tcase int:
\t\treturn t != 0
\tcase int64:
\t\treturn t != 0
\tcase float64:
\t\treturn t != 0
\tcase []string:
\t\treturn len(t) > 0
\tcase []int64:
\t\treturn len(t) > 0
\tcase []float64:
\t\treturn len(t) > 0
\tdefault:
\t\treturn true
\t}
}",
        )),
        "rtJsonify" => Some((
            &["\"encoding/json\"", "\"github.com/tidwall/gjson\""],
            "\
func rtJsonify[T any](jsonStr, path string, target *T) {
\tvar raw string
\tif path != \"\" {
\t\tres := gjson.Get(jsonStr, path)
\t\tif !res.Exists() {
\t\t\tpanic(\"parsmith: jsonify path not found: \" + path)
\t\t}
\t\traw = res.Raw
\t} else {
\t\traw = jsonStr
\t}
\tif err := json.Unmarshal([]byte(raw), target); err != nil {
\t\tpanic(\"parsmith: json unmarshal failed: \" + err.Error())
\t}
}",
        )),
        _ => None,
    }
}

/// Formats a Rust `&str` as a valid Go double-quoted string literal (`"..."`).
pub fn go_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || (c as u32) == 0x7f => {
                let _ = write!(out, "\\x{:02x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Formats a slice of strings as a Go `[]string{...}` composite literal.
pub fn go_str_array(items: &[String]) -> String {
    let inner = items
        .iter()
        .map(|s| go_str(s))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[]string{{{inner}}}")
}

/// Formats an [`IndexMap<String, String>`] as a Go `map[string]string{...}` composite literal.
pub fn go_str_map(map: &IndexMap<String, String>) -> String {
    if map.is_empty() {
        return "map[string]string{}".to_owned();
    }
    let inner = map
        .iter()
        .map(|(k, v)| format!("{}: {}", go_str(k), go_str(v)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("map[string]string{{{inner}}}")
}

/// Renders a portable DSL regex pattern as a Go raw string literal (`` `...` ``),
/// falling back to a double-quoted string if the pattern contains a backtick or carriage return.
pub fn go_regex_raw(raw_pattern: &str) -> String {
    let pattern = normalize_regex_pattern(raw_pattern);
    if !pattern.contains('`') && !pattern.contains('\r') {
        format!("`{pattern}`")
    } else {
        go_str(&pattern)
    }
}
