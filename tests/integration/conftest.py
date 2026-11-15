"""Session fixtures for parsmith integration tests."""

from __future__ import annotations

from collections.abc import Iterator
import pytest

from tests.fake_server import FakeServer
from tests.integration.helpers import (
    HTML_FIXTURE,
    cleanup_registered_modules,
    close_js_worker,
)


@pytest.fixture(scope="session", autouse=True)
def _cleanup_integration_resources() -> Iterator[None]:
    """Ensure the persistent Node.js worker and dynamic modules are cleaned up after test session."""
    yield
    close_js_worker()
    cleanup_registered_modules()


@pytest.fixture(scope="session")
def fake_server() -> Iterator[FakeServer]:
    """Start a session-scoped loopback FakeServer on an ephemeral port (127.0.0.1:0)."""
    with FakeServer(host="127.0.0.1", port=0) as server:
        yield server


@pytest.fixture(scope="session")
def html() -> str:
    """Return the contents of tests/integration/fixtures/dsl_coverage.html."""
    return HTML_FIXTURE.read_text(encoding="utf-8")
