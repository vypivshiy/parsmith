_T = TypeVar("_T")


@dataclass(slots=True, frozen=True)
class Ok(Generic[_T]):
    status: int
    headers: dict[str, str]
    value: _T
    ok: Literal[True] = True

    @property
    def is_ok(self) -> Literal[True]:
        return True


@dataclass(slots=True, frozen=True)
class UnknownErr:
    status: int
    headers: dict[str, str]
    body: Any = None
    ok: Literal[False] = False

    @property
    def is_ok(self) -> Literal[False]:
        return False

    @property
    def value(self) -> Any:
        return self.body


@dataclass(slots=True, frozen=True)
class TransportErr:
    status: int = 0
    headers: dict[str, str] = field(default_factory=dict)
    cause: str = ""
    exception: BaseException | None = None
    ok: Literal[False] = False

    @property
    def is_ok(self) -> Literal[False]:
        return False


@dataclass(slots=True, frozen=True)
class DecodeErr:
    status: int
    headers: dict[str, str]
    body: Any
    cause: str
    exception: Exception | None = None
    ok: Literal[False] = False

    @property
    def is_ok(self) -> Literal[False]:
        return False


@dataclass(slots=True, frozen=True)
class ErrMatcher:
    status: int
    check: Callable[[dict[str, Any]], bool] | None
    factory: Callable[..., Any]


def rt_dispatch_response(
    matchers: list[ErrMatcher],
    resp: RawResponse,
    value_fn: Callable[[Any], Any] | None = None,
) -> Any:
    status = resp.status
    headers = resp.headers
    raw_text = resp.text
    parsed_body: Any = None
    json_err: Exception | None = None
    try:
        parsed_body = json.loads(raw_text)
    except Exception as e:
        json_err = e

    last_decode_err: Exception | None = None
    matched_error_rule = False
    for matcher in matchers:
        if status != matcher.status:
            continue
        if matcher.check is not None:
            if (
                json_err is not None
                or not isinstance(parsed_body, dict)
                or not matcher.check(parsed_body)
            ):
                continue
        matched_error_rule = True
        if json_err is not None:
            last_decode_err = json_err
            continue
        try:
            return matcher.factory(headers=headers, value=parsed_body)
        except (ValueError, KeyError, TypeError) as e:
            last_decode_err = e

    if 200 <= status < 300 and not matched_error_rule:
        if value_fn is None:
            return Ok(status=status, headers=headers, value=None)
        if json_err is not None:
            return DecodeErr(
                status=status,
                headers=headers,
                body=raw_text,
                cause=str(json_err),
                exception=json_err,
            )
        try:
            val = value_fn(parsed_body)
            return Ok(status=status, headers=headers, value=val)
        except (ValueError, KeyError, TypeError) as e:
            return DecodeErr(
                status=status,
                headers=headers,
                body=parsed_body,
                cause=str(e),
                exception=e,
            )

    if matched_error_rule and last_decode_err is not None:
        return DecodeErr(
            status=status,
            headers=headers,
            body=parsed_body if json_err is None else raw_text,
            cause=str(last_decode_err),
            exception=last_decode_err,
        )

    fallback_body: Any = parsed_body if json_err is None else (raw_text if raw_text else None)
    return UnknownErr(status=status, headers=headers, body=fallback_body)
