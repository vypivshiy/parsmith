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

export declare function rtDispatchResponse(
  matchers: ErrMatcher[],
  resp: RawResponse,
  valueFn?: ((body: any) => any) | null,
): any;

export declare function rtRestCall(
  transport: HttpTransport,
  req: HttpRequest,
  matchers: ErrMatcher[],
  valueFn?: ((body: any) => any) | null,
): Promise<any>;
