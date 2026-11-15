export interface HttpRequest {
  method: string;
  url: string;
  headers: Record<string, string>;
  params: Record<string, string | string[]>;
  cookies: Record<string, string>;
  body?: unknown;
  bodyKind?: "empty" | "json" | "form" | "raw";
  extra?: Record<string, unknown>;
}

export interface RawResponse {
  status: number;
  headers: Record<string, string>;
  text: string;
}

export class TransportError extends Error {
  readonly cause: unknown;

  constructor(cause: unknown) {
    super(cause instanceof Error ? cause.message : String(cause));
    this.name = "TransportError";
    this.cause = cause;
  }
}

export interface HttpTransport {
  send(req: HttpRequest): Promise<RawResponse>;
}

export interface AxiosLike {
  request(config: Record<string, any>): Promise<{
    status: number;
    headers?: Record<string, any>;
    data?: any;
  }>;
}

export class FetchTransport implements HttpTransport {
  private _fetchFn: typeof globalThis.fetch;

  constructor(fetchFn: typeof globalThis.fetch = globalThis.fetch) {
    this._fetchFn = fetchFn;
  }

  async send(req: HttpRequest): Promise<RawResponse> {
    let url = req.url;
    if (req.params && Object.keys(req.params).length > 0) {
      const sp = new URLSearchParams();
      for (const [k, v] of Object.entries(req.params)) {
        if (Array.isArray(v)) {
          for (const item of v) {
            if (item != null) sp.append(k, String(item));
          }
        } else if (v != null) {
          sp.append(k, String(v));
        }
      }
      const qs = sp.toString();
      if (qs) {
        url += (url.includes("?") ? "&" : "?") + qs;
      }
    }
    const headers: Record<string, string> = { ...(req.headers ?? {}) };
    if (req.cookies && Object.keys(req.cookies).length > 0) {
      const cookieStr = Object.entries(req.cookies)
        .map(([k, v]) => `${k}=${v}`)
        .join("; ");
      if (cookieStr) {
        headers["Cookie"] = headers["Cookie"] ? `${headers["Cookie"]}; ${cookieStr}` : cookieStr;
      }
    }
    let bodyInit: BodyInit | undefined = undefined;
    const bodyKind = req.bodyKind ?? "empty";
    if (bodyKind === "json" && req.body !== undefined) {
      bodyInit = typeof req.body === "string" ? req.body : JSON.stringify(req.body);
      if (!Object.keys(headers).some((k) => k.toLowerCase() === "content-type")) {
        headers["Content-Type"] = "application/json";
      }
    } else if (bodyKind === "form" && req.body != null) {
      if (req.body instanceof URLSearchParams) {
        bodyInit = req.body.toString();
      } else if (typeof req.body === "object" && !Array.isArray(req.body)) {
        const sp = new URLSearchParams();
        for (const [k, v] of Object.entries(req.body as Record<string, any>)) {
          if (v != null) sp.append(k, String(v));
        }
        bodyInit = sp.toString();
      } else {
        bodyInit = String(req.body);
      }
      if (!Object.keys(headers).some((k) => k.toLowerCase() === "content-type")) {
        headers["Content-Type"] = "application/x-www-form-urlencoded";
      }
    } else if (bodyKind === "raw" && req.body != null) {
      bodyInit = String(req.body);
    }
    const init: RequestInit = {
      ...(req.extra ?? {}),
      method: req.method,
      headers,
    };
    if (bodyInit !== undefined) {
      init.body = bodyInit;
    }
    try {
      const fn = this._fetchFn ?? globalThis.fetch;
      const resp = await fn(url, init);
      const text =
        typeof resp.text === "function" ? await resp.text() : String((resp as any).text ?? "");
      const respHeaders: Record<string, string> = {};
      if (resp.headers) {
        if (typeof (resp.headers as any).forEach === "function") {
          (resp.headers as any).forEach((v: string, k: string) => {
            respHeaders[String(k).toLowerCase()] = String(v);
          });
        } else if (typeof resp.headers === "object") {
          for (const [k, v] of Object.entries(resp.headers)) {
            if (v != null) respHeaders[String(k).toLowerCase()] = String(v);
          }
        }
      }
      return {
        status: Number(resp.status),
        headers: respHeaders,
        text,
      };
    } catch (err) {
      throw new TransportError(err);
    }
  }
}

export class AxiosTransport implements HttpTransport {
  private _instance: AxiosLike;

  constructor(instance: AxiosLike) {
    this._instance = instance;
  }

  async send(req: HttpRequest): Promise<RawResponse> {
    const headers: Record<string, string> = { ...(req.headers ?? {}) };
    if (req.cookies && Object.keys(req.cookies).length > 0) {
      const cookieStr = Object.entries(req.cookies)
        .map(([k, v]) => `${k}=${v}`)
        .join("; ");
      if (cookieStr) {
        headers["Cookie"] = headers["Cookie"] ? `${headers["Cookie"]}; ${cookieStr}` : cookieStr;
      }
    }
    const config: Record<string, any> = {
      ...(req.extra ?? {}),
      method: req.method,
      url: req.url,
      headers,
      responseType: "text",
      transformResponse: [(d: any) => d],
      validateStatus: () => true,
    };
    if (req.params && Object.keys(req.params).length > 0) {
      config.params = req.params;
    }
    const bodyKind = req.bodyKind ?? "empty";
    if (bodyKind === "json" && req.body !== undefined) {
      config.data = req.body;
      if (!Object.keys(headers).some((k) => k.toLowerCase() === "content-type")) {
        headers["Content-Type"] = "application/json";
      }
    } else if (bodyKind === "form" && req.body != null) {
      if (req.body instanceof URLSearchParams) {
        config.data = req.body.toString();
      } else if (typeof req.body === "object" && !Array.isArray(req.body)) {
        const sp = new URLSearchParams();
        for (const [k, v] of Object.entries(req.body as Record<string, any>)) {
          if (v != null) sp.append(k, String(v));
        }
        config.data = sp.toString();
      } else {
        config.data = String(req.body);
      }
      if (!Object.keys(headers).some((k) => k.toLowerCase() === "content-type")) {
        headers["Content-Type"] = "application/x-www-form-urlencoded";
      }
    } else if (bodyKind === "raw" && req.body != null) {
      config.data = req.body;
    }
    const normalizeAxiosResp = (resp: any): RawResponse => {
      const respHeaders: Record<string, string> = {};
      if (resp.headers && typeof resp.headers === "object") {
        for (const [k, v] of Object.entries(resp.headers)) {
          if (v != null) respHeaders[String(k).toLowerCase()] = String(v);
        }
      }
      const rawData = resp.data;
      const text =
        typeof rawData === "string"
          ? rawData
          : rawData == null
            ? ""
            : JSON.stringify(rawData);
      return {
        status: Number(resp.status),
        headers: respHeaders,
        text,
      };
    };
    try {
      const resp = await this._instance.request(config);
      return normalizeAxiosResp(resp);
    } catch (err: any) {
      if (
        err != null &&
        typeof err === "object" &&
        err.response != null &&
        typeof err.response.status === "number"
      ) {
        return normalizeAxiosResp(err.response);
      }
      throw new TransportError(err);
    }
  }
}

export function viaFetch(fetchFn?: typeof globalThis.fetch): HttpTransport {
  return new FetchTransport(fetchFn);
}

export function viaAxios(instance: AxiosLike): HttpTransport {
  return new AxiosTransport(instance);
}

export interface RtRequestOptions {
  headers?: Record<string, any>;
  params?: Record<string, any>;
  cookies?: Record<string, any>;
  body?: unknown;
  bodyKind?: "empty" | "json" | "form" | "raw";
  extra?: Record<string, unknown>;
}

export function rtBuildRequest(
  method: string,
  url: string,
  opts: RtRequestOptions = {},
): HttpRequest {
  const cleanHeaders: Record<string, string> = {};
  if (opts.headers) {
    for (const [k, v] of Object.entries(opts.headers)) {
      if (v != null) {
        cleanHeaders[String(k)] = String(v);
      }
    }
  }
  const cleanParams: Record<string, string | string[]> = {};
  if (opts.params) {
    for (const [k, v] of Object.entries(opts.params)) {
      if (v == null) continue;
      if (Array.isArray(v)) {
        cleanParams[String(k)] = v.filter((x) => x != null).map((x) => String(x));
      } else {
        cleanParams[String(k)] = String(v);
      }
    }
  }
  const cleanCookies: Record<string, string> = {};
  if (opts.cookies) {
    for (const [k, v] of Object.entries(opts.cookies)) {
      if (v != null) {
        cleanCookies[String(k)] = String(v);
      }
    }
  }
  const bodyKind = opts.bodyKind ?? "empty";
  let cleanBody: unknown = opts.body;
  if (
    bodyKind === "form" &&
    opts.body != null &&
    typeof opts.body === "object" &&
    !Array.isArray(opts.body) &&
    !(opts.body instanceof URLSearchParams)
  ) {
    const formObj: Record<string, string> = {};
    for (const [k, v] of Object.entries(opts.body as Record<string, any>)) {
      if (v != null) {
        formObj[String(k)] = String(v);
      }
    }
    cleanBody = formObj;
  }
  const extraDict: Record<string, unknown> = {};
  if (opts.extra) {
    for (const [k, v] of Object.entries(opts.extra)) {
      if (k === "headers" && v != null && typeof v === "object" && !Array.isArray(v)) {
        for (const [hk, hv] of Object.entries(v as Record<string, any>)) {
          if (hv != null) cleanHeaders[String(hk)] = String(hv);
        }
      } else if (k === "params" && v != null && typeof v === "object" && !Array.isArray(v)) {
        if (v instanceof URLSearchParams) {
          for (const [pk, pv] of v.entries()) {
            cleanParams[String(pk)] = String(pv);
          }
        } else {
          for (const [pk, pv] of Object.entries(v as Record<string, any>)) {
            if (pv == null) continue;
            if (Array.isArray(pv)) {
              cleanParams[String(pk)] = pv.filter((x) => x != null).map((x) => String(x));
            } else {
              cleanParams[String(pk)] = String(pv);
            }
          }
        }
      } else if (k === "cookies" && v != null && typeof v === "object" && !Array.isArray(v)) {
        for (const [ck, cv] of Object.entries(v as Record<string, any>)) {
          if (cv != null) cleanCookies[String(ck)] = String(cv);
        }
      } else if (v !== undefined) {
        extraDict[String(k)] = v;
      }
    }
  }
  return {
    method,
    url,
    headers: cleanHeaders,
    params: cleanParams,
    cookies: cleanCookies,
    body: cleanBody,
    bodyKind,
    extra: extraDict,
  };
}

export function rtExtractParams(
  args: any[],
  keys: Array<string[]>,
): [Record<string, any>, Record<string, unknown>] {
  const out: Record<string, any> = {};
  let extra: Record<string, unknown> = {};
  if (
    args.length > 0 &&
    typeof args[0] === "object" &&
    args[0] !== null &&
    !Array.isArray(args[0])
  ) {
    const obj = args[0] as Record<string, any>;
    const known = new Set<string>();
    for (const names of keys) {
      const camel = names[0];
      for (const n of names) {
        known.add(n);
        if (out[camel] === undefined && obj[n] !== undefined) {
          out[camel] = obj[n];
        }
      }
    }
    for (const [k, v] of Object.entries(obj)) {
      if (!known.has(k) && v !== undefined) {
        if (k === "extra" && typeof v === "object" && v !== null && !Array.isArray(v)) {
          extra = { ...extra, ...(v as Record<string, unknown>) };
        } else {
          extra[k] = v;
        }
      }
    }
    if (
      args.length > 1 &&
      typeof args[1] === "object" &&
      args[1] !== null &&
      !Array.isArray(args[1])
    ) {
      extra = { ...extra, ...(args[1] as Record<string, unknown>) };
    }
  } else {
    for (let i = 0; i < keys.length; i++) {
      out[keys[i][0]] = args[i];
    }
    if (
      args.length > keys.length &&
      typeof args[keys.length] === "object" &&
      args[keys.length] !== null &&
      !Array.isArray(args[keys.length])
    ) {
      extra = args[keys.length] as Record<string, unknown>;
    }
  }
  return [out, extra];
}
