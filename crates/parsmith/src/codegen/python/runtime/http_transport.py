@dataclass(slots=True, frozen=True)
class HttpRequest:
    method: str
    url: str
    headers: dict[str, str] = field(default_factory=dict)
    params: dict[str, str | list[str]] = field(default_factory=dict)
    cookies: dict[str, str] = field(default_factory=dict)
    body: Any = None
    body_kind: Literal["empty", "json", "form", "raw"] = "empty"
    extra: dict[str, Any] = field(default_factory=dict)


@dataclass(slots=True, frozen=True)
class RawResponse:
    status: int
    headers: dict[str, str]
    text: str


class TransportError(Exception):
    def __init__(self, cause: BaseException) -> None:
        super().__init__(str(cause))
        self.cause = cause


class SyncTransport(Protocol):
    def send(self, req: HttpRequest) -> RawResponse: ...


class AsyncTransport(Protocol):
    async def send(self, req: HttpRequest) -> RawResponse: ...


class HttpxTransport:
    def __init__(self, client: Any = None) -> None:
        self._client = client

    def send(self, req: HttpRequest) -> RawResponse:
        httpx: Any = importlib.import_module("httpx")
        kw: dict[str, Any] = {}
        if req.headers:
            kw["headers"] = req.headers
        if req.params:
            kw["params"] = req.params
        if req.cookies:
            kw["cookies"] = req.cookies
        if req.body_kind == "json":
            kw["json"] = req.body
        elif req.body_kind == "form":
            kw["data"] = req.body
        elif req.body_kind == "raw" and req.body is not None:
            kw["content"] = req.body
        kw.update(req.extra)
        try:
            if self._client is not None:
                resp = self._client.request(req.method, req.url, **kw)
            else:
                with httpx.Client() as client:
                    resp = client.request(req.method, req.url, **kw)
            return RawResponse(
                status=int(resp.status_code),
                headers={str(k).lower(): str(v) for k, v in resp.headers.items()},
                text=str(resp.text),
            )
        except httpx.HTTPError as e:
            raise TransportError(e) from e


class AsyncHttpxTransport:
    def __init__(self, client: Any = None) -> None:
        self._client = client

    async def send(self, req: HttpRequest) -> RawResponse:
        httpx: Any = importlib.import_module("httpx")
        kw: dict[str, Any] = {}
        if req.headers:
            kw["headers"] = req.headers
        if req.params:
            kw["params"] = req.params
        if req.cookies:
            kw["cookies"] = req.cookies
        if req.body_kind == "json":
            kw["json"] = req.body
        elif req.body_kind == "form":
            kw["data"] = req.body
        elif req.body_kind == "raw" and req.body is not None:
            kw["content"] = req.body
        kw.update(req.extra)
        try:
            if self._client is not None:
                resp = await self._client.request(req.method, req.url, **kw)
            else:
                async with httpx.AsyncClient() as client:
                    resp = await client.request(req.method, req.url, **kw)
            return RawResponse(
                status=int(resp.status_code),
                headers={str(k).lower(): str(v) for k, v in resp.headers.items()},
                text=str(resp.text),
            )
        except httpx.HTTPError as e:
            raise TransportError(e) from e


class RequestsTransport:
    def __init__(self, session: Any = None) -> None:
        self._session = session

    def send(self, req: HttpRequest) -> RawResponse:
        requests: Any = importlib.import_module("requests")
        kw: dict[str, Any] = {}
        if req.headers:
            kw["headers"] = req.headers
        if req.params:
            kw["params"] = req.params
        if req.cookies:
            kw["cookies"] = req.cookies
        if req.body_kind == "json":
            kw["json"] = req.body
        elif req.body_kind in ("form", "raw") and req.body is not None:
            kw["data"] = req.body
        kw.update(req.extra)
        try:
            if self._session is not None:
                resp = self._session.request(req.method, req.url, **kw)
            else:
                with requests.Session() as session:
                    resp = session.request(req.method, req.url, **kw)
            return RawResponse(
                status=int(resp.status_code),
                headers={str(k).lower(): str(v) for k, v in resp.headers.items()},
                text=str(resp.text),
            )
        except requests.RequestException as e:
            raise TransportError(e) from e


class AiohttpTransport:
    def __init__(self, session: Any = None) -> None:
        self._session = session

    async def send(self, req: HttpRequest) -> RawResponse:
        aiohttp: Any = importlib.import_module("aiohttp")
        kw: dict[str, Any] = {}
        if req.headers:
            kw["headers"] = req.headers
        if req.params:
            kw["params"] = req.params
        if req.cookies:
            kw["cookies"] = req.cookies
        if req.body_kind == "json":
            kw["json"] = req.body
        elif req.body_kind in ("form", "raw") and req.body is not None:
            kw["data"] = req.body
        kw.update(req.extra)
        try:
            if self._session is not None:
                async with self._session.request(req.method, req.url, **kw) as resp:
                    text = await resp.text()
                    return RawResponse(
                        status=int(resp.status),
                        headers={str(k).lower(): str(v) for k, v in resp.headers.items()},
                        text=str(text),
                    )
            else:
                async with aiohttp.ClientSession() as session:
                    async with session.request(req.method, req.url, **kw) as resp:
                        text = await resp.text()
                        return RawResponse(
                            status=int(resp.status),
                            headers={str(k).lower(): str(v) for k, v in resp.headers.items()},
                            text=str(text),
                        )
        except aiohttp.ClientError as e:
            raise TransportError(e) from e


class UrllibTransport:
    def __init__(self, timeout: float | None = None) -> None:
        self._timeout = timeout

    def send(self, req: HttpRequest) -> RawResponse:
        url = req.url
        if req.params:
            pairs: list[tuple[str, str]] = []
            for k, v in req.params.items():
                if isinstance(v, list):
                    for item in v:
                        pairs.append((k, str(item)))
                elif v is not None:
                    pairs.append((k, str(v)))
            if pairs:
                qs = urllib.parse.urlencode(pairs)
                sep = "&" if "?" in url else "?"
                url = f"{url}{sep}{qs}"
        headers: dict[str, str] = dict(req.headers)
        if req.cookies:
            cookie_str = "; ".join(f"{k}={v}" for k, v in req.cookies.items())
            if "Cookie" in headers:
                headers["Cookie"] = f"{headers['Cookie']}; {cookie_str}"
            else:
                headers["Cookie"] = cookie_str
        data_bytes: bytes | None = None
        if req.body_kind == "json" and req.body is not None:
            data_bytes = json.dumps(req.body).encode("utf-8")
            if not any(k.lower() == "content-type" for k in headers):
                headers["Content-Type"] = "application/json"
        elif req.body_kind == "form" and req.body is not None:
            if isinstance(req.body, dict):
                data_bytes = urllib.parse.urlencode(req.body, doseq=True).encode("utf-8")
            else:
                data_bytes = str(req.body).encode("utf-8")
            if not any(k.lower() == "content-type" for k in headers):
                headers["Content-Type"] = "application/x-www-form-urlencoded"
        elif req.body_kind == "raw" and req.body is not None:
            if isinstance(req.body, bytes):
                data_bytes = req.body
            else:
                data_bytes = str(req.body).encode("utf-8")
        ureq = urllib.request.Request(url, data=data_bytes, headers=headers, method=req.method)
        timeout = req.extra.get("timeout", self._timeout)
        try:
            with urllib.request.urlopen(ureq, timeout=timeout) as resp:
                raw_bytes: bytes = resp.read()
                resp_headers = {str(k).lower(): str(v) for k, v in resp.headers.items()}
                return RawResponse(
                    status=int(resp.status),
                    headers=resp_headers,
                    text=raw_bytes.decode("utf-8", errors="replace"),
                )
        except urllib.error.HTTPError as e:
            err_bytes = e.read() if hasattr(e, "read") else b""
            err_headers = (
                {str(k).lower(): str(v) for k, v in e.headers.items()}
                if e.headers is not None
                else {}
            )
            return RawResponse(
                status=int(e.code),
                headers=err_headers,
                text=err_bytes.decode("utf-8", errors="replace"),
            )
        except urllib.error.URLError as e:
            raise TransportError(e) from e


def via_httpx(client: Any = None) -> HttpxTransport:
    return HttpxTransport(client)


def via_httpx_async(client: Any = None) -> AsyncHttpxTransport:
    return AsyncHttpxTransport(client)


def via_requests(session: Any = None) -> RequestsTransport:
    return RequestsTransport(session)


def via_aiohttp(session: Any = None) -> AiohttpTransport:
    return AiohttpTransport(session)


def via_urllib(timeout: float | None = None) -> UrllibTransport:
    return UrllibTransport(timeout)


def rt_build_request(
    method: str,
    url: str,
    *,
    headers: Mapping[str, Any] | None = None,
    params: Mapping[str, Any] | None = None,
    cookies: Mapping[str, Any] | None = None,
    body: Any = None,
    body_kind: Literal["empty", "json", "form", "raw"] = "empty",
    extra: Mapping[str, Any] | None = None,
) -> HttpRequest:
    clean_headers: dict[str, str] = {}
    if headers:
        for k, v in headers.items():
            if v is not None:
                clean_headers[str(k)] = str(v)
    clean_params: dict[str, str | list[str]] = {}
    if params:
        for k, v in params.items():
            if v is None:
                continue
            if isinstance(v, list):
                clean_params[str(k)] = [str(x) for x in v if x is not None]
            else:
                clean_params[str(k)] = str(v)
    clean_cookies: dict[str, str] = {}
    if cookies:
        for k, v in cookies.items():
            if v is not None:
                clean_cookies[str(k)] = str(v)
    clean_body: Any = body
    if body_kind == "form" and isinstance(body, dict):
        clean_body = {str(k): str(v) for k, v in body.items() if v is not None}
    extra_dict: dict[str, Any] = {}
    if extra:
        for k, v in extra.items():
            if k == "headers" and isinstance(v, Mapping):
                for hk, hv in v.items():
                    if hv is not None:
                        clean_headers[str(hk)] = str(hv)
            elif k == "params" and isinstance(v, Mapping):
                for pk, pv in v.items():
                    if pv is None:
                        continue
                    if isinstance(pv, list):
                        clean_params[str(pk)] = [str(x) for x in pv if x is not None]
                    else:
                        clean_params[str(pk)] = str(pv)
            elif k == "cookies" and isinstance(v, Mapping):
                for ck, cv in v.items():
                    if cv is not None:
                        clean_cookies[str(ck)] = str(cv)
            elif v is not None:
                extra_dict[str(k)] = v
    return HttpRequest(
        method=method,
        url=url,
        headers=clean_headers,
        params=clean_params,
        cookies=clean_cookies,
        body=clean_body,
        body_kind=body_kind,
        extra=extra_dict,
    )
