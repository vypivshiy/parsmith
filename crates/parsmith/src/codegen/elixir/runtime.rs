//! Modular Elixir runtime (`Rt`) for `ElixirEmitter`.
//!
//! Splits runtime definitions into:
//! 1. Core / string / regex / collection / JSON projection helpers (always emitted).
//! 2. HTML DOM helpers (`Floki`, emitted when `module_has_html(module)` is `true`).
//! 3. HTTP / REST transport helpers (`:httpc` + `via_req`, emitted when `module_uses_http(module)` is `true`).

use crate::ast::Module;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::elixir::{elixir_ident, elixir_pascal, format_elixir_module_prefix};
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, module_has_html, module_uses_http};

/// Core error, string, regex, collection, and GJSON path runtime helpers (always included in `Rt`).
pub const ELIXIR_CORE_RUNTIME: &str = r###"defmodule Error do
  defexception [:context, :message]

  def new(context, message) do
    %__MODULE__{context: to_string(context), message: to_string(message)}
  end

  @impl true
  def message(%__MODULE__{context: "", message: msg}), do: msg
  def message(%__MODULE__{context: nil, message: msg}), do: msg
  def message(%__MODULE__{context: ctx, message: msg}), do: "#{ctx}: #{msg}"
end

defmodule RtError do
  defexception [:context, :message]

  def new(context, message) do
    %__MODULE__{context: to_string(context), message: to_string(message)}
  end

  @impl true
  def message(%__MODULE__{context: "", message: msg}), do: msg
  def message(%__MODULE__{context: nil, message: msg}), do: msg
  def message(%__MODULE__{context: ctx, message: msg}), do: "#{ctx}: #{msg}"
end

defmodule JsonError do
  defexception [:context, :message]

  def new(context, message) do
    %__MODULE__{context: to_string(context), message: to_string(message)}
  end

  @impl true
  def message(%__MODULE__{context: "", message: msg}), do: msg
  def message(%__MODULE__{context: nil, message: msg}), do: msg
  def message(%__MODULE__{context: ctx, message: msg}), do: "#{ctx}: #{msg}"
end

def format_error(%{__exception__: true} = err), do: Exception.message(err)
def format_error(err) when is_binary(err), do: err
def format_error(err), do: inspect(err)

def unmatched_table_row, do: :__parsmith_unmatched_table_row__

def fallback(fun, default) when is_function(fun, 0) do
  try do
    fun.()
  rescue
    _ in [Error, RtError, JsonError] -> default
  end
end

def wrap_result(fun) when is_function(fun, 0) do
  try do
    {:ok, fun.()}
  rescue
    e in [Error, RtError, JsonError] -> {:error, e}
  end
end

def assert_true(true, _context, _message), do: :ok
def assert_true(_, context, message), do: raise(Error.new(context, message))

def ltrim(value, nil), do: String.trim_leading(value)
def ltrim(value, ""), do: String.trim_leading(value)

def ltrim(value, chars) do
  set = MapSet.new(String.codepoints(chars))

  value
  |> String.codepoints()
  |> Enum.drop_while(&MapSet.member?(set, &1))
  |> Enum.join("")
end

def rtrim(value, nil), do: String.trim_trailing(value)
def rtrim(value, ""), do: String.trim_trailing(value)

def rtrim(value, chars) do
  set = MapSet.new(String.codepoints(chars))

  value
  |> String.codepoints()
  |> Enum.reverse()
  |> Enum.drop_while(&MapSet.member?(set, &1))
  |> Enum.reverse()
  |> Enum.join("")
end

def trim(value, nil), do: String.trim(value)
def trim(value, ""), do: String.trim(value)
def trim(value, chars), do: value |> ltrim(chars) |> rtrim(chars)

def normalize_space(value) do
  value
  |> String.split(~r/\s+/u, trim: true)
  |> Enum.join(" ")
end

def rm_prefix(value, prefix), do: String.replace_prefix(value, prefix, "")
def rm_suffix(value, suffix), do: String.replace_suffix(value, suffix, "")
def rm_prefix_suffix(value, part), do: value |> rm_prefix(part) |> rm_suffix(part)

def fmt_value(template, value) do
  str_val = to_string(value)

  if String.contains?(template, "{{}}") do
    String.replace(template, "{{}}", str_val)
  else
    String.replace(template, "{}", str_val, global: false)
  end
end

def repl(value, old, new), do: String.replace(value, old, new)

def repl_map(value, pairs) do
  Enum.reduce(pairs, value, fn {old, new}, acc ->
    String.replace(acc, old, new)
  end)
end

def lower(value), do: String.downcase(value)
def upper(value), do: String.upcase(value)

def unescape(value) do
  s =
    value
    |> String.replace("&quot;", "\"")
    |> String.replace("&#39;", "'")
    |> String.replace("&apos;", "'")
    |> String.replace("&lt;", "<")
    |> String.replace("&gt;", ">")
    |> String.replace("&nbsp;", "\u00A0")
    |> String.replace("&copy;", "\u00A9")
    |> String.replace("&reg;", "\u00AE")
    |> String.replace("&trade;", "\u2122")
    |> String.replace("&euro;", "\u20AC")
    |> String.replace("&pound;", "\u00A3")
    |> String.replace("&yen;", "\u00A5")
    |> String.replace("&cent;", "\u00A2")
    |> String.replace("&sect;", "\u00A7")
    |> String.replace("&deg;", "\u00B0")
    |> String.replace("&plusmn;", "\u00B1")
    |> String.replace("&times;", "\u00D7")
    |> String.replace("&divide;", "\u00F7")
    |> String.replace("&ndash;", "\u2013")
    |> String.replace("&mdash;", "\u2014")
    |> String.replace("&lsquo;", "\u2018")
    |> String.replace("&rsquo;", "\u2019")
    |> String.replace("&ldquo;", "\u201C")
    |> String.replace("&rdquo;", "\u201D")
    |> String.replace("&bull;", "\u2022")
    |> String.replace("&hellip;", "\u2026")
    |> String.replace("&laquo;", "\u00AB")
    |> String.replace("&raquo;", "\u00BB")
    |> String.replace("&amp;", "&")

  s =
    Regex.replace(~r/&#[xX]([0-9a-fA-F]+);/u, s, fn full, hex ->
      case Integer.parse(hex, 16) do
        {cp, ""} when cp >= 0 and cp <= 0x10FFFF -> <<cp::utf8>>
        _ -> full
      end
    end)

  s =
    Regex.replace(~r/&#(\d+);/u, s, fn full, dec ->
      case Integer.parse(dec, 10) do
        {cp, ""} when cp >= 0 and cp <= 0x10FFFF -> <<cp::utf8>>
        _ -> full
      end
    end)

  s =
    Regex.replace(~r/\\u([0-9a-fA-F]{4})/u, s, fn full, hex ->
      case Integer.parse(hex, 16) do
        {cp, ""} when cp >= 0 and cp <= 0x10FFFF -> <<cp::utf8>>
        _ -> full
      end
    end)

  s =
    Regex.replace(~r/\\x([0-9a-fA-F]{2})/u, s, fn full, hex ->
      case Integer.parse(hex, 16) do
        {cp, ""} when cp >= 0 and cp <= 0x10FFFF -> <<cp::utf8>>
        _ -> full
      end
    end)

  s
  |> String.replace("\\b", "\b")
  |> String.replace("\\f", "\f")
  |> String.replace("\\n", "\n")
  |> String.replace("\\r", "\r")
  |> String.replace("\\t", "\t")
end

def item_at(values, index, context) when is_list(values) do
  len = length(values)
  pos = if index < 0, do: len + index, else: index

  if pos < 0 or pos >= len do
    raise Error.new(context, "index out of bounds")
  else
    Enum.at(values, pos)
  end
end

def index(value, idx, context) when is_binary(value) do
  value
  |> String.codepoints()
  |> item_at(idx, context)
end

def slice_vec(values, start_idx, end_idx) when is_list(values) do
  len = length(values)
  s = if start_idx < 0, do: max(len + start_idx, 0), else: min(start_idx, len)
  e = if end_idx < 0, do: max(len + end_idx, 0), else: min(end_idx, len)
  if s >= e, do: [], else: Enum.slice(values, s, e - s)
end

def slice(value, start_idx, end_idx) when is_binary(value) do
  value
  |> String.codepoints()
  |> slice_vec(start_idx, end_idx)
  |> Enum.join("")
end

def len(values) when is_list(values), do: length(values)
def str_len(value) when is_binary(value), do: length(String.codepoints(value))

def unique(values) when is_list(values), do: Enum.uniq(values)

def to_int(value, context) when is_binary(value) do
  case Integer.parse(String.trim(value)) do
    {n, ""} -> n
    _ -> raise Error.new(context, "invalid integer: #{value}")
  end
end

def to_float(value, context) when is_binary(value) do
  case Float.parse(String.trim(value)) do
    {f, ""} -> f
    _ -> raise Error.new(context, "invalid float: #{value}")
  end
end

def string_bool(value) when is_binary(value), do: value != ""
def num_bool(value) when is_integer(value), do: value != 0
def float_bool(value) when is_float(value), do: value != 0.0
def seq_bool(values) when is_list(values), do: values != []

def regex_search(value, pattern, context) do
  re = Regex.compile!(pattern, "u")

  case Regex.run(re, value) do
    [_, group1 | _] -> group1
    [full] -> full
    nil -> raise Error.new(context, "regex did not match")
  end
end

def regex_all(value, pattern, _context \\ "") do
  re = Regex.compile!(pattern, "u")

  Regex.scan(re, value)
  |> Enum.map(fn
    [_, group1 | _] -> group1
    [full] -> full
  end)
end

def regex_sub(value, pattern, replacement, _context \\ "") do
  re = Regex.compile!(pattern, "u")
  Regex.replace(re, value, replacement)
end

def regex_split(value, pattern, _context \\ "") do
  re = Regex.compile!(pattern, "u")
  Regex.split(re, value)
end

def regex_match?(value, pattern) do
  case Regex.compile(pattern, "u") do
    {:ok, re} -> Regex.match?(re, value)
    _ -> false
  end
end

def contains?(value, needle), do: String.contains?(value, needle)
def starts?(value, prefix), do: String.starts_with?(value, prefix)
def ends?(value, suffix), do: String.ends_with?(value, suffix)

defp next_gjson_segment(path) do
  do_next_gjson_segment(String.codepoints(path), [], false)
end

defp do_next_gjson_segment([], acc, has_escape) do
  {Enum.join(Enum.reverse(acc), ""), has_escape, nil}
end

defp do_next_gjson_segment(["\\", next | rest], acc, _has_escape)
     when next in [".", "\\", "#"] do
  do_next_gjson_segment(rest, [next | acc], true)
end

defp do_next_gjson_segment(["." | rest], acc, has_escape) do
  {Enum.join(Enum.reverse(acc), ""), has_escape, Enum.join(rest, "")}
end

defp do_next_gjson_segment([ch | rest], acc, has_escape) do
  do_next_gjson_segment(rest, [ch | acc], has_escape)
end

def json_opt(value, path) do
  try do
    case json_path(value, path, "") do
      {:ok, v} -> {:ok, v}
      :missing -> :error
    end
  rescue
    _ in [JsonError, Error] -> :error
  end
end

def json_path(value, "", _context), do: {:ok, value}
def json_path(value, nil, _context), do: {:ok, value}

def json_path(value, path, context) when is_binary(path) do
  do_json_path(value, path, context)
end

defp do_json_path(current, path, context) do
  {segment, has_escape, rem} = next_gjson_segment(path)

  if not has_escape and segment == "#" do
    cond do
      is_nil(current) ->
        :missing

      not is_list(current) ->
        raise JsonError.new(context, "expected JSON array for '#' operator")

      is_nil(rem) ->
        {:ok, length(current)}

      true ->
        projected =
          Enum.flat_map(current, fn item ->
            case json_opt(item, rem) do
              {:ok, v} when not is_nil(v) -> [v]
              _ -> []
            end
          end)

        {:ok, projected}
    end
  else
    next_result =
      cond do
        not has_escape and is_list(current) ->
          case Integer.parse(segment) do
            {idx, ""} when idx >= 0 and idx < length(current) ->
              {:ok, Enum.at(current, idx)}

            _ ->
              :missing
          end

        is_map(current) and not is_struct(current) ->
          if Map.has_key?(current, segment) do
            {:ok, Map.get(current, segment)}
          else
            :missing
          end

        true ->
          :missing
      end

    case {next_result, rem} do
      {:missing, _} -> :missing
      {{:ok, next_val}, nil} -> {:ok, next_val}
      {{:ok, nil}, _} -> :missing
      {{:ok, next_val}, subpath} -> do_json_path(next_val, subpath, context)
    end
  end
end

def json_path!(value, path, context) do
  case json_path(value, path, context) do
    {:ok, v} -> v
    :missing -> raise JsonError.new(context, "missing JSON path '#{path}'")
  end
end

def extract_json_value(value, nil, _context), do: {:ok, value}
def extract_json_value(value, "", _context), do: {:ok, value}

def extract_json_value(value, path, context) do
  try do
    {:ok, json_path!(value, path, context)}
  rescue
    e in [JsonError, Error] -> {:error, e}
  end
end

def decode_json(raw, context) when is_binary(raw) do
  case JSON.decode(raw) do
    {:ok, val} -> {:ok, val}
    {:error, err} -> {:error, JsonError.new(context, format_error(err))}
  end
end

def decode_json!(raw, context) when is_binary(raw) do
  case decode_json(raw, context) do
    {:ok, val} -> val
    {:error, err} -> raise err
  end
end

def parse_json(raw, path, context) when is_binary(raw) do
  parsed = decode_json!(raw, context)
  json_path!(parsed, path, context)
end

def expect_object!(value, _context) when is_map(value) and not is_struct(value), do: value
def expect_object!(_value, context), do: raise(JsonError.new(context, "expected JSON object"))

def expect_array!(value, _context) when is_list(value), do: value
def expect_array!(_value, context), do: raise(JsonError.new(context, "expected JSON array"))

def decode_string(value, _context) when is_binary(value), do: value
def decode_string(_value, context), do: raise(JsonError.new(context, "expected string"))

def decode_int(value, _context) when is_integer(value), do: value
def decode_int(_value, context), do: raise(JsonError.new(context, "expected integer"))

def decode_float(value, _context) when is_float(value), do: value
def decode_float(value, _context) when is_integer(value), do: value * 1.0
def decode_float(_value, context), do: raise(JsonError.new(context, "expected float"))

def decode_bool(value, _context) when is_boolean(value), do: value
def decode_bool(_value, context), do: raise(JsonError.new(context, "expected boolean"))

def decode_list(value, item_fn, context) do
  value
  |> expect_array!(context)
  |> Enum.map(item_fn)
end

def decode_map(value, key_fn, val_fn, context) do
  value
  |> expect_object!(context)
  |> Map.new(fn {k, v} -> {key_fn.(k), val_fn.(v)} end)
end

def decode_int_key(k, context) when is_binary(k) do
  case Integer.parse(k) do
    {n, ""} -> n
    _ -> raise JsonError.new(context, "invalid integer map key '#{k}'")
  end
end

def decode_float_key(k, context) when is_binary(k) do
  case Float.parse(k) do
    {f, ""} -> f
    _ -> raise JsonError.new(context, "invalid float map key '#{k}'")
  end
end

def decode_bool_key("true", _context), do: true
def decode_bool_key("false", _context), do: false
def decode_bool_key(k, context), do: raise(JsonError.new(context, "invalid boolean map key '#{k}'"))"###;

/// HTML DOM extraction runtime helpers (`Floki`), emitted when `module_has_html(module)` is `true`.
pub const ELIXIR_HTML_RUNTIME: &str = r###"@type html_node :: tuple()
@type nodes :: [html_node()]

def parse_doc(html) when is_binary(html) do
  src = if String.trim(html) == "", do: "<html><body></body></html>", else: html
  [{"__document__", [], normalize_rcdata_nodes(Floki.parse_document!(src))}]
end

defp normalize_rcdata_nodes(nodes) when is_list(nodes) do
  Enum.map(nodes, &normalize_rcdata_node/1)
end

defp normalize_rcdata_node({tag, attrs, children}) when tag in ["title", "textarea"] do
  {tag, attrs,
   Enum.map(children, fn
     text when is_binary(text) -> unescape(text)
     other -> normalize_rcdata_node(other)
   end)}
end

defp normalize_rcdata_node({tag, attrs, children}) when is_list(children) do
  {tag, attrs, normalize_rcdata_nodes(children)}
end

defp normalize_rcdata_node(other), do: other

def find_in_node({"__document__", _attrs, children}, query) do
  Floki.find(children, query)
end

def find_in_node({tag, attrs, _children} = node, query) do
  res = Floki.find([node], query)

  case Floki.find([{tag, attrs, []}], query) do
    [_ | _] -> Enum.drop(res, 1)
    [] -> res
  end
end

def find_in_node(_, _query), do: []

def select_opt(roots, queries) when is_list(roots) and is_list(queries) do
  Enum.find_value(queries, [], fn query ->
    Enum.find_value(roots, nil, fn root ->
      case find_in_node(root, query) do
        [first | _] -> [first]
        [] -> nil
      end
    end)
  end)
end

def select_first(roots, queries, context) do
  case select_opt(roots, queries) do
    [_ | _] = found -> found
    [] -> raise Error.new(context, "selector matched no elements")
  end
end

def select(roots, queries, context), do: select_first(roots, queries, context)

def select_all(roots, queries, _context \\ "") when is_list(roots) and is_list(queries) do
  Enum.find_value(queries, [], fn query ->
    out = Enum.flat_map(roots, fn root -> find_in_node(root, query) end)
    if out != [], do: out, else: nil
  end)
end

def select_all_first(roots, queries, context \\ ""), do: select_all(roots, queries, context)

def select_remove(roots, query) when is_list(roots) do
  Enum.map(roots, fn
    {tag, attrs, children} -> {tag, attrs, Floki.filter_out(children, query)}
    other -> other
  end)
end

def css_remove(roots, query), do: select_remove(roots, query)

def text([first | _], _context), do: Floki.text([first], js: true, style: true)
def text([], context), do: raise(Error.new(context, "value has no selected node"))

def text_all(roots) when is_list(roots) do
  Enum.map(roots, fn node -> Floki.text([node], js: true, style: true) end)
end

def raw_html([{"__document__", _attrs, children} | _], _inner, _context) do
  Floki.raw_html(children)
end

def raw_html([{_tag, _attrs, children} = node | _], inner, _context) do
  if inner, do: Floki.raw_html(children), else: Floki.raw_html([node])
end

def raw_html([], _inner, context) do
  raise Error.new(context, "value has no selected node")
end

def raw(roots, inner, context), do: raw_html(roots, inner, context)

def raw_all(roots, inner) when is_list(roots) do
  Enum.map(roots, fn node -> raw_html([node], inner, "") end)
end

def attr([node | _], names, context) when is_list(names) do
  found =
    Enum.find_value(names, nil, fn name ->
      case Floki.attribute([node], name) do
        [val | _] -> {:ok, val}
        [] -> nil
      end
    end)

  case found do
    {:ok, val} -> val
    nil -> raise Error.new(context, "attribute not found")
  end
end

def attr([], _names, context) do
  raise Error.new(context, "value has no selected node")
end

def attr_list(roots, names, context) when is_list(roots) and is_list(names) do
  Enum.map(roots, fn node -> attr([node], names, context) end)
end

def attr_multi(roots, names) when is_list(roots) and is_list(names) do
  for node <- roots,
      name <- names,
      val <- Floki.attribute([node], name),
      val != "" do
    val
  end
end

def node_at(roots, index, context) when is_list(roots) do
  [item_at(roots, index, context)]
end

def node_bool(roots) when is_list(roots), do: roots != []

def pred_css?(roots, query) when is_list(roots) do
  Enum.any?(roots, fn root -> find_in_node(root, query) != [] end)
end

def pred_attr([node | _], name) do
  case Floki.attribute([node], name) do
    [val | _] -> val
    [] -> nil
  end
end

def pred_attr([], _name), do: nil

def pred_has_attr?(roots, name), do: pred_attr(roots, name) != nil

def pred_text(roots) when is_list(roots) do
  Enum.map_join(roots, "", fn node -> Floki.text([node], js: true, style: true) end)
end"###;

/// HTTP & REST transport runtime helpers (`:httpc` + `via_req`), emitted when `module_uses_http(module)` is `true`.
pub const ELIXIR_HTTP_RUNTIME: &str = r###"defmodule HttpRequest do
  defstruct method: "GET",
            url: "",
            headers: %{},
            params: [],
            cookies: %{},
            body: nil,
            body_kind: :empty
end

defmodule RawResponse do
  defstruct status: 200,
            headers: %{},
            body: "",
            text: ""
end

defmodule TransportError do
  defexception [:cause, :message]

  def new(cause) do
    %__MODULE__{cause: cause, message: inspect(cause)}
  end

  @impl true
  def message(%__MODULE__{message: msg}), do: msg
end

defmodule DecodeErr do
  @derive JSON.Encoder
  defstruct [:status, :body, :cause, headers: %{}, ok: false]
end

defmodule TransportErr do
  @derive JSON.Encoder
  defstruct [:cause, status: 0, ok: false]
end

defmodule UnknownErr do
  @derive JSON.Encoder
  defstruct [:status, :body, :value, headers: %{}, ok: false]
end

defmodule ErrorMatcher do
  defstruct status: 0,
            required_keys: [],
            conditions: [],
            has_schema: true,
            decode: nil
end

def via_httpc(options \\ []) do
  %{kind: :httpc, options: options}
end

def via_req(options_or_fun \\ [])

def via_req(fun) when is_function(fun, 1) do
  %{kind: :custom, fun: fun}
end

def via_req(options) do
  %{kind: :req, options: options}
end

defp merge_request_opts(%HttpRequest{} = req, opts) when is_list(opts) or is_map(opts) do
  extra_headers =
    case Access.get(opts, :headers) do
      h when is_map(h) -> Map.new(h, fn {k, v} -> {to_string(k), to_string(v)} end)
      h when is_list(h) -> Map.new(h, fn {k, v} -> {to_string(k), to_string(v)} end)
      _ -> %{}
    end

  extra_params =
    case Access.get(opts, :params) do
      p when is_map(p) -> Enum.map(p, fn {k, v} -> {to_string(k), to_string(v)} end)
      p when is_list(p) -> Enum.map(p, fn {k, v} -> {to_string(k), to_string(v)} end)
      _ -> []
    end

  extra_cookies =
    case Access.get(opts, :cookies) do
      c when is_map(c) -> Map.new(c, fn {k, v} -> {to_string(k), to_string(v)} end)
      c when is_list(c) -> Map.new(c, fn {k, v} -> {to_string(k), to_string(v)} end)
      _ -> %{}
    end

  %HttpRequest{
    req
    | headers: Map.merge(req.headers, extra_headers),
      params: req.params ++ extra_params,
      cookies: Map.merge(req.cookies, extra_cookies)
  }
end

defp merge_request_opts(%HttpRequest{} = req, _opts), do: req

def send_request(client, %HttpRequest{} = raw_req, call_opts \\ []) do
  req = merge_request_opts(raw_req, call_opts)

  full_url =
    case req.params do
      [] ->
        req.url

      pairs ->
        qs = URI.encode_query(pairs)

        if qs == "" do
          req.url
        else
          if String.contains?(req.url, "?"), do: req.url <> "&" <> qs, else: req.url <> "?" <> qs
        end
    end

  headers_map =
    if map_size(req.cookies) > 0 do
      cookie_str =
        req.cookies
        |> Enum.map_join("; ", fn {k, v} -> "#{k}=#{v}" end)

      Map.put(req.headers, "Cookie", cookie_str)
    else
      req.headers
    end

  {content_type, body_bin} =
    case req.body_kind do
      :json when not is_nil(req.body) ->
        ct = find_header_ci(headers_map, "content-type") || "application/json"
        {ct, JSON.encode!(req.body)}

      :form when is_list(req.body) ->
        ct = find_header_ci(headers_map, "content-type") || "application/x-www-form-urlencoded"
        {ct, URI.encode_query(req.body)}

      :raw when not is_nil(req.body) ->
        ct = find_header_ci(headers_map, "content-type") || "text/plain"
        {ct, to_string(req.body)}

      _ ->
        {find_header_ci(headers_map, "content-type"), nil}
    end

  final_req = %HttpRequest{
    req
    | url: full_url,
      headers: headers_map,
      body: body_bin
  }

  case client do
    %{kind: :custom, fun: fun} ->
      try do
        {:ok, fun.(final_req)}
      rescue
        e -> {:error, TransportError.new(e)}
      end

    fun when is_function(fun, 1) ->
      try do
        {:ok, fun.(final_req)}
      rescue
        e -> {:error, TransportError.new(e)}
      end

    %{kind: :req, options: req_opts} ->
      send_via_req(final_req, content_type, req_opts)

    %{kind: :httpc, options: httpc_opts} ->
      send_via_httpc(final_req, content_type, httpc_opts)

    _ ->
      send_via_httpc(final_req, content_type, [])
  end
end

defp find_header_ci(headers, target_lower) do
  Enum.find_value(headers, nil, fn {k, v} ->
    if String.downcase(to_string(k)) == target_lower, do: v, else: nil
  end)
end

defp send_via_httpc(%HttpRequest{} = req, content_type, opts) do
  _ = :inets.start()
  _ = :ssl.start()

  method_atom =
    req.method
    |> String.downcase()
    |> String.to_atom()

  url_charlist = String.to_charlist(req.url)
  needs_body_tuple = method_atom in [:post, :put, :patch] or not is_nil(req.body)

  headers_charlist =
    req.headers
    |> Enum.reject(fn {k, _v} ->
      needs_body_tuple and String.downcase(to_string(k)) == "content-type"
    end)
    |> Enum.map(fn {k, v} -> {String.to_charlist(to_string(k)), String.to_charlist(to_string(v))} end)

  request_tuple =
    if needs_body_tuple do
      ct_charlist = String.to_charlist(content_type || "application/octet-stream")
      body_payload = req.body || ""
      {url_charlist, headers_charlist, ct_charlist, body_payload}
    else
      {url_charlist, headers_charlist}
    end

  timeout = Access.get(opts, :timeout, 15_000)
  http_opts = [{:timeout, timeout}, {:autoredirect, true}]

  case :httpc.request(method_atom, request_tuple, http_opts, [{:body_format, :binary}]) do
    {:ok, {{_http_ver, status, _reason}, resp_headers, resp_body}} ->
      norm_headers =
        Map.new(resp_headers, fn {k, v} ->
          {String.downcase(List.to_string(k)), List.to_string(v)}
        end)

      bin_body = if is_binary(resp_body), do: resp_body, else: IO.iodata_to_binary(resp_body)

      {:ok,
       %RawResponse{
         status: status,
         headers: norm_headers,
         body: bin_body,
         text: bin_body
       }}

    {:error, reason} ->
      {:error, TransportError.new(reason)}
  end
end

defp send_via_req(%HttpRequest{} = req, content_type, req_opts) do
  try do
    method_atom = req.method |> String.downcase() |> String.to_atom()

    headers_map =
      if not is_nil(content_type) do
        Map.put_new(req.headers, "content-type", content_type)
      else
        req.headers
      end

    base_opts = [
      method: method_atom,
      url: req.url,
      headers: headers_map,
      body: req.body,
      decode_body: false,
      retry: false
    ]

    merged_opts =
      if is_list(req_opts) do
        Keyword.merge(base_opts, req_opts)
      else
        base_opts
      end

    case apply(Req, :request, [merged_opts]) do
      {:ok, resp} ->
        norm_headers =
          Enum.reduce(Map.get(resp, :headers, %{}), %{}, fn
            {k, v}, acc when is_list(v) -> Map.put(acc, String.downcase(to_string(k)), Enum.join(v, ", "))
            {k, v}, acc -> Map.put(acc, String.downcase(to_string(k)), to_string(v))
          end)

        body_str = to_string(Map.get(resp, :body, ""))
        status = Map.get(resp, :status, 200)

        {:ok,
         %RawResponse{
           status: status,
           headers: norm_headers,
           body: body_str,
           text: body_str
         }}

      {:error, exc} ->
        {:error, TransportError.new(exc)}
    end
  rescue
    e -> {:error, TransportError.new(e)}
  end
end

def fetch_html(client, %HttpRequest{} = req, context, response_path, response_join, opts \\ []) do
  case send_request(client, req, opts) do
    {:ok, %RawResponse{status: status, body: body}} ->
      if status >= 400 do
        {:error, Error.new(context, "HTTP #{status}: #{body}")}
      else
        if is_nil(response_path) or response_path == "" do
          {:ok, body}
        else
          try do
            body_val = parse_json(body, response_path, context)

            extracted =
              case {body_val, response_join} do
                {items, sep} when is_list(items) and is_binary(sep) and sep != "" ->
                  Enum.map_join(items, sep, &to_string/1)

                {s, _} when is_binary(s) ->
                  s

                {other, _} ->
                  JSON.encode!(other)
              end

            {:ok, extracted}
          rescue
            e in [Error, RtError, JsonError] -> {:error, e}
          end
        end
      end

    {:error, %TransportError{} = err} ->
      {:error, err}
  end
end

def match_error_json?(nil, _required_keys, _conditions), do: false

def match_error_json?(body, required_keys, conditions) do
  keys_ok =
    Enum.all?(required_keys, fn key ->
      case json_opt(body, key) do
        {:ok, _} -> true
        :error -> false
      end
    end)

  conds_ok =
    Enum.all?(conditions, fn
      {:null, path} ->
        case json_opt(body, path) do
          {:ok, nil} -> true
          :error -> true
          _ -> false
        end

      {:bool, path, expected} ->
        case json_opt(body, path) do
          {:ok, v} when is_boolean(v) -> v == expected
          _ -> false
        end

      {:int, path, expected} ->
        case json_opt(body, path) do
          {:ok, v} when is_integer(v) -> v == expected
          _ -> false
        end

      {:float, path, expected} ->
        case json_opt(body, path) do
          {:ok, v} when is_number(v) -> v * 1.0 == expected * 1.0
          _ -> false
        end

      {:str, path, expected} ->
        case json_opt(body, path) do
          {:ok, v} when is_binary(v) -> v == expected
          _ -> false
        end
    end)

  keys_ok and conds_ok
end

defp dispatch_rest_errors(status, text, parsed_res, headers, matchers) do
  Enum.reduce_while(matchers, {:nomatch, nil}, fn %ErrorMatcher{} = m, {:nomatch, last_err} ->
    if m.status != status do
      {:cont, {:nomatch, last_err}}
    else
      case parsed_res do
        {:ok, val} ->
          if match_error_json?(val, m.required_keys, m.conditions) do
            case m.decode.(status, val, headers) do
              {:ok, err_struct} -> {:halt, {:matched, err_struct}}
              {:error, dec_err} -> {:cont, {:nomatch, dec_err}}
            end
          else
            {:cont, {:nomatch, last_err}}
          end

        {:error, _} ->
          if m.required_keys == [] and m.conditions == [] and not m.has_schema do
            case m.decode.(status, text, headers) do
              {:ok, err_struct} -> {:halt, {:matched, err_struct}}
              {:error, _} -> {:cont, {:nomatch, last_err}}
            end
          else
            {:cont, {:nomatch, last_err}}
          end
      end
    end
  end)
end

def rest_call(client, %HttpRequest{} = req, response_path, context, matchers, decode_fn, opts \\ []) do
  case send_request(client, req, opts) do
    {:error, %TransportError{cause: cause}} ->
      {:error, %TransportErr{status: 0, cause: inspect(cause)}}

    {:ok, %RawResponse{status: status, headers: headers, body: text}} ->
      parsed_res = JSON.decode(text)

      case dispatch_rest_errors(status, text, parsed_res, headers, matchers) do
        {:matched, err_struct} ->
          {:error, err_struct}

        {:nomatch, last_decode_err} ->
          if status >= 200 and status < 300 do
            case parsed_res do
              {:error, err} ->
                {:error,
                 %DecodeErr{
                   status: status,
                   body: text,
                   cause: format_error(err),
                   headers: headers
                 }}

              {:ok, parsed} ->
                case extract_json_value(parsed, response_path, context) do
                  {:error, err} ->
                    {:error,
                     %DecodeErr{
                       status: status,
                       body: parsed,
                       cause: format_error(err),
                       headers: headers
                     }}

                  {:ok, target} ->
                    case decode_fn.(target) do
                      {:ok, value} ->
                        {:ok, value}

                      {:error, err} ->
                        {:error,
                         %DecodeErr{
                           status: status,
                           body: parsed,
                           cause: format_error(err),
                           headers: headers
                         }}
                    end
                end
            end
          else
            err_body =
              case parsed_res do
                {:ok, v} -> v
                {:error, _} -> text
              end

            if not is_nil(last_decode_err) do
              {:error,
               %DecodeErr{
                 status: status,
                 body: err_body,
                 cause: format_error(last_decode_err),
                 headers: headers
               }}
            else
              {:error,
               %UnknownErr{
                 status: status,
                 body: err_body,
                 value: err_body,
                 headers: headers
               }}
            end
          end
      end
  end
end

def rest_call_void(client, %HttpRequest{} = req, _context, matchers, opts \\ []) do
  case send_request(client, req, opts) do
    {:error, %TransportError{cause: cause}} ->
      {:error, %TransportErr{status: 0, cause: inspect(cause)}}

    {:ok, %RawResponse{status: status, headers: headers, body: text}} ->
      parsed_res = JSON.decode(text)

      case dispatch_rest_errors(status, text, parsed_res, headers, matchers) do
        {:matched, err_struct} ->
          {:error, err_struct}

        {:nomatch, last_decode_err} ->
          if status >= 200 and status < 300 do
            {:ok, nil}
          else
            err_body =
              case parsed_res do
                {:ok, v} -> v
                {:error, _} -> text
              end

            if not is_nil(last_decode_err) do
              {:error,
               %DecodeErr{
                 status: status,
                 body: err_body,
                 cause: format_error(last_decode_err),
                 headers: headers
               }}
            else
              {:error,
               %UnknownErr{
                 status: status,
                 body: err_body,
                 value: err_body,
                 headers: headers
               }}
            end
          end
      end
  end
end"###;

/// Writes the body of `Rt` (used both inside inline `defmodule <Prefix>Rt` and in `<runtime_name>.ex`) to `writer`.
pub fn write_elixir_runtime_body(
    module: &Module,
    builder: &ModuleBuilder,
    options: &CodegenOptions,
    writer: &mut CodeWriter,
) {
    let include_html = module_has_html(module);
    let include_http = module_uses_http(module);

    for imp in builder.runtime_imports() {
        writer.write_block(imp);
    }
    if !builder.runtime_imports().is_empty() {
        writer.blankline();
    }

    writer.write_block(ELIXIR_CORE_RUNTIME.trim_end());

    if include_html {
        writer.blankline();
        writer.write_block(ELIXIR_HTML_RUNTIME.trim_end());
    }

    if include_http {
        writer.blankline();
        writer.write_block(ELIXIR_HTTP_RUNTIME.trim_end());
    }

    let prefix = format_elixir_module_prefix(&options.package);
    for json_def in &module.json_defs {
        let fn_name = format!(
            "project_{}",
            elixir_ident(&json_def.name).trim_start_matches('_')
        );
        let mod_name = format!("{prefix}{}Json", elixir_pascal(&json_def.name));
        writer.blankline();
        wln!(
            writer,
            "def {fn_name}(data), do: apply({mod_name}, :project, [data])"
        );
    }

    for def in builder.runtime_defs().values() {
        writer.blankline();
        writer.write_block(&def.code);
    }
}

/// Generates the standalone Elixir shared runtime module (`<runtime_name>.ex`) for `-R / --separate-runtime`.
pub fn emit_elixir_runtime_module(
    module: &Module,
    builder: &ModuleBuilder,
    options: &CodegenOptions,
) -> String {
    let mut writer = CodeWriter::with_indent("  ");
    writer.writeln("# Auto-generated runtime helpers by parsmith. DO NOT EDIT.");
    writer.blankline();

    let prefix = format_elixir_module_prefix(&options.package);
    let clean_rt = options
        .runtime_name
        .trim_start_matches('.')
        .trim_end_matches(".ex");
    let rt_mod = format!("{prefix}{}", elixir_pascal(clean_rt));

    wln!(writer, "defmodule {rt_mod} do");
    writer.indented(|writer| {
        write_elixir_runtime_body(module, builder, options, writer);
    });
    writer.writeln("end");
    writer.finish()
}
