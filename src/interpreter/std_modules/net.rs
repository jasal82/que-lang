//! std.net module — Network utilities (ping, port checking, DNS resolution).

use crate::error::*;
use crate::value::Value;
use super::super::helpers::duration_to_ms;
use super::super::Interpreter;
use super::StdModule;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "net",
        functions: &[
            "ping", "port_open", "resolve",
            "wait_for_port", "wait_for_url",
        ],
    }
}

impl Interpreter {
    pub(crate) fn call_net(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            "ping" => {
                let host = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(sig_type("net.ping", "String for host")),
                };
                let timeout_ms = match args.get(1) {
                    Some(Value::Int(ms)) => *ms as u64,
                    Some(Value::Duration(val, unit)) => duration_to_ms(*val, *unit) as u64,
                    Some(Value::Null) | None => 5000,
                    _ => return Err(sig_type("net.ping", "Int or Duration for timeout")),
                };
                Ok(Value::Bool(net_ping(&host, timeout_ms)))
            }
            "port_open" => {
                let host = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(sig_type("net.port_open", "String for host")),
                };
                let port = match args.get(1) {
                    Some(Value::Int(p)) => *p as u16,
                    _ => return Err(sig_type("net.port_open", "Int for port")),
                };
                let timeout_ms = match args.get(2) {
                    Some(Value::Int(ms)) => *ms as u64,
                    Some(Value::Duration(val, unit)) => duration_to_ms(*val, *unit) as u64,
                    Some(Value::Null) | None => 3000,
                    _ => return Err(sig_type("net.port_open", "Int or Duration for timeout")),
                };
                Ok(Value::Bool(net_port_open(&host, port, timeout_ms)))
            }
            "resolve" => {
                let host = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(sig_type("net.resolve", "String for hostname")),
                };
                let addrs = net_resolve(&host).map_err(|e| sig_err(e))?;
                Ok(Value::List(addrs.into_iter().map(Value::String).collect()))
            }
            "wait_for_port" => {
                let host = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(sig_type("net.wait_for_port", "String for host")),
                };
                let port = match args.get(1) {
                    Some(Value::Int(p)) => *p as u16,
                    _ => return Err(sig_type("net.wait_for_port", "Int for port")),
                };
                let timeout_ms = match args.get(2) {
                    Some(Value::Int(ms)) => *ms as u64,
                    Some(Value::Duration(val, unit)) => duration_to_ms(*val, *unit) as u64,
                    Some(Value::Null) | None => 30000,
                    _ => return Err(sig_type("net.wait_for_port", "Int or Duration for timeout")),
                };
                let interval_ms = match args.get(3) {
                    Some(Value::Int(ms)) => *ms as u64,
                    Some(Value::Duration(val, unit)) => duration_to_ms(*val, *unit) as u64,
                    Some(Value::Null) | None => 500,
                    _ => return Err(sig_type("net.wait_for_port", "Int or Duration for interval")),
                };
                Ok(Value::Bool(net_wait_for_port(&host, port, timeout_ms, interval_ms)))
            }
            "wait_for_url" => {
                let url = match args.first() {
                    Some(Value::String(s)) => s.clone(),
                    _ => return Err(sig_type("net.wait_for_url", "String for URL")),
                };
                let timeout_ms = match args.get(1) {
                    Some(Value::Int(ms)) => *ms as u64,
                    Some(Value::Duration(val, unit)) => duration_to_ms(*val, *unit) as u64,
                    Some(Value::Null) | None => 30000,
                    _ => return Err(sig_type("net.wait_for_url", "Int or Duration for timeout")),
                };
                let interval_ms = match args.get(2) {
                    Some(Value::Int(ms)) => *ms as u64,
                    Some(Value::Duration(val, unit)) => duration_to_ms(*val, *unit) as u64,
                    Some(Value::Null) | None => 1000,
                    _ => return Err(sig_type("net.wait_for_url", "Int or Duration for interval")),
                };
                let expected_status = match args.get(3) {
                    Some(Value::Int(s)) => Some(*s as u16),
                    Some(Value::Null) | None => None,
                    _ => return Err(sig_type("net.wait_for_url", "Int for expected_status")),
                };
                Ok(Value::Bool(net_wait_for_url(&url, timeout_ms, interval_ms, expected_status)))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("unknown function 'net.{}'", func),
            ))),
        }
    }
}

// ── Private helpers ────────────────────────────────────────────────────────

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

fn sig_type(name: &str, expected: &str) -> Signal {
    Signal::Error(QueError::new(
        ErrorKind::TypeMismatch,
        format!("{}: expected {}", name, expected),
    ))
}

fn net_ping(host: &str, timeout_ms: u64) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let timeout = Duration::from_millis(timeout_ms);
    for port in [80, 443] {
        let addr = format!("{}:{}", host, port);
        if let Ok(mut addrs) = addr.to_socket_addrs() {
            for sock_addr in &mut addrs {
                if TcpStream::connect_timeout(&sock_addr, timeout).is_ok() {
                    return true;
                }
            }
        }
    }
    let addr = format!("{}:80", host);
    addr.to_socket_addrs().is_ok()
}

fn net_port_open(host: &str, port: u16, timeout_ms: u64) -> bool {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let timeout = Duration::from_millis(timeout_ms);
    let addr = format!("{}:{}", host, port);
    if let Ok(addrs) = addr.to_socket_addrs() {
        for sock_addr in addrs {
            if TcpStream::connect_timeout(&sock_addr, timeout).is_ok() {
                return true;
            }
        }
    }
    false
}

fn net_resolve(host: &str) -> Result<Vec<String>, String> {
    use std::net::ToSocketAddrs;
    let addr = format!("{}:0", host);
    let addrs = addr.to_socket_addrs()
        .map_err(|e| format!("failed to resolve '{}': {}", host, e))?;
    let mut ips: Vec<String> = addrs.map(|a| a.ip().to_string()).collect();
    ips.dedup();
    Ok(ips)
}

fn net_wait_for_port(host: &str, port: u16, timeout_ms: u64, interval_ms: u64) -> bool {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let interval = Duration::from_millis(interval_ms);

    while Instant::now() < deadline {
        if net_port_open(host, port, interval_ms.min(1000)) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(interval.min(remaining));
    }
    false
}

fn net_wait_for_url(url: &str, timeout_ms: u64, interval_ms: u64, expected_status: Option<u16>) -> bool {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let interval = Duration::from_millis(interval_ms);

    while Instant::now() < deadline {
        if check_url(url, expected_status) {
            return true;
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(interval.min(remaining));
    }
    false
}

fn check_url(url: &str, expected_status: Option<u16>) -> bool {
    match crate::http::probe_status(url, 5_000) {
        Some(status) => match expected_status {
            Some(expected) => status == expected,
            None => (200..300).contains(&status),
        },
        None => false,
    }
}
