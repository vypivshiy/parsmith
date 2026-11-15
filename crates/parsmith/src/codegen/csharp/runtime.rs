//! Modular C# (.NET 8+) runtime (`Rt`) for `CSharpEmitter`.
//!
//! Splits runtime definitions into:
//! 1. Core / string / Unicode Rune / regex / collection / JSON projection helpers (always emitted).
//! 2. HTML DOM helpers (`AngleSharp`, emitted when `module_has_html(module)` is `true`).
//! 3. XPath 1.0 helpers (`AngleSharp.XPath`, emitted only when `module_has_xpath(module)` is `true`).
//! 4. HTTP / REST helpers (`HttpClient`, emitted when `module_uses_http(module)` is `true`).

use indexmap::IndexSet;

use crate::ast::{Cond, CondOp, Module, Pipeline, StepOp};
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::csharp::validate_csharp_namespace;
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, module_has_html, module_uses_http};

/// Checks whether a [`Cond`] tree contains at least one `CondOp::Xpath` predicate.
pub fn cond_has_xpath(cond: &Cond) -> bool {
    match cond {
        Cond::Atom(CondOp::Xpath { .. }) => true,
        Cond::Atom(_) => false,
        Cond::Not(inner) => cond_has_xpath(inner),
        Cond::And(items) | Cond::Or(items) => items.iter().any(cond_has_xpath),
    }
}

/// Checks whether a [`Pipeline`] contains at least one XPath step or predicate.
pub fn pipeline_has_xpath(pipeline: &Pipeline) -> bool {
    pipeline.steps.iter().any(|step| match &step.op {
        StepOp::Xpath { .. } | StepOp::XpathAll { .. } | StepOp::XpathRemove { .. } => true,
        StepOp::Filter { cond } | StepOp::Assert { cond, .. } | StepOp::Match { cond } => {
            cond_has_xpath(cond)
        }
        _ => false,
    })
}

/// Checks whether a compiled [`Module`] uses any XPath operations (`StepOp::Xpath*` or `CondOp::Xpath`).
pub fn module_has_xpath(module: &Module) -> bool {
    for s in &module.structs {
        if s.init_fields
            .iter()
            .any(|f| pipeline_has_xpath(&f.pipeline))
            || s.pre_validate.as_ref().is_some_and(pipeline_has_xpath)
            || s.checks.iter().any(|c| pipeline_has_xpath(&c.pipeline))
            || s.split_doc.as_ref().is_some_and(pipeline_has_xpath)
            || s.key_pipeline.as_ref().is_some_and(pipeline_has_xpath)
            || s.value_pipeline.as_ref().is_some_and(pipeline_has_xpath)
            || s.table_pipeline.as_ref().is_some_and(pipeline_has_xpath)
            || s.rows_pipeline.as_ref().is_some_and(pipeline_has_xpath)
            || s.match_pipeline.as_ref().is_some_and(pipeline_has_xpath)
            || s.fields.iter().any(|f| pipeline_has_xpath(&f.pipeline))
        {
            return true;
        }
    }
    module
        .functions
        .iter()
        .any(|f| pipeline_has_xpath(&f.pipeline))
}

/// Core error, string, Unicode Rune, regex, collection, and JSON projection runtime helpers.
pub const CSHARP_CORE_RUNTIME: &str = r##"public class RtError : Exception
{
    public string Context { get; }
    public string RawMessage { get; }

    public RtError(string context, string message, Exception? inner = null)
        : base(string.IsNullOrEmpty(context) ? message : $"{context}: {message}", inner)
    {
        this.Context = context;
        this.RawMessage = message;
    }
}

public sealed class RtAssertionError : RtError
{
    public RtAssertionError(string context, string message)
        : base(context, message)
    {
    }
}

public sealed class RtJsonSchemaError : RtError
{
    public RtJsonSchemaError(string context, string message)
        : base(context, message)
    {
    }
}

public sealed class RtJsonFieldMissingError : RtError
{
    public RtJsonFieldMissingError(string context, string message)
        : base(context, message)
    {
    }
}

public sealed class RtJsonPathError : RtError
{
    public RtJsonPathError(string context, string message)
        : base(context, message)
    {
    }
}

public sealed class TransportError : RtError
{
    public Exception? Cause { get; }

    public TransportError(string context, string message, Exception? cause = null)
        : base(context, message, cause)
    {
        this.Cause = cause;
    }
}

public sealed class HttpStatusError : RtError
{
    public int Status { get; }
    public string Body { get; }

    public HttpStatusError(string context, int status, string body)
        : base(context, $"HTTP {status}: {body}")
    {
        this.Status = status;
        this.Body = body;
    }
}

public const string RT_UNMATCHED_TABLE_ROW = "__PARSMITH_UNMATCHED_TABLE_ROW__";

public static T Fallback<T>(Func<T> action, T fallbackValue)
{
    try
    {
        return action();
    }
    catch (Exception)
    {
        return fallbackValue;
    }
}

public static void AssertTrue(bool cond, string context, string message)
{
    if (!cond)
    {
        throw new RtAssertionError(context, message);
    }
}

public static string Trim(string value, string? chars = null) =>
    string.IsNullOrEmpty(chars) ? value.Trim() : value.Trim(chars.ToCharArray());

public static List<string> Trim(IEnumerable<string> values, string? chars = null) =>
    values.Select(v => Trim(v, chars)).ToList();

public static string Ltrim(string value, string? chars = null) =>
    string.IsNullOrEmpty(chars) ? value.TrimStart() : value.TrimStart(chars.ToCharArray());

public static List<string> Ltrim(IEnumerable<string> values, string? chars = null) =>
    values.Select(v => Ltrim(v, chars)).ToList();

public static string Rtrim(string value, string? chars = null) =>
    string.IsNullOrEmpty(chars) ? value.TrimEnd() : value.TrimEnd(chars.ToCharArray());

public static List<string> Rtrim(IEnumerable<string> values, string? chars = null) =>
    values.Select(v => Rtrim(v, chars)).ToList();

public static string NormalizeSpace(string value) =>
    string.Join(" ", value.Split((char[]?)null, StringSplitOptions.RemoveEmptyEntries));

public static List<string> NormalizeSpace(IEnumerable<string> values) =>
    values.Select(NormalizeSpace).ToList();

public static string RmPrefix(string value, string prefix) =>
    !string.IsNullOrEmpty(prefix) && value.StartsWith(prefix, StringComparison.Ordinal)
        ? value.Substring(prefix.Length)
        : value;

public static List<string> RmPrefix(IEnumerable<string> values, string prefix) =>
    values.Select(v => RmPrefix(v, prefix)).ToList();

public static string RmSuffix(string value, string suffix) =>
    !string.IsNullOrEmpty(suffix) && value.EndsWith(suffix, StringComparison.Ordinal)
        ? value.Substring(0, value.Length - suffix.Length)
        : value;

public static List<string> RmSuffix(IEnumerable<string> values, string suffix) =>
    values.Select(v => RmSuffix(v, suffix)).ToList();

public static string RmPrefixSuffix(string value, string part) =>
    RmSuffix(RmPrefix(value, part), part);

public static List<string> RmPrefixSuffix(IEnumerable<string> values, string part) =>
    values.Select(v => RmPrefixSuffix(v, part)).ToList();

public static string FmtValue(string template, string value)
{
    if (template.Contains("{{}}", StringComparison.Ordinal))
    {
        return template.Replace("{{}}", value, StringComparison.Ordinal);
    }
    int idx = template.IndexOf("{}", StringComparison.Ordinal);
    if (idx >= 0)
    {
        return string.Concat(template.AsSpan(0, idx), value, template.AsSpan(idx + 2));
    }
    return template;
}

public static List<string> FmtValue(string template, IEnumerable<string> values) =>
    values.Select(v => FmtValue(template, v)).ToList();

public static string Repl(string value, string oldVal, string newVal) =>
    value.Replace(oldVal, newVal, StringComparison.Ordinal);

public static List<string> Repl(IEnumerable<string> values, string oldVal, string newVal) =>
    values.Select(v => Repl(v, oldVal, newVal)).ToList();

public static string ReplMap(string value, (string OldVal, string NewVal)[] pairs)
{
    var cur = value;
    foreach (var (oldVal, newVal) in pairs)
    {
        cur = cur.Replace(oldVal, newVal, StringComparison.Ordinal);
    }
    return cur;
}

public static List<string> ReplMap(IEnumerable<string> values, (string OldVal, string NewVal)[] pairs) =>
    values.Select(v => ReplMap(v, pairs)).ToList();

public static string Lower(string value) => value.ToLowerInvariant();

public static List<string> Lower(IEnumerable<string> values) =>
    values.Select(Lower).ToList();

public static string Upper(string value) => value.ToUpperInvariant();

public static List<string> Upper(IEnumerable<string> values) =>
    values.Select(Upper).ToList();

public static List<string> Split(string value, string sep) =>
    value.Split(sep, StringSplitOptions.None).ToList();

public static string Join(IEnumerable<string> values, string sep) =>
    string.Join(sep, values);

private static readonly Regex ReHexEntity = new(@"&#[xX]([0-9a-fA-F]+);", RegexOptions.Compiled | RegexOptions.CultureInvariant);
private static readonly Regex ReDecEntity = new(@"&#(\d+);", RegexOptions.Compiled | RegexOptions.CultureInvariant);
private static readonly Regex ReUnicodeEscape = new(@"\\u([0-9a-fA-F]{4})", RegexOptions.Compiled | RegexOptions.CultureInvariant);
private static readonly Regex ReHexEscape = new(@"\\x([0-9a-fA-F]{2})", RegexOptions.Compiled | RegexOptions.CultureInvariant);

public static string Unescape(string value)
{
    var s = value
        .Replace("&quot;", "\"", StringComparison.Ordinal)
        .Replace("&#39;", "'", StringComparison.Ordinal)
        .Replace("&apos;", "'", StringComparison.Ordinal)
        .Replace("&lt;", "<", StringComparison.Ordinal)
        .Replace("&gt;", ">", StringComparison.Ordinal)
        .Replace("&nbsp;", "\u00a0", StringComparison.Ordinal)
        .Replace("&copy;", "\u00a9", StringComparison.Ordinal)
        .Replace("&reg;", "\u00ae", StringComparison.Ordinal)
        .Replace("&trade;", "\u2122", StringComparison.Ordinal)
        .Replace("&euro;", "\u20ac", StringComparison.Ordinal)
        .Replace("&pound;", "\u00a3", StringComparison.Ordinal)
        .Replace("&yen;", "\u00a5", StringComparison.Ordinal)
        .Replace("&cent;", "\u00a2", StringComparison.Ordinal)
        .Replace("&sect;", "\u00a7", StringComparison.Ordinal)
        .Replace("&deg;", "\u00b0", StringComparison.Ordinal)
        .Replace("&plusmn;", "\u00b1", StringComparison.Ordinal)
        .Replace("&times;", "\u00d7", StringComparison.Ordinal)
        .Replace("&divide;", "\u00f7", StringComparison.Ordinal)
        .Replace("&ndash;", "\u2013", StringComparison.Ordinal)
        .Replace("&mdash;", "\u2014", StringComparison.Ordinal)
        .Replace("&lsquo;", "\u2018", StringComparison.Ordinal)
        .Replace("&rsquo;", "\u2019", StringComparison.Ordinal)
        .Replace("&ldquo;", "\u201c", StringComparison.Ordinal)
        .Replace("&rdquo;", "\u201d", StringComparison.Ordinal)
        .Replace("&bull;", "\u2022", StringComparison.Ordinal)
        .Replace("&hellip;", "\u2026", StringComparison.Ordinal)
        .Replace("&laquo;", "\u00ab", StringComparison.Ordinal)
        .Replace("&raquo;", "\u00bb", StringComparison.Ordinal)
        .Replace("&amp;", "&", StringComparison.Ordinal);

    s = ReHexEntity.Replace(s, m =>
        int.TryParse(m.Groups[1].Value, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out int cp) && Rune.TryCreate(cp, out var r)
            ? r.ToString()
            : m.Value);
    s = ReDecEntity.Replace(s, m =>
        int.TryParse(m.Groups[1].Value, NumberStyles.Integer, CultureInfo.InvariantCulture, out int cp) && Rune.TryCreate(cp, out var r)
            ? r.ToString()
            : m.Value);
    s = System.Net.WebUtility.HtmlDecode(s);
    s = ReUnicodeEscape.Replace(s, m =>
        int.TryParse(m.Groups[1].Value, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out int cp) && Rune.TryCreate(cp, out var r)
            ? r.ToString()
            : m.Value);
    s = ReHexEscape.Replace(s, m =>
        int.TryParse(m.Groups[1].Value, NumberStyles.HexNumber, CultureInfo.InvariantCulture, out int cp) && Rune.TryCreate(cp, out var r)
            ? r.ToString()
            : m.Value);

    return s
        .Replace("\\b", "\b", StringComparison.Ordinal)
        .Replace("\\f", "\f", StringComparison.Ordinal)
        .Replace("\\n", "\n", StringComparison.Ordinal)
        .Replace("\\r", "\r", StringComparison.Ordinal)
        .Replace("\\t", "\t", StringComparison.Ordinal);
}

public static List<string> Unescape(IEnumerable<string> values) =>
    values.Select(Unescape).ToList();

public static string StrIndex(string value, long index, string context)
{
    var runes = value.EnumerateRunes().ToArray();
    long pos = index < 0 ? runes.Length + index : index;
    if (pos < 0 || pos >= runes.Length)
    {
        throw new RtError(context, "index out of bounds");
    }
    return runes[pos].ToString();
}

public static T ItemAt<T>(IReadOnlyList<T> values, long index, string context)
{
    long pos = index < 0 ? values.Count + index : index;
    if (pos < 0 || pos >= values.Count)
    {
        throw new RtError(context, "index out of bounds");
    }
    return values[(int)pos];
}

private static (int Start, int End) NormalizeSliceRange(long len, long start, long end)
{
    long s = start < 0 ? Math.Max(0L, len + start) : Math.Min(len, start);
    long e = end < 0 ? Math.Max(0L, len + end) : Math.Min(len, end);
    return s >= e ? (0, 0) : ((int)s, (int)e);
}

public static string StrSlice(string value, long start, long end)
{
    var runes = value.EnumerateRunes().ToArray();
    var (s, e) = NormalizeSliceRange(runes.Length, start, end);
    if (s >= e)
    {
        return string.Empty;
    }
    var sb = new StringBuilder(e - s);
    for (int i = s; i < e; i++)
    {
        sb.Append(runes[i].ToString());
    }
    return sb.ToString();
}

public static List<T> SliceList<T>(IReadOnlyList<T> values, long start, long end)
{
    var (s, e) = NormalizeSliceRange(values.Count, start, end);
    if (s >= e)
    {
        return new List<T>();
    }
    var res = new List<T>(e - s);
    for (int i = s; i < e; i++)
    {
        res.Add(values[i]);
    }
    return res;
}

public static long StrLen(string value) =>
    value.EnumerateRunes().Count();

public static long Len<T>(IReadOnlyCollection<T> values) =>
    values.Count;

public static List<T> Unique<T>(IEnumerable<T> values)
{
    var seen = new HashSet<T>();
    var res = new List<T>();
    foreach (var item in values)
    {
        if (seen.Add(item))
        {
            res.Add(item);
        }
    }
    return res;
}

public static long ToInt(string value, string context)
{
    if (long.TryParse(value.Trim(), NumberStyles.Integer, CultureInfo.InvariantCulture, out long res))
    {
        return res;
    }
    throw new RtError(context, $"invalid integer: '{value}'");
}

public static List<long> ToInt(IEnumerable<string> values, string context) =>
    values.Select(v => ToInt(v, context)).ToList();

public static double ToFloat(string value, string context)
{
    if (double.TryParse(value.Trim(), NumberStyles.Float | NumberStyles.AllowThousands, CultureInfo.InvariantCulture, out double res))
    {
        return res;
    }
    throw new RtError(context, $"invalid float: '{value}'");
}

public static List<double> ToFloat(IEnumerable<string> values, string context) =>
    values.Select(v => ToFloat(v, context)).ToList();

public static bool StringBool(string? value) => !string.IsNullOrEmpty(value);

public static bool NumBool(long value) => value != 0L;

public static bool FloatBool(double value) => value != 0.0 && !double.IsNaN(value);

public static bool SeqBool<T>(IReadOnlyCollection<T>? values) => values is not null && values.Count > 0;

public static string RegexSearch(string value, string pattern, string context)
{
    var re = new Regex(pattern, RegexOptions.CultureInvariant);
    var m = re.Match(value);
    if (!m.Success)
    {
        throw new RtError(context, "regex did not match");
    }
    return m.Groups.Count > 1 ? m.Groups[1].Value : m.Groups[0].Value;
}

public static List<string> RegexSearch(IEnumerable<string> values, string pattern, string context) =>
    values.Select(v => RegexSearch(v, pattern, context)).ToList();

public static List<string> RegexAll(string value, string pattern, string context)
{
    var re = new Regex(pattern, RegexOptions.CultureInvariant);
    var matches = re.Matches(value);
    var res = new List<string>(matches.Count);
    foreach (Match m in matches)
    {
        if (m.Success)
        {
            res.Add(m.Groups.Count > 1 ? m.Groups[1].Value : m.Groups[0].Value);
        }
    }
    return res;
}

public static string RegexSub(string value, string pattern, string replacement, string context)
{
    var re = new Regex(pattern, RegexOptions.CultureInvariant);
    return re.Replace(value, replacement);
}

public static List<string> RegexSub(IEnumerable<string> values, string pattern, string replacement, string context) =>
    values.Select(v => RegexSub(v, pattern, replacement, context)).ToList();

public static List<string> RegexSplit(string value, string pattern, string context)
{
    var re = new Regex(pattern, RegexOptions.CultureInvariant);
    return re.Split(value).ToList();
}

public static bool RegexMatch(string value, string pattern) =>
    Regex.IsMatch(value, pattern, RegexOptions.CultureInvariant);

public static bool ReAny(IEnumerable<string> values, string pattern) =>
    values.Any(v => RegexMatch(v, pattern));

public static bool ReAllMatch(IEnumerable<string> values, string pattern) =>
    values.All(v => RegexMatch(v, pattern));

public static bool Contains(string value, string needle) =>
    value.Contains(needle, StringComparison.Ordinal);

public static bool Starts(string value, string needle) =>
    value.StartsWith(needle, StringComparison.Ordinal);

public static bool Ends(string value, string needle) =>
    value.EndsWith(needle, StringComparison.Ordinal);

public static string JsonKindName(JsonNode? node)
{
    if (node is null) return "null";
    return node.GetValueKind() switch
    {
        JsonValueKind.Object => "object",
        JsonValueKind.Array => "array",
        JsonValueKind.String => "string",
        JsonValueKind.Number => "number",
        JsonValueKind.True or JsonValueKind.False => "boolean",
        JsonValueKind.Null => "null",
        _ => "undefined",
    };
}

public static JsonObject ExpectObject(JsonNode? node, string context)
{
    if (node is JsonObject obj)
    {
        return obj;
    }
    throw new RtJsonSchemaError(context, $"Expected object, got {JsonKindName(node)}");
}

private static (string Segment, bool HasEscape, string? Remaining) NextGjsonSegment(string path)
{
    var sb = new StringBuilder(path.Length);
    bool hasEscape = false;
    for (int i = 0; i < path.Length; i++)
    {
        char c = path[i];
        if (c == '\\' && i + 1 < path.Length)
        {
            char next = path[i + 1];
            if (next == '.' || next == '\\' || next == '#')
            {
                hasEscape = true;
                sb.Append(next);
                i++;
                continue;
            }
            sb.Append('\\');
        }
        else if (c == '.')
        {
            return (sb.ToString(), hasEscape, path.Substring(i + 1));
        }
        else
        {
            sb.Append(c);
        }
    }
    return (sb.ToString(), hasEscape, null);
}

public static bool TryJsonPath(JsonNode? node, string path, string context, out JsonNode? result) =>
    TryJsonPath(node, path, context, out result, out _);

public static bool TryJsonPath(JsonNode? node, string path, string context, out JsonNode? result, out string? missingSegment)
{
    missingSegment = null;
    if (string.IsNullOrEmpty(path))
    {
        result = node;
        return true;
    }
    JsonNode? current = node;
    string? rem = path;
    while (rem is not null)
    {
        var (segment, hasEscape, nextRem) = NextGjsonSegment(rem);
        if (current is null)
        {
            missingSegment = segment;
            result = null;
            return false;
        }
        rem = nextRem;
        if (!hasEscape && segment == "#")
        {
            if (current is not JsonArray arr)
            {
                throw new RtJsonPathError(context, "Expected list for '#' operator");
            }
            if (rem is null)
            {
                result = JsonValue.Create((long)arr.Count);
                return true;
            }
            var projected = new JsonArray();
            foreach (var item in arr)
            {
                if (TryJsonPath(item, rem, context, out var subVal, out _) && subVal is not null)
                {
                    projected.Add(subVal.DeepClone());
                }
            }
            result = projected;
            return true;
        }

        if (!hasEscape && int.TryParse(segment, NumberStyles.None, CultureInfo.InvariantCulture, out int idx))
        {
            if (current is JsonArray a)
            {
                if (idx < 0 || idx >= a.Count)
                {
                    missingSegment = segment;
                    result = null;
                    return false;
                }
                current = a[idx];
                continue;
            }
            if (current is JsonObject oByIdx && oByIdx.TryGetPropertyValue(segment, out var byIdxProp))
            {
                current = byIdxProp;
                continue;
            }
            missingSegment = segment;
            result = null;
            return false;
        }

        if (current is JsonObject obj && obj.TryGetPropertyValue(segment, out var propVal))
        {
            current = propVal;
        }
        else
        {
            missingSegment = segment;
            result = null;
            return false;
        }
    }
    result = current;
    return true;
}

public static JsonNode? JsonPath(JsonNode? node, string path, string context, string? fieldName = null)
{
    if (TryJsonPath(node, path, context, out var res, out var missingSeg))
    {
        return res;
    }
    if (fieldName is not null)
    {
        throw new RtJsonFieldMissingError(context, $"Required JSON field '{path}' (mapped to '{fieldName}') is missing");
    }
    throw new RtJsonPathError(context, $"Missing key '{missingSeg ?? path}' in path '{path}'");
}

public static JsonNode? ParseJson(string raw, string path, string context)
{
    JsonNode? parsed;
    try
    {
        parsed = JsonNode.Parse(raw);
    }
    catch (Exception ex)
    {
        throw new RtError(context, ex.Message, ex);
    }
    if (string.IsNullOrEmpty(path))
    {
        return parsed;
    }
    return JsonPath(parsed, path, context);
}

public static JsonNode? ExtractJsonValue(JsonNode? node, string? path, string context)
{
    if (string.IsNullOrEmpty(path))
    {
        return node;
    }
    return JsonPath(node, path!, context);
}

public static string DecodeString(JsonNode? node, string context)
{
    if (node is JsonValue val && val.GetValueKind() == JsonValueKind.String)
    {
        if (val.TryGetValue<string>(out var s) && s is not null)
        {
            return s;
        }
        if (val.TryGetValue<JsonElement>(out var el) && el.ValueKind == JsonValueKind.String)
        {
            return el.GetString()!;
        }
    }
    throw new RtJsonSchemaError(context, $"Expected string, got {JsonKindName(node)}");
}

public static long DecodeInt(JsonNode? node, string context)
{
    if (node is JsonValue val && val.GetValueKind() == JsonValueKind.Number)
    {
        if (val.TryGetValue<long>(out var l))
        {
            return l;
        }
        if (val.TryGetValue<int>(out var i))
        {
            return i;
        }
        if (val.TryGetValue<JsonElement>(out var el) && el.ValueKind == JsonValueKind.Number && el.TryGetInt64(out var elLong))
        {
            return elLong;
        }
    }
    throw new RtJsonSchemaError(context, $"Expected integer, got {JsonKindName(node)}");
}

public static double DecodeFloat(JsonNode? node, string context)
{
    if (node is JsonValue val && val.GetValueKind() == JsonValueKind.Number)
    {
        if (val.TryGetValue<double>(out var d))
        {
            return d;
        }
        if (val.TryGetValue<float>(out var f))
        {
            return f;
        }
        if (val.TryGetValue<long>(out var l))
        {
            return l;
        }
        if (val.TryGetValue<int>(out var i))
        {
            return i;
        }
        if (val.TryGetValue<JsonElement>(out var el) && el.ValueKind == JsonValueKind.Number && el.TryGetDouble(out var elDouble))
        {
            return elDouble;
        }
    }
    throw new RtJsonSchemaError(context, $"Expected float, got {JsonKindName(node)}");
}

public static bool DecodeBool(JsonNode? node, string context)
{
    if (node is JsonValue val)
    {
        var kind = val.GetValueKind();
        if (kind == JsonValueKind.True) return true;
        if (kind == JsonValueKind.False) return false;
    }
    throw new RtJsonSchemaError(context, $"Expected boolean, got {JsonKindName(node)}");
}

public static List<T> DecodeList<T>(JsonNode? node, string context, Func<JsonNode?, T> itemDecoder)
{
    if (node is not JsonArray arr)
    {
        throw new RtJsonSchemaError(context, $"Expected array, got {JsonKindName(node)}");
    }
    var res = new List<T>(arr.Count);
    foreach (var item in arr)
    {
        res.Add(itemDecoder(item));
    }
    return res;
}

public static Dictionary<K, V> DecodeDict<K, V>(
    JsonNode? node,
    string context,
    Func<string, string, K> keyParser,
    Func<JsonNode?, V> valDecoder)
    where K : notnull
{
    if (node is not JsonObject obj)
    {
        throw new RtJsonSchemaError(context, $"Expected object, got {JsonKindName(node)}");
    }
    var res = new Dictionary<K, V>(obj.Count);
    foreach (var kv in obj)
    {
        var k = keyParser(kv.Key, context);
        var v = valDecoder(kv.Value);
        res[k] = v;
    }
    return res;
}

public static string ParseStringKey(string raw, string context) => raw;

public static long ParseIntKey(string raw, string context) => ToInt(raw, context);

public static double ParseFloatKey(string raw, string context) => ToFloat(raw, context);

public static bool ParseBoolKey(string raw, string context) =>
    bool.TryParse(raw.Trim(), out bool b) ? b : throw new RtJsonSchemaError(context, $"Invalid boolean map key '{raw}'");"##;

/// HTML DOM extraction runtime helpers (`AngleSharp`), emitted when `module_has_html(module)` is `true`.
pub const CSHARP_HTML_RUNTIME: &str = r#"public const string RT_FALLBACK_HTML_STR = "<html><body></body></html>";

public static IElement ParseHtml(string? html)
{
    var src = string.IsNullOrWhiteSpace(html) ? RT_FALLBACK_HTML_STR : html!;
    var parser = new HtmlParser();
    var doc = parser.ParseDocument(src);
    return doc.DocumentElement ?? parser.ParseDocument(RT_FALLBACK_HTML_STR).DocumentElement!;
}

private static IElement? QueryOne(IElement root, string query)
{
    if (ReferenceEquals(root, root.Owner?.DocumentElement))
    {
        return root.Owner!.QuerySelector(query);
    }
    return root.QuerySelector(query);
}

private static List<IElement> QueryAll(IElement root, string query)
{
    if (ReferenceEquals(root, root.Owner?.DocumentElement))
    {
        return root.Owner!.QuerySelectorAll(query).ToList();
    }
    return root.QuerySelectorAll(query).ToList();
}

public static IElement CssSelect(IElement root, string[] queries, string context)
{
    foreach (var q in queries)
    {
        var m = QueryOne(root, q);
        if (m is not null)
        {
            return m;
        }
    }
    throw new RtError(context, "selector matched no elements");
}

public static IElement CssSelect(IEnumerable<IElement> roots, string[] queries, string context)
{
    foreach (var q in queries)
    {
        foreach (var root in roots)
        {
            var m = QueryOne(root, q);
            if (m is not null)
            {
                return m;
            }
        }
    }
    throw new RtError(context, "selector matched no elements");
}

public static IElement? CssSelectOpt(IElement root, string[] queries)
{
    foreach (var q in queries)
    {
        var m = QueryOne(root, q);
        if (m is not null)
        {
            return m;
        }
    }
    return null;
}

public static IElement? CssSelectOpt(IEnumerable<IElement> roots, string[] queries)
{
    foreach (var q in queries)
    {
        foreach (var root in roots)
        {
            var m = QueryOne(root, q);
            if (m is not null)
            {
                return m;
            }
        }
    }
    return null;
}

public static List<IElement> CssSelectAll(IElement root, string[] queries)
{
    foreach (var q in queries)
    {
        var list = QueryAll(root, q);
        if (list.Count > 0)
        {
            return list;
        }
    }
    return new List<IElement>();
}

public static List<IElement> CssSelectAll(IEnumerable<IElement> roots, string[] queries)
{
    foreach (var q in queries)
    {
        var list = new List<IElement>();
        foreach (var root in roots)
        {
            list.AddRange(QueryAll(root, q));
        }
        if (list.Count > 0)
        {
            return list;
        }
    }
    return new List<IElement>();
}

public static IElement CssRemove(IElement root, string query)
{
    var clone = (IElement)root.Clone(true);
    foreach (var el in clone.QuerySelectorAll(query).ToArray())
    {
        el.Remove();
    }
    return clone;
}

public static List<IElement> CssRemove(IEnumerable<IElement> roots, string query) =>
    roots.Select(r => CssRemove(r, query)).ToList();

public static string Text(IElement? root, string context)
{
    if (root is null)
    {
        throw new RtError(context, "value has no selected node");
    }
    return root.TextContent;
}

public static List<string> TextAll(IEnumerable<IElement> roots) =>
    roots.Select(r => r.TextContent).ToList();

public static string RawHtml(IElement? root, bool inner, string context)
{
    if (root is null)
    {
        throw new RtError(context, "value has no selected node");
    }
    return inner ? root.InnerHtml : root.OuterHtml;
}

public static List<string> RawHtmlAll(IEnumerable<IElement> roots, bool inner) =>
    roots.Select(r => inner ? r.InnerHtml : r.OuterHtml).ToList();

public static string Attr(IElement? root, string[] names, string context)
{
    if (root is null)
    {
        throw new RtError(context, "value has no selected node");
    }
    foreach (var name in names)
    {
        var val = root.GetAttribute(name);
        if (val is not null)
        {
            return val;
        }
    }
    throw new RtError(context, "attribute not found");
}

public static List<string> AttrList(IEnumerable<IElement> roots, string[] names, string context)
{
    var res = new List<string>();
    foreach (var root in roots)
    {
        res.Add(Attr(root, names, context));
    }
    return res;
}

public static List<string> AttrMulti(IElement? root, string[] names)
{
    var res = new List<string>();
    if (root is null)
    {
        return res;
    }
    foreach (var name in names)
    {
        var val = root.GetAttribute(name);
        if (!string.IsNullOrEmpty(val))
        {
            res.Add(val);
        }
    }
    return res;
}

public static List<string> AttrMulti(IEnumerable<IElement> roots, string[] names)
{
    var res = new List<string>();
    foreach (var root in roots)
    {
        foreach (var name in names)
        {
            var val = root.GetAttribute(name);
            if (!string.IsNullOrEmpty(val))
            {
                res.Add(val);
            }
        }
    }
    return res;
}

public static bool NodeBool(IElement? el) => el is not null;

public static bool NodeBool(IEnumerable<IElement>? els) => els is not null && els.Any();

public static bool PredCss(IElement? el, string query) =>
    el is not null && QueryOne(el, query) is not null;

public static bool PredCss(IEnumerable<IElement> els, string query) =>
    els.Any(e => QueryOne(e, query) is not null);

public static bool PredHasAttr(IElement? el, string[] names) =>
    el is not null && names.Any(el.HasAttribute);

public static bool PredHasAttr(IEnumerable<IElement> els, string[] names)
{
    var first = els.FirstOrDefault();
    return first is not null && names.Any(first.HasAttribute);
}

public static string PredAttr(IElement? el, string name) =>
    el?.GetAttribute(name) ?? string.Empty;

public static string PredAttr(IEnumerable<IElement> els, string name) =>
    els.FirstOrDefault()?.GetAttribute(name) ?? string.Empty;

public static string PredText(IElement? el) =>
    el?.TextContent ?? string.Empty;

public static string PredText(IEnumerable<IElement> els) =>
    string.Concat(els.Select(e => e.TextContent));"#;

/// XPath 1.0 extraction runtime helpers (`AngleSharp.XPath`), emitted only when `module_has_xpath(module)` is `true`.
pub const CSHARP_XPATH_RUNTIME: &str = r#"private static string NormalizeXPath(IElement el, string query)
{
    if (!ReferenceEquals(el, el.Owner?.DocumentElement) && query.StartsWith("//", StringComparison.Ordinal))
    {
        return "." + query;
    }
    return query;
}

private static List<IElement> XpathQueryAll(IElement root, string query)
{
    var q = NormalizeXPath(root, query);
    var nodes = root.SelectNodes(q);
    var res = new List<IElement>();
    if (nodes is null)
    {
        return res;
    }
    foreach (var n in nodes)
    {
        if (n is IElement el)
        {
            res.Add(el);
        }
    }
    return res;
}

public static IElement XpathSelect(IElement root, string[] queries, string context)
{
    foreach (var q in queries)
    {
        var list = XpathQueryAll(root, q);
        if (list.Count > 0)
        {
            return list[0];
        }
    }
    throw new RtError(context, "xpath matched no elements");
}

public static IElement XpathSelect(IEnumerable<IElement> roots, string[] queries, string context)
{
    foreach (var q in queries)
    {
        foreach (var root in roots)
        {
            var list = XpathQueryAll(root, q);
            if (list.Count > 0)
            {
                return list[0];
            }
        }
    }
    throw new RtError(context, "xpath matched no elements");
}

public static IElement? XpathSelectOpt(IElement root, string[] queries)
{
    foreach (var q in queries)
    {
        var list = XpathQueryAll(root, q);
        if (list.Count > 0)
        {
            return list[0];
        }
    }
    return null;
}

public static IElement? XpathSelectOpt(IEnumerable<IElement> roots, string[] queries)
{
    foreach (var q in queries)
    {
        foreach (var root in roots)
        {
            var list = XpathQueryAll(root, q);
            if (list.Count > 0)
            {
                return list[0];
            }
        }
    }
    return null;
}

public static List<IElement> XpathSelectAll(IElement root, string[] queries)
{
    foreach (var q in queries)
    {
        var list = XpathQueryAll(root, q);
        if (list.Count > 0)
        {
            return list;
        }
    }
    return new List<IElement>();
}

public static List<IElement> XpathSelectAll(IEnumerable<IElement> roots, string[] queries)
{
    foreach (var q in queries)
    {
        var list = new List<IElement>();
        foreach (var root in roots)
        {
            list.AddRange(XpathQueryAll(root, q));
        }
        if (list.Count > 0)
        {
            return list;
        }
    }
    return new List<IElement>();
}

public static IElement XpathRemove(IElement root, string query)
{
    var clone = (IElement)root.Clone(true);
    foreach (var el in XpathQueryAll(clone, query))
    {
        el.Remove();
    }
    return clone;
}

public static List<IElement> XpathRemove(IEnumerable<IElement> roots, string query) =>
    roots.Select(r => XpathRemove(r, query)).ToList();

public static bool PredXpath(IElement? el, string query) =>
    el is not null && XpathQueryAll(el, query).Count > 0;

public static bool PredXpath(IEnumerable<IElement> els, string query) =>
    els.Any(e => XpathQueryAll(e, query).Count > 0);"#;

/// HTTP & REST transport runtime helpers (`System.Net.Http.HttpClient`), emitted when `module_uses_http(module)` is `true`.
pub const CSHARP_HTTP_RUNTIME: &str = r#"public sealed class HttpRequestOptions
{
    public List<KeyValuePair<string, string>> Headers { get; } = new();
    public List<KeyValuePair<string, string>> Params { get; } = new();
    public List<KeyValuePair<string, string>> Cookies { get; } = new();
    public TimeSpan? Timeout { get; set; }
    public CancellationToken CancellationToken { get; set; } = CancellationToken.None;
}

public delegate void HttpOption(HttpRequestOptions options);

public static HttpOption WithHeader(string key, string value) =>
    opts => opts.Headers.Add(new KeyValuePair<string, string>(key, value));

public static HttpOption WithHeaders(IEnumerable<KeyValuePair<string, string>> headers) =>
    opts => opts.Headers.AddRange(headers);

public static HttpOption WithParam(string key, string value) =>
    opts => opts.Params.Add(new KeyValuePair<string, string>(key, value));

public static HttpOption WithParams(IEnumerable<KeyValuePair<string, string>> parameters) =>
    opts => opts.Params.AddRange(parameters);

public static HttpOption WithCookie(string key, string value) =>
    opts => opts.Cookies.Add(new KeyValuePair<string, string>(key, value));

public static HttpOption WithCookies(IEnumerable<KeyValuePair<string, string>> cookies) =>
    opts => opts.Cookies.AddRange(cookies);

public static HttpOption WithTimeout(TimeSpan timeout) =>
    opts => opts.Timeout = timeout;

public static HttpOption WithTimeoutSeconds(double seconds) =>
    opts => opts.Timeout = TimeSpan.FromSeconds(seconds);

public static HttpOption WithCancellationToken(CancellationToken token) =>
    opts => opts.CancellationToken = token;

public enum ErrorCondKind
{
    Null,
    Bool,
    Int,
    Float,
    Str,
}

public readonly struct ErrorCond
{
    public string Path { get; }
    public ErrorCondKind Kind { get; }
    public bool BoolVal { get; }
    public long IntVal { get; }
    public double FloatVal { get; }
    public string StrVal { get; }

    private ErrorCond(string path, ErrorCondKind kind, bool b, long i, double f, string s)
    {
        this.Path = path;
        this.Kind = kind;
        this.BoolVal = b;
        this.IntVal = i;
        this.FloatVal = f;
        this.StrVal = s;
    }

    public static ErrorCond Null(string path) => new(path, ErrorCondKind.Null, false, 0L, 0.0, string.Empty);
    public static ErrorCond Bool(string path, bool val) => new(path, ErrorCondKind.Bool, val, 0L, 0.0, string.Empty);
    public static ErrorCond Int(string path, long val) => new(path, ErrorCondKind.Int, false, val, 0.0, string.Empty);
    public static ErrorCond Float(string path, double val) => new(path, ErrorCondKind.Float, false, 0L, val, string.Empty);
    public static ErrorCond Str(string path, string val) => new(path, ErrorCondKind.Str, false, 0L, 0.0, val);
}

public static bool MatchErrorJson(JsonNode? body, string[] requiredKeys, ErrorCond[] conditions)
{
    if (body is null)
    {
        return false;
    }
    foreach (var key in requiredKeys)
    {
        if (!TryJsonPath(body, key, string.Empty, out _))
        {
            return false;
        }
    }
    foreach (var cond in conditions)
    {
        bool hasVal = TryJsonPath(body, cond.Path, string.Empty, out var val);
        bool matched = cond.Kind switch
        {
            ErrorCondKind.Null => !hasVal || val is null,
            ErrorCondKind.Bool => hasVal && val is JsonValue bv && (
                (cond.BoolVal && bv.GetValueKind() == JsonValueKind.True) ||
                (!cond.BoolVal && bv.GetValueKind() == JsonValueKind.False)),
            ErrorCondKind.Int => hasVal && val is not null && TryMatchInt(val, cond.IntVal),
            ErrorCondKind.Float => hasVal && val is not null && TryMatchFloat(val, cond.FloatVal),
            ErrorCondKind.Str => hasVal && val is not null && TryMatchStr(val, cond.StrVal),
            _ => false,
        };
        if (!matched)
        {
            return false;
        }
    }
    return true;
}

private static bool TryMatchInt(JsonNode node, long expected)
{
    try
    {
        return DecodeInt(node, string.Empty) == expected;
    }
    catch
    {
        return false;
    }
}

private static bool TryMatchFloat(JsonNode node, double expected)
{
    try
    {
        return Math.Abs(DecodeFloat(node, string.Empty) - expected) < 1e-12;
    }
    catch
    {
        return false;
    }
}

private static bool TryMatchStr(JsonNode node, string expected)
{
    try
    {
        return DecodeString(node, string.Empty) == expected;
    }
    catch
    {
        return false;
    }
}

public static JsonArray? ToJsonArray<T>(IEnumerable<T>? items)
{
    if (items is null)
    {
        return null;
    }
    var arr = new JsonArray();
    foreach (var item in items)
    {
        arr.Add(JsonValue.Create(item));
    }
    return arr;
}

public static string BuildUrl(string baseUrl, List<KeyValuePair<string, string>> queryPairs)
{
    if (queryPairs.Count == 0)
    {
        return baseUrl;
    }
    var sb = new StringBuilder();
    for (int i = 0; i < queryPairs.Count; i++)
    {
        if (i > 0) sb.Append('&');
        var kv = queryPairs[i];
        var encodedKey = Uri.EscapeDataString(kv.Key).Replace("%5B", "[", StringComparison.Ordinal).Replace("%5D", "]", StringComparison.Ordinal);
        sb.Append(encodedKey);
        sb.Append('=');
        sb.Append(Uri.EscapeDataString(kv.Value));
    }
    char sep = baseUrl.Contains('?', StringComparison.Ordinal) ? '&' : '?';
    return $"{baseUrl}{sep}{sb}";
}

public static bool TryParseJsonNode(string text, out JsonNode? node)
{
    if (string.IsNullOrWhiteSpace(text))
    {
        node = null;
        return false;
    }
    try
    {
        node = JsonNode.Parse(text);
        return true;
    }
    catch
    {
        node = null;
        return false;
    }
}

public static async Task<(int Status, string Body, Dictionary<string, string> Headers)> ExecuteHttpAsync(
    HttpClient client,
    string method,
    string baseUrl,
    List<KeyValuePair<string, string>> queryPairs,
    List<KeyValuePair<string, string>> headers,
    List<KeyValuePair<string, string>> cookies,
    string? body,
    List<KeyValuePair<string, string>>? formPairs,
    HttpOption[]? options)
{
    var opts = new HttpRequestOptions();
    if (options is not null)
    {
        foreach (var opt in options)
        {
            opt?.Invoke(opts);
        }
    }

    var allParams = new List<KeyValuePair<string, string>>(queryPairs.Count + opts.Params.Count);
    allParams.AddRange(queryPairs);
    allParams.AddRange(opts.Params);

    var fullUrl = BuildUrl(baseUrl, allParams);
    using var req = new HttpRequestMessage(new HttpMethod(method.ToUpperInvariant()), fullUrl);

    string? contentType = null;
    var allHeaders = new List<KeyValuePair<string, string>>(headers.Count + opts.Headers.Count);
    allHeaders.AddRange(headers);
    allHeaders.AddRange(opts.Headers);

    foreach (var kv in allHeaders)
    {
        if (string.Equals(kv.Key, "Content-Type", StringComparison.OrdinalIgnoreCase))
        {
            contentType = kv.Value;
        }
        else if (!req.Headers.TryAddWithoutValidation(kv.Key, kv.Value))
        {
            req.Content?.Headers.TryAddWithoutValidation(kv.Key, kv.Value);
        }
    }

    var allCookies = new List<KeyValuePair<string, string>>(cookies.Count + opts.Cookies.Count);
    allCookies.AddRange(cookies);
    allCookies.AddRange(opts.Cookies);
    if (allCookies.Count > 0)
    {
        var cookieStr = string.Join("; ", allCookies.Select(c => $"{c.Key}={c.Value}"));
        req.Headers.TryAddWithoutValidation("Cookie", cookieStr);
    }

    if (formPairs is not null)
    {
        req.Content = new FormUrlEncodedContent(formPairs);
    }
    else if (body is not null)
    {
        req.Content = new StringContent(body, Encoding.UTF8);
        if (contentType is not null)
        {
            req.Content.Headers.Remove("Content-Type");
            req.Content.Headers.TryAddWithoutValidation("Content-Type", contentType);
        }
    }

    using var cts = opts.Timeout.HasValue
        ? CancellationTokenSource.CreateLinkedTokenSource(opts.CancellationToken)
        : null;
    if (cts is not null && opts.Timeout.HasValue)
    {
        cts.CancelAfter(opts.Timeout.Value);
    }
    var token = cts?.Token ?? opts.CancellationToken;

    using var resp = await client.SendAsync(req, token).ConfigureAwait(false);
    int status = (int)resp.StatusCode;
    string respText = await resp.Content.ReadAsStringAsync(token).ConfigureAwait(false);

    var respHeaders = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
    foreach (var h in resp.Headers)
    {
        respHeaders[h.Key.ToLowerInvariant()] = string.Join(", ", h.Value);
    }
    if (resp.Content?.Headers is not null)
    {
        foreach (var h in resp.Content.Headers)
        {
            respHeaders[h.Key.ToLowerInvariant()] = string.Join(", ", h.Value);
        }
    }

    return (status, respText, respHeaders);
}

public static async Task<string> FetchHtmlAsync(
    HttpClient client,
    string context,
    string method,
    string baseUrl,
    List<KeyValuePair<string, string>> queryPairs,
    List<KeyValuePair<string, string>> headers,
    List<KeyValuePair<string, string>> cookies,
    string? body,
    List<KeyValuePair<string, string>>? formPairs,
    string? responsePath,
    string? responseJoin,
    HttpOption[]? options)
{
    int status;
    string respBody;
    try
    {
        (status, respBody, _) = await ExecuteHttpAsync(
            client, method, baseUrl, queryPairs, headers, cookies, body, formPairs, options).ConfigureAwait(false);
    }
    catch (Exception ex) when (ex is not RtError)
    {
        throw new TransportError(context, ex.Message, ex);
    }

    if (status >= 400)
    {
        throw new HttpStatusError(context, status, respBody);
    }

    if (!string.IsNullOrEmpty(responsePath))
    {
        var extracted = ParseJson(respBody, responsePath!, context);
        if (extracted is JsonArray arr && responseJoin is not null)
        {
            var parts = new List<string>(arr.Count);
            foreach (var item in arr)
            {
                if (item is JsonValue v && v.GetValueKind() == JsonValueKind.String && v.TryGetValue<string>(out var s) && s is not null)
                {
                    parts.Add(s);
                }
                else
                {
                    parts.Add(item?.ToJsonString() ?? string.Empty);
                }
            }
            return string.Join(responseJoin, parts);
        }
        if (extracted is JsonValue sv && sv.GetValueKind() == JsonValueKind.String && sv.TryGetValue<string>(out var strVal) && strVal is not null)
        {
            return strVal;
        }
        return extracted?.ToJsonString() ?? string.Empty;
    }

    return respBody;
}"#;

fn ensure_public_static_helper(code: &str) -> String {
    let trimmed = code.trim_start();
    if trimmed.starts_with("public ") {
        code.to_owned()
    } else if trimmed.starts_with("static ") {
        let prefix_len = code.len() - trimmed.len();
        format!("{}public {trimmed}", &code[..prefix_len])
    } else {
        let prefix_len = code.len() - trimmed.len();
        format!("{}public static {trimmed}", &code[..prefix_len])
    }
}

/// Normalizes an import string into a valid C# `using ...;` directive.
pub fn normalize_csharp_using(imp: &str) -> String {
    let trimmed = imp.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let with_prefix = if trimmed.starts_with("using ") {
        trimmed.to_owned()
    } else {
        format!("using {trimmed}")
    };
    if with_prefix.ends_with(';') {
        with_prefix
    } else {
        format!("{with_prefix};")
    }
}

/// Collects the standard and conditional `using` directives for a C# module or runtime file.
pub fn collect_csharp_usings(module: &Module, builder: &ModuleBuilder) -> IndexSet<String> {
    let include_html = module_has_html(module);
    let include_xpath = module_has_xpath(module);
    let include_http = module_uses_http(module);

    let mut usings = IndexSet::new();
    usings.insert("using System;".to_owned());
    usings.insert("using System.Collections.Generic;".to_owned());
    usings.insert("using System.Globalization;".to_owned());
    usings.insert("using System.Linq;".to_owned());
    usings.insert("using System.Text;".to_owned());
    usings.insert("using System.Text.Json;".to_owned());
    usings.insert("using System.Text.Json.Nodes;".to_owned());
    usings.insert("using System.Text.Json.Serialization;".to_owned());
    usings.insert("using System.Text.RegularExpressions;".to_owned());

    if include_http {
        usings.insert("using System.Net.Http;".to_owned());
        usings.insert("using System.Net.Http.Headers;".to_owned());
        usings.insert("using System.Threading;".to_owned());
        usings.insert("using System.Threading.Tasks;".to_owned());
    }
    if include_html {
        usings.insert("using AngleSharp.Dom;".to_owned());
        usings.insert("using AngleSharp.Html.Dom;".to_owned());
        usings.insert("using AngleSharp.Html.Parser;".to_owned());
    }
    if include_xpath {
        usings.insert("using AngleSharp.XPath;".to_owned());
    }

    for imp in builder.imports() {
        let u = normalize_csharp_using(imp);
        if !u.is_empty() {
            usings.insert(u);
        }
    }
    for imp in builder.runtime_imports() {
        let u = normalize_csharp_using(imp);
        if !u.is_empty() {
            usings.insert(u);
        }
    }

    usings
}

/// Writes the body of `public static partial class Rt` to `writer`.
pub fn write_csharp_runtime_body(
    module: &Module,
    builder: &ModuleBuilder,
    writer: &mut CodeWriter,
) {
    let include_html = module_has_html(module);
    let include_xpath = module_has_xpath(module);
    let include_http = module_uses_http(module);

    writer.write_block(CSHARP_CORE_RUNTIME.trim_end());

    if include_html {
        writer.blankline();
        writer.write_block(CSHARP_HTML_RUNTIME.trim_end());
    }

    if include_xpath {
        writer.blankline();
        writer.write_block(CSHARP_XPATH_RUNTIME.trim_end());
    }

    if include_http {
        writer.blankline();
        writer.write_block(CSHARP_HTTP_RUNTIME.trim_end());
    }

    for def in builder.runtime_defs().values() {
        writer.blankline();
        let pub_code = ensure_public_static_helper(&def.code);
        writer.write_block(&pub_code);
    }
}

/// Generates the standalone C# shared runtime file (`<runtime_name>.cs`) for `-R / --separate-runtime`.
pub fn emit_csharp_runtime_module(
    module: &Module,
    builder: &ModuleBuilder,
    options: &CodegenOptions,
) -> String {
    let mut writer = CodeWriter::with_indent("    ");
    writer.writeln("// Auto-generated runtime helpers by parsmith. DO NOT EDIT.");
    writer.writeln("#nullable enable");
    writer.writeln(
        "#pragma warning disable CS8600, CS8601, CS8602, CS8603, CS8604, CS8618, CS8625, CS0162, CS0219",
    );
    writer.blankline();

    let usings = collect_csharp_usings(module, builder);
    for u in &usings {
        writer.writeln(u);
    }
    writer.blankline();

    let pkg = options.package.trim();
    if !pkg.is_empty() && pkg != "main" && validate_csharp_namespace(pkg) {
        wln!(writer, "namespace {pkg};");
        writer.blankline();
    }

    writer.writeln("public static partial class Rt");
    writer.writeln("{");
    writer.indented(|writer| {
        write_csharp_runtime_body(module, builder, writer);
    });
    writer.writeln("}");
    writer.finish()
}
