#[derive(Debug, Clone)]
pub enum ErrorCond {
    Null(&'static str),
    Bool(&'static str, bool),
    Int(&'static str, i64),
    Float(&'static str, f64),
    Str(&'static str, &'static str),
}

pub struct ErrorMatcher<E> {
    pub status: u16,
    pub required_keys: &'static [&'static str],
    pub conditions: &'static [ErrorCond],
    pub decode: fn(Value) -> Result<E, RtError>,
}

pub fn match_error_json(
    body: &Value,
    required_keys: &[&str],
    conditions: &[ErrorCond],
) -> bool {
    if body.is_null() {
        return false;
    }
    for &key in required_keys {
        if json_opt(body, key).is_none() {
            return false;
        }
    }
    for cond in conditions {
        let matched = match cond {
            ErrorCond::Null(path) => json_opt(body, path).map_or(true, |v| v.is_null()),
            ErrorCond::Bool(path, expected) => {
                json_opt(body, path).and_then(|v| v.as_bool()) == Some(*expected)
            }
            ErrorCond::Int(path, expected) => {
                json_opt(body, path).and_then(|v| v.as_i64()) == Some(*expected)
            }
            ErrorCond::Float(path, expected) => {
                json_opt(body, path).and_then(|v| v.as_f64()) == Some(*expected)
            }
            ErrorCond::Str(path, expected) => {
                json_opt(body, path).and_then(|v| v.as_str()) == Some(*expected)
            }
        };
        if !matched {
            return false;
        }
    }
    true
}

pub async fn fetch_html(
    req: reqwest::RequestBuilder,
    context: &str,
    response_path: Option<&str>,
    response_join: Option<&str>,
) -> Result<String, RtError> {
    let resp = req
        .send()
        .await
        .map_err(|e| RtError::new(context, e.to_string()))?;
    let status = resp.status().as_u16();
    let body = resp
        .text()
        .await
        .map_err(|e| RtError::new(context, e.to_string()))?;
    if status >= 400 {
        return Err(RtError::new(context, format!("HTTP {status}: {body}")));
    }
    if let Some(path) = response_path {
        let body_val = parse_json(&body, path, context)?;
        return Ok(match (body_val, response_join) {
            (Value::Array(items), Some(sep)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| item.to_string())
                })
                .collect::<Vec<_>>()
                .join(sep),
            (Value::String(s), _) => s,
            (other, _) => other.to_string(),
        });
    }
    Ok(body)
}

fn dispatch_rest_errors<E>(
    status: u16,
    text: &str,
    parsed_res: &Result<Value, serde_json::Error>,
    matchers: &[ErrorMatcher<E>],
) -> Result<Option<RtError>, E> {
    let mut last_decode_err: Option<RtError> = None;
    for m in matchers {
        if m.status != status {
            continue;
        }
        match parsed_res {
            Ok(val) => {
                if match_error_json(val, m.required_keys, m.conditions) {
                    match (m.decode)(val.clone()) {
                        Ok(err_val) => return Err(err_val),
                        Err(e) => last_decode_err = Some(e),
                    }
                }
            }
            Err(_) => {
                if m.required_keys.is_empty() && m.conditions.is_empty() {
                    if let Ok(err_val) = (m.decode)(Value::String(text.to_owned())) {
                        return Err(err_val);
                    }
                }
            }
        }
    }
    Ok(last_decode_err)
}

pub async fn rest_call<T, E>(
    req: reqwest::RequestBuilder,
    response_path: Option<&str>,
    context: &str,
    matchers: &[ErrorMatcher<E>],
    transport_err: fn(reqwest::Error) -> E,
    decode_err: fn(u16, RtError) -> E,
    unknown_err: fn(u16, Value) -> E,
) -> Result<T, E>
where
    T: Decode,
{
    let resp = req.send().await.map_err(transport_err)?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(transport_err)?;
    let parsed_res: Result<Value, serde_json::Error> = serde_json::from_str(&text);

    let last_decode_err = dispatch_rest_errors(status, &text, &parsed_res, matchers)?;

    if (200..300).contains(&status) {
        let parsed = parsed_res
            .map_err(|e| decode_err(status, RtError::new(context, e.to_string())))?;
        let target = extract_json_value(&parsed, response_path, context)
            .map_err(|e| decode_err(status, e))?;
        return decode::<T>(target, context).map_err(|e| decode_err(status, e));
    }

    if let Some(err) = last_decode_err {
        return Err(decode_err(status, err));
    }

    let unknown_val = parsed_res.unwrap_or_else(|_| Value::String(text));
    Err(unknown_err(status, unknown_val))
}

pub async fn rest_call_void<E>(
    req: reqwest::RequestBuilder,
    _context: &str,
    matchers: &[ErrorMatcher<E>],
    transport_err: fn(reqwest::Error) -> E,
    decode_err: fn(u16, RtError) -> E,
    unknown_err: fn(u16, Value) -> E,
) -> Result<(), E> {
    let resp = req.send().await.map_err(transport_err)?;
    let status = resp.status().as_u16();
    let text = resp.text().await.map_err(transport_err)?;
    let parsed_res: Result<Value, serde_json::Error> = serde_json::from_str(&text);

    let last_decode_err = dispatch_rest_errors(status, &text, &parsed_res, matchers)?;

    if (200..300).contains(&status) {
        return Ok(());
    }

    if let Some(err) = last_decode_err {
        return Err(decode_err(status, err));
    }

    let unknown_val = parsed_res.unwrap_or_else(|_| Value::String(text));
    Err(unknown_err(status, unknown_val))
}
