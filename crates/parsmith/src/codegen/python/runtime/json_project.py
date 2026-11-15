class RtJsonError(ValueError):
    pass


class RtJsonPathError(RtJsonError):
    pass


class RtJsonFieldMissingError(RtJsonError):
    pass


class RtJsonSchemaError(RtJsonError):
    pass


def rt_expect_dict(data: Any, target: str = "") -> dict[str, Any]:
    if not isinstance(data, dict):
        prefix = f"{target}: " if target else ""
        raise RtJsonSchemaError(f"{prefix}Expected dict, got {type(data).__name__}")
    return data


def rt_expect_list(data: Any, target: str = "") -> list[Any]:
    if not isinstance(data, list):
        prefix = f"{target}: " if target else ""
        raise RtJsonSchemaError(f"{prefix}Expected list, got {type(data).__name__}")
    return data


def rt_resolve_dotpath(
    data: Any, path: str, is_optional: bool = False, target: str = ""
) -> Any:
    prefix = f"{target}: " if target else ""
    raw_segs: list[str] = []
    buf: list[str] = []
    i = 0
    n = len(path)
    while i < n:
        ch = path[i]
        if ch == "\\" and i + 1 < n and path[i + 1] in (".", "\\", "#"):
            buf.append(ch)
            buf.append(path[i + 1])
            i += 2
        elif ch == ".":
            raw_segs.append("".join(buf))
            buf = []
            i += 1
        else:
            buf.append(ch)
            i += 1
    raw_segs.append("".join(buf))
    current: Any = data
    for idx_seg, raw_seg in enumerate(raw_segs):
        if raw_seg == "#":
            if current is None:
                if is_optional:
                    return None
                raise RtJsonPathError(
                    f"{prefix}Cannot traverse segment '#' on null object in path '{path}'"
                )
            if not isinstance(current, list):
                if is_optional:
                    return None
                raise RtJsonPathError(
                    f"{prefix}Expected list for '#' in path '{path}', got {type(current).__name__}"
                )
            if idx_seg == len(raw_segs) - 1:
                return len(current)
            subpath = ".".join(raw_segs[idx_seg + 1 :])
            out: list[Any] = []
            for item in current:
                v = rt_resolve_dotpath(item, subpath, is_optional=True, target=target)
                if v is not None:
                    out.append(v)
            return out
        has_escape = "\\" in raw_seg
        if has_escape:
            seg_chars: list[str] = []
            j = 0
            m = len(raw_seg)
            while j < m:
                if (
                    raw_seg[j] == "\\"
                    and j + 1 < m
                    and raw_seg[j + 1] in (".", "\\", "#")
                ):
                    seg_chars.append(raw_seg[j + 1])
                    j += 2
                else:
                    seg_chars.append(raw_seg[j])
                    j += 1
            seg = "".join(seg_chars)
        else:
            seg = raw_seg
        if current is None:
            if is_optional:
                return None
            raise RtJsonPathError(
                f"{prefix}Cannot traverse segment '{seg}' on null object in path '{path}'"
            )
        if not has_escape and seg.isdigit():
            idx = int(seg)
            if not isinstance(current, list):
                if is_optional:
                    return None
                raise RtJsonPathError(
                    f"{prefix}Expected list for index '{idx}' in path '{path}', got {type(current).__name__}"
                )
            if not (0 <= idx < len(current)):
                if is_optional:
                    return None
                raise RtJsonPathError(
                    f"{prefix}Index {idx} out of bounds (len={len(current)}) in path '{path}'"
                )
            current = current[idx]
        elif isinstance(current, dict):
            if seg not in current:
                if is_optional:
                    return None
                raise RtJsonPathError(
                    f"{prefix}Missing key '{seg}' in path '{path}'"
                )
            current = current[seg]
        else:
            if is_optional:
                return None
            raise RtJsonPathError(
                f"{prefix}Cannot access key '{seg}' on non-dict {type(current).__name__} in path '{path}'"
            )
    return current


def rt_json_req(data: dict[str, Any], key: str, target: str = "") -> Any:
    fname = target.rsplit(".", 1)[-1] if "." in target else key
    qual = target if (not target or "." in target) else f"{target}.{fname}"
    prefix = f"{qual}: " if qual else ""
    if key not in data:
        raise RtJsonFieldMissingError(
            f"{prefix}Required JSON field '{key}' (mapped to '{fname}') is missing"
        )
    val = data[key]
    if val is None:
        raise RtJsonFieldMissingError(
            f"{prefix}Field '{key}' is null, but '{fname}' is not nullable"
        )
    return val


def rt_json_req_path(data: Any, path: str, target: str = "") -> Any:
    val = rt_resolve_dotpath(data, path, is_optional=False, target=target)
    if val is None:
        prefix = f"{target}: " if target else ""
        fname = target.rsplit(".", 1)[-1] if "." in target else path
        raise RtJsonFieldMissingError(
            f"{prefix}Field '{path}' is null, but '{fname}' is not nullable"
        )
    return val
