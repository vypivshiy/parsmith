//! Modular PHP 8.1+ runtime (`final class Rt`, error classes, and HTTP/REST transport) for `PhpEmitter`.

use crate::ast::Module;
use crate::codegen::builder::ModuleBuilder;
use crate::codegen::php::dom::PhpDom;
use crate::codegen::php::json::php_json_projector_method;
use crate::codegen::php::{PhpEmitter, normalize_php_namespace, php_pascal};
use crate::codegen::writer::{CodeWriter, wln};
use crate::codegen::{CodegenOptions, PhpHtmlLib, module_uses_http};

/// Core PHP error classes and `final class Rt` body (string, regex, collection, GJSON, and DOM helpers).
pub const PHP_ERROR_CLASSES: &str = r#"class RtError extends \RuntimeException
{
    public readonly string $context;

    public function __construct(string $context, string $message = '', ?\Throwable $previous = null)
    {
        $this->context = $context;
        $full = ($message === '' || $context === '') ? ($message !== '' ? $message : $context) : ($context . ': ' . $message);
        parent::__construct($full, 0, $previous);
    }
}

class RtAssertionError extends RtError
{
}

class RtRegexError extends RtError
{
}

class RtJsonError extends RtError
{
}

class RtJsonPathError extends RtJsonError
{
}

class RtJsonFieldMissingError extends RtJsonError
{
}

class RtJsonSchemaError extends RtJsonError
{
}"#;

/// HTTP transport and monadic REST result classes (`HttpRequest`, `RawResponse`, `TransportError`,
/// `SyncTransport`, `CurlTransport`, `StreamTransport`, `GuzzleTransport`, `Ok`, `UnknownErr`,
/// `TransportErr`, `DecodeErr`, `ErrMatcher`).
pub const PHP_HTTP_CLASSES: &str = r#"readonly class HttpRequest
{
    /**
     * @param array<string, string> $headers
     * @param array<string, string|list<string>> $params
     * @param array<string, string> $cookies
     * @param array<string, mixed> $extra
     */
    public function __construct(
        public string $method,
        public string $url,
        public array $headers = [],
        public array $params = [],
        public array $cookies = [],
        public mixed $body = null,
        public string $bodyKind = 'empty',
        public array $extra = [],
    ) {
    }
}

readonly class RawResponse
{
    /** @param array<string, string> $headers */
    public function __construct(
        public int $status,
        public array $headers,
        public string $text,
    ) {
    }
}

class TransportError extends \RuntimeException
{
    public function __construct(
        public readonly \Throwable $cause,
    ) {
        parent::__construct($cause->getMessage(), 0, $cause);
    }
}

interface SyncTransport
{
    public function send(HttpRequest $req): RawResponse;
}

final class CurlTransport implements SyncTransport
{
    public function __construct(
        private readonly ?float $timeout = null,
    ) {
    }

    public function send(HttpRequest $req): RawResponse
    {
        if (!function_exists('curl_init')) {
            return (new StreamTransport($this->timeout))->send($req);
        }
        $url = Rt::buildUrl($req->url, $req->params);
        $headers = $req->headers;
        if (!empty($req->cookies)) {
            $pairs = [];
            foreach ($req->cookies as $k => $v) {
                $pairs[] = $k . '=' . $v;
            }
            $cookieStr = implode('; ', $pairs);
            $headers['Cookie'] = isset($headers['Cookie']) ? ($headers['Cookie'] . '; ' . $cookieStr) : $cookieStr;
        }
        $bodyStr = Rt::encodeRequestBody($req, $headers);
        $headerLines = ['Expect:'];
        foreach ($headers as $k => $v) {
            $headerLines[] = $k . ': ' . $v;
        }

        $ch = curl_init($url);
        if ($ch === false) {
            throw new TransportError(new \RuntimeException('Failed to initialize cURL'));
        }
        $respHeaders = [];
        curl_setopt($ch, CURLOPT_CUSTOMREQUEST, $req->method);
        curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
        curl_setopt($ch, CURLOPT_HTTPHEADER, $headerLines);
        curl_setopt(
            $ch,
            CURLOPT_HEADERFUNCTION,
            static function ($curl, string $line) use (&$respHeaders): int {
                $len = strlen($line);
                $parts = explode(':', $line, 2);
                if (count($parts) === 2) {
                    $respHeaders[strtolower(trim($parts[0]))] = trim($parts[1]);
                }
                return $len;
            }
        );
        if ($bodyStr !== null) {
            curl_setopt($ch, CURLOPT_POSTFIELDS, $bodyStr);
        }
        $timeout = $req->extra['timeout'] ?? $this->timeout;
        if (is_numeric($timeout)) {
            $ms = max(1, (int) round(((float) $timeout) * 1000));
            curl_setopt($ch, CURLOPT_TIMEOUT_MS, $ms);
            curl_setopt($ch, CURLOPT_CONNECTTIMEOUT_MS, $ms);
        }
        $raw = curl_exec($ch);
        if ($raw === false) {
            $err = curl_error($ch);
            curl_close($ch);
            throw new TransportError(new \RuntimeException($err !== '' ? $err : 'cURL request failed'));
        }
        $status = (int) curl_getinfo($ch, CURLINFO_HTTP_CODE);
        curl_close($ch);
        return new RawResponse(status: $status, headers: $respHeaders, text: (string) $raw);
    }
}

final class StreamTransport implements SyncTransport
{
    public function __construct(
        private readonly ?float $timeout = null,
    ) {
    }

    public function send(HttpRequest $req): RawResponse
    {
        $url = Rt::buildUrl($req->url, $req->params);
        $headers = $req->headers;
        if (!empty($req->cookies)) {
            $pairs = [];
            foreach ($req->cookies as $k => $v) {
                $pairs[] = $k . '=' . $v;
            }
            $cookieStr = implode('; ', $pairs);
            $headers['Cookie'] = isset($headers['Cookie']) ? ($headers['Cookie'] . '; ' . $cookieStr) : $cookieStr;
        }
        $bodyStr = Rt::encodeRequestBody($req, $headers);
        $headerLines = ['Connection: close'];
        foreach ($headers as $k => $v) {
            $headerLines[] = $k . ': ' . $v;
        }
        $timeout = $req->extra['timeout'] ?? $this->timeout ?? 30.0;
        $httpOpts = [
            'method' => $req->method,
            'header' => implode("\r\n", $headerLines),
            'ignore_errors' => true,
            'timeout' => (float) $timeout,
            'protocol_version' => 1.1,
        ];
        if ($bodyStr !== null) {
            $httpOpts['content'] = $bodyStr;
        }
        $ctx = stream_context_create(['http' => $httpOpts]);
        $fp = @fopen($url, 'rb', false, $ctx);
        if ($fp === false) {
            $err = error_get_last();
            throw new TransportError(new \RuntimeException($err['message'] ?? 'Stream transport request failed'));
        }
        $meta = stream_get_meta_data($fp);
        $text = (string) stream_get_contents($fp);
        fclose($fp);

        $status = 200;
        $respHeaders = [];
        $wrapperData = $meta['wrapper_data'] ?? [];
        if (is_array($wrapperData)) {
            foreach ($wrapperData as $line) {
                if (!is_string($line)) {
                    continue;
                }
                if (preg_match('/^HTTP\/\d+(?:\.\d+)?\s+(\d+)/i', $line, $m) === 1) {
                    $status = (int) $m[1];
                    $respHeaders = [];
                } else {
                    $parts = explode(':', $line, 2);
                    if (count($parts) === 2) {
                        $respHeaders[strtolower(trim($parts[0]))] = trim($parts[1]);
                    }
                }
            }
        }
        return new RawResponse(status: $status, headers: $respHeaders, text: $text);
    }
}

final class GuzzleTransport implements SyncTransport
{
    public function __construct(
        private readonly object $client,
    ) {
    }

    public function send(HttpRequest $req): RawResponse
    {
        $url = Rt::buildUrl($req->url, $req->params);
        $headers = $req->headers;
        if (!empty($req->cookies)) {
            $pairs = [];
            foreach ($req->cookies as $k => $v) {
                $pairs[] = $k . '=' . $v;
            }
            $cookieStr = implode('; ', $pairs);
            $headers['Cookie'] = isset($headers['Cookie']) ? ($headers['Cookie'] . '; ' . $cookieStr) : $cookieStr;
        }
        $bodyStr = Rt::encodeRequestBody($req, $headers);
        $options = [
            'headers' => $headers,
            'http_errors' => false,
        ];
        if ($bodyStr !== null) {
            $options['body'] = $bodyStr;
        }
        foreach ($req->extra as $k => $v) {
            $options[$k] = $v;
        }
        try {
            /** @var object $resp */
            $resp = $this->client->request($req->method, $url, $options);
            $status = (int) $resp->getStatusCode();
            $respHeaders = [];
            foreach ($resp->getHeaders() as $k => $vals) {
                $respHeaders[strtolower((string) $k)] = is_array($vals) ? implode(', ', $vals) : (string) $vals;
            }
            $text = (string) $resp->getBody();
            return new RawResponse(status: $status, headers: $respHeaders, text: $text);
        } catch (\Throwable $e) {
            throw new TransportError($e);
        }
    }
}

readonly class Ok implements \JsonSerializable
{
    /** @param array<string, string> $headers */
    public function __construct(
        public int $status,
        public array $headers,
        public mixed $value,
        public bool $ok = true,
        public bool $is_ok = true,
    ) {
    }

    public function isOk(): bool
    {
        return true;
    }

    public function jsonSerialize(): mixed
    {
        return [
            'ok' => true,
            'kind' => 'ok',
            'status' => $this->status,
            'headers' => $this->headers,
            'value' => $this->value,
        ];
    }
}

readonly class UnknownErr implements \JsonSerializable
{
    public mixed $value;

    /** @param array<string, string> $headers */
    public function __construct(
        public int $status,
        public array $headers,
        public mixed $body = null,
        public bool $ok = false,
        public bool $is_ok = false,
    ) {
        $this->value = $body;
    }

    public function isOk(): bool
    {
        return false;
    }

    public function jsonSerialize(): mixed
    {
        return [
            'ok' => false,
            'kind' => 'unknown',
            'status' => $this->status,
            'headers' => $this->headers,
            'body' => $this->body,
            'value' => $this->value,
        ];
    }
}

readonly class TransportErr implements \JsonSerializable
{
    /** @param array<string, string> $headers */
    public function __construct(
        public int $status = 0,
        public array $headers = [],
        public string $cause = '',
        public ?\Throwable $exception = null,
        public bool $ok = false,
        public bool $is_ok = false,
    ) {
    }

    public function isOk(): bool
    {
        return false;
    }

    public function jsonSerialize(): mixed
    {
        return [
            'ok' => false,
            'kind' => 'transport',
            'status' => $this->status,
            'headers' => $this->headers,
            'cause' => $this->cause,
        ];
    }
}

readonly class DecodeErr implements \JsonSerializable
{
    /** @param array<string, string> $headers */
    public function __construct(
        public int $status,
        public array $headers,
        public mixed $body,
        public string $cause,
        public ?\Throwable $exception = null,
        public bool $ok = false,
        public bool $is_ok = false,
    ) {
    }

    public function isOk(): bool
    {
        return false;
    }

    public function jsonSerialize(): mixed
    {
        return [
            'ok' => false,
            'kind' => 'decode',
            'status' => $this->status,
            'headers' => $this->headers,
            'body' => $this->body,
            'cause' => $this->cause,
        ];
    }
}

readonly class ErrMatcher
{
    public function __construct(
        public int $status,
        public ?\Closure $check,
        public \Closure $factory,
    ) {
    }
}

function via_curl(?float $timeout = null): SyncTransport
{
    return Rt::viaCurl($timeout);
}

function via_stream(?float $timeout = null): SyncTransport
{
    return Rt::viaStream($timeout);
}

function via_guzzle(object $client): SyncTransport
{
    return Rt::viaGuzzle($client);
}"#;

/// Methods inside `final class Rt` for DOM, strings, regex, collections, JSON, and HTTP/REST.
pub const PHP_RT_METHODS: &str = r#"public const UNMATCHED_TABLE_ROW = '__PARSMITH_UNMATCHED_TABLE_ROW__';

public static function parseHtml(mixed $html): \DOMElement
{
    if ($html instanceof \DOMElement) {
        return $html;
    }
    if ($html instanceof \DOMDocument && $html->documentElement !== null) {
        return $html->documentElement;
    }
    if ($html instanceof \DOMNode && $html->ownerDocument?->documentElement !== null) {
        return $html->ownerDocument->documentElement;
    }
    $str = is_string($html) ? $html : '';
    if (trim($str) === '') {
        $str = '<html><body></body></html>';
    } elseif (preg_match('/<html[\s>]/i', $str) !== 1) {
        $str = '<html><body>' . $str . '</body></html>';
    }
    $doc = new \DOMDocument('1.0', 'UTF-8');
    $prev = libxml_use_internal_errors(true);
    $encoded = mb_encode_numericentity($str, [0x80, 0x10FFFF, 0, 0x1FFFFF], 'UTF-8');
    $doc->loadHTML(
        $encoded,
        LIBXML_HTML_NOIMPLIED | LIBXML_HTML_NODEFDTD | LIBXML_NOERROR | LIBXML_NOWARNING
    );
    $doc->encoding = 'UTF-8';
    foreach ($doc->getElementsByTagName('script') as $script) {
        if (str_contains($script->textContent, '&#')) {
            $decoded = mb_decode_numericentity(
                $script->textContent,
                [0x80, 0x10FFFF, 0, 0x1FFFFF],
                'UTF-8'
            );
            while ($script->firstChild !== null) {
                $script->removeChild($script->firstChild);
            }
            $script->appendChild($doc->createTextNode($decoded));
        }
    }
    libxml_clear_errors();
    libxml_use_internal_errors($prev);
    if ($doc->documentElement === null) {
        $doc->loadHTML(
            '<html><body></body></html>',
            LIBXML_HTML_NOIMPLIED | LIBXML_HTML_NODEFDTD | LIBXML_NOERROR | LIBXML_NOWARNING
        );
    }
    return $doc->documentElement;
}

public static function parseCrawler(mixed $html): object
{
    $cls = 'Symfony\\Component\\DomCrawler\\Crawler';
    if (is_object($html) && is_a($html, $cls)) {
        return $html;
    }
    return new $cls($html);
}

private static function cloneDomElement(\DOMElement $el): \DOMElement
{
    $newDoc = new \DOMDocument('1.0', 'UTF-8');
    $newDoc->encoding = 'UTF-8';
    /** @var \DOMElement $imported */
    $imported = $newDoc->importNode($el, true);
    $newDoc->appendChild($imported);
    return $imported;
}

/** @return list<\DOMElement> */
public static function xpathQueryDom(\DOMElement $el, string $xpath): array
{
    $doc = $el->ownerDocument;
    if ($doc === null) {
        return [];
    }
    $xp = new \DOMXPath($doc);
    $list = @$xp->query($xpath, $el);
    if (
        ($list === false || $list->length === 0)
        && strtolower($el->tagName) === 'html'
        && $el === $doc->documentElement
    ) {
        $list = @$xp->query($xpath, $doc);
    }
    if (!$list instanceof \DOMNodeList) {
        return [];
    }
    $out = [];
    foreach ($list as $node) {
        if ($node instanceof \DOMElement) {
            $out[] = $node;
        }
    }
    return $out;
}

/** @param list<string> $queries */
public static function xpathFirstOpt(mixed $node, array $queries): ?object
{
    $roots = is_array($node) ? $node : ($node !== null ? [$node] : []);
    foreach ($queries as $q) {
        foreach ($roots as $root) {
            if ($root instanceof \DOMElement) {
                $matches = self::xpathQueryDom($root, $q);
                if (!empty($matches)) {
                    return $matches[0];
                }
            } elseif (is_object($root) && method_exists($root, 'filterXPath')) {
                $sub = $root->filterXPath($q);
                if ($sub->count() > 0) {
                    return $sub->first();
                }
            }
        }
    }
    return null;
}

/** @param list<string> $queries */
public static function xpathFirst(mixed $node, array $queries, string $ctx = ''): object
{
    $found = self::xpathFirstOpt($node, $queries);
    if ($found === null) {
        throw new RtError($ctx, 'selector matched no elements');
    }
    return $found;
}

/**
 * @param list<string> $queries
 * @return list<object>
 */
public static function xpathAll(mixed $node, array $queries): array
{
    $roots = is_array($node) ? $node : ($node !== null ? [$node] : []);
    foreach ($queries as $q) {
        $out = [];
        foreach ($roots as $root) {
            if ($root instanceof \DOMElement) {
                foreach (self::xpathQueryDom($root, $q) as $m) {
                    $out[] = $m;
                }
            } elseif (is_object($root) && method_exists($root, 'filterXPath')) {
                $sub = $root->filterXPath($q);
                if ($sub->count() > 0) {
                    foreach ($sub->each(static fn(object $c): object => $c) as $c) {
                        $out[] = $c;
                    }
                }
            }
        }
        if (!empty($out)) {
            return $out;
        }
    }
    return [];
}

public static function xpathRemove(mixed $node, string $xpath): mixed
{
    if (is_array($node)) {
        return array_values(array_map(static fn(mixed $n): mixed => self::xpathRemove($n, $xpath), $node));
    }
    if ($node instanceof \DOMElement) {
        $cloned = self::cloneDomElement($node);
        foreach (self::xpathQueryDom($cloned, $xpath) as $m) {
            $m->parentNode?->removeChild($m);
        }
        return $cloned;
    }
    if (is_object($node) && method_exists($node, 'filterXPath')) {
        $domNode = $node->getNode(0);
        if (!$domNode instanceof \DOMElement) {
            return $node;
        }
        $cloned = self::cloneDomElement($domNode);
        $cls = get_class($node);
        $crawler = new $cls($cloned);
        $crawler->filterXPath($xpath)->each(static function (object $c): void {
            $n = $c->getNode(0);
            $n?->parentNode?->removeChild($n);
        });
        return $crawler;
    }
    return $node;
}

/** @param list<string> $queries */
public static function cssFirstOpt(mixed $node, array $queries): ?object
{
    $roots = is_array($node) ? $node : ($node !== null ? [$node] : []);
    foreach ($queries as $q) {
        foreach ($roots as $root) {
            if (is_object($root) && method_exists($root, 'filter')) {
                $sub = $root->filter($q);
                if ($sub->count() > 0) {
                    return $sub->first();
                }
            }
        }
    }
    return null;
}

/** @param list<string> $queries */
public static function cssFirst(mixed $node, array $queries, string $ctx = ''): object
{
    $found = self::cssFirstOpt($node, $queries);
    if ($found === null) {
        throw new RtError($ctx, 'selector matched no elements');
    }
    return $found;
}

/**
 * @param list<string> $queries
 * @return list<object>
 */
public static function cssAll(mixed $node, array $queries): array
{
    $roots = is_array($node) ? $node : ($node !== null ? [$node] : []);
    foreach ($queries as $q) {
        $out = [];
        foreach ($roots as $root) {
            if (is_object($root) && method_exists($root, 'filter')) {
                $sub = $root->filter($q);
                if ($sub->count() > 0) {
                    foreach ($sub->each(static fn(object $c): object => $c) as $c) {
                        $out[] = $c;
                    }
                }
            }
        }
        if (!empty($out)) {
            return $out;
        }
    }
    return [];
}

public static function cssRemove(mixed $node, string $query): mixed
{
    if (is_array($node)) {
        return array_values(array_map(static fn(mixed $n): mixed => self::cssRemove($n, $query), $node));
    }
    if (is_object($node) && method_exists($node, 'filter')) {
        $domNode = $node->getNode(0);
        if (!$domNode instanceof \DOMElement) {
            return $node;
        }
        $cloned = self::cloneDomElement($domNode);
        $cls = get_class($node);
        $crawler = new $cls($cloned);
        $crawler->filter($query)->each(static function (object $c): void {
            $n = $c->getNode(0);
            $n?->parentNode?->removeChild($n);
        });
        return $crawler;
    }
    return $node;
}

private static function unwrapDomElement(mixed $node): ?\DOMElement
{
    if ($node instanceof \DOMElement) {
        return $node;
    }
    if (is_object($node) && method_exists($node, 'getNode')) {
        $n = $node->getNode(0);
        if ($n instanceof \DOMElement) {
            return $n;
        }
    }
    return null;
}

public static function text(mixed $node): string
{
    if (is_array($node)) {
        $first = $node[0] ?? null;
        return $first !== null ? self::text($first) : '';
    }
    $el = self::unwrapDomElement($node);
    if ($el !== null) {
        return $el->textContent;
    }
    if (is_object($node) && method_exists($node, 'text')) {
        return (string) $node->text('', false);
    }
    return '';
}

/** @return list<string> */
public static function textAll(mixed $nodes): array
{
    $list = is_array($nodes) ? $nodes : ($nodes !== null ? [$nodes] : []);
    $out = [];
    foreach ($list as $n) {
        $out[] = self::text($n);
    }
    return $out;
}

public static function raw(mixed $node, bool $inner = false): string
{
    if (is_array($node)) {
        $first = $node[0] ?? null;
        return $first !== null ? self::raw($first, $inner) : '';
    }
    $el = self::unwrapDomElement($node);
    if ($el !== null) {
        $doc = $el->ownerDocument;
        if ($doc === null) {
            return '';
        }
        $doc->encoding = 'UTF-8';
        if (!$inner) {
            return (string) $doc->saveHTML($el);
        }
        $parts = [];
        foreach ($el->childNodes as $child) {
            $parts[] = (string) $doc->saveHTML($child);
        }
        return implode('', $parts);
    }
    return '';
}

/** @return list<string> */
public static function rawAll(mixed $nodes, bool $inner = false): array
{
    $list = is_array($nodes) ? $nodes : ($nodes !== null ? [$nodes] : []);
    $out = [];
    foreach ($list as $n) {
        $out[] = self::raw($n, $inner);
    }
    return $out;
}

/** @param list<string> $names */
public static function attr(mixed $node, array $names, string $ctx = ''): string
{
    $target = is_array($node) ? ($node[0] ?? null) : $node;
    $el = self::unwrapDomElement($target);
    if ($el === null) {
        throw new RtError($ctx, 'value has no selected node');
    }
    foreach ($names as $name) {
        if ($el->hasAttribute($name)) {
            return $el->getAttribute($name);
        }
    }
    throw new RtError($ctx, 'attribute not found');
}

/**
 * @param list<string> $names
 * @return list<string>
 */
public static function attrList(mixed $nodes, array $names, string $ctx = ''): array
{
    $list = is_array($nodes) ? $nodes : ($nodes !== null ? [$nodes] : []);
    $out = [];
    foreach ($list as $n) {
        $out[] = self::attr($n, $names, $ctx);
    }
    return $out;
}

/**
 * @param list<string> $names
 * @return list<string>
 */
public static function attrMulti(mixed $nodes, array $names): array
{
    $list = is_array($nodes) ? $nodes : ($nodes !== null ? [$nodes] : []);
    $out = [];
    foreach ($list as $n) {
        $el = self::unwrapDomElement($n);
        if ($el === null) {
            continue;
        }
        foreach ($names as $name) {
            if ($el->hasAttribute($name)) {
                $val = $el->getAttribute($name);
                if ($val !== '') {
                    $out[] = $val;
                }
            }
        }
    }
    return $out;
}

public static function nodeBool(mixed $node): bool
{
    if ($node === null) {
        return false;
    }
    if (is_array($node)) {
        return !empty($node);
    }
    if ($node instanceof \DOMElement) {
        return true;
    }
    if (is_object($node) && method_exists($node, 'count')) {
        return $node->count() > 0;
    }
    return true;
}

public static function predXpath(mixed $node, string $xpath): bool
{
    return self::xpathFirstOpt($node, [$xpath]) !== null;
}

public static function predCss(mixed $node, string $css): bool
{
    return self::cssFirstOpt($node, [$css]) !== null;
}

/** @param list<string> $attrs */
public static function predHasAttr(mixed $node, array $attrs): bool
{
    $el = self::unwrapDomElement(is_array($node) ? ($node[0] ?? null) : $node);
    if ($el === null) {
        return false;
    }
    foreach ($attrs as $a) {
        if ($el->hasAttribute($a)) {
            return true;
        }
    }
    return false;
}

public static function predAttr(mixed $node, string $name): string
{
    $el = self::unwrapDomElement(is_array($node) ? ($node[0] ?? null) : $node);
    if ($el === null || !$el->hasAttribute($name)) {
        return '';
    }
    return $el->getAttribute($name);
}

public static function trim(string $value, string $chars = ''): string
{
    if ($chars === '') {
        return (string) preg_replace('/^\s+|\s+$/u', '', $value);
    }
    $quoted = preg_quote($chars, '/');
    return (string) preg_replace('/^[' . $quoted . ']+|[' . $quoted . ']+$/u', '', $value);
}

public static function ltrim(string $value, string $chars = ''): string
{
    if ($chars === '') {
        return (string) preg_replace('/^\s+/u', '', $value);
    }
    $quoted = preg_quote($chars, '/');
    return (string) preg_replace('/^[' . $quoted . ']+/u', '', $value);
}

public static function rtrim(string $value, string $chars = ''): string
{
    if ($chars === '') {
        return (string) preg_replace('/\s+$/u', '', $value);
    }
    $quoted = preg_quote($chars, '/');
    return (string) preg_replace('/[' . $quoted . ']+$/u', '', $value);
}

public static function normalizeSpace(string $value): string
{
    $trimmed = self::trim($value);
    if ($trimmed === '') {
        return '';
    }
    $parts = preg_split('/\s+/u', $trimmed, -1, PREG_SPLIT_NO_EMPTY);
    return is_array($parts) ? implode(' ', $parts) : $trimmed;
}

public static function rmPrefix(string $value, string $prefix): string
{
    if ($prefix !== '' && str_starts_with($value, $prefix)) {
        return substr($value, strlen($prefix));
    }
    return $value;
}

public static function rmSuffix(string $value, string $suffix): string
{
    if ($suffix !== '' && str_ends_with($value, $suffix)) {
        return substr($value, 0, strlen($value) - strlen($suffix));
    }
    return $value;
}

public static function rmPrefixSuffix(string $value, string $substr): string
{
    return self::rmSuffix(self::rmPrefix($value, $substr), $substr);
}

public static function fmt(string $template, mixed $value): string
{
    $s = (string) $value;
    if (str_contains($template, '{{}}')) {
        return str_replace('{{}}', $s, $template);
    }
    $pos = strpos($template, '{}');
    if ($pos !== false) {
        return substr_replace($template, $s, $pos, 2);
    }
    return $template;
}

/** @param array<string, string> $replacements */
public static function replMap(string $value, array $replacements): string
{
    foreach ($replacements as $old => $new) {
        $value = str_replace((string) $old, (string) $new, $value);
    }
    return $value;
}

public static function lower(string $value): string
{
    return mb_strtolower($value, 'UTF-8');
}

public static function upper(string $value): string
{
    return mb_strtoupper($value, 'UTF-8');
}

public static function unescape(string $value): string
{
    $s = html_entity_decode($value, ENT_QUOTES | ENT_HTML5, 'UTF-8');
    $s = (string) preg_replace_callback(
        '/&#[xX]([0-9a-fA-F]+);/',
        static fn(array $m): string => mb_chr((int) hexdec($m[1]), 'UTF-8') ?: $m[0],
        $s
    );
    $s = (string) preg_replace_callback(
        '/&#(\d+);/',
        static fn(array $m): string => mb_chr((int) $m[1], 'UTF-8') ?: $m[0],
        $s
    );
    $s = (string) preg_replace_callback(
        '/\\\\u([0-9a-fA-F]{4})/',
        static fn(array $m): string => mb_chr((int) hexdec($m[1]), 'UTF-8') ?: $m[0],
        $s
    );
    $s = (string) preg_replace_callback(
        '/\\\\x([0-9a-fA-F]{2})/',
        static fn(array $m): string => mb_chr((int) hexdec($m[1]), 'UTF-8') ?: $m[0],
        $s
    );
    return strtr($s, [
        '\\b' => "\x08",
        '\\f' => "\x0C",
        '\\n' => "\n",
        '\\r' => "\r",
        '\\t' => "\t",
    ]);
}

public static function strLen(string $value): int
{
    return mb_strlen($value, 'UTF-8');
}

public static function strIndex(string $value, int $index, string $ctx = ''): string
{
    $len = mb_strlen($value, 'UTF-8');
    $pos = $index < 0 ? $len + $index : $index;
    if ($pos < 0 || $pos >= $len) {
        throw new RtError($ctx, 'index out of bounds');
    }
    return mb_substr($value, $pos, 1, 'UTF-8');
}

public static function itemAt(array $items, int $index, string $ctx = ''): mixed
{
    $vals = array_values($items);
    $len = count($vals);
    $pos = $index < 0 ? $len + $index : $index;
    if ($pos < 0 || $pos >= $len) {
        throw new RtError($ctx, 'index out of bounds');
    }
    return $vals[$pos];
}

private static function normalizeSliceRange(int $len, int $start, int $end): array
{
    $s = $start < 0 ? max(0, $len + $start) : min($len, $start);
    $e = $end < 0 ? max(0, $len + $end) : min($len, $end);
    if ($s >= $e) {
        return [0, 0];
    }
    return [$s, $e - $s];
}

public static function strSlice(string $value, int $start, int $end): string
{
    $len = mb_strlen($value, 'UTF-8');
    [$offset, $length] = self::normalizeSliceRange($len, $start, $end);
    if ($length <= 0) {
        return '';
    }
    return mb_substr($value, $offset, $length, 'UTF-8');
}

public static function sliceVec(array $items, int $start, int $end): array
{
    $vals = array_values($items);
    [$offset, $length] = self::normalizeSliceRange(count($vals), $start, $end);
    if ($length <= 0) {
        return [];
    }
    return array_slice($vals, $offset, $length);
}

public static function unique(array $items): array
{
    $out = [];
    foreach ($items as $item) {
        if (!in_array($item, $out, true)) {
            $out[] = $item;
        }
    }
    return $out;
}

public static function toInt(string $value, string $ctx = ''): int
{
    $trimmed = trim($value);
    if ($trimmed === '' || preg_match('/^[+-]?\d+$/', $trimmed) !== 1) {
        throw new RtError($ctx, "invalid integer: '$value'");
    }
    return (int) $trimmed;
}

public static function toFloat(string $value, string $ctx = ''): float
{
    $trimmed = trim($value);
    if ($trimmed === '' || !is_numeric($trimmed)) {
        throw new RtError($ctx, "invalid float: '$value'");
    }
    return (float) $trimmed;
}

public static function assertTrue(bool $cond, string $msg = ''): void
{
    if (!$cond) {
        throw new RtAssertionError($msg !== '' ? $msg : 'parsmith assertion failed');
    }
}

public static function reSearch(string $pattern, string $value, string $msg = ''): string
{
    if (@preg_match($pattern, $value, $m) !== 1) {
        throw new RtRegexError($msg !== '' ? $msg : "parsmith re-match failed: $pattern");
    }
    return $m[1] ?? $m[0];
}

/** @return list<string> */
public static function reAll(string $pattern, string $value): array
{
    $count = @preg_match_all($pattern, $value, $m);
    if ($count === false || $count === 0) {
        return [];
    }
    if (isset($m[1]) && is_array($m[1])) {
        return array_values(array_map('strval', $m[1]));
    }
    return array_values(array_map('strval', $m[0] ?? []));
}

public static function reSub(string $pattern, string $repl, string $value): string
{
    $res = @preg_replace($pattern, $repl, $value);
    return is_string($res) ? $res : $value;
}

/** @return list<string> */
public static function reSplit(string $pattern, string $value): array
{
    $res = @preg_split($pattern, $value);
    return is_array($res) ? array_values($res) : [$value];
}

public static function reMatch(string $pattern, string $value): bool
{
    return @preg_match($pattern, $value) === 1;
}

/** @param list<string> $values */
public static function reAny(string $pattern, array $values): bool
{
    foreach ($values as $v) {
        if (self::reMatch($pattern, (string) $v)) {
            return true;
        }
    }
    return false;
}

/** @param list<string> $values */
public static function reAllMatch(string $pattern, array $values): bool
{
    foreach ($values as $v) {
        if (!self::reMatch($pattern, (string) $v)) {
            return false;
        }
    }
    return true;
}

public static function typeName(mixed $val): string
{
    if ($val === null) {
        return 'NoneType';
    }
    if (is_bool($val)) {
        return 'bool';
    }
    if (is_int($val)) {
        return 'int';
    }
    if (is_float($val)) {
        return 'float';
    }
    if (is_string($val)) {
        return 'str';
    }
    if ($val instanceof \stdClass) {
        return 'dict';
    }
    if (is_array($val)) {
        return array_is_list($val) ? 'list' : 'dict';
    }
    return get_debug_type($val);
}

public static function isJsonObject(mixed $val): bool
{
    return $val instanceof \stdClass || (is_array($val) && !empty($val) && !array_is_list($val));
}

/** @return array<string, mixed> */
public static function expectDict(mixed $data, string $target = ''): array
{
    if ($data instanceof \stdClass) {
        return get_object_vars($data);
    }
    if (is_array($data) && !empty($data) && !array_is_list($data)) {
        return $data;
    }
    $prefix = $target !== '' ? "$target: " : '';
    throw new RtJsonSchemaError($prefix . 'Expected dict, got ' . self::typeName($data));
}

/** @return list<mixed> */
public static function expectList(mixed $data, string $target = ''): array
{
    if (is_array($data) && array_is_list($data)) {
        return $data;
    }
    $prefix = $target !== '' ? "$target: " : '';
    throw new RtJsonSchemaError($prefix . 'Expected list, got ' . self::typeName($data));
}

public static function expectStr(mixed $data, string $target = ''): string
{
    if (is_string($data)) {
        return $data;
    }
    $prefix = $target !== '' ? "$target: " : '';
    throw new RtJsonSchemaError($prefix . 'Expected str, got ' . self::typeName($data));
}

public static function expectInt(mixed $data, string $target = ''): int
{
    if (is_int($data)) {
        return $data;
    }
    $prefix = $target !== '' ? "$target: " : '';
    throw new RtJsonSchemaError($prefix . 'Expected int, got ' . self::typeName($data));
}

public static function expectFloat(mixed $data, string $target = ''): float
{
    if (is_int($data) || is_float($data)) {
        return (float) $data;
    }
    $prefix = $target !== '' ? "$target: " : '';
    throw new RtJsonSchemaError($prefix . 'Expected float, got ' . self::typeName($data));
}

public static function expectBool(mixed $data, string $target = ''): bool
{
    if (is_bool($data)) {
        return $data;
    }
    $prefix = $target !== '' ? "$target: " : '';
    throw new RtJsonSchemaError($prefix . 'Expected bool, got ' . self::typeName($data));
}

/**
 * @param array<string, mixed> $dict
 * @return array<string, mixed>
 */
public static function mapDict(array $dict, callable $fn): array
{
    $out = [];
    foreach ($dict as $k => $v) {
        $out[(string) $k] = $fn($v);
    }
    return $out;
}

public static function serializeDict(mixed $val): mixed
{
    if ($val === null) {
        return null;
    }
    if ($val instanceof \stdClass) {
        return $val;
    }
    if (is_array($val)) {
        $obj = new \stdClass();
        foreach ($val as $k => $v) {
            $obj->{(string) $k} = (is_array($v) && !array_is_list($v)) ? self::serializeDict($v) : $v;
        }
        return $obj;
    }
    return $val;
}

public static function toPlainPhp(mixed $val): mixed
{
    if ($val instanceof \stdClass) {
        $vars = get_object_vars($val);
        if (empty($vars)) {
            return new \stdClass();
        }
        $out = [];
        foreach ($vars as $k => $v) {
            $out[(string) $k] = self::toPlainPhp($v);
        }
        return $out;
    }
    if (is_array($val)) {
        $out = [];
        foreach ($val as $k => $v) {
            $out[$k] = self::toPlainPhp($v);
        }
        return $out;
    }
    return $val;
}

public static function parseJson(string $raw, string $path = '', string $target = ''): mixed
{
    try {
        $decoded = json_decode($raw, false, 512, JSON_THROW_ON_ERROR);
    } catch (\Throwable $e) {
        throw new RtJsonError($target, $e->getMessage(), $e);
    }
    if ($path !== '') {
        return self::resolveDotpath($decoded, $path, false, $target);
    }
    return $decoded;
}

public static function jsonGet(mixed $data, string $key): mixed
{
    if ($data instanceof \stdClass) {
        return property_exists($data, $key) ? $data->{$key} : null;
    }
    if (is_array($data)) {
        return $data[$key] ?? null;
    }
    return null;
}

public static function jsonHasKey(mixed $data, string $key): bool
{
    if ($data instanceof \stdClass) {
        return property_exists($data, $key);
    }
    if (is_array($data)) {
        return array_key_exists($key, $data);
    }
    return false;
}

public static function resolveDotpath(
    mixed $data,
    string $path,
    bool $isOptional = false,
    string $target = '',
): mixed {
    if ($path === '') {
        return $data;
    }
    $prefix = $target !== '' ? "$target: " : '';
    $rawSegs = [];
    $buf = '';
    $n = strlen($path);
    $i = 0;
    while ($i < $n) {
        $ch = $path[$i];
        if ($ch === '\\' && $i + 1 < $n && ($path[$i + 1] === '.' || $path[$i + 1] === '\\' || $path[$i + 1] === '#')) {
            $buf .= $ch . $path[$i + 1];
            $i += 2;
        } elseif ($ch === '.') {
            $rawSegs[] = $buf;
            $buf = '';
            $i += 1;
        } else {
            $buf .= $ch;
            $i += 1;
        }
    }
    $rawSegs[] = $buf;

    $current = $data;
    $segCount = count($rawSegs);
    for ($idxSeg = 0; $idxSeg < $segCount; $idxSeg++) {
        $rawSeg = $rawSegs[$idxSeg];
        if ($rawSeg === '#') {
            if ($current === null) {
                if ($isOptional) {
                    return null;
                }
                throw new RtJsonPathError($prefix . "Cannot traverse segment '#' on null object in path '$path'");
            }
            if (!is_array($current) || !array_is_list($current)) {
                if ($isOptional) {
                    return null;
                }
                throw new RtJsonPathError(
                    $prefix . "Expected list for '#' in path '$path', got " . self::typeName($current)
                );
            }
            if ($idxSeg === $segCount - 1) {
                return count($current);
            }
            $subpath = implode('.', array_slice($rawSegs, $idxSeg + 1));
            $out = [];
            foreach ($current as $item) {
                $v = self::resolveDotpath($item, $subpath, true, $target);
                if ($v !== null) {
                    $out[] = $v;
                }
            }
            return $out;
        }

        $hasEscape = str_contains($rawSeg, '\\');
        if ($hasEscape) {
            $seg = '';
            $m = strlen($rawSeg);
            $j = 0;
            while ($j < $m) {
                if ($rawSeg[$j] === '\\' && $j + 1 < $m && ($rawSeg[$j + 1] === '.' || $rawSeg[$j + 1] === '\\' || $rawSeg[$j + 1] === '#')) {
                    $seg .= $rawSeg[$j + 1];
                    $j += 2;
                } else {
                    $seg .= $rawSeg[$j];
                    $j += 1;
                }
            }
        } else {
            $seg = $rawSeg;
        }

        if ($current === null) {
            if ($isOptional) {
                return null;
            }
            throw new RtJsonPathError($prefix . "Cannot traverse segment '$seg' on null object in path '$path'");
        }

        if (!$hasEscape && ctype_digit($seg)) {
            $idx = (int) $seg;
            if (!is_array($current) || !array_is_list($current)) {
                if ($isOptional) {
                    return null;
                }
                throw new RtJsonPathError(
                    $prefix . "Expected list for index '$idx' in path '$path', got " . self::typeName($current)
                );
            }
            if ($idx < 0 || $idx >= count($current)) {
                if ($isOptional) {
                    return null;
                }
                throw new RtJsonPathError(
                    $prefix . "Index $idx out of bounds (len=" . count($current) . ") in path '$path'"
                );
            }
            $current = $current[$idx];
        } elseif (self::isJsonObject($current) || (is_array($current) && !array_is_list($current))) {
            if (!self::jsonHasKey($current, $seg)) {
                if ($isOptional) {
                    return null;
                }
                throw new RtJsonPathError($prefix . "Missing key '$seg' in path '$path'");
            }
            $current = self::jsonGet($current, $seg);
        } else {
            if ($isOptional) {
                return null;
            }
            throw new RtJsonPathError(
                $prefix . "Cannot access key '$seg' on non-dict " . self::typeName($current) . " in path '$path'"
            );
        }
    }

    return $current;
}

public static function jsonPath(mixed $data, string $path, string $target = ''): mixed
{
    return self::resolveDotpath($data, $path, false, $target);
}

public static function jsonReq(mixed $data, string $key, string $target = ''): mixed
{
    $dotPos = strrpos($target, '.');
    $fname = $dotPos !== false ? substr($target, $dotPos + 1) : $key;
    $qual = ($target === '' || $dotPos !== false) ? $target : ($target . '.' . $fname);
    $prefix = $qual !== '' ? "$qual: " : '';
    if (!self::jsonHasKey($data, $key)) {
        throw new RtJsonFieldMissingError(
            $prefix . "Required JSON field '$key' (mapped to '$fname') is missing"
        );
    }
    $val = self::jsonGet($data, $key);
    if ($val === null) {
        throw new RtJsonFieldMissingError(
            $prefix . "Field '$key' is null, but '$fname' is not nullable"
        );
    }
    return $val;
}

public static function jsonReqPath(mixed $data, string $path, string $target = ''): mixed
{
    $val = self::resolveDotpath($data, $path, false, $target);
    if ($val === null) {
        $prefix = $target !== '' ? "$target: " : '';
        $dotPos = strrpos($target, '.');
        $fname = $dotPos !== false ? substr($target, $dotPos + 1) : $path;
        throw new RtJsonFieldMissingError(
            $prefix . "Field '$path' is null, but '$fname' is not nullable"
        );
    }
    return $val;
}

public static function hasErrorKey(mixed $body, string $path): bool
{
    if (!self::isJsonObject($body)) {
        return false;
    }
    if (!str_contains($path, '.') && !str_contains($path, '\\')) {
        return self::jsonHasKey($body, $path);
    }
    return self::resolveDotpath($body, $path, true) !== null;
}"#;

/// HTTP & REST helper methods inside `final class Rt` (`viaCurl`, `viaStream`, `viaGuzzle`,
/// `buildRequest`, `buildUrl`, `encodeRequestBody`, `fetchHtml`, `restCall`, `dispatchResponse`).
pub const PHP_RT_HTTP_METHODS: &str = r#"public static function viaCurl(?float $timeout = null): SyncTransport
{
    return new CurlTransport($timeout);
}

public static function viaStream(?float $timeout = null): SyncTransport
{
    return new StreamTransport($timeout);
}

public static function viaGuzzle(object $client): SyncTransport
{
    return new GuzzleTransport($client);
}

/**
 * @param array<string, mixed> $headers
 * @param array<string, mixed> $params
 * @param array<string, mixed> $cookies
 * @param array<int|string, mixed> $extra
 */
public static function buildRequest(
    string $method,
    string $url,
    array $headers = [],
    array $params = [],
    array $cookies = [],
    mixed $body = null,
    string $bodyKind = 'empty',
    array $extra = [],
): HttpRequest {
    if (isset($extra[0]) && is_array($extra[0]) && count($extra) === 1) {
        $extra = $extra[0];
    } elseif (isset($extra['extra']) && is_array($extra['extra']) && count($extra) === 1) {
        $extra = $extra['extra'];
    }
    $cleanHeaders = [];
    foreach ($headers as $k => $v) {
        if ($v !== null) {
            $cleanHeaders[(string) $k] = (string) $v;
        }
    }
    $cleanParams = [];
    foreach ($params as $k => $v) {
        if ($v === null) {
            continue;
        }
        if (is_array($v)) {
            $items = [];
            foreach ($v as $x) {
                if ($x !== null) {
                    $items[] = is_bool($x) ? ($x ? 'true' : 'false') : (string) $x;
                }
            }
            $cleanParams[(string) $k] = $items;
        } else {
            $cleanParams[(string) $k] = is_bool($v) ? ($v ? 'true' : 'false') : (string) $v;
        }
    }
    $cleanCookies = [];
    foreach ($cookies as $k => $v) {
        if ($v !== null) {
            $cleanCookies[(string) $k] = (string) $v;
        }
    }
    $cleanBody = $body;
    if ($bodyKind === 'form' && is_array($body)) {
        $form = [];
        foreach ($body as $k => $v) {
            if ($v !== null) {
                $form[(string) $k] = is_bool($v) ? ($v ? 'true' : 'false') : (string) $v;
            }
        }
        $cleanBody = $form;
    }
    $extraDict = [];
    foreach ($extra as $k => $v) {
        if ($k === 'headers' && is_array($v)) {
            foreach ($v as $hk => $hv) {
                if ($hv !== null) {
                    $cleanHeaders[(string) $hk] = (string) $hv;
                }
            }
        } elseif ($k === 'params' && is_array($v)) {
            foreach ($v as $pk => $pv) {
                if ($pv === null) {
                    continue;
                }
                if (is_array($pv)) {
                    $items = [];
                    foreach ($pv as $x) {
                        if ($x !== null) {
                            $items[] = is_bool($x) ? ($x ? 'true' : 'false') : (string) $x;
                        }
                    }
                    $cleanParams[(string) $pk] = $items;
                } else {
                    $cleanParams[(string) $pk] = is_bool($pv) ? ($pv ? 'true' : 'false') : (string) $pv;
                }
            }
        } elseif ($k === 'cookies' && is_array($v)) {
            foreach ($v as $ck => $cv) {
                if ($cv !== null) {
                    $cleanCookies[(string) $ck] = (string) $cv;
                }
            }
        } elseif ($v !== null) {
            $extraDict[(string) $k] = $v;
        }
    }
    return new HttpRequest(
        method: $method,
        url: $url,
        headers: $cleanHeaders,
        params: $cleanParams,
        cookies: $cleanCookies,
        body: $cleanBody,
        bodyKind: $bodyKind,
        extra: $extraDict,
    );
}

/** @param array<string, string|list<string>> $params */
public static function buildUrl(string $url, array $params): string
{
    if (empty($params)) {
        return $url;
    }
    $parts = [];
    foreach ($params as $k => $v) {
        if (is_array($v)) {
            foreach ($v as $item) {
                $parts[] = rawurlencode((string) $k) . '=' . rawurlencode((string) $item);
            }
        } else {
            $parts[] = rawurlencode((string) $k) . '=' . rawurlencode((string) $v);
        }
    }
    if (empty($parts)) {
        return $url;
    }
    $sep = str_contains($url, '?') ? '&' : '?';
    return $url . $sep . implode('&', $parts);
}

/** @param array<string, string> $headers */
public static function encodeRequestBody(HttpRequest $req, array &$headers): ?string
{
    if ($req->bodyKind === 'json' && $req->body !== null) {
        $hasCt = false;
        foreach (array_keys($headers) as $hk) {
            if (strtolower($hk) === 'content-type') {
                $hasCt = true;
                break;
            }
        }
        if (!$hasCt) {
            $headers['Content-Type'] = 'application/json';
        }
        return json_encode($req->body, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
    }
    if ($req->bodyKind === 'form' && $req->body !== null) {
        $hasCt = false;
        foreach (array_keys($headers) as $hk) {
            if (strtolower($hk) === 'content-type') {
                $hasCt = true;
                break;
            }
        }
        if (!$hasCt) {
            $headers['Content-Type'] = 'application/x-www-form-urlencoded';
        }
        if (is_array($req->body)) {
            return http_build_query($req->body, '', '&', PHP_QUERY_RFC3986);
        }
        return (string) $req->body;
    }
    if ($req->bodyKind === 'raw' && $req->body !== null) {
        return (string) $req->body;
    }
    return null;
}

public static function fetchHtml(
    SyncTransport $transport,
    HttpRequest $req,
    string $responsePath = '',
    string $responseJoin = '',
): string {
    $resp = $transport->send($req);
    if ($responsePath !== '') {
        $data = json_decode($resp->text, false, 512, JSON_THROW_ON_ERROR);
        $extracted = self::resolveDotpath($data, $responsePath, false);
        if ($responseJoin !== '' && is_array($extracted)) {
            return implode($responseJoin, array_map('strval', $extracted));
        }
        return is_string($extracted) ? $extracted : (string) json_encode($extracted);
    }
    return $resp->text;
}

/**
 * @param list<ErrMatcher> $matchers
 * @return Ok|UnknownErr|TransportErr|DecodeErr|object
 */
public static function restCall(
    SyncTransport $transport,
    HttpRequest $req,
    array $matchers,
    ?callable $valueFn = null,
): object {
    try {
        $resp = $transport->send($req);
    } catch (TransportError $e) {
        return new TransportErr(status: 0, headers: [], cause: $e->getMessage(), exception: $e->cause);
    } catch (\Throwable $e) {
        return new TransportErr(status: 0, headers: [], cause: $e->getMessage(), exception: $e);
    }
    return self::dispatchResponse($matchers, $resp, $valueFn);
}

/**
 * @param list<ErrMatcher> $matchers
 * @return Ok|UnknownErr|TransportErr|DecodeErr|object
 */
public static function dispatchResponse(
    array $matchers,
    RawResponse $resp,
    ?callable $valueFn = null,
): object {
    $status = $resp->status;
    $headers = $resp->headers;
    $rawText = $resp->text;
    $parsedBody = null;
    $jsonErr = null;
    try {
        $parsedBody = json_decode($rawText, false, 512, JSON_THROW_ON_ERROR);
    } catch (\Throwable $e) {
        $jsonErr = $e;
    }

    $lastDecodeErr = null;
    $matchedErrorRule = false;
    foreach ($matchers as $matcher) {
        if ($status !== $matcher->status) {
            continue;
        }
        if ($matcher->check !== null) {
            if ($jsonErr !== null || !self::isJsonObject($parsedBody) || !($matcher->check)($parsedBody)) {
                continue;
            }
        }
        $matchedErrorRule = true;
        if ($jsonErr !== null) {
            $lastDecodeErr = $jsonErr;
            continue;
        }
        try {
            return ($matcher->factory)($headers, $parsedBody);
        } catch (\Throwable $e) {
            $lastDecodeErr = $e;
        }
    }

    if ($status >= 200 && $status < 300 && !$matchedErrorRule) {
        if ($valueFn === null) {
            return new Ok(status: $status, headers: $headers, value: null);
        }
        if ($jsonErr !== null) {
            return new DecodeErr(
                status: $status,
                headers: $headers,
                body: $rawText,
                cause: $jsonErr->getMessage(),
                exception: $jsonErr,
            );
        }
        try {
            $val = $valueFn($parsedBody);
            return new Ok(status: $status, headers: $headers, value: $val);
        } catch (\Throwable $e) {
            return new DecodeErr(
                status: $status,
                headers: $headers,
                body: self::toPlainPhp($parsedBody),
                cause: $e->getMessage(),
                exception: $e,
            );
        }
    }

    if ($matchedErrorRule && $lastDecodeErr !== null) {
        return new DecodeErr(
            status: $status,
            headers: $headers,
            body: $jsonErr === null ? self::toPlainPhp($parsedBody) : $rawText,
            cause: $lastDecodeErr->getMessage(),
            exception: $lastDecodeErr,
        );
    }

    $fallbackBody = $jsonErr === null ? self::toPlainPhp($parsedBody) : ($rawText !== '' ? $rawText : null);
    return new UnknownErr(status: $status, headers: $headers, body: $fallbackBody);
}

/** @param list<mixed> $args */
public static function __callStatic(string $name, array $args): mixed
{
    if (str_starts_with($name, 'project') && strlen($name) > 7) {
        $schema = substr($name, 7);
        $ns = __NAMESPACE__ !== '' ? (__NAMESPACE__ . '\\') : '';
        $cls = $ns . $schema . 'Json';
        if (class_exists($cls) && method_exists($cls, $name)) {
            return $cls::$name(...$args);
        }
    }
    throw new \BadMethodCallException("Undefined static method Rt::$name");
}"#;

fn ensure_public_static_helper(code: &str) -> String {
    let trimmed = code.trim_start();
    if trimmed.starts_with("public static function ")
        || trimmed.starts_with("private static function ")
    {
        code.to_owned()
    } else if trimmed.starts_with("static function ") {
        let prefix_len = code.len() - trimmed.len();
        format!("{}public {trimmed}", &code[..prefix_len])
    } else if trimmed.starts_with("public function ") {
        let prefix_len = code.len() - trimmed.len();
        let rest = trimmed.strip_prefix("public function ").unwrap_or(trimmed);
        format!("{}public static function {rest}", &code[..prefix_len])
    } else if trimmed.starts_with("function ") {
        let prefix_len = code.len() - trimmed.len();
        format!("{}public static {trimmed}", &code[..prefix_len])
    } else {
        code.to_owned()
    }
}

/// Writes the PHP runtime classes and `final class Rt` to `writer`.
pub fn write_php_runtime_body(
    module: &Module,
    builder: &ModuleBuilder,
    options: &CodegenOptions,
    writer: &mut CodeWriter,
) {
    let include_http = module_uses_http(module) || options.separate_runtime;

    writer.write_block(PHP_ERROR_CLASSES.trim_end());
    writer.blankline();

    if include_http {
        writer.write_block(PHP_HTTP_CLASSES.trim_end());
        writer.blankline();
    }

    writer.writeln("final class Rt");
    writer.writeln("{");
    writer.indented(|writer| {
        writer.write_block(PHP_RT_METHODS.trim_end());
        writer.blankline();
        writer.write_block(PHP_RT_HTTP_METHODS.trim_end());

        let emitter = PhpEmitter::<PhpDom>::new(options.clone());
        for json_def in &module.json_defs {
            writer.blankline();
            let schema_name = php_pascal(&json_def.name);
            let model_name = format!("{schema_name}Json");
            let method_name = php_json_projector_method(&json_def.name);
            emitter.emit_json_projector_method_on_class(
                json_def,
                &schema_name,
                &model_name,
                &method_name,
                module,
                writer,
            );
        }

        for def in builder.runtime_defs().values() {
            writer.blankline();
            let static_code = ensure_public_static_helper(&def.code);
            writer.write_block(&static_code);
        }
    });
    writer.writeln("}");
    writer.blankline();

    // Also emit top-level wrappers for extension runtime helpers so both `helper_fn($x)` and `Rt::helper_fn($x)` work
    for def in builder.runtime_defs().values() {
        let name = &def.name;
        wln!(
            writer,
            "if (!function_exists(__NAMESPACE__ . '\\\\{name}')) {{"
        );
        writer.indented(|writer| {
            wln!(writer, "function {name}(mixed ...$args): mixed");
            writer.writeln("{");
            writer.indented(|writer| {
                wln!(writer, "return Rt::{name}(...$args);");
            });
            writer.writeln("}");
        });
        writer.writeln("}");
        writer.blankline();
    }
}

/// Generates the standalone PHP shared runtime file (`<runtime_name>.php`) for `-R / --separate-runtime`.
pub fn emit_php_runtime_module(
    module: &Module,
    builder: &ModuleBuilder,
    options: &CodegenOptions,
) -> String {
    let mut writer = CodeWriter::with_indent("    ");
    writer.writeln("<?php");
    writer.writeln("declare(strict_types=1);");
    writer.writeln("// Auto-generated runtime helpers by parsmith. DO NOT EDIT.");
    writer.blankline();

    if let Some(ns) = normalize_php_namespace(&options.package)
        && !ns.is_empty()
    {
        wln!(writer, "namespace {ns};");
        writer.blankline();
    }

    if options.php_html_lib == PhpHtmlLib::DomCrawler {
        writer.writeln("use Symfony\\Component\\DomCrawler\\Crawler;");
        writer.blankline();
    }

    for imp in builder.runtime_imports() {
        let trimmed = imp.trim();
        if !trimmed.is_empty() {
            if trimmed.ends_with(';') {
                writer.write_block(trimmed);
            } else {
                wln!(writer, "{trimmed};");
            }
        }
    }
    if !builder.runtime_imports().is_empty() {
        writer.blankline();
    }

    write_php_runtime_body(module, builder, options, &mut writer);
    writer.finish()
}
