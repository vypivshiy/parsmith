type rtHeaders = map[string][]string

type rtReqOpts struct {
	Headers rtHeaders
	Cookies rtHeaders
	Params  map[string][]string
	Body    string
	Timeout time.Duration
}

// rtReqOpt is a functional option for per-call request overrides.
type rtReqOpt func(*rtReqOpts)

// RtHttpOption is an exported alias for rtReqOpt.
type RtHttpOption = rtReqOpt

// WithHeader adds a per-call header value (additive — does not replace DSL headers).
func WithHeader(key, val string) RtHttpOption {
	return func(o *rtReqOpts) {
		if o.Headers == nil {
			o.Headers = rtHeaders{}
		}
		o.Headers[key] = append(o.Headers[key], val)
	}
}

// WithHeaders adds multiple per-call header key-value pairs.
func WithHeaders(headers map[string]string) RtHttpOption {
	return func(o *rtReqOpts) {
		if o.Headers == nil {
			o.Headers = rtHeaders{}
		}
		for k, v := range headers {
			o.Headers[k] = append(o.Headers[k], v)
		}
	}
}

// WithParam adds a per-call query parameter value.
func WithParam(key, val string) RtHttpOption {
	return func(o *rtReqOpts) {
		if o.Params == nil {
			o.Params = map[string][]string{}
		}
		o.Params[key] = append(o.Params[key], val)
	}
}

// WithParams adds multiple per-call query parameters.
func WithParams(params map[string]string) RtHttpOption {
	return func(o *rtReqOpts) {
		if o.Params == nil {
			o.Params = map[string][]string{}
		}
		for k, v := range params {
			o.Params[k] = append(o.Params[k], v)
		}
	}
}

// WithCookie adds a per-call cookie value.
func WithCookie(name, val string) RtHttpOption {
	return func(o *rtReqOpts) {
		if o.Cookies == nil {
			o.Cookies = rtHeaders{}
		}
		o.Cookies[name] = append(o.Cookies[name], val)
	}
}

// WithCookies adds multiple per-call cookies.
func WithCookies(cookies map[string]string) RtHttpOption {
	return func(o *rtReqOpts) {
		if o.Cookies == nil {
			o.Cookies = rtHeaders{}
		}
		for k, v := range cookies {
			o.Cookies[k] = append(o.Cookies[k], v)
		}
	}
}

// WithTimeout sets a per-call request timeout duration.
func WithTimeout(d time.Duration) RtHttpOption {
	return func(o *rtReqOpts) {
		o.Timeout = d
	}
}

type rtErrMatcher struct {
	Status  int
	Check   func([]byte) bool
	Factory func(int, []byte) error
}

func rtDispatchErr(matchers []rtErrMatcher, status int, headers rtHeaders, body []byte) error {
	_ = headers
	var lastDecodeErr error
	for _, m := range matchers {
		if m.Status == status {
			if m.Check == nil || m.Check(body) {
				if err := m.Factory(status, body); err != nil {
					if _, isDec := err.(*DecodeErr); isDec {
						lastDecodeErr = err
						continue
					}
					return err
				}
			}
		}
	}
	if status >= 400 && lastDecodeErr != nil {
		return lastDecodeErr
	}
	return nil
}

// DecodeErr — response body failed JSON parsing or schema validation.
type DecodeErr struct {
	Status int
	Body   string
	Cause  string
}

func (e *DecodeErr) Error() string {
	return fmt.Sprintf("parsmith: decode error (HTTP %d): %s", e.Status, e.Cause)
}

func (e *DecodeErr) IsError() bool {
	return true
}

// UnknownErr — unmatched HTTP >= 400 response.
type UnknownErr struct {
	Status int
	Body   string
}

func (e *UnknownErr) Error() string {
	return fmt.Sprintf("parsmith: unexpected HTTP %d: %s", e.Status, e.Body)
}

func (e *UnknownErr) IsError() bool {
	return true
}

// TransportErr — network/transport level failure.
type TransportErr struct {
	Cause string
}

func (e *TransportErr) Error() string {
	return "parsmith: transport error: " + e.Cause
}

func (e *TransportErr) IsError() bool {
	return true
}
