func rtResolveOpts(dslOpts *rtReqOpts, opts ...rtReqOpt) *rtReqOpts {
	_opts := &rtReqOpts{}
	if dslOpts != nil {
		_opts.Body = dslOpts.Body
		_opts.Timeout = dslOpts.Timeout
		if len(dslOpts.Headers) > 0 {
			_opts.Headers = rtHeaders{}
			for k, vs := range dslOpts.Headers {
				_opts.Headers[k] = append([]string(nil), vs...)
			}
		}
		if len(dslOpts.Cookies) > 0 {
			_opts.Cookies = rtHeaders{}
			for k, vs := range dslOpts.Cookies {
				_opts.Cookies[k] = append([]string(nil), vs...)
			}
		}
		if len(dslOpts.Params) > 0 {
			_opts.Params = map[string][]string{}
			for k, vs := range dslOpts.Params {
				_opts.Params[k] = append([]string(nil), vs...)
			}
		}
	}
	for _, o := range opts {
		if o != nil {
			o(_opts)
		}
	}
	return _opts
}

func rtBuildURL(reqURL string, params map[string][]string) string {
	if len(params) == 0 {
		return reqURL
	}
	u, err := url.Parse(reqURL)
	if err != nil {
		return reqURL
	}
	q := u.Query()
	for k, vs := range params {
		for _, v := range vs {
			q.Add(k, v)
		}
	}
	u.RawQuery = q.Encode()
	return u.String()
}

func rtDoRequest(ctx context.Context, client *http.Client, method, reqURL string, dslOpts *rtReqOpts, opts ...rtReqOpt) (*http.Response, []byte, error) {
	if client == nil {
		client = http.DefaultClient
	}
	_opts := rtResolveOpts(dslOpts, opts...)
	if _opts.Timeout > 0 {
		var cancel context.CancelFunc
		ctx, cancel = context.WithTimeout(ctx, _opts.Timeout)
		defer cancel()
	}
	finalURL := rtBuildURL(reqURL, _opts.Params)
	req, err := http.NewRequestWithContext(ctx, method, finalURL, nil)
	if err != nil {
		return nil, nil, err
	}
	if len(_opts.Headers) > 0 {
		for k, vs := range _opts.Headers {
			for _, v := range vs {
				req.Header.Add(k, v)
			}
		}
	}
	if len(_opts.Cookies) > 0 {
		for name, vs := range _opts.Cookies {
			for _, v := range vs {
				req.AddCookie(&http.Cookie{Name: name, Value: v})
			}
		}
	}
	if _opts.Body != "" {
		req.Body = io.NopCloser(strings.NewReader(_opts.Body))
		req.ContentLength = int64(len(_opts.Body))
	}
	resp, err := client.Do(req)
	if err != nil {
		return nil, nil, err
	}
	defer resp.Body.Close()
	body, err := io.ReadAll(resp.Body)
	if err != nil {
		return resp, nil, err
	}
	return resp, body, nil
}

func rtDo(ctx context.Context, client *http.Client, method, reqURL string, dslOpts *rtReqOpts, opts ...rtReqOpt) (*http.Response, []byte, error) {
	return rtDoRequest(ctx, client, method, reqURL, dslOpts, opts...)
}

func rtExtractPath(raw []byte, responsePath string) (gjson.Result, error) {
	res := gjson.GetBytes(raw, responsePath)
	if !res.Exists() {
		return res, fmt.Errorf("parsmith: response-path %q not found", responsePath)
	}
	return res, nil
}

func rtExtractJSONSlice(raw []byte, responsePath string) ([]byte, error) {
	if responsePath != "" {
		res, err := rtExtractPath(raw, responsePath)
		if err != nil {
			return nil, err
		}
		return []byte(res.Raw), nil
	}
	return raw, nil
}

func rtRestCall(client *http.Client, matchers []rtErrMatcher, method, reqURL string, dslOpts *rtReqOpts, opts ...rtReqOpt) (int, []byte, error) {
	resp, bodyBytes, err := rtDoRequest(context.Background(), client, method, reqURL, dslOpts, opts...)
	if err != nil {
		return 0, nil, &TransportErr{Cause: err.Error()}
	}
	headers := rtHeaders{}
	for k, vs := range resp.Header {
		headers[k] = vs
	}
	if errObj := rtDispatchErr(matchers, resp.StatusCode, headers, bodyBytes); errObj != nil {
		return resp.StatusCode, nil, errObj
	}
	if resp.StatusCode >= 400 {
		return resp.StatusCode, nil, &UnknownErr{Status: resp.StatusCode, Body: string(bodyBytes)}
	}
	return resp.StatusCode, bodyBytes, nil
}

func rtRestCallValue[T any](client *http.Client, matchers []rtErrMatcher, method, reqURL, responsePath string, dslOpts *rtReqOpts, opts ...rtReqOpt) (*T, error) {
	status, body, err := rtRestCall(client, matchers, method, reqURL, dslOpts, opts...)
	if err != nil {
		return nil, err
	}
	slice, perr := rtExtractJSONSlice(body, responsePath)
	if perr != nil {
		return nil, &DecodeErr{Status: status, Body: string(body), Cause: perr.Error()}
	}
	var val T
	if perr := json.Unmarshal(slice, &val); perr != nil {
		return nil, &DecodeErr{Status: status, Body: string(body), Cause: perr.Error()}
	}
	return &val, nil
}

func rtRestCallSlice[T any](client *http.Client, matchers []rtErrMatcher, method, reqURL, responsePath string, dslOpts *rtReqOpts, opts ...rtReqOpt) ([]T, error) {
	status, body, err := rtRestCall(client, matchers, method, reqURL, dslOpts, opts...)
	if err != nil {
		return nil, err
	}
	slice, perr := rtExtractJSONSlice(body, responsePath)
	if perr != nil {
		return nil, &DecodeErr{Status: status, Body: string(body), Cause: perr.Error()}
	}
	if !gjson.ParseBytes(slice).IsArray() {
		return nil, &DecodeErr{Status: status, Body: string(body), Cause: "expected JSON array"}
	}
	val := make([]T, 0)
	if perr := json.Unmarshal(slice, &val); perr != nil {
		return nil, &DecodeErr{Status: status, Body: string(body), Cause: perr.Error()}
	}
	return val, nil
}

func rtRestCallVoid(client *http.Client, matchers []rtErrMatcher, method, reqURL string, dslOpts *rtReqOpts, opts ...rtReqOpt) (struct{}, error) {
	_, _, err := rtRestCall(client, matchers, method, reqURL, dslOpts, opts...)
	if err != nil {
		return struct{}{}, err
	}
	return struct{}{}, nil
}

func rtFetchHTML(ctx context.Context, client *http.Client, structName, method, reqURL, responsePath, responseJoin string, dslOpts *rtReqOpts, opts ...rtReqOpt) (string, error) {
	resp, body, err := rtDoRequest(ctx, client, method, reqURL, dslOpts, opts...)
	if err != nil {
		return "", &TransportErr{Cause: err.Error()}
	}
	if resp.StatusCode >= 400 {
		return "", fmt.Errorf("%s: HTTP %d: %s", structName, resp.StatusCode, string(body))
	}
	if responsePath != "" {
		if responseJoin != "" {
			_parts := gjson.GetBytes(body, responsePath).Array()
			_partsStrs := make([]string, len(_parts))
			for _i, _p := range _parts {
				_partsStrs[_i] = _p.String()
			}
			return strings.Join(_partsStrs, responseJoin), nil
		}
		return gjson.GetBytes(body, responsePath).String(), nil
	}
	return string(body), nil
}
