export interface Ok<T> {
  readonly ok: true;
  readonly kind: "ok";
  readonly status: number;
  readonly headers: Record<string, string>;
  readonly value: T;
}

export interface UnknownErr {
  readonly ok: false;
  readonly kind: "unknown";
  readonly status: number;
  readonly headers: Record<string, string>;
  readonly body: unknown;
}

export interface TransportErr {
  readonly ok: false;
  readonly kind: "transport";
  readonly status: 0;
  readonly cause: string;
  readonly exception: unknown;
}

export interface DecodeErr {
  readonly ok: false;
  readonly kind: "decode";
  readonly status: number;
  readonly headers: Record<string, string>;
  readonly body: unknown;
  readonly cause: string;
  readonly exception: unknown;
}

export interface ErrMatcher {
  readonly status: number;
  readonly check: ((body: Record<string, any>) => boolean) | null;
  readonly factory: (headers: Record<string, string>, value: any) => any;
}

export function rtDispatchResponse(
  matchers: ErrMatcher[],
  resp: RawResponse,
  valueFn: ((body: any) => any) | null = null,
): any {
  const status = resp.status;
  const headers = resp.headers ?? {};
  const rawText = resp.text ?? "";
  let parsedBody: any = null;
  let jsonErr: unknown = null;
  try {
    parsedBody = JSON.parse(rawText);
  } catch (err) {
    jsonErr = err;
  }

  let lastDecodeErr: unknown = null;
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

export async function rtRestCall(
  transport: HttpTransport,
  req: HttpRequest,
  matchers: ErrMatcher[],
  valueFn: ((body: any) => any) | null = null,
): Promise<any> {
  let resp: RawResponse;
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
