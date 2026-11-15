"""Cross-target codegen execution helpers for Python, Node.js, Go, Rust, Elixir, PHP, C#, and Kotlin E2E tests."""

from __future__ import annotations

import atexit
import base64
from collections.abc import Iterator
from contextlib import contextmanager
import dataclasses
import hashlib
import itertools
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import types
from typing import Any

import parsmith

INTEGRATION_DIR = Path(__file__).resolve().parent
SCHEMAS_DIR = INTEGRATION_DIR / "schemas"
FIXTURES_DIR = INTEGRATION_DIR / "fixtures"
HTML_FIXTURE = FIXTURES_DIR / "dsl_coverage.html"
JS_WORKER_PATH = INTEGRATION_DIR / "js_worker.mjs"
REPO_ROOT = INTEGRATION_DIR.parent.parent

PY_HTML_TARGETS: tuple[str, ...] = (
    "py-bs4",
    "py-lxml",
    "py-parsel",
    "py-selectolax",
)

_WORD_SPLIT_RE = re.compile(
    r"[-_\s]+|(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])"
)
_CAMEL_IDENT_RE = re.compile(r"^[a-z][a-zA-Z0-9]*$")
_CAMEL_BOUNDARY_RE = re.compile(r"([a-z0-9])([A-Z])")
_MOD_COUNTER = itertools.count(1)
_REGISTERED_MODULES: list[str] = []


def _require_bin(name: str) -> str:
    """Resolve an executable from PATH or raise a RuntimeError."""
    resolved = shutil.which(name)
    if resolved is None:
        raise RuntimeError(f"Required executable '{name}' not found in PATH")
    return resolved


def to_pascal_case(name: str) -> str:
    """Convert a KDL struct or identifier name into PascalCase matching parsmith."""
    parts = [p for p in re.split(r"[-_\s]+", name) if p]
    return "".join(p[:1].upper() + p[1:] for p in parts)


def to_snake_case(name: str) -> str:
    """Convert a KDL function or field name into snake_case."""
    parts = [p for p in _WORD_SPLIT_RE.split(name) if p]
    return "_".join(p.lower() for p in parts)


def camel_key_to_snake(key: str) -> str:
    """Convert pure camelCase identifier keys to snake_case without mangling dynamic keys."""
    if not _CAMEL_IDENT_RE.match(key):
        return key
    return _CAMEL_BOUNDARY_RE.sub(r"\1_\2", key).lower()


def normalize_for_parity(
    value: Any,
    *,
    camel_to_snake: bool = False,
    strip_none_dict_values: bool = False,
) -> Any:
    """Normalize extracted Python/JS values (dataclasses, dicts, lists) for parity comparison."""
    if dataclasses.is_dataclass(value) and not isinstance(value, type):
        out: dict[str, Any] = {}
        for f in dataclasses.fields(value):
            f_val = getattr(value, f.name)
            if strip_none_dict_values and f_val is None:
                continue
            norm_key = camel_key_to_snake(f.name) if camel_to_snake else f.name
            out[norm_key] = normalize_for_parity(
                f_val,
                camel_to_snake=camel_to_snake,
                strip_none_dict_values=strip_none_dict_values,
            )
        return out

    if isinstance(value, dict):
        out_dict: dict[Any, Any] = {}
        for k, v in value.items():
            if strip_none_dict_values and v is None:
                continue
            norm_k = (
                camel_key_to_snake(k)
                if camel_to_snake and isinstance(k, str)
                else k
            )
            out_dict[norm_k] = normalize_for_parity(
                v,
                camel_to_snake=camel_to_snake,
                strip_none_dict_values=strip_none_dict_values,
            )
        return out_dict

    if isinstance(value, (list, tuple)):
        return [
            normalize_for_parity(
                item,
                camel_to_snake=camel_to_snake,
                strip_none_dict_values=strip_none_dict_values,
            )
            for item in value
        ]

    return value


def assert_schema_lint_clean(schema_file: str) -> None:
    """Assert that a schema file in tests/integration/schemas/ passes parsmith.check() cleanly."""
    schema_path = SCHEMAS_DIR / schema_file
    assert schema_path.is_file(), f"Missing schema file: {schema_path}"
    res = parsmith.check(schema_path)
    assert res.ok is True, res.format_text()
    assert res.error_count == 0
    assert res.warning_count == 0


def load_py_module(
    schema_src_or_path: str | Path,
    *,
    target: str = "py-bs4",
    models: str = "typeddict",
    separate_runtime: bool = False,
    http_io: str = "both",
) -> tuple[types.ModuleType, types.ModuleType | None]:
    """Compile a KDL schema to Python and load the generated module (and optional runtime module)."""
    uid = next(_MOD_COUNTER)
    mod_stem = f"_parsmith_e2e_mod_{uid}"
    rt_stem = f"_parsmith_e2e_rt_{uid}"

    res = parsmith.generate(
        schema_src_or_path,
        target,
        models=models,
        separate_runtime=separate_runtime,
        runtime_name=rt_stem,
        module_stem=mod_stem,
        http_io=http_io,
    )
    assert res.ok, f"parsmith.generate failed for {target}:\n{res.format_text()}"

    rt_mod: types.ModuleType | None = None
    if separate_runtime:
        rt_filename = f"{rt_stem}.py"
        assert rt_filename in res.files, (
            f"Expected {rt_filename} in {list(res.files.keys())}"
        )
        rt_code = res[rt_filename]
        rt_mod = types.ModuleType(rt_stem)
        sys.modules[rt_stem] = rt_mod
        _REGISTERED_MODULES.append(rt_stem)
        exec(rt_code, rt_mod.__dict__)

    mod_filename = f"{mod_stem}.py"
    primary_code = (
        res[mod_filename] if mod_filename in res.files else res.primary_code()
    )
    assert primary_code is not None, "Expected generated Python code"
    primary_mod = types.ModuleType(mod_stem)
    sys.modules[mod_stem] = primary_mod
    _REGISTERED_MODULES.append(mod_stem)
    exec(primary_code, primary_mod.__dict__)
    return primary_mod, rt_mod


@contextmanager
def loaded_py_module(
    schema_src_or_path: str | Path,
    *,
    target: str = "py-bs4",
    models: str = "typeddict",
    separate_runtime: bool = False,
    http_io: str = "both",
) -> Iterator[tuple[types.ModuleType, types.ModuleType | None]]:
    """Context manager around load_py_module that unregisters transient modules on exit."""
    primary_mod, rt_mod = load_py_module(
        schema_src_or_path,
        target=target,
        models=models,
        separate_runtime=separate_runtime,
        http_io=http_io,
    )
    try:
        yield primary_mod, rt_mod
    finally:
        sys.modules.pop(primary_mod.__name__, None)
        if rt_mod is not None:
            sys.modules.pop(rt_mod.__name__, None)


def cleanup_registered_modules() -> None:
    """Remove any dynamically loaded Python modules from sys.modules."""
    while _REGISTERED_MODULES:
        name = _REGISTERED_MODULES.pop()
        sys.modules.pop(name, None)


def generate_js_code(schema_src_or_path: str | Path) -> str:
    """Compile a KDL schema for the JavaScript target and return the .js source string."""
    res = parsmith.generate(schema_src_or_path, "js", module_stem="bundle")
    assert res.ok, f"parsmith.generate failed for js:\n{res.format_text()}"
    return res["bundle.js"]


def run_py_schema(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
    *,
    target: str = "py-bs4",
    models: str = "typeddict",
    separate_runtime: bool = False,
) -> Any:
    """Compile KDL schema for Python, instantiate struct_name(doc_text), and return .parse()."""
    class_name = to_pascal_case(struct_name)
    with loaded_py_module(
        schema_src_or_path,
        target=target,
        models=models,
        separate_runtime=separate_runtime,
    ) as (mod, _):
        cls = getattr(mod, class_name, None) or getattr(mod, struct_name)
        return cls(doc_text).parse()


def run_py_fn(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
    *,
    target: str = "py-bs4",
    separate_runtime: bool = False,
) -> Any:
    """Compile KDL schema for Python and invoke standalone fn_name(input_text)."""
    snake_name = to_snake_case(fn_name)
    with loaded_py_module(
        schema_src_or_path,
        target=target,
        separate_runtime=separate_runtime,
    ) as (mod, _):
        fn = getattr(mod, snake_name, None) or getattr(mod, fn_name)
        return fn(input_text)


def _discover_jsdom_pkg() -> str | None:
    """Locate a package.json with an adjacent node_modules/jsdom installation."""
    candidates: list[Path] = [
        REPO_ROOT / "package.json",
        REPO_ROOT / "playground" / "package.json",
    ]
    try:
        proc = subprocess.run(
            ["git", "rev-parse", "--git-common-dir"],
            cwd=REPO_ROOT,
            capture_output=True,
            text=True,
            check=False,
        )
        if proc.returncode == 0 and proc.stdout.strip():
            common_git = Path(proc.stdout.strip())
            if not common_git.is_absolute():
                common_git = (REPO_ROOT / common_git).resolve()
            candidates.append(common_git.parent / "package.json")
            candidates.append(common_git.parent / "playground" / "package.json")
    except Exception:
        pass

    for pkg_json in candidates:
        if (
            pkg_json.is_file()
            and (pkg_json.parent / "node_modules" / "jsdom").is_dir()
        ):
            return str(pkg_json.resolve())
    return None


class _JsWorkerSession:
    """Persistent Node.js ESM worker process speaking JSONL over stdin/stdout."""

    def __init__(self) -> None:
        self._proc: subprocess.Popen[str] | None = None
        self._req_id = 0

    def _ensure_proc(self) -> None:
        if self._proc is None or self._proc.poll() is not None:
            node_bin = _require_bin("node")
            env = os.environ.copy()
            if "PARSMITH_JSDOM_PKG" not in env:
                pkg_path = _discover_jsdom_pkg()
                if pkg_path is not None:
                    env["PARSMITH_JSDOM_PKG"] = pkg_path
            self._proc = subprocess.Popen(
                [node_bin, str(JS_WORKER_PATH)],
                cwd=REPO_ROOT,
                env=env,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                encoding="utf-8",
            )

    def request(self, payload: dict[str, Any]) -> Any:
        self._ensure_proc()
        self._req_id += 1
        req_obj = {"id": self._req_id, **payload}
        line_out = json.dumps(req_obj)
        assert (
            self._proc is not None
            and self._proc.stdin is not None
            and self._proc.stdout is not None
        )
        try:
            self._proc.stdin.write(line_out + "\n")
            self._proc.stdin.flush()
            line_in = self._proc.stdout.readline()
        except (BrokenPipeError, OSError) as exc:
            self._proc = None
            raise RuntimeError(
                "JS worker process crashed or disconnected"
            ) from exc

        if not line_in:
            stderr = (
                self._proc.stderr.read()
                if self._proc is not None and self._proc.stderr is not None
                else ""
            )
            self._proc = None
            raise RuntimeError(f"JS worker exited unexpectedly: {stderr}")

        resp: dict[str, Any] = json.loads(line_in)
        if not resp.get("ok"):
            err_msg = str(resp.get("error", "Unknown JS execution error"))
            raise RuntimeError(err_msg)
        return resp.get("result")

    def close(self) -> None:
        if self._proc is not None and self._proc.poll() is None:
            try:
                if self._proc.stdin is not None:
                    self._proc.stdin.close()
                self._proc.terminate()
                self._proc.wait(timeout=2)
            except Exception:
                pass
        self._proc = None


JS_WORKER = _JsWorkerSession()


def close_js_worker() -> None:
    """Terminate the shared worker processes if running."""
    _close_all_workers()


def run_js_schema(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
) -> Any:
    """Compile KDL schema to JS, execute struct_name(doc_text).parse() in Node.js + jsdom."""
    code = generate_js_code(schema_src_or_path)
    class_name = to_pascal_case(struct_name)
    return JS_WORKER.request(
        {
            "action": "parse",
            "code": code,
            "className": class_name,
            "html": doc_text,
        }
    )


def run_js_fn(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
) -> Any:
    """Compile KDL schema to JS and invoke standalone fn_name(input_text) in Node.js + jsdom."""
    code = generate_js_code(schema_src_or_path)
    return JS_WORKER.request(
        {
            "action": "fn",
            "code": code,
            "fnName": fn_name,
            "input": input_text,
            "html": input_text,
        }
    )


def run_js_rest(
    schema_src_or_path: str | Path,
    class_name: str,
    method_name: str = "fetch",
    *,
    args: list[Any] | None = None,
) -> dict[str, Any]:
    """Compile KDL schema to JS and invoke a static REST method via FetchTransport."""
    code = generate_js_code(schema_src_or_path)
    res = JS_WORKER.request(
        {
            "action": "rest",
            "code": code,
            "className": to_pascal_case(class_name),
            "methodName": method_name,
            "args": args or [],
        }
    )
    assert isinstance(res, dict)
    return res


def run_js_fetch(
    schema_src_or_path: str | Path,
    class_name: str,
    method_name: str = "fetch",
    *,
    args: list[Any] | None = None,
    check_method: str | None = None,
    skip_parse: bool = False,
) -> dict[str, Any]:
    """Compile KDL schema to JS and invoke an HTML/(raw)struct static fetch method."""
    code = generate_js_code(schema_src_or_path)
    res = JS_WORKER.request(
        {
            "action": "fetch",
            "code": code,
            "className": to_pascal_case(class_name),
            "methodName": method_name,
            "args": args or [],
            "checkMethod": check_method,
            "skipParse": skip_parse,
        }
    )
    assert isinstance(res, dict)
    return res


# ── Cached Go & Rust Toolchain Runners ───────────────────────────────────────

_EXE_SUFFIX = ".exe" if os.name == "nt" else ""
_GO_CACHE_DIR = REPO_ROOT / "target" / "e2e_go_cache"
_GO_MOD_DIR = _GO_CACHE_DIR / "runner_mod"
_GO_BINS_DIR = _GO_CACHE_DIR / "bins"

_RUST_CACHE_DIR = REPO_ROOT / "target" / "e2e_rust_cache"
_RUST_CRATE_DIR = _RUST_CACHE_DIR / "runner_crate"
_RUST_BINS_DIR = _RUST_CACHE_DIR / "bins"
_RUST_TARGET_DIR = REPO_ROOT / "target" / "e2e_rust_target"

_GO_MOD_CONTENT = """module parsmith_e2e_runner

go 1.26

require (
\tgithub.com/PuerkitoBio/goquery v1.12.0
\tgithub.com/tidwall/gjson v1.18.0
)

require (
\tgithub.com/andybalholm/cascadia v1.3.3 // indirect
\tgithub.com/tidwall/match v1.1.1 // indirect
\tgithub.com/tidwall/pretty v1.2.0 // indirect
\tgolang.org/x/net v0.52.0 // indirect
)
"""

_GO_SUM_CONTENT = """github.com/PuerkitoBio/goquery v1.12.0 h1:pAcL4g3WRXekcB9AU/y1mbKez2dbY2AajVhtkO8RIBo=
github.com/PuerkitoBio/goquery v1.12.0/go.mod h1:802ej+gV2y7bbIhOIoPY5sT183ZW0YFofScC4q/hIpQ=
github.com/andybalholm/cascadia v1.3.3 h1:AG2YHrzJIm4BZ19iwJ/DAua6Btl3IwJX+VI4kktS1LM=
github.com/andybalholm/cascadia v1.3.3/go.mod h1:xNd9bqTn98Ln4DwST8/nG+H0yuB8Hmgu1YHNnWw0GeA=
github.com/tidwall/gjson v1.18.0 h1:FIDeeyB800efLX89e5a8Y0BNH+LOngJyGrIWxG2FKQY=
github.com/tidwall/gjson v1.18.0/go.mod h1:/wbyibRr2FHMks5tjHJ5F8dMZh3AcwJEMf5vlfC0lxk=
github.com/tidwall/match v1.1.1 h1:+Ho715JplO36QYgwN9PGYNhgZvoUSc9X2c80KVTi+GA=
github.com/tidwall/match v1.1.1/go.mod h1:eRSPERbgtNPcGhD8UCthc6PmLEQXEWd3PRB5JTxsfmM=
github.com/tidwall/pretty v1.2.0 h1:RWIZEg2iJ8/g6fDDYzMpobmaoGh5OLl4AXtGUGPcqCs=
github.com/tidwall/pretty v1.2.0/go.mod h1:ITEVvHYasfjBbM0u2Pg8T2nJnzm8xPwvNhhsoaGGjNU=
golang.org/x/net v0.52.0 h1:He/TN1l0e4mmR3QqHMT2Xab3Aj3L9qjbhRm78/6jrW0=
golang.org/x/net v0.52.0/go.mod h1:R1MAz7uMZxVMualyPXb+VaqGSa3LIaUqk0eEt3w36Sw=
"""

_RUST_CARGO_TOML = """[package]
name = "parsmith_e2e_runner"
version = "0.1.0"
edition = "2021"

[workspace]

[dependencies]
dom_query = "0.28"
regex = "1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
reqwest = { version = "0.12", default-features = false, features = ["json"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
"""


def _ensure_go_workspace() -> None:
    _GO_MOD_DIR.mkdir(parents=True, exist_ok=True)
    _GO_BINS_DIR.mkdir(parents=True, exist_ok=True)
    go_mod = _GO_MOD_DIR / "go.mod"
    if (
        not go_mod.is_file()
        or go_mod.read_text(encoding="utf-8") != _GO_MOD_CONTENT
    ):
        go_mod.write_text(_GO_MOD_CONTENT, encoding="utf-8")
    go_sum = _GO_MOD_DIR / "go.sum"
    if (
        not go_sum.is_file()
        or go_sum.read_text(encoding="utf-8") != _GO_SUM_CONTENT
    ):
        go_sum.write_text(_GO_SUM_CONTENT, encoding="utf-8")


def _ensure_rust_workspace() -> None:
    (_RUST_CRATE_DIR / "src").mkdir(parents=True, exist_ok=True)
    _RUST_BINS_DIR.mkdir(parents=True, exist_ok=True)
    _RUST_TARGET_DIR.mkdir(parents=True, exist_ok=True)
    cargo_toml = _RUST_CRATE_DIR / "Cargo.toml"
    if (
        not cargo_toml.is_file()
        or cargo_toml.read_text(encoding="utf-8") != _RUST_CARGO_TOML
    ):
        cargo_toml.write_text(_RUST_CARGO_TOML, encoding="utf-8")


def _hash_files(files: dict[str, str]) -> str:
    h = hashlib.sha256()
    for name in sorted(files):
        h.update(name.encode("utf-8"))
        h.update(b"\0")
        h.update(files[name].encode("utf-8"))
        h.update(b"\0")
    return h.hexdigest()[:24]


def _build_go_binary(files: dict[str, str]) -> Path:
    _ensure_go_workspace()
    digest = _hash_files(files)
    bin_path = _GO_BINS_DIR / f"go_{digest}{_EXE_SUFFIX}"
    if bin_path.is_file():
        return bin_path

    for existing in _GO_MOD_DIR.iterdir():
        if existing.name in ("go.mod", "go.sum"):
            continue
        if existing.is_dir():
            shutil.rmtree(existing)
        else:
            existing.unlink()
    for fname, content in files.items():
        dest = _GO_MOD_DIR / fname
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(content, encoding="utf-8")

    go_bin = _require_bin("go")
    tmp_bin = _GO_BINS_DIR / f"go_{digest}_tmp_{os.getpid()}{_EXE_SUFFIX}"
    proc = subprocess.run(
        [go_bin, "build", "-o", str(tmp_bin), "."],
        cwd=_GO_MOD_DIR,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"go build failed:\n{proc.stderr}\n{proc.stdout}"
        )
    tmp_bin.replace(bin_path)
    return bin_path


def _build_rust_binary(files: dict[str, str]) -> Path:
    _ensure_rust_workspace()
    digest = _hash_files(files)
    bin_path = _RUST_BINS_DIR / f"rust_{digest}{_EXE_SUFFIX}"
    if bin_path.is_file():
        return bin_path

    src_dir = _RUST_CRATE_DIR / "src"
    for existing in src_dir.glob("*.rs"):
        existing.unlink()
    for fname, content in files.items():
        (src_dir / fname).write_text(content, encoding="utf-8")

    cargo_bin = _require_bin("cargo")
    env = os.environ.copy()
    env["CARGO_TARGET_DIR"] = str(_RUST_TARGET_DIR)
    proc = subprocess.run(
        [cargo_bin, "build", "--quiet", "--bin", "parsmith_e2e_runner"],
        cwd=_RUST_CRATE_DIR,
        env=env,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if proc.returncode != 0:
        raise RuntimeError(
            f"cargo build failed:\n{proc.stderr}\n{proc.stdout}"
        )
    built_exe = _RUST_TARGET_DIR / "debug" / f"parsmith_e2e_runner{_EXE_SUFFIX}"
    shutil.copy2(built_exe, bin_path)
    return bin_path


def _get_or_build_go_schema_binary(
    schema_src_or_path: str | Path,
) -> Path:
    res = parsmith.generate(
        schema_src_or_path,
        "go",
        package="main",
        runtime_name="parsmith_runtime",
        module_stem="schema",
    )
    assert res.ok, f"parsmith.generate(go) failed:\n{res.format_text()}"
    info = parsmith.inspect_schema(schema_src_or_path)
    assert info.ok, f"parsmith.inspect_schema failed:\n{info.format_text()}"

    lines = [
        "package main",
        "",
        "import (",
        '\t"encoding/json"',
        '\t"fmt"',
        '\t"io"',
        '\t"os"',
        ")",
        "",
        "func main() {",
        "\tif len(os.Args) < 2 {",
        '\t\tfmt.Fprintln(os.Stderr, "missing target argument")',
        "\t\tos.Exit(2)",
        "\t}",
        "\ttarget := os.Args[1]",
        "\tinputBytes, err := io.ReadAll(os.Stdin)",
        "\tif err != nil {",
        "\t\tfmt.Fprintln(os.Stderr, err)",
        "\t\tos.Exit(1)",
        "\t}",
        "\tdocText := string(inputBytes)",
        "\t_ = docText",
        "\tvar result any",
        "\tswitch target {",
    ]
    for s in info.structs:
        if s.kind == "rest":
            continue
        pascal = to_pascal_case(s.name)
        cases = f'"struct:{pascal}"'
        if s.name != pascal:
            cases += f', "struct:{s.name}"'
        lines.append(f"\tcase {cases}:")
        if s.kind == "raw":
            lines.append(f"\t\tp := New{pascal}(docText)")
            lines.append("\t\tresult = p.Parse()")
        else:
            lines.append(f"\t\tp, err := New{pascal}(docText)")
            lines.append("\t\tif err != nil {")
            lines.append("\t\t\tfmt.Fprintln(os.Stderr, err)")
            lines.append("\t\t\tos.Exit(1)")
            lines.append("\t\t}")
            lines.append("\t\tresult = p.Parse()")

    for fn in info.functions:
        pascal_fn = to_pascal_case(fn.name)
        snake_fn = to_snake_case(fn.name)
        cases = f'"fn:{snake_fn}"'
        if fn.name != snake_fn:
            cases += f', "fn:{fn.name}"'
        lines.append(f"\tcase {cases}:")
        lines.append(f"\t\tresult = {pascal_fn}(docText)")

    lines.extend(
        [
            "\tdefault:",
            '\t\tfmt.Fprintf(os.Stderr, "unknown target: %s\\n", target)',
            "\t\tos.Exit(2)",
            "\t}",
            "\tout, err := json.Marshal(result)",
            "\tif err != nil {",
            "\t\tfmt.Fprintln(os.Stderr, err)",
            "\t\tos.Exit(1)",
            "\t}",
            "\t_, _ = os.Stdout.Write(out)",
            "}",
            "",
        ]
    )

    files = dict(res.files)
    files["main.go"] = "\n".join(lines)
    return _build_go_binary(files)


def _get_or_build_rust_schema_binary(
    schema_src_or_path: str | Path,
) -> Path:
    res = parsmith.generate(
        schema_src_or_path,
        "rust",
        runtime_name="parsmith_runtime",
        module_stem="schema",
    )
    assert res.ok, f"parsmith.generate(rust) failed:\n{res.format_text()}"
    info = parsmith.inspect_schema(schema_src_or_path)
    assert info.ok, f"parsmith.inspect_schema failed:\n{info.format_text()}"

    lines = [
        "#![allow(dead_code, unused_imports)]",
        "mod schema;",
        "",
        "use std::io::Read;",
        "",
        "fn main() {",
        "    let target = std::env::args().nth(1).unwrap_or_else(|| {",
        '        eprintln!("missing target argument");',
        "        std::process::exit(2);",
        "    });",
        "    let mut doc_text = String::new();",
        "    std::io::stdin().read_to_string(&mut doc_text).unwrap_or_else(|e| {",
        '        eprintln!("{e}");',
        "        std::process::exit(1);",
        "    });",
        "    let _ = &doc_text;",
        "    match target.as_str() {",
    ]
    for s in info.structs:
        if s.kind == "rest":
            continue
        pascal = to_pascal_case(s.name).replace("-", "")
        pattern = f'"struct:{pascal}"'
        if s.name != pascal:
            pattern += f' | "struct:{s.name}"'
        lines.extend(
            [
                f"        {pattern} => {{",
                f"            let mut parser = schema::{pascal}Parser::new(doc_text).unwrap_or_else(|e| {{",
                '                eprintln!("{e}");',
                "                std::process::exit(1);",
                "            });",
                "            let res = parser.parse().unwrap_or_else(|e| {",
                '                eprintln!("{e}");',
                "                std::process::exit(1);",
                "            });",
                "            let out = serde_json::to_string(&res).unwrap();",
                '            print!("{out}");',
                "        }",
            ]
        )

    for fn in info.functions:
        snake_fn = to_snake_case(fn.name)
        pattern = f'"fn:{snake_fn}"'
        if fn.name != snake_fn:
            pattern += f' | "fn:{fn.name}"'
        lines.extend(
            [
                f"        {pattern} => {{",
                f"            let res = schema::{snake_fn}(doc_text).unwrap_or_else(|e| {{",
                '                eprintln!("{e}");',
                "                std::process::exit(1);",
                "            });",
                "            let out = serde_json::to_string(&res).unwrap();",
                '            print!("{out}");',
                "        }",
            ]
        )

    lines.extend(
        [
            "        other => {",
            '            eprintln!("unknown target: {other}");',
            "            std::process::exit(2);",
            "        }",
            "    }",
            "}",
            "",
        ]
    )

    files = dict(res.files)
    files["main.rs"] = "\n".join(lines)
    return _build_rust_binary(files)


def run_go_schema(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
) -> Any:
    """Compile KDL schema to Go, execute New<Struct>(doc_text).Parse(), and return decoded JSON."""
    bin_path = _get_or_build_go_schema_binary(schema_src_or_path)
    proc = subprocess.run(
        [str(bin_path), f"struct:{to_pascal_case(struct_name)}"],
        input=doc_text.encode("utf-8"),
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            err_msg or f"Go schema runner exited with {proc.returncode}"
        )
    return json.loads(proc.stdout.decode("utf-8"))


def run_go_fn(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
) -> Any:
    """Compile KDL schema to Go, execute <Fn>(input_text), and return decoded JSON."""
    bin_path = _get_or_build_go_schema_binary(schema_src_or_path)
    proc = subprocess.run(
        [str(bin_path), f"fn:{to_snake_case(fn_name)}"],
        input=input_text.encode("utf-8"),
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            err_msg or f"Go fn runner exited with {proc.returncode}"
        )
    return json.loads(proc.stdout.decode("utf-8"))


def run_rust_schema(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
) -> Any:
    """Compile KDL schema to Rust, execute <Struct>Parser::new(doc_text)?.parse()?, and return decoded JSON."""
    bin_path = _get_or_build_rust_schema_binary(schema_src_or_path)
    pascal = to_pascal_case(struct_name).replace("-", "")
    proc = subprocess.run(
        [str(bin_path), f"struct:{pascal}"],
        input=doc_text.encode("utf-8"),
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            err_msg or f"Rust schema runner exited with {proc.returncode}"
        )
    return json.loads(proc.stdout.decode("utf-8"))


def run_rust_fn(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
) -> Any:
    """Compile KDL schema to Rust, execute <fn_name>(input_text)?, and return decoded JSON."""
    bin_path = _get_or_build_rust_schema_binary(schema_src_or_path)
    proc = subprocess.run(
        [str(bin_path), f"fn:{to_snake_case(fn_name)}"],
        input=input_text.encode("utf-8"),
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            err_msg or f"Rust fn runner exited with {proc.returncode}"
        )
    return json.loads(proc.stdout.decode("utf-8"))


def run_go_custom_program(
    files: dict[str, str],
) -> str:
    """Compile a custom Go `package main` file set in the cached workspace and return stdout."""
    bin_path = _build_go_binary(files)
    proc = subprocess.run(
        [str(bin_path)],
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        out_msg = proc.stdout.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            f"Go custom program failed (exit {proc.returncode}):\n{err_msg}\n{out_msg}"
        )
    return proc.stdout.decode("utf-8")


def run_rust_custom_program(
    files: dict[str, str],
) -> str:
    """Compile a custom Rust `src/main.rs` + modules file set in the cached workspace and return stdout."""
    bin_path = _build_rust_binary(files)
    proc = subprocess.run(
        [str(bin_path)],
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        out_msg = proc.stdout.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            f"Rust custom program failed (exit {proc.returncode}):\n{err_msg}\n{out_msg}"
        )
    return proc.stdout.decode("utf-8")


# ── Cached Elixir, PHP, C#, & Kotlin Toolchain Runners ───────────────────────

_ELIXIR_CACHE_DIR = REPO_ROOT / "target" / "e2e_elixir_cache"
_ELIXIR_BINS_DIR = _ELIXIR_CACHE_DIR / "bins"
_ELIXIR_WORKER_PATH = _ELIXIR_CACHE_DIR / "elixir_worker.exs"

_PHP_CACHE_DIR = REPO_ROOT / "target" / "e2e_php_cache"
_PHP_BINS_DIR = _PHP_CACHE_DIR / "bins"

_CSHARP_CACHE_DIR = REPO_ROOT / "target" / "e2e_csharp_cache"
_CSHARP_WORKER_PROJ_DIR = _CSHARP_CACHE_DIR / "worker_proj"
_CSHARP_BINS_DIR = _CSHARP_CACHE_DIR / "bins"

_KOTLIN_CACHE_DIR = REPO_ROOT / "target" / "e2e_kotlin_cache"
_KOTLIN_WORKER_DIR = _KOTLIN_CACHE_DIR / "worker"
_KOTLIN_BINS_DIR = _KOTLIN_CACHE_DIR / "bins"

_ELIXIR_MIX_EXS = """defmodule ParsmithE2eRunner.MixProject do
  use Mix.Project
  def project do
    [
      app: :parsmith_e2e_runner,
      version: "0.1.0",
      elixir: "~> 1.18",
      deps: [{:floki, "~> 0.38"}]
    ]
  end
end
"""

_ELIXIR_WORKER_EXS = """defmodule ParsmithElixirWorker do
  def loop do
    case IO.read(:stdio, :line) do
      :eof ->
        :ok

      {:error, _} ->
        :ok

      line ->
        trimmed = String.trim(line)

        if trimmed != "" do
          resp = handle_line(trimmed)
          IO.puts(JSON.encode!(resp))
        end

        loop()
    end
  end

  defp handle_line(line) do
    req = JSON.decode!(line)
    id = req["id"]

    try do
      beam_dir = req["beam_dir"]
      done_file = Path.join(beam_dir, ".done")

      if not File.exists?(done_file) do
        ex_files = req["ex_files"] || []

        case Kernel.ParallelCompiler.compile_to_path(ex_files, beam_dir, return_diagnostics: true) do
          {:ok, _, _} ->
            File.write!(done_file, "ok")

          {:error, errors, _warnings} ->
            raise "Elixir compile failed: #{inspect(errors)}"
        end
      end

      :code.add_patha(String.to_charlist(beam_dir))

      case req["action"] do
        "compile" ->
          %{"id" => id, "ok" => true, "result" => "ok"}

        "struct" ->
          mod = String.to_atom(req["mod"])
          doc = req["doc"]
          parser = apply(mod, :new, [doc])
          res = apply(mod, :parse, [parser])
          %{"id" => id, "ok" => true, "result" => JSON.decode!(JSON.encode!(res))}

        "fn" ->
          mod = String.to_atom(req["mod"])
          fn_atom = String.to_atom(req["fn_name"])
          doc = req["doc"]
          res = apply(mod, fn_atom, [doc])
          %{"id" => id, "ok" => true, "result" => JSON.decode!(JSON.encode!(res))}
      end
    rescue
      e ->
        %{"id" => id, "ok" => false, "error" => Exception.message(e)}
    end
  end
end

ParsmithElixirWorker.loop()
"""


def _ensure_elixir_workspace() -> tuple[str, Path]:
    elixir_cmd = _require_bin("elixir")
    _ELIXIR_CACHE_DIR.mkdir(parents=True, exist_ok=True)
    _ELIXIR_BINS_DIR.mkdir(parents=True, exist_ok=True)

    mix_exs = _ELIXIR_CACHE_DIR / "mix.exs"
    if (
        not mix_exs.is_file()
        or mix_exs.read_text(encoding="utf-8") != _ELIXIR_MIX_EXS
    ):
        mix_exs.write_text(_ELIXIR_MIX_EXS, encoding="utf-8")

    floki_ebin = _ELIXIR_CACHE_DIR / "_build" / "dev" / "lib" / "floki" / "ebin"
    if not floki_ebin.is_dir():
        mix_cmd = _require_bin("mix")
        subprocess.run(
            [mix_cmd, "local.hex", "--force", "--if-missing"],
            cwd=_ELIXIR_CACHE_DIR,
            capture_output=True,
            check=True,
        )
        subprocess.run(
            [mix_cmd, "deps.get"],
            cwd=_ELIXIR_CACHE_DIR,
            capture_output=True,
            check=True,
        )
        subprocess.run(
            [mix_cmd, "deps.compile"],
            cwd=_ELIXIR_CACHE_DIR,
            capture_output=True,
            check=True,
        )

    if (
        not _ELIXIR_WORKER_PATH.is_file()
        or _ELIXIR_WORKER_PATH.read_text(encoding="utf-8") != _ELIXIR_WORKER_EXS
    ):
        _ELIXIR_WORKER_PATH.write_text(_ELIXIR_WORKER_EXS, encoding="utf-8")

    return elixir_cmd, floki_ebin


class _ElixirWorkerSession:
    """Persistent Elixir BEAM worker process speaking JSONL over stdin/stdout."""

    def __init__(self) -> None:
        self._proc: subprocess.Popen[str] | None = None
        self._req_id = 0

    def _ensure_proc(self) -> None:
        if self._proc is None or self._proc.poll() is not None:
            elixir_cmd, floki_ebin = _ensure_elixir_workspace()
            self._proc = subprocess.Popen(
                [elixir_cmd, "-pa", str(floki_ebin), str(_ELIXIR_WORKER_PATH)],
                cwd=_ELIXIR_CACHE_DIR,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                encoding="utf-8",
            )

    def request(self, payload: dict[str, Any]) -> Any:
        self._ensure_proc()
        self._req_id += 1
        req_obj = {"id": self._req_id, **payload}
        line_out = json.dumps(req_obj)
        assert (
            self._proc is not None
            and self._proc.stdin is not None
            and self._proc.stdout is not None
        )
        try:
            self._proc.stdin.write(line_out + "\n")
            self._proc.stdin.flush()
            line_in = self._proc.stdout.readline()
        except (BrokenPipeError, OSError) as exc:
            self._proc = None
            raise RuntimeError(
                "Elixir worker process crashed or disconnected"
            ) from exc

        if not line_in:
            stderr = (
                self._proc.stderr.read()
                if self._proc is not None and self._proc.stderr is not None
                else ""
            )
            self._proc = None
            raise RuntimeError(f"Elixir worker exited unexpectedly: {stderr}")

        resp: dict[str, Any] = json.loads(line_in)
        if not resp.get("ok"):
            raise RuntimeError(
                str(resp.get("error", "Unknown Elixir execution error"))
            )
        return resp.get("result")

    def close(self) -> None:
        if self._proc is not None and self._proc.poll() is None:
            try:
                if self._proc.stdin is not None:
                    self._proc.stdin.close()
                self._proc.terminate()
                self._proc.wait(timeout=2)
            except Exception:
                pass
        self._proc = None


ELIXIR_WORKER = _ElixirWorkerSession()


def _prepare_elixir_schema_beam(
    schema_src_or_path: str | Path,
) -> tuple[Path, list[str], str]:
    raw_key = (
        Path(schema_src_or_path).read_text(encoding="utf-8")
        if isinstance(schema_src_or_path, Path)
        else str(schema_src_or_path)
    )
    digest = _hash_files({"kdl": raw_key})
    pkg = f"E2eMod{digest[:12].capitalize()}"
    beam_dir = _ELIXIR_BINS_DIR / f"beam_{digest}"
    beam_dir.mkdir(parents=True, exist_ok=True)

    ex_files: list[str] = []
    if not (beam_dir / ".done").is_file():
        res = parsmith.generate(
            schema_src_or_path,
            "elixir",
            package=pkg,
            runtime_name="parsmith_runtime",
            module_stem="schema",
        )
        assert res.ok, f"parsmith.generate(elixir) failed:\n{res.format_text()}"
        ordered_names = sorted(
            res.files.keys(),
            key=lambda k: (0 if "runtime" in k else 1, k),
        )
        for fname in ordered_names:
            fpath = beam_dir / fname
            fpath.write_text(res.files[fname], encoding="utf-8")
            ex_files.append(str(fpath))
    return beam_dir, ex_files, pkg


def run_elixir_schema(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
) -> Any:
    """Compile KDL schema to Elixir, execute <Struct>.new(doc) |> <Struct>.parse(), and return decoded JSON."""
    beam_dir, ex_files, pkg = _prepare_elixir_schema_beam(schema_src_or_path)
    pascal = to_pascal_case(struct_name).replace("-", "")
    return ELIXIR_WORKER.request(
        {
            "action": "struct",
            "beam_dir": str(beam_dir),
            "ex_files": ex_files,
            "mod": f"Elixir.{pkg}.{pascal}",
            "doc": doc_text,
        }
    )


def run_elixir_fn(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
) -> Any:
    """Compile KDL schema to Elixir, execute Functions.<fn_name>(input_text), and return decoded JSON."""
    beam_dir, ex_files, pkg = _prepare_elixir_schema_beam(schema_src_or_path)
    return ELIXIR_WORKER.request(
        {
            "action": "fn",
            "beam_dir": str(beam_dir),
            "ex_files": ex_files,
            "mod": f"Elixir.{pkg}.Functions",
            "fn_name": to_snake_case(fn_name),
            "doc": input_text,
        }
    )


def run_elixir_custom_program(
    files: dict[str, str],
    eval_code: str,
) -> str:
    """Compile a custom set of Elixir `.ex` files and run `eval_code` in the cached Mix/Floki environment."""
    elixir_cmd, floki_ebin = _ensure_elixir_workspace()
    digest = _hash_files({**files, "__eval__": eval_code})
    beam_dir = _ELIXIR_BINS_DIR / f"custom_{digest}"
    beam_dir.mkdir(parents=True, exist_ok=True)

    ex_files: list[str] = []
    ordered_names = sorted(
        files.keys(),
        key=lambda k: (0 if "rt" in k or "runtime" in k else 1, k),
    )
    for fname in ordered_names:
        dest = beam_dir / fname
        dest.write_text(files[fname], encoding="utf-8")
        ex_files.append(str(dest))

    ELIXIR_WORKER.request(
        {
            "action": "compile",
            "beam_dir": str(beam_dir),
            "ex_files": ex_files,
        }
    )

    runner_exs = beam_dir / "_runner.exs"
    runner_exs.write_text(eval_code, encoding="utf-8")
    proc = subprocess.run(
        [
            elixir_cmd,
            "-pa",
            str(floki_ebin),
            "-pa",
            str(beam_dir),
            str(runner_exs),
        ],
        cwd=beam_dir,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        out_msg = proc.stdout.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            f"Elixir custom program failed (exit {proc.returncode}):\n{err_msg}\n{out_msg}"
        )
    return proc.stdout.decode("utf-8")


def _ensure_php_workspace(*, need_dom_crawler: bool = False) -> tuple[str, Path | None]:
    php_bin = _require_bin("php")
    _PHP_CACHE_DIR.mkdir(parents=True, exist_ok=True)
    _PHP_BINS_DIR.mkdir(parents=True, exist_ok=True)

    autoload_path = _PHP_CACHE_DIR / "vendor" / "autoload.php"
    if need_dom_crawler and not autoload_path.is_file():
        composer_cmd = _require_bin("composer")
        proc = subprocess.run(
            [
                composer_cmd,
                "require",
                "--no-interaction",
                "--prefer-dist",
                "symfony/dom-crawler",
                "symfony/css-selector",
            ],
            cwd=_PHP_CACHE_DIR,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
        if proc.returncode != 0:
            raise RuntimeError(
                f"composer require failed:\n{proc.stderr}\n{proc.stdout}"
            )
    return php_bin, (autoload_path if autoload_path.is_file() else None)


def _get_or_build_php_schema_script(
    schema_src_or_path: str | Path,
    *,
    html_lib: str = "native",
) -> tuple[str, Path]:
    php_bin, autoload_path = _ensure_php_workspace(
        need_dom_crawler=(html_lib != "native")
    )
    res = parsmith.generate(
        schema_src_or_path,
        "php",
        php_html_lib=html_lib,
        runtime_name="parsmith_runtime",
        module_stem="schema",
    )
    assert res.ok, f"parsmith.generate(php) failed:\n{res.format_text()}"
    info = parsmith.inspect_schema(schema_src_or_path)
    assert info.ok, f"parsmith.inspect_schema failed:\n{info.format_text()}"

    digest = _hash_files(
        {
            **res.files,
            "__lib__": html_lib,
        }
    )
    script_dir = _PHP_BINS_DIR / f"php_{digest}"
    runner_php = script_dir / "runner.php"
    if runner_php.is_file():
        return php_bin, runner_php

    script_dir.mkdir(parents=True, exist_ok=True)
    for fname, content in res.files.items():
        (script_dir / fname).write_text(content, encoding="utf-8")

    lines = [
        "<?php",
        "declare(strict_types=1);",
    ]
    if autoload_path is not None:
        autoload_s = str(autoload_path).replace("\\", "/")
        lines.append(f"require_once '{autoload_s}';")
    lines.extend(
        [
            "require_once __DIR__ . '/schema.php';",
            "$target = $argv[1] ?? '';",
            "$docText = (string) stream_get_contents(STDIN);",
            "try {",
            "    switch ($target) {",
        ]
    )
    for s in info.structs:
        if s.kind == "rest":
            continue
        pascal = to_pascal_case(s.name).replace("-", "")
        lines.append(f"        case 'struct:{pascal}':")
        if s.name != pascal:
            lines.append(f"        case 'struct:{s.name}':")
        lines.extend(
            [
                f"            $cls = class_exists('{pascal}') ? '{pascal}' : '{pascal}Parser';",
                "            $p = new $cls($docText);",
                "            $result = $p->parse();",
                "            break;",
            ]
        )
    for fn in info.functions:
        snake_fn = to_snake_case(fn.name)
        lines.append(f"        case 'fn:{snake_fn}':")
        if fn.name != snake_fn:
            lines.append(f"        case 'fn:{fn.name}':")
        lines.extend(
            [
                f"            $fn = function_exists('{snake_fn}') ? '{snake_fn}' : '_{snake_fn}';",
                "            $result = $fn($docText);",
                "            break;",
            ]
        )
    lines.extend(
        [
            "        default:",
            "            fwrite(STDERR, \"unknown target: $target\\n\");",
            "            exit(2);",
            "    }",
            "    echo json_encode($result, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION | JSON_THROW_ON_ERROR);",
            "} catch (\\Throwable $e) {",
            "    fwrite(STDERR, get_class($e) . ': ' . $e->getMessage() . \"\\n\");",
            "    exit(1);",
            "}",
            "",
        ]
    )
    runner_php.write_text("\n".join(lines), encoding="utf-8")
    return php_bin, runner_php


def run_php_schema(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
    *,
    html_lib: str = "native",
) -> Any:
    """Compile KDL schema to PHP, execute (new <Struct>($doc))->parse(), and return decoded JSON."""
    php_bin, runner_php = _get_or_build_php_schema_script(
        schema_src_or_path,
        html_lib=html_lib,
    )
    pascal = to_pascal_case(struct_name).replace("-", "")
    proc = subprocess.run(
        [php_bin, str(runner_php), f"struct:{pascal}"],
        input=doc_text.encode("utf-8"),
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            err_msg or f"PHP schema runner exited with {proc.returncode}"
        )
    return json.loads(proc.stdout.decode("utf-8"))


def run_php_fn(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
    *,
    html_lib: str = "native",
) -> Any:
    """Compile KDL schema to PHP, execute <fn_name>($input), and return decoded JSON."""
    php_bin, runner_php = _get_or_build_php_schema_script(
        schema_src_or_path,
        html_lib=html_lib,
    )
    proc = subprocess.run(
        [php_bin, str(runner_php), f"fn:{to_snake_case(fn_name)}"],
        input=input_text.encode("utf-8"),
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            err_msg or f"PHP fn runner exited with {proc.returncode}"
        )
    return json.loads(proc.stdout.decode("utf-8"))


def run_php_custom_program(
    files: dict[str, str],
) -> str:
    """Write a set of PHP files into the cached PHP workspace, execute `main.php`, and return stdout."""
    php_bin, autoload_path = _ensure_php_workspace(need_dom_crawler=False)
    _ = autoload_path
    digest = _hash_files({**files, "__entry__": "main.php"})
    run_dir = _PHP_BINS_DIR / f"custom_{digest}"
    run_dir.mkdir(parents=True, exist_ok=True)
    for fname, content in files.items():
        dest = run_dir / fname
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_text(content, encoding="utf-8")

    proc = subprocess.run(
        [php_bin, str(run_dir / "main.php")],
        cwd=run_dir,
        capture_output=True,
        check=False,
    )
    if proc.returncode != 0:
        err_msg = proc.stderr.decode("utf-8", errors="replace").strip()
        out_msg = proc.stdout.decode("utf-8", errors="replace").strip()
        raise RuntimeError(
            f"PHP custom program failed (exit {proc.returncode}):\n{err_msg}\n{out_msg}"
        )
    return proc.stdout.decode("utf-8")


_CSHARP_WORKER_PROGRAM = """#nullable enable
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.Loader;
using System.Text;
using System.Threading.Tasks;
using Microsoft.CodeAnalysis;
using Microsoft.CodeAnalysis.CSharp;

public static class Program
{
    private static readonly Dictionary<string, Assembly> Assemblies = new();

    private static string B64(string s) => Convert.ToBase64String(Encoding.UTF8.GetBytes(s));
    private static string Unb64(string s) => Encoding.UTF8.GetString(Convert.FromBase64String(s));

    public static void Main()
    {
        Console.InputEncoding = Encoding.UTF8;
        Console.OutputEncoding = Encoding.UTF8;
        var realOut = Console.Out;

        var tpa = ((string?)AppContext.GetData("TRUSTED_PLATFORM_ASSEMBLIES") ?? "")
            .Split(Path.PathSeparator, StringSplitOptions.RemoveEmptyEntries);
        var refs = tpa
            .Where(p => !p.EndsWith("Microsoft.CodeAnalysis.dll", StringComparison.OrdinalIgnoreCase)
                     && !p.EndsWith("Microsoft.CodeAnalysis.CSharp.dll", StringComparison.OrdinalIgnoreCase))
            .Select(p => (MetadataReference)MetadataReference.CreateFromFile(p))
            .ToList();

        string? line;
        while ((line = Console.ReadLine()) is not null)
        {
            if (line.Length == 0) continue;
            var parts = line.Split('\\t');
            var cmd = parts[0];
            try
            {
                if (cmd == "COMPILE")
                {
                    var dllPath = parts[1];
                    if (!File.Exists(dllPath))
                    {
                        var trees = new List<SyntaxTree>();
                        for (int i = 2; i < parts.Length; i++)
                        {
                            var src = File.ReadAllText(parts[i], Encoding.UTF8);
                            trees.Add(CSharpSyntaxTree.ParseText(src, path: parts[i], encoding: Encoding.UTF8));
                        }
                        var comp = CSharpCompilation.Create(
                            Path.GetFileNameWithoutExtension(dllPath) + "_" + Guid.NewGuid().ToString("N"),
                            trees,
                            refs,
                            new CSharpCompilationOptions(OutputKind.DynamicallyLinkedLibrary, nullableContextOptions: NullableContextOptions.Enable));
                        var tmpDll = dllPath + ".tmp." + Environment.ProcessId + ".dll";
                        var emitRes = comp.Emit(tmpDll);
                        if (!emitRes.Success)
                        {
                            var errs = string.Join("\\n", emitRes.Diagnostics.Where(d => d.Severity == DiagnosticSeverity.Error));
                            realOut.WriteLine("ERR\\t" + B64("C# compile failed:\\n" + errs));
                            continue;
                        }
                        File.Move(tmpDll, dllPath, overwrite: true);
                    }
                    realOut.WriteLine("OK\\t");
                }
                else if (cmd == "RUN")
                {
                    var dllPath = parts[1];
                    var target = parts[2];
                    var docText = Unb64(parts[3]);
                    if (!Assemblies.TryGetValue(dllPath, out var asm))
                    {
                        var alc = new AssemblyLoadContext(dllPath, isCollectible: false);
                        using var fs = File.OpenRead(dllPath);
                        asm = alc.LoadFromStream(fs);
                        Assemblies[dllPath] = asm;
                    }
                    var runnerType = asm.GetType("Runner") ?? throw new Exception("Runner class not found");
                    var method = runnerType.GetMethod("RunTarget", BindingFlags.Public | BindingFlags.Static)
                        ?? throw new Exception("RunTarget method not found");
                    var res = (string)method.Invoke(null, new object[] { target, docText })!;
                    realOut.WriteLine("OK\\t" + B64(res));
                }
                else if (cmd == "RUN_MAIN")
                {
                    var dllPath = parts[1];
                    var mainArgs = new string[parts.Length - 2];
                    for (int i = 2; i < parts.Length; i++)
                    {
                        mainArgs[i - 2] = Unb64(parts[i]);
                    }
                    if (!Assemblies.TryGetValue(dllPath, out var asm))
                    {
                        var alc = new AssemblyLoadContext(dllPath, isCollectible: false);
                        using var fs = File.OpenRead(dllPath);
                        asm = alc.LoadFromStream(fs);
                        Assemblies[dllPath] = asm;
                    }
                    var progType = asm.GetType("Program") ?? throw new Exception("Program class not found");
                    var mainMethod = progType.GetMethod("Main", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static)
                        ?? throw new Exception("Program.Main method not found");
                    var invokeArgs = mainMethod.GetParameters().Length == 0 ? Array.Empty<object>() : new object[] { mainArgs };
                    var sw = new StringWriter();
                    Console.SetOut(sw);
                    try
                    {
                        var ret = mainMethod.Invoke(null, invokeArgs);
                        if (ret is Task task)
                        {
                            task.GetAwaiter().GetResult();
                        }
                    }
                    finally
                    {
                        Console.SetOut(realOut);
                    }
                    realOut.WriteLine("OK\\t" + B64(sw.ToString()));
                }
            }
            catch (TargetInvocationException tie)
            {
                var inner = tie.InnerException ?? tie;
                realOut.WriteLine("ERR\\t" + B64(inner.GetType().Name + ": " + inner.Message));
            }
            catch (Exception ex)
            {
                realOut.WriteLine("ERR\\t" + B64(ex.GetType().Name + ": " + ex.Message));
            }
        }
    }
}
"""


def _find_roslyn_dir(dotnet_bin: str) -> Path:
    proc = subprocess.run(
        [dotnet_bin, "--list-sdks"],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=True,
    )
    for line in reversed(proc.stdout.strip().splitlines()):
        if "[" in line and "]" in line:
            ver, rest = line.split(" [", 1)
            sdk_base = Path(rest.rstrip("]")) / ver.strip()
            roslyn = sdk_base / "Roslyn" / "bincore"
            if (roslyn / "Microsoft.CodeAnalysis.CSharp.dll").is_file():
                return roslyn
    raise RuntimeError("Could not find Roslyn bincore in .NET SDK")


def _ensure_csharp_worker_binary() -> Path:
    dotnet_bin = _require_bin("dotnet")
    _CSHARP_WORKER_PROJ_DIR.mkdir(parents=True, exist_ok=True)
    _CSHARP_BINS_DIR.mkdir(parents=True, exist_ok=True)

    worker_exe = (
        _CSHARP_WORKER_PROJ_DIR / "bin" / f"CSharpWorker{_EXE_SUFFIX}"
    )
    roslyn_dir = _find_roslyn_dir(dotnet_bin)
    csproj_content = f"""<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <OutputType>Exe</OutputType>
    <TargetFramework>net8.0</TargetFramework>
    <Nullable>enable</Nullable>
    <NoWarn>$(NoWarn);NU1902;NU1903</NoWarn>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="AngleSharp" Version="1.2.0" />
    <PackageReference Include="AngleSharp.XPath" Version="2.0.4" />
    <Reference Include="Microsoft.CodeAnalysis">
      <HintPath>{roslyn_dir / "Microsoft.CodeAnalysis.dll"}</HintPath>
    </Reference>
    <Reference Include="Microsoft.CodeAnalysis.CSharp">
      <HintPath>{roslyn_dir / "Microsoft.CodeAnalysis.CSharp.dll"}</HintPath>
    </Reference>
  </ItemGroup>
</Project>
"""
    csproj_path = _CSHARP_WORKER_PROJ_DIR / "CSharpWorker.csproj"
    prog_path = _CSHARP_WORKER_PROJ_DIR / "Program.cs"
    stamp_path = _CSHARP_WORKER_PROJ_DIR / "bin" / ".built_hash"
    expected_stamp = _hash_files(
        {"csproj": csproj_content, "prog": _CSHARP_WORKER_PROGRAM}
    )
    need_build = (
        not worker_exe.is_file()
        or not stamp_path.is_file()
        or stamp_path.read_text(encoding="utf-8") != expected_stamp
    )
    if (
        not csproj_path.is_file()
        or csproj_path.read_text(encoding="utf-8") != csproj_content
    ):
        csproj_path.write_text(csproj_content, encoding="utf-8")
        need_build = True
    if (
        not prog_path.is_file()
        or prog_path.read_text(encoding="utf-8") != _CSHARP_WORKER_PROGRAM
    ):
        prog_path.write_text(_CSHARP_WORKER_PROGRAM, encoding="utf-8")
        need_build = True

    if need_build:
        for stray in _CSHARP_WORKER_PROJ_DIR.glob("*.cs"):
            if stray.name != "Program.cs":
                stray.unlink()
        proc = subprocess.run(
            [
                dotnet_bin,
                "build",
                "-c",
                "Debug",
                "-v",
                "q",
                "--nologo",
                "-o",
                str(_CSHARP_WORKER_PROJ_DIR / "bin"),
            ],
            cwd=_CSHARP_WORKER_PROJ_DIR,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
        if proc.returncode != 0:
            raise RuntimeError(
                f"Failed to build CSharpWorker:\n{proc.stderr}\n{proc.stdout}"
            )
        stamp_path.write_text(expected_stamp, encoding="utf-8")
    return worker_exe


class _CSharpWorkerSession:
    """Persistent .NET 8 Roslyn compiler and AssemblyLoadContext runner."""

    def __init__(self) -> None:
        self._proc: subprocess.Popen[str] | None = None

    def _ensure_proc(self) -> None:
        if self._proc is None or self._proc.poll() is not None:
            worker_exe = _ensure_csharp_worker_binary()
            self._proc = subprocess.Popen(
                [str(worker_exe)],
                cwd=_CSHARP_WORKER_PROJ_DIR,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                encoding="utf-8",
            )

    def _send(self, line: str) -> str:
        self._ensure_proc()
        assert (
            self._proc is not None
            and self._proc.stdin is not None
            and self._proc.stdout is not None
        )
        try:
            self._proc.stdin.write(line + "\n")
            self._proc.stdin.flush()
            resp = self._proc.stdout.readline()
        except (BrokenPipeError, OSError) as exc:
            self._proc = None
            raise RuntimeError("C# worker crashed or disconnected") from exc

        if not resp:
            stderr = (
                self._proc.stderr.read()
                if self._proc is not None and self._proc.stderr is not None
                else ""
            )
            self._proc = None
            raise RuntimeError(f"C# worker exited unexpectedly: {stderr}")

        status, _, payload = resp.rstrip("\r\n").partition("\t")
        if status != "OK":
            err_msg = (
                base64.b64decode(payload).decode("utf-8", errors="replace")
                if payload
                else "Unknown C# worker error"
            )
            raise RuntimeError(err_msg)
        return (
            base64.b64decode(payload).decode("utf-8") if payload else ""
        )

    def compile(self, dll_path: Path, cs_files: list[Path]) -> None:
        if dll_path.is_file():
            return
        files_part = "\t".join(str(p) for p in cs_files)
        self._send(f"COMPILE\t{dll_path}\t{files_part}")

    def run_target(self, dll_path: Path, target: str, doc_text: str) -> str:
        b64_doc = base64.b64encode(doc_text.encode("utf-8")).decode("ascii")
        return self._send(f"RUN\t{dll_path}\t{target}\t{b64_doc}")

    def run_main(self, dll_path: Path, args: list[str]) -> str:
        b64_args = [
            base64.b64encode(a.encode("utf-8")).decode("ascii") for a in args
        ]
        parts = ["RUN_MAIN", str(dll_path), *b64_args]
        return self._send("\t".join(parts))

    def close(self) -> None:
        if self._proc is not None and self._proc.poll() is None:
            try:
                if self._proc.stdin is not None:
                    self._proc.stdin.close()
                self._proc.terminate()
                self._proc.wait(timeout=2)
            except Exception:
                pass
        self._proc = None


CSHARP_WORKER = _CSharpWorkerSession()


def _get_or_build_csharp_schema_dll(
    schema_src_or_path: str | Path,
) -> Path:
    res = parsmith.generate(
        schema_src_or_path,
        "csharp",
        runtime_name="parsmith_runtime",
        module_stem="schema",
    )
    assert res.ok, f"parsmith.generate(csharp) failed:\n{res.format_text()}"
    info = parsmith.inspect_schema(schema_src_or_path)
    assert info.ok, f"parsmith.inspect_schema failed:\n{info.format_text()}"

    lines = [
        "#nullable enable",
        "using System;",
        "using System.Text.Json;",
        "",
        "public static class Runner",
        "{",
        "    public static string RunTarget(string target, string docText)",
        "    {",
        "        switch (target)",
        "        {",
    ]
    for s in info.structs:
        if s.kind == "rest":
            continue
        pascal = to_pascal_case(s.name).replace("-", "")
        lines.append(f'            case "struct:{pascal}":')
        if s.name != pascal:
            lines.append(f'            case "struct:{s.name}":')
        lines.append(
            f"                return JsonSerializer.Serialize(new {pascal}(docText).Parse());"
        )
    for fn in info.functions:
        pascal_fn = to_pascal_case(fn.name).replace("-", "")
        snake_fn = to_snake_case(fn.name)
        lines.append(f'            case "fn:{snake_fn}":')
        if fn.name != snake_fn:
            lines.append(f'            case "fn:{fn.name}":')
        lines.append(
            f"                return JsonSerializer.Serialize(Functions.{pascal_fn}(docText));"
        )
    lines.extend(
        [
            "            default:",
            '                throw new ArgumentException($"unknown target: {target}");',
            "        }",
            "    }",
            "}",
            "",
        ]
    )

    files = dict(res.files)
    files["Runner.cs"] = "\n".join(lines)
    digest = _hash_files(files)
    _CSHARP_BINS_DIR.mkdir(parents=True, exist_ok=True)
    dll_path = _CSHARP_BINS_DIR / f"cs_{digest}.dll"
    if not dll_path.is_file():
        src_dir = _CSHARP_BINS_DIR / f"src_{digest}"
        src_dir.mkdir(parents=True, exist_ok=True)
        cs_paths: list[Path] = []
        for fname, content in sorted(files.items()):
            p = src_dir / fname
            p.write_text(content, encoding="utf-8")
            cs_paths.append(p)
        CSHARP_WORKER.compile(dll_path, cs_paths)
    return dll_path


def run_csharp_schema(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
) -> Any:
    """Compile KDL schema to C#, execute new <Struct>(docText).Parse(), and return decoded JSON."""
    dll_path = _get_or_build_csharp_schema_dll(schema_src_or_path)
    pascal = to_pascal_case(struct_name).replace("-", "")
    out = CSHARP_WORKER.run_target(dll_path, f"struct:{pascal}", doc_text)
    return json.loads(out)


def run_csharp_fn(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
) -> Any:
    """Compile KDL schema to C#, execute Functions.<Fn>(input_text), and return decoded JSON."""
    dll_path = _get_or_build_csharp_schema_dll(schema_src_or_path)
    out = CSHARP_WORKER.run_target(
        dll_path, f"fn:{to_snake_case(fn_name)}", input_text
    )
    return json.loads(out)


def run_csharp_custom_program(
    files: dict[str, str],
) -> str:
    """Compile a custom C# `Program.cs` + modules file set in the cached workspace and return stdout."""
    digest = _hash_files(files)
    _CSHARP_BINS_DIR.mkdir(parents=True, exist_ok=True)
    dll_path = _CSHARP_BINS_DIR / f"cs_custom_{digest}.dll"
    if not dll_path.is_file():
        src_dir = _CSHARP_BINS_DIR / f"src_custom_{digest}"
        src_dir.mkdir(parents=True, exist_ok=True)
        cs_paths: list[Path] = []
        for fname, content in sorted(files.items()):
            p = src_dir / fname
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding="utf-8")
            cs_paths.append(p)
        CSHARP_WORKER.compile(dll_path, cs_paths)
    return CSHARP_WORKER.run_main(dll_path, [])


_KOTLIN_WORKER_JAVA = """import org.jetbrains.kotlin.cli.common.ExitCode;
import org.jetbrains.kotlin.cli.jvm.K2JVMCompiler;
import java.io.*;
import java.lang.reflect.InvocationTargetException;
import java.net.URL;
import java.net.URLClassLoader;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.StandardCopyOption;
import java.util.*;

public class KotlinWorker {
    private static final Map<String, URLClassLoader> LOADERS = new HashMap<>();

    private static String b64(String s) {
        return Base64.getEncoder().encodeToString(s.getBytes(StandardCharsets.UTF_8));
    }

    private static String unb64(String s) {
        return new String(Base64.getDecoder().decode(s), StandardCharsets.UTF_8);
    }

    public static void main(String[] args) throws Exception {
        String runtimeCp = args[0];
        K2JVMCompiler compiler = new K2JVMCompiler();
        BufferedReader reader = new BufferedReader(new InputStreamReader(System.in, StandardCharsets.UTF_8));
        PrintStream realOut = System.out;
        String line;
        while ((line = reader.readLine()) != null) {
            if (line.isEmpty()) continue;
            String[] parts = line.split("\\t");
            String cmd = parts[0];
            try {
                if ("COMPILE".equals(cmd)) {
                    String jarPath = parts[1];
                    File jarFile = new File(jarPath);
                    if (!jarFile.isFile()) {
                        File tmpJar = new File(jarPath + ".tmp." + ProcessHandle.current().pid() + ".jar");
                        List<String> cArgs = new ArrayList<>();
                        cArgs.add("-no-stdlib");
                        cArgs.add("-no-reflect");
                        cArgs.add("-cp");
                        cArgs.add(runtimeCp);
                        cArgs.add("-d");
                        cArgs.add(tmpJar.getAbsolutePath());
                        for (int i = 2; i < parts.length; i++) {
                            cArgs.add(parts[i]);
                        }
                        ByteArrayOutputStream errBuf = new ByteArrayOutputStream();
                        PrintStream errPs = new PrintStream(errBuf, true, StandardCharsets.UTF_8);
                        ExitCode code = compiler.exec(errPs, cArgs.toArray(new String[0]));
                        if (code != ExitCode.OK) {
                            realOut.println("ERR\\t" + b64("kotlinc failed (" + code + "):\\n" + errBuf.toString(StandardCharsets.UTF_8)));
                            realOut.flush();
                            continue;
                        }
                        Files.move(tmpJar.toPath(), jarFile.toPath(), StandardCopyOption.REPLACE_EXISTING);
                    }
                    realOut.println("OK\\t");
                    realOut.flush();
                } else if ("RUN".equals(cmd)) {
                    String jarPath = parts[1];
                    String target = parts[2];
                    String docText = unb64(parts[3]);
                    URLClassLoader loader = LOADERS.get(jarPath);
                    if (loader == null) {
                        loader = new URLClassLoader(new URL[]{new File(jarPath).toURI().toURL()}, KotlinWorker.class.getClassLoader());
                        LOADERS.put(jarPath, loader);
                    }
                    Class<?> cls = Class.forName("RunnerKt", true, loader);
                    Object res = cls.getMethod("runTarget", String.class, String.class).invoke(null, target, docText);
                    realOut.println("OK\\t" + b64((String) res));
                    realOut.flush();
                } else if ("RUN_MAIN".equals(cmd)) {
                    String jarPath = parts[1];
                    String[] mainArgs = new String[parts.length - 2];
                    for (int i = 2; i < parts.length; i++) {
                        mainArgs[i - 2] = unb64(parts[i]);
                    }
                    URLClassLoader loader = LOADERS.get(jarPath);
                    if (loader == null) {
                        loader = new URLClassLoader(new URL[]{new File(jarPath).toURI().toURL()}, KotlinWorker.class.getClassLoader());
                        LOADERS.put(jarPath, loader);
                    }
                    Class<?> cls = Class.forName("MainKt", true, loader);
                    ByteArrayOutputStream outBuf = new ByteArrayOutputStream();
                    PrintStream capturePs = new PrintStream(outBuf, true, StandardCharsets.UTF_8);
                    System.setOut(capturePs);
                    try {
                        try {
                            cls.getMethod("main", String[].class).invoke(null, (Object) mainArgs);
                        } catch (NoSuchMethodException nsme) {
                            cls.getMethod("main").invoke(null);
                        }
                    } finally {
                        System.setOut(realOut);
                    }
                    realOut.println("OK\\t" + b64(outBuf.toString(StandardCharsets.UTF_8)));
                    realOut.flush();
                }
            } catch (InvocationTargetException ite) {
                Throwable cause = ite.getTargetException() != null ? ite.getTargetException() : ite;
                String msg = cause.getMessage() != null ? cause.getMessage() : cause.toString();
                realOut.println("ERR\\t" + b64(cause.getClass().getSimpleName() + ": " + msg));
                realOut.flush();
            } catch (Throwable t) {
                String msg = t.getMessage() != null ? t.getMessage() : t.toString();
                realOut.println("ERR\\t" + b64(t.getClass().getSimpleName() + ": " + msg));
                realOut.flush();
            }
        }
    }
}
"""


def _resolve_kotlin_classpaths() -> tuple[str, str]:
    gradle_files = Path.home() / ".gradle" / "caches" / "modules-2" / "files-2.1"

    def find_opt(pattern: str) -> str | None:
        if not gradle_files.is_dir():
            return None
        matches = sorted(gradle_files.rglob(pattern))
        return str(matches[-1]) if matches else None

    if find_opt("jsoup-1.18.3.jar") is None or find_opt("kotlin-compiler-embeddable-2.1.10.jar") is None:
        gradle_cmd = _require_bin("gradle")
        _KOTLIN_CACHE_DIR.mkdir(parents=True, exist_ok=True)
        (_KOTLIN_CACHE_DIR / "settings.gradle.kts").write_text(
            'rootProject.name = "parsmith-e2e-kt"\n', encoding="utf-8"
        )
        (_KOTLIN_CACHE_DIR / "build.gradle.kts").write_text(
            'plugins { kotlin("jvm") version "2.1.10" }\n'
            "repositories { mavenCentral() }\n"
            "dependencies {\n"
            '    implementation("org.jsoup:jsoup:1.18.3")\n'
            '    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.8.0")\n'
            "}\n",
            encoding="utf-8",
        )
        subprocess.run(
            [gradle_cmd, "classes", "--console=plain", "-q"],
            cwd=_KOTLIN_CACHE_DIR,
            check=True,
        )

    def find_req(pattern: str) -> str:
        res = find_opt(pattern)
        if res is None:
            raise RuntimeError(f"Missing required Kotlin JAR in Gradle cache: {pattern}")
        return res

    runtime_jars = [
        find_req("kotlin-stdlib-2.1.10.jar"),
        find_req("jsoup-1.18.3.jar"),
        find_req("kotlinx-serialization-core-jvm-1.8.0.jar"),
        find_req("kotlinx-serialization-json-jvm-1.8.0.jar"),
    ]
    compiler_jars = [
        find_req("kotlin-compiler-embeddable-2.1.10.jar"),
        find_req("kotlin-reflect-*.jar"),
        find_req("kotlin-script-runtime-2.1.10.jar"),
        find_req("kotlin-daemon-embeddable-2.1.10.jar"),
        find_req("trove4j-*.jar"),
        find_req("kotlinx-coroutines-core-jvm-1.8.0.jar"),
        find_req("jna-5.*.jar"),
        find_req("annotations-23.0.0.jar"),
        *runtime_jars,
    ]
    return os.pathsep.join(compiler_jars), os.pathsep.join(runtime_jars)


def _ensure_kotlin_worker() -> tuple[str, str, Path]:
    compiler_cp, runtime_cp = _resolve_kotlin_classpaths()
    _KOTLIN_WORKER_DIR.mkdir(parents=True, exist_ok=True)
    _KOTLIN_BINS_DIR.mkdir(parents=True, exist_ok=True)

    java_file = _KOTLIN_WORKER_DIR / "KotlinWorker.java"
    class_file = _KOTLIN_WORKER_DIR / "KotlinWorker.class"
    if (
        not class_file.is_file()
        or not java_file.is_file()
        or java_file.read_text(encoding="utf-8") != _KOTLIN_WORKER_JAVA
    ):
        javac_bin = _require_bin("javac")
        java_file.write_text(_KOTLIN_WORKER_JAVA, encoding="utf-8")
        proc = subprocess.run(
            [javac_bin, "-cp", compiler_cp, str(java_file)],
            cwd=_KOTLIN_WORKER_DIR,
            capture_output=True,
            text=True,
            encoding="utf-8",
            check=False,
        )
        if proc.returncode != 0:
            raise RuntimeError(
                f"javac KotlinWorker.java failed:\n{proc.stderr}\n{proc.stdout}"
            )
    return compiler_cp, runtime_cp, _KOTLIN_WORKER_DIR


class _KotlinWorkerSession:
    """Persistent JVM K2JVMCompiler and URLClassLoader execution worker."""

    def __init__(self) -> None:
        self._proc: subprocess.Popen[str] | None = None

    def _ensure_proc(self) -> None:
        if self._proc is None or self._proc.poll() is not None:
            java_bin = _require_bin("java")
            compiler_cp, runtime_cp, worker_dir = _ensure_kotlin_worker()
            full_cp = compiler_cp + os.pathsep + str(worker_dir)
            self._proc = subprocess.Popen(
                [java_bin, "-cp", full_cp, "KotlinWorker", runtime_cp],
                cwd=worker_dir,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                encoding="utf-8",
            )

    def _send(self, line: str) -> str:
        self._ensure_proc()
        assert (
            self._proc is not None
            and self._proc.stdin is not None
            and self._proc.stdout is not None
        )
        try:
            self._proc.stdin.write(line + "\n")
            self._proc.stdin.flush()
            resp = self._proc.stdout.readline()
        except (BrokenPipeError, OSError) as exc:
            self._proc = None
            raise RuntimeError("Kotlin worker crashed or disconnected") from exc

        if not resp:
            stderr = (
                self._proc.stderr.read()
                if self._proc is not None and self._proc.stderr is not None
                else ""
            )
            self._proc = None
            raise RuntimeError(f"Kotlin worker exited unexpectedly: {stderr}")

        status, _, payload = resp.rstrip("\r\n").partition("\t")
        if status != "OK":
            err_msg = (
                base64.b64decode(payload).decode("utf-8", errors="replace")
                if payload
                else "Unknown Kotlin worker error"
            )
            raise RuntimeError(err_msg)
        return (
            base64.b64decode(payload).decode("utf-8") if payload else ""
        )

    def compile(self, jar_path: Path, kt_files: list[Path]) -> None:
        if jar_path.is_file():
            return
        files_part = "\t".join(str(p) for p in kt_files)
        self._send(f"COMPILE\t{jar_path}\t{files_part}")

    def run_target(self, jar_path: Path, target: str, doc_text: str) -> str:
        b64_doc = base64.b64encode(doc_text.encode("utf-8")).decode("ascii")
        return self._send(f"RUN\t{jar_path}\t{target}\t{b64_doc}")

    def run_main(self, jar_path: Path, args: list[str]) -> str:
        b64_args = [
            base64.b64encode(a.encode("utf-8")).decode("ascii") for a in args
        ]
        parts = ["RUN_MAIN", str(jar_path), *b64_args]
        return self._send("\t".join(parts))

    def close(self) -> None:
        if self._proc is not None and self._proc.poll() is None:
            try:
                if self._proc.stdin is not None:
                    self._proc.stdin.close()
                self._proc.terminate()
                self._proc.wait(timeout=2)
            except Exception:
                pass
        self._proc = None


KOTLIN_WORKER = _KotlinWorkerSession()


def _get_or_build_kotlin_schema_jar(
    schema_src_or_path: str | Path,
) -> Path:
    res = parsmith.generate(
        schema_src_or_path,
        "kotlin",
        runtime_name="parsmith_runtime",
        module_stem="schema",
    )
    assert res.ok, f"parsmith.generate(kotlin) failed:\n{res.format_text()}"
    info = parsmith.inspect_schema(schema_src_or_path)
    assert info.ok, f"parsmith.inspect_schema failed:\n{info.format_text()}"

    lines = [
        "fun runTarget(target: String, docText: String): String = when (target) {",
    ]
    for s in info.structs:
        if s.kind == "rest":
            continue
        pascal = to_pascal_case(s.name).replace("-", "")
        conds = [f'"struct:{pascal}"']
        if s.name != pascal:
            conds.append(f'"struct:{s.name}"')
        lines.append(
            f"    {', '.join(conds)} -> Rt.toJsonString({pascal}(docText).parse())"
        )
    for fn in info.functions:
        snake_fn = to_snake_case(fn.name)
        conds = [f'"fn:{snake_fn}"']
        if fn.name != snake_fn:
            conds.append(f'"fn:{fn.name}"')
        lines.append(
            f"    {', '.join(conds)} -> Rt.toJsonString({snake_fn}(docText))"
        )
    lines.extend(
        [
            '    else -> throw IllegalArgumentException("unknown target: $target")',
            "}",
            "",
        ]
    )

    files = dict(res.files)
    files["Runner.kt"] = "\n".join(lines)
    digest = _hash_files(files)
    _KOTLIN_BINS_DIR.mkdir(parents=True, exist_ok=True)
    jar_path = _KOTLIN_BINS_DIR / f"kt_{digest}.jar"
    if not jar_path.is_file():
        src_dir = _KOTLIN_BINS_DIR / f"src_{digest}"
        src_dir.mkdir(parents=True, exist_ok=True)
        kt_paths: list[Path] = []
        for fname, content in sorted(files.items()):
            p = src_dir / fname
            p.write_text(content, encoding="utf-8")
            kt_paths.append(p)
        KOTLIN_WORKER.compile(jar_path, kt_paths)
    return jar_path


def run_kotlin_schema(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
) -> Any:
    """Compile KDL schema to Kotlin, execute <Struct>(docText).parse(), and return decoded JSON."""
    jar_path = _get_or_build_kotlin_schema_jar(schema_src_or_path)
    pascal = to_pascal_case(struct_name).replace("-", "")
    out = KOTLIN_WORKER.run_target(jar_path, f"struct:{pascal}", doc_text)
    return json.loads(out)


def run_kotlin_fn(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
) -> Any:
    """Compile KDL schema to Kotlin, execute <fn_name>(input_text), and return decoded JSON."""
    jar_path = _get_or_build_kotlin_schema_jar(schema_src_or_path)
    out = KOTLIN_WORKER.run_target(
        jar_path, f"fn:{to_snake_case(fn_name)}", input_text
    )
    return json.loads(out)


def run_kotlin_custom_program(
    files: dict[str, str],
) -> str:
    """Compile a custom Kotlin `Main.kt` + modules file set in the cached workspace and return stdout."""
    digest = _hash_files(files)
    _KOTLIN_BINS_DIR.mkdir(parents=True, exist_ok=True)
    jar_path = _KOTLIN_BINS_DIR / f"kt_custom_{digest}.jar"
    if not jar_path.is_file():
        src_dir = _KOTLIN_BINS_DIR / f"src_custom_{digest}"
        src_dir.mkdir(parents=True, exist_ok=True)
        kt_paths: list[Path] = []
        for fname, content in sorted(files.items()):
            p = src_dir / fname
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding="utf-8")
            kt_paths.append(p)
        KOTLIN_WORKER.compile(jar_path, kt_paths)
    return KOTLIN_WORKER.run_main(jar_path, [])


def _close_all_workers() -> None:
    JS_WORKER.close()
    ELIXIR_WORKER.close()
    CSHARP_WORKER.close()
    KOTLIN_WORKER.close()


atexit.register(_close_all_workers)


def assert_cross_target_parity(
    schema_src_or_path: str | Path,
    struct_name: str,
    doc_text: str,
    *,
    strip_none_dict_values: bool = False,
) -> Any:
    """Run schema across all 4 Python backends, dataclass, separate_runtime, JS, Go, Rust, Elixir, PHP (native & dom-crawler), C#, and Kotlin; assert parity."""
    baseline_raw = run_py_schema(
        schema_src_or_path,
        struct_name,
        doc_text,
        target="py-bs4",
        models="typeddict",
    )
    baseline = normalize_for_parity(
        baseline_raw,
        strip_none_dict_values=strip_none_dict_values,
    )

    for target in PY_HTML_TARGETS[1:]:
        target_res = normalize_for_parity(
            run_py_schema(
                schema_src_or_path,
                struct_name,
                doc_text,
                target=target,
                models="typeddict",
            ),
            strip_none_dict_values=strip_none_dict_values,
        )
        assert target_res == baseline, (
            f"Target {target} differed from py-bs4 for {struct_name}"
        )

    dc_raw = run_py_schema(
        schema_src_or_path,
        struct_name,
        doc_text,
        target="py-bs4",
        models="dataclass",
    )
    dc_res = normalize_for_parity(
        dc_raw,
        strip_none_dict_values=strip_none_dict_values,
    )
    assert dc_res == baseline, (
        f"Dataclass mode differed from typeddict for {struct_name}"
    )

    sep_raw = run_py_schema(
        schema_src_or_path,
        struct_name,
        doc_text,
        target="py-bs4",
        models="typeddict",
        separate_runtime=True,
    )
    sep_res = normalize_for_parity(
        sep_raw,
        strip_none_dict_values=strip_none_dict_values,
    )
    assert sep_res == baseline, (
        f"separate_runtime=True differed from inline runtime for {struct_name}"
    )

    js_raw = run_js_schema(schema_src_or_path, struct_name, doc_text)
    js_res = normalize_for_parity(
        js_raw,
        camel_to_snake=True,
        strip_none_dict_values=strip_none_dict_values,
    )
    assert js_res == baseline, (
        f"JS target differed from py-bs4 for {struct_name}"
    )

    go_raw = run_go_schema(schema_src_or_path, struct_name, doc_text)
    go_res = normalize_for_parity(
        go_raw,
        strip_none_dict_values=strip_none_dict_values,
    )
    assert go_res == baseline, (
        f"Go target differed from py-bs4 for {struct_name}"
    )

    rust_raw = run_rust_schema(schema_src_or_path, struct_name, doc_text)
    rust_res = normalize_for_parity(
        rust_raw,
        strip_none_dict_values=strip_none_dict_values,
    )
    assert rust_res == baseline, (
        f"Rust target differed from py-bs4 for {struct_name}"
    )

    ex_raw = run_elixir_schema(schema_src_or_path, struct_name, doc_text)
    ex_res = normalize_for_parity(
        ex_raw,
        strip_none_dict_values=strip_none_dict_values,
    )
    assert ex_res == baseline, (
        f"Elixir target differed from py-bs4 for {struct_name}"
    )

    for html_lib in ("native", "dom-crawler"):
        php_raw = run_php_schema(
            schema_src_or_path,
            struct_name,
            doc_text,
            html_lib=html_lib,
        )
        php_res = normalize_for_parity(
            php_raw,
            strip_none_dict_values=strip_none_dict_values,
        )
        assert php_res == baseline, (
            f"PHP ({html_lib}) target differed from py-bs4 for {struct_name}"
        )

    cs_raw = run_csharp_schema(schema_src_or_path, struct_name, doc_text)
    cs_res = normalize_for_parity(
        cs_raw,
        strip_none_dict_values=strip_none_dict_values,
    )
    assert cs_res == baseline, (
        f"C# target differed from py-bs4 for {struct_name}"
    )

    kt_raw = run_kotlin_schema(schema_src_or_path, struct_name, doc_text)
    kt_res = normalize_for_parity(
        kt_raw,
        strip_none_dict_values=strip_none_dict_values,
    )
    assert kt_res == baseline, (
        f"Kotlin target differed from py-bs4 for {struct_name}"
    )

    return baseline_raw


def assert_fn_cross_target_parity(
    schema_src_or_path: str | Path,
    fn_name: str,
    input_text: str,
) -> Any:
    """Run standalone fn across all 4 Python backends, separate_runtime, JS, Go, Rust, Elixir, PHP, C#, and Kotlin; assert parity."""
    baseline = run_py_fn(
        schema_src_or_path,
        fn_name,
        input_text,
        target="py-bs4",
    )
    for target in PY_HTML_TARGETS[1:]:
        res = run_py_fn(schema_src_or_path, fn_name, input_text, target=target)
        assert res == baseline, (
            f"Target {target} differed from py-bs4 for fn {fn_name}"
        )

    sep_res = run_py_fn(
        schema_src_or_path,
        fn_name,
        input_text,
        target="py-bs4",
        separate_runtime=True,
    )
    assert sep_res == baseline, (
        f"separate_runtime=True differed for fn {fn_name}"
    )

    js_res = run_js_fn(schema_src_or_path, fn_name, input_text)
    assert js_res == baseline, (
        f"JS target differed from py-bs4 for fn {fn_name}"
    )

    go_res = run_go_fn(schema_src_or_path, fn_name, input_text)
    assert go_res == baseline, (
        f"Go target differed from py-bs4 for fn {fn_name}"
    )

    rust_res = run_rust_fn(schema_src_or_path, fn_name, input_text)
    assert rust_res == baseline, (
        f"Rust target differed from py-bs4 for fn {fn_name}"
    )

    ex_res = run_elixir_fn(schema_src_or_path, fn_name, input_text)
    assert ex_res == baseline, (
        f"Elixir target differed from py-bs4 for fn {fn_name}"
    )

    for html_lib in ("native", "dom-crawler"):
        php_res = run_php_fn(
            schema_src_or_path,
            fn_name,
            input_text,
            html_lib=html_lib,
        )
        assert php_res == baseline, (
            f"PHP ({html_lib}) target differed from py-bs4 for fn {fn_name}"
        )

    cs_res = run_csharp_fn(schema_src_or_path, fn_name, input_text)
    assert cs_res == baseline, (
        f"C# target differed from py-bs4 for fn {fn_name}"
    )

    kt_res = run_kotlin_fn(schema_src_or_path, fn_name, input_text)
    assert kt_res == baseline, (
        f"Kotlin target differed from py-bs4 for fn {fn_name}"
    )

    return baseline
