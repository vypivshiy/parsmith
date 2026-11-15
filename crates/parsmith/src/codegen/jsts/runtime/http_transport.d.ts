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

export declare class TransportError extends Error {
  readonly cause: unknown;
  constructor(cause: unknown);
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

export declare class FetchTransport implements HttpTransport {
  constructor(fetchFn?: typeof globalThis.fetch);
  send(req: HttpRequest): Promise<RawResponse>;
}

export declare class AxiosTransport implements HttpTransport {
  constructor(instance: AxiosLike);
  send(req: HttpRequest): Promise<RawResponse>;
}

export declare function viaFetch(fetchFn?: typeof globalThis.fetch): HttpTransport;
export declare function viaAxios(instance: AxiosLike): HttpTransport;

export interface RtRequestOptions {
  headers?: Record<string, any>;
  params?: Record<string, any>;
  cookies?: Record<string, any>;
  body?: unknown;
  bodyKind?: "empty" | "json" | "form" | "raw";
  extra?: Record<string, unknown>;
}

export declare function rtBuildRequest(
  method: string,
  url: string,
  opts?: RtRequestOptions,
): HttpRequest;

export declare function rtExtractParams(
  args: any[],
  keys: Array<string[]>,
): [Record<string, any>, Record<string, unknown>];
