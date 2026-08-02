//! std.http module — HTTP client operations.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "http",
        functions: &[
            "get", "post", "put", "patch", "delete",
            "request", "download", "upload", "url_encode", "url_decode", "query_string",
        ],
    }
}

impl Interpreter {
    pub(crate) fn call_http(&mut self, func: &str, args: &[Value]) -> IResult {
        // Requests that change something on the other end are announced, not
        // sent. `get`/`head`/`request` are left alone: `request` takes its
        // method as data, so suppressing it would mean guessing.
        if matches!(func, "post" | "put" | "patch" | "delete" | "upload" | "download") {
            let target = args.first().map(|v| v.display_string()).unwrap_or_default();
            if self.dry_run_skip(format!("http.{} {}", func, target)) {
                let mut map = std::collections::BTreeMap::new();
                map.insert("status".to_string(), Value::Int(200));
                map.insert("headers".to_string(), Value::Map(Default::default()));
                map.insert("body".to_string(), Value::String(String::new()));
                map.insert("ok".to_string(), Value::Bool(true));
                return Ok(Value::Ok(Box::new(Value::Map(map))));
            }
        }
        match func {
            "get" => match crate::http::http_get(args) {
                Ok(v) => Ok(Value::Ok(Box::new(v))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
            },
            "post" => match crate::http::http_post(args) {
                Ok(v) => Ok(Value::Ok(Box::new(v))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
            },
            "put" => match crate::http::http_put(args) {
                Ok(v) => Ok(Value::Ok(Box::new(v))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
            },
            "patch" => match crate::http::http_patch(args) {
                Ok(v) => Ok(Value::Ok(Box::new(v))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
            },
            "delete" => match crate::http::http_delete(args) {
                Ok(v) => Ok(Value::Ok(Box::new(v))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
            },
            "request" => match crate::http::http_request(args) {
                Ok(v) => Ok(Value::Ok(Box::new(v))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
            },
            "download" => match crate::http::http_download(args) {
                Ok(v) => Ok(Value::Ok(Box::new(v))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
            },
            "upload" => match crate::http::http_upload(args) {
                Ok(v) => Ok(Value::Ok(Box::new(v))),
                Err(e) => Ok(Value::Err(Box::new(Value::String(e)))),
            },
            "url_encode" => match crate::http::url_encode(args) {
                Ok(v) => Ok(v),
                Err(e) => Err(Signal::Error(QueError::new(ErrorKind::Runtime, e))),
            },
            "url_decode" => match crate::http::url_decode(args) {
                Ok(v) => Ok(v),
                Err(e) => Err(Signal::Error(QueError::new(ErrorKind::Runtime, e))),
            },
            "query_string" => match crate::http::query_string(args) {
                Ok(v) => Ok(v),
                Err(e) => Err(Signal::Error(QueError::new(ErrorKind::Runtime, e))),
            },
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'http.{}'", func),
            ))),
        }
    }
}
