/// HTTP client functionality for the Que language.
///
/// Provides built-in functions for making HTTP requests:
///   http_get, http_post, http_put, http_patch, http_delete,
///   http_request, http_download, http_upload, url_encode, url_decode, query_string

use crate::value::Value;
use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

// ── TLS ──────────────────────────────────────────────────────────────

/// Build a ureq Agent, optionally configured with a custom CA-bundle PEM file.
///
/// `tls_opts` — optional Map containing any of:
///   `"ca_bundle"` — Path or String path to a PEM file with extra trusted CA certs.
///
/// When `ca_bundle` is provided, the returned agent trusts both the standard
/// WebPKI root CAs and any additional CAs in the bundle.
/// When absent, a default ureq agent (WebPKI roots) is returned.
fn build_tls_agent(tls_opts: Option<&BTreeMap<String, Value>>) -> Result<ureq::Agent, String> {
    let ca_bundle = tls_opts
        .and_then(|m| m.get("ca_bundle"))
        .and_then(|v| v.as_path());

    match ca_bundle {
        None => Ok(ureq::agent()),
        Some(path) => {
            use rustls::{ClientConfig, RootCertStore};

            // Ensure the ring crypto provider is installed
            let _ = rustls::crypto::ring::default_provider().install_default();

            let mut root_store = RootCertStore::empty();
            // Include standard WebPKI root CAs
            root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

            // Load extra CAs from the user-supplied bundle
            let ca_file = std::fs::File::open(&path)
                .map_err(|e| format!("TLS: cannot open ca_bundle '{}': {}", path, e))?;
            let mut reader = std::io::BufReader::new(ca_file);
            for cert_result in rustls_pemfile::certs(&mut reader) {
                let cert = cert_result
                    .map_err(|e| format!("TLS: error reading PEM from '{}': {}", path, e))?;
                root_store
                    .add(cert)
                    .map_err(|e| format!("TLS: invalid certificate in '{}': {}", path, e))?;
            }

            let config = ClientConfig::builder()
                .with_root_certificates(root_store)
                .with_no_client_auth();

            Ok(ureq::AgentBuilder::new()
                .tls_config(Arc::new(config))
                .build())
        }
    }
}

/// Issue a HEAD-like probe and return the status code, or `None` if the
/// request could not be completed at all.
///
/// This exists so readiness polling goes through the same TLS stack as every
/// other request instead of shelling out to `curl`, which is neither present
/// on a default Windows install nor able to honour a configured CA bundle.
pub fn probe_status(url: &str, timeout_ms: u64) -> Option<u16> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build();
    match agent.get(url).call() {
        Ok(resp) => Some(resp.status()),
        // A 4xx/5xx is a real answer from a server that is up, so it is worth
        // reporting: `wait_for_url` with `status: 503` is a legitimate wait.
        Err(ureq::Error::Status(code, _)) => Some(code),
        Err(_) => None,
    }
}

/// Extract an optional `tls` options map from a Value.
fn extract_tls_opts(val: &Value) -> Result<Option<BTreeMap<String, Value>>, String> {
    match val {
        Value::Map(m) => Ok(Some(m.clone())),
        Value::Null => Ok(None),
        other => Err(format!("tls options must be a Map, got {}", other.type_name())),
    }
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Extract a header map from a Value::Map (string keys → string values).
fn extract_headers(val: &Value) -> Result<Vec<(String, String)>, String> {
    match val {
        Value::Map(m) => {
            let mut headers = Vec::new();
            for (k, v) in m {
                let v_str = match v {
                    Value::String(s) => s.clone(),
                    Value::Int(n) => n.to_string(),
                    Value::Float(f) => f.to_string(),
                    Value::Bool(b) => b.to_string(),
                    other => other.display_string(),
                };
                headers.push((k.clone(), v_str));
            }
            Ok(headers)
        }
        Value::Null => Ok(Vec::new()),
        other => Err(format!("headers must be a Map, got {}", other.type_name())),
    }
}

/// Build a Que Map from ureq response headers.
fn response_headers_to_map(response: &ureq::Response) -> BTreeMap<String, Value> {
    let mut map = BTreeMap::new();
    for name in response.headers_names() {
        if let Some(val) = response.header(&name) {
            map.insert(name.to_lowercase(), Value::String(val.to_string()));
        }
    }
    map
}

/// Build the standard response Map: {status, headers, body}.
fn build_response_map(status: u16, headers: BTreeMap<String, Value>, body: String) -> Value {
    let mut map = BTreeMap::new();
    map.insert("status".into(), Value::Int(status as i64));
    map.insert("headers".into(), Value::Map(headers));
    map.insert("body".into(), Value::String(body));
    map.insert("ok".into(), Value::Bool((200..300).contains(&status)));
    Value::Map(map)
}

/// Apply headers to a ureq request.
fn apply_headers(mut req: ureq::Request, headers: &[(String, String)]) -> ureq::Request {
    for (k, v) in headers {
        req = req.set(k, v);
    }
    req
}

/// Read the response body and build a result value.
fn read_response(response: ureq::Response) -> Result<Value, String> {
    let status = response.status();
    let headers = response_headers_to_map(&response);
    let body = response
        .into_string()
        .map_err(|e| format!("failed to read response body: {}", e))?;
    Ok(build_response_map(status, headers, body))
}

/// Execute a ureq request and handle errors uniformly.
fn execute_request(request: ureq::Request, body: Option<&str>) -> Result<Value, String> {
    let result = match body {
        Some(b) => request.send_string(b),
        None => request.call(),
    };
    match result {
        Ok(response) => read_response(response),
        Err(ureq::Error::Status(code, response)) => {
            // HTTP error status (4xx, 5xx) — still return a response map
            read_response(response).or_else(|_| {
                Ok(build_response_map(
                    code,
                    BTreeMap::new(),
                    String::new(),
                ))
            })
        }
        Err(ureq::Error::Transport(e)) => Err(format!("HTTP transport error: {}", e)),
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// `http_get(url: String, headers?: Map, tls?: Map) -> Ok(Map) | Err(String)`
pub fn http_get(args: &[Value]) -> Result<Value, String> {
    let url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!("http_get() url must be a String, got {}", other.type_name())),
        None => return Err("http_get() requires at least 1 argument (url)".into()),
    };
    let headers = match args.get(1) {
        Some(h) => extract_headers(h)?,
        None => Vec::new(),
    };
    let tls_opts = match args.get(2) {
        Some(v) => extract_tls_opts(v)?,
        None => None,
    };
    let agent = build_tls_agent(tls_opts.as_ref())?;
    let req = apply_headers(agent.get(url), &headers);
    execute_request(req, None)
}

/// `http_post(url: String, body: String, headers?: Map, tls?: Map) -> Ok(Map) | Err(String)`
pub fn http_post(args: &[Value]) -> Result<Value, String> {
    let url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!("http_post() url must be a String, got {}", other.type_name())),
        None => return Err("http_post() requires at least 2 arguments (url, body)".into()),
    };
    let body = match args.get(1) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => String::new(),
        Some(other) => other.display_string(),
        None => return Err("http_post() requires a body argument".into()),
    };
    let headers = match args.get(2) {
        Some(h) => extract_headers(h)?,
        None => Vec::new(),
    };
    let tls_opts = match args.get(3) {
        Some(v) => extract_tls_opts(v)?,
        None => None,
    };
    let agent = build_tls_agent(tls_opts.as_ref())?;
    let req = apply_headers(agent.post(url), &headers);
    execute_request(req, Some(&body))
}

/// `http_put(url: String, body: String, headers?: Map, tls?: Map) -> Ok(Map) | Err(String)`
pub fn http_put(args: &[Value]) -> Result<Value, String> {
    let url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!("http_put() url must be a String, got {}", other.type_name())),
        None => return Err("http_put() requires at least 2 arguments (url, body)".into()),
    };
    let body = match args.get(1) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => String::new(),
        Some(other) => other.display_string(),
        None => return Err("http_put() requires a body argument".into()),
    };
    let headers = match args.get(2) {
        Some(h) => extract_headers(h)?,
        None => Vec::new(),
    };
    let tls_opts = match args.get(3) {
        Some(v) => extract_tls_opts(v)?,
        None => None,
    };
    let agent = build_tls_agent(tls_opts.as_ref())?;
    let req = apply_headers(agent.put(url), &headers);
    execute_request(req, Some(&body))
}

/// `http_patch(url: String, body: String, headers?: Map, tls?: Map) -> Ok(Map) | Err(String)`
pub fn http_patch(args: &[Value]) -> Result<Value, String> {
    let url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!("http_patch() url must be a String, got {}", other.type_name())),
        None => return Err("http_patch() requires at least 2 arguments (url, body)".into()),
    };
    let body = match args.get(1) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Null) => String::new(),
        Some(other) => other.display_string(),
        None => return Err("http_patch() requires a body argument".into()),
    };
    let headers = match args.get(2) {
        Some(h) => extract_headers(h)?,
        None => Vec::new(),
    };
    let tls_opts = match args.get(3) {
        Some(v) => extract_tls_opts(v)?,
        None => None,
    };
    let agent = build_tls_agent(tls_opts.as_ref())?;
    let req = apply_headers(agent.request("PATCH", url), &headers);
    execute_request(req, Some(&body))
}

/// `http_delete(url: String, headers?: Map, tls?: Map) -> Ok(Map) | Err(String)`
pub fn http_delete(args: &[Value]) -> Result<Value, String> {
    let url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!("http_delete() url must be a String, got {}", other.type_name())),
        None => return Err("http_delete() requires at least 1 argument (url)".into()),
    };
    let headers = match args.get(1) {
        Some(h) => extract_headers(h)?,
        None => Vec::new(),
    };
    let tls_opts = match args.get(2) {
        Some(v) => extract_tls_opts(v)?,
        None => None,
    };
    let agent = build_tls_agent(tls_opts.as_ref())?;
    let req = apply_headers(agent.delete(url), &headers);
    execute_request(req, None)
}

/// `http_request(options: Map) -> Ok(Map) | Err(String)`
///
/// Options map keys:
///   "method"  - HTTP method string (default: "GET")
///   "url"     - request URL (required)
///   "headers" - Map of header name → value
///   "body"    - request body string
///   "timeout" - timeout in seconds (Int or Float)
///   "tls"     - Map with optional `"ca_bundle"` key (Path to PEM CA bundle)
pub fn http_request(args: &[Value]) -> Result<Value, String> {
    let opts = match args.first() {
        Some(Value::Map(m)) => m,
        Some(other) => return Err(format!(
            "http_request() requires a Map argument, got {}",
            other.type_name()
        )),
        None => return Err("http_request() requires 1 argument (options Map)".into()),
    };

    let url = match opts.get("url") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => return Err(format!("http_request() url must be a String, got {}", other.type_name())),
        None => return Err("http_request() options must include \"url\"".into()),
    };

    let method = match opts.get("method") {
        Some(Value::String(s)) => s.to_uppercase(),
        None => "GET".into(),
        Some(other) => return Err(format!("http_request() method must be a String, got {}", other.type_name())),
    };

    let headers = match opts.get("headers") {
        Some(h) => extract_headers(h)?,
        None => Vec::new(),
    };

    let body = match opts.get("body") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Null) | None => None,
        Some(other) => Some(other.display_string()),
    };

    let tls_opts = match opts.get("tls") {
        Some(v) => extract_tls_opts(v)?,
        None => None,
    };
    let agent = build_tls_agent(tls_opts.as_ref())?;
    let mut req = agent.request(&method, &url);

    // Apply timeout if specified
    if let Some(Value::Int(secs)) = opts.get("timeout") {
        req = req.timeout(std::time::Duration::from_secs(*secs as u64));
    } else if let Some(Value::Float(secs)) = opts.get("timeout") {
        req = req.timeout(std::time::Duration::from_secs_f64(*secs));
    }

    let req = apply_headers(req, &headers);
    execute_request(req, body.as_deref())
}

/// `http_download(url: String, dest: String|Path, headers?: Map, tls?: Map) -> Ok(Map) | Err(String)`
///
/// Downloads a URL to a local file. Returns a map with {status, headers, size}.
pub fn http_download(args: &[Value]) -> Result<Value, String> {
    let url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!("http_download() url must be a String, got {}", other.type_name())),
        None => return Err("http_download() requires at least 2 arguments (url, dest)".into()),
    };

    let dest = match args.get(1) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Path(p)) => p.clone(),
        Some(other) => return Err(format!(
            "http_download() dest must be a String or Path, got {}",
            other.type_name()
        )),
        None => return Err("http_download() requires a dest argument".into()),
    };

    let headers = match args.get(2) {
        Some(h) => extract_headers(h)?,
        None => Vec::new(),
    };

    let tls_opts = match args.get(3) {
        Some(v) => extract_tls_opts(v)?,
        None => None,
    };
    let agent = build_tls_agent(tls_opts.as_ref())?;
    let req = apply_headers(agent.get(url), &headers);

    match req.call() {
        Ok(response) => {
            let status = response.status();
            let resp_headers = response_headers_to_map(&response);

            // Stream the body to disk. Buffering it first meant the largest
            // artefact a script could download was bounded by RAM, and a
            // release tarball or a container layer is exactly the thing a
            // build script downloads. Writing to a sibling temp file and
            // renaming keeps a half-finished download from being mistaken
            // for a complete one by the next run.
            if let Some(parent) = std::path::Path::new(&dest).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("failed to create directory: {}", e))?;
                }
            }
            let partial = format!("{}.que-partial", dest);
            let size = {
                let mut reader = response.into_reader();
                let file = std::fs::File::create(&partial)
                    .map_err(|e| format!("failed to write file '{}': {}", partial, e))?;
                let mut writer = std::io::BufWriter::new(file);
                let copied = std::io::copy(&mut reader, &mut writer)
                    .map_err(|e| format!("failed to read response body: {}", e))?;
                writer
                    .flush()
                    .map_err(|e| format!("failed to write file '{}': {}", partial, e))?;
                copied as usize
            };
            std::fs::rename(&partial, &dest).map_err(|e| {
                let _ = std::fs::remove_file(&partial);
                format!("failed to write file '{}': {}", dest, e)
            })?;

            let mut map = BTreeMap::new();
            map.insert("status".into(), Value::Int(status as i64));
            map.insert("headers".into(), Value::Map(resp_headers));
            map.insert("size".into(), Value::Int(size as i64));
            map.insert("path".into(), Value::Path(dest));
            map.insert("ok".into(), Value::Bool((200..300).contains(&status)));
            Ok(Value::Map(map))
        }
        Err(ureq::Error::Status(code, response)) => {
            let resp_headers = response_headers_to_map(&response);
            let mut map = BTreeMap::new();
            map.insert("status".into(), Value::Int(code as i64));
            map.insert("headers".into(), Value::Map(resp_headers));
            map.insert("size".into(), Value::Int(0));
            map.insert("path".into(), Value::Path(dest));
            map.insert("ok".into(), Value::Bool(false));
            Ok(Value::Map(map))
        }
        Err(ureq::Error::Transport(e)) => Err(format!("HTTP transport error: {}", e)),
    }
}

/// `http_upload(url: String, path: Path, headers?: Map, tls?: Map) -> Ok(Map) | Err(String)`
///
/// Streams a file body directly from disk using a PUT request, avoiding buffering
/// the entire file in memory. Suitable for large files (hundreds of MB).
///
/// The response map has the same shape as other http functions: {status, headers, body, ok}.
pub fn http_upload(args: &[Value]) -> Result<Value, String> {
    let url = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!(
            "http_upload() url must be a String, got {}", other.type_name()
        )),
        None => return Err("http_upload() requires at least 2 arguments (url, path)".into()),
    };

    let file_path = match args.get(1) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Path(p)) => p.clone(),
        Some(other) => return Err(format!(
            "http_upload() path must be a String or Path, got {}", other.type_name()
        )),
        None => return Err("http_upload() requires a file path argument".into()),
    };

    let headers = match args.get(2) {
        Some(h) => extract_headers(h)?,
        None => Vec::new(),
    };

    let tls_opts = match args.get(3) {
        Some(v) => extract_tls_opts(v)?,
        None => None,
    };

    let file = std::fs::File::open(&file_path)
        .map_err(|e| format!("http_upload(): cannot open '{}': {}", file_path, e))?;
    let file_size = file.metadata().map(|m| m.len()).unwrap_or(0);

    let agent = build_tls_agent(tls_opts.as_ref())?;
    let mut req = agent.put(url);
    req = apply_headers(req, &headers);
    if file_size > 0 {
        req = req.set("Content-Length", &file_size.to_string());
    }

    let result = req.send(file);
    match result {
        Ok(response) => read_response(response),
        Err(ureq::Error::Status(code, response)) => {
            read_response(response).or_else(|_| {
                Ok(build_response_map(code, std::collections::BTreeMap::new(), String::new()))
            })
        }
        Err(ureq::Error::Transport(e)) => Err(format!("HTTP transport error: {}", e)),
    }
}

/// `url_encode(s: String) -> String`
///
/// Percent-encode a string for use in URLs.
pub fn url_encode(args: &[Value]) -> Result<Value, String> {
    let s = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!("url_encode() requires a String, got {}", other.type_name())),
        None => return Err("url_encode() requires 1 argument".into()),
    };

    // Encode all characters except unreserved ones per RFC 3986
    let encoded: String = s
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect();

    Ok(Value::String(encoded))
}

/// `url_decode(s: String) -> Ok(String) | Err(String)`
///
/// Decode a percent-encoded string.
pub fn url_decode(args: &[Value]) -> Result<Value, String> {
    let s = match args.first() {
        Some(Value::String(s)) => s.as_str(),
        Some(other) => return Err(format!("url_decode() requires a String, got {}", other.type_name())),
        None => return Err("url_decode() requires 1 argument".into()),
    };

    let mut decoded = Vec::new();
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = &s[i + 1..i + 3];
            match u8::from_str_radix(hex, 16) {
                Ok(byte) => {
                    decoded.push(byte);
                    i += 3;
                }
                Err(_) => {
                    decoded.push(bytes[i]);
                    i += 1;
                }
            }
        } else if bytes[i] == b'+' {
            decoded.push(b' ');
            i += 1;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }

    match String::from_utf8(decoded) {
        Ok(s) => Ok(Value::String(s)),
        Err(e) => Err(format!("url_decode: invalid UTF-8: {}", e)),
    }
}

/// `query_string(params: Map) -> String`
///
/// Build a URL query string from a map of key-value pairs.
/// Values are percent-encoded automatically.
pub fn query_string(args: &[Value]) -> Result<Value, String> {
    let map = match args.first() {
        Some(Value::Map(m)) => m,
        Some(other) => return Err(format!("query_string() requires a Map, got {}", other.type_name())),
        None => return Err("query_string() requires 1 argument".into()),
    };

    let pairs: Vec<String> = map
        .iter()
        .map(|(k, v)| {
            let v_str = match v {
                Value::String(s) => s.clone(),
                Value::Int(n) => n.to_string(),
                Value::Float(f) => f.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Null => String::new(),
                other => other.display_string(),
            };
            let encoded_k = encode_component(k);
            let encoded_v = encode_component(&v_str);
            format!("{}={}", encoded_k, encoded_v)
        })
        .collect();

    Ok(Value::String(pairs.join("&")))
}

/// Percent-encode a single query component.
fn encode_component(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                String::from(b as char)
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}

// ── Unit tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_encode() {
        let result = url_encode(&[Value::String("hello world".into())]).unwrap();
        assert_eq!(result, Value::String("hello%20world".into()));
    }

    #[test]
    fn test_url_encode_special_chars() {
        let result = url_encode(&[Value::String("foo=bar&baz=qux".into())]).unwrap();
        assert_eq!(result, Value::String("foo%3Dbar%26baz%3Dqux".into()));
    }

    #[test]
    fn test_url_encode_unreserved() {
        let result = url_encode(&[Value::String("hello-world_2.0~test".into())]).unwrap();
        assert_eq!(result, Value::String("hello-world_2.0~test".into()));
    }

    #[test]
    fn test_url_decode() {
        let result = url_decode(&[Value::String("hello%20world".into())]).unwrap();
        assert_eq!(result, Value::String("hello world".into()));
    }

    #[test]
    fn test_url_decode_plus() {
        let result = url_decode(&[Value::String("hello+world".into())]).unwrap();
        assert_eq!(result, Value::String("hello world".into()));
    }

    #[test]
    fn test_query_string() {
        let mut map = BTreeMap::new();
        map.insert("name".into(), Value::String("Alice Bob".into()));
        map.insert("age".into(), Value::Int(30));
        let result = query_string(&[Value::Map(map)]).unwrap();
        // BTreeMap is sorted, so "age" comes before "name"
        assert_eq!(result, Value::String("age=30&name=Alice%20Bob".into()));
    }

    #[test]
    fn test_query_string_empty() {
        let map = BTreeMap::new();
        let result = query_string(&[Value::Map(map)]).unwrap();
        assert_eq!(result, Value::String("".into()));
    }

    #[test]
    fn test_extract_headers_map() {
        let mut map = BTreeMap::new();
        map.insert("Content-Type".into(), Value::String("application/json".into()));
        map.insert("X-Count".into(), Value::Int(42));
        let headers = extract_headers(&Value::Map(map)).unwrap();
        assert_eq!(headers.len(), 2);
        assert!(headers.contains(&("Content-Type".into(), "application/json".into())));
        assert!(headers.contains(&("X-Count".into(), "42".into())));
    }

    #[test]
    fn test_extract_headers_null() {
        let headers = extract_headers(&Value::Null).unwrap();
        assert!(headers.is_empty());
    }

    #[test]
    fn test_url_roundtrip() {
        let original = "hello world & foo=bar";
        let encoded = url_encode(&[Value::String(original.into())]).unwrap();
        if let Value::String(enc) = encoded {
            let decoded = url_decode(&[Value::String(enc)]).unwrap();
            assert_eq!(decoded, Value::String(original.into()));
        } else {
            panic!("expected string");
        }
    }
}
