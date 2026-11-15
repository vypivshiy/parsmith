export function rtDispatchResponse(matchers, resp, valueFn = null) {
  const status = resp.status;
  const headers = resp.headers ?? {};
  const rawText = resp.text ?? "";
  let parsedBody = null;
  let jsonErr = null;
  try {
    parsedBody = JSON.parse(rawText);
  } catch (err) {
    jsonErr = err;
  }

  let lastDecodeErr = null;
  let matchedErrorRule = false;
  for (const matcher of matchers) {
    if (status !== matcher.status) {
      continue;
    }
    if (matcher.check !== null) {
      if (
        jsonErr !== null ||
        typeof parsedBody !== "object" ||
        parsedBody === null ||
        Array.isArray(parsedBody) ||
        !matcher.check(parsedBody)
      ) {
        continue;
      }
    }
    matchedErrorRule = true;
    if (jsonErr !== null) {
      lastDecodeErr = jsonErr;
      continue;
    }
    try {
      return matcher.factory(headers, parsedBody);
    } catch (err) {
      lastDecodeErr = err;
    }
  }

  if (status >= 200 && status < 300 && !matchedErrorRule) {
    if (valueFn === null) {
      return {
        ok: true,
        kind: "ok",
        status,
        headers,
        value: null,
      };
    }
    if (jsonErr !== null) {
      return {
        ok: false,
        kind: "decode",
        status,
        headers,
        body: rawText,
        cause: jsonErr instanceof Error ? jsonErr.message : String(jsonErr),
        exception: jsonErr,
      };
    }
    try {
      const val = valueFn(parsedBody);
      return {
        ok: true,
        kind: "ok",
        status,
        headers,
        value: val,
      };
    } catch (err) {
      return {
        ok: false,
        kind: "decode",
        status,
        headers,
        body: parsedBody,
        cause: err instanceof Error ? err.message : String(err),
        exception: err,
      };
    }
  }

  if (matchedErrorRule && lastDecodeErr !== null) {
    return {
      ok: false,
      kind: "decode",
      status,
      headers,
      body: jsonErr === null ? parsedBody : rawText,
      cause: lastDecodeErr instanceof Error ? lastDecodeErr.message : String(lastDecodeErr),
      exception: lastDecodeErr,
    };
  }

  const fallbackBody = jsonErr === null ? parsedBody : rawText ? rawText : null;
  return {
    ok: false,
    kind: "unknown",
    status,
    headers,
    body: fallbackBody,
  };
}

export async function rtRestCall(transport, req, matchers, valueFn = null) {
  let resp;
  try {
    resp = await transport.send(req);
  } catch (err) {
    if (err instanceof TransportError) {
      const causeVal = err.cause ?? err;
      return {
        ok: false,
        kind: "transport",
        status: 0,
        cause: String(causeVal),
        exception: causeVal,
      };
    }
    throw err;
  }
  return rtDispatchResponse(matchers, resp, valueFn);
}
