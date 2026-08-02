//! std.time module — DateTime construction and timezone handling.

use crate::error::*;
use crate::value::Value;
use super::super::Interpreter;
use super::StdModule;

use chrono::{Datelike, Timelike, TimeZone, Duration, NaiveDate};
use chrono_tz::Tz;
use std::collections::BTreeMap;

pub(super) fn module() -> StdModule {
    StdModule {
        name: "time",
        functions: &["now", "timestamp", "of", "parse", "from_timestamp", "timezone"],
    }
}

/// Build a `Value::Instance { type_name: "DateTime", .. }` from timestamp ms and tz name.
pub(crate) fn make_datetime_value(timestamp_ms: i64, tz_str: &str) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert("_timestamp_ms".to_string(), Value::Int(timestamp_ms));
    fields.insert("_tz".to_string(), Value::String(tz_str.to_string()));
    Value::Instance {
        type_name: "DateTime".to_string(),
        fields,
    }
}

/// Reconstruct a `chrono::DateTime<Tz>` from the internal fields.
pub(crate) fn extract_chrono_dt(fields: &BTreeMap<String, Value>) -> Result<chrono::DateTime<Tz>, Signal> {
    let ts_ms = match fields.get("_timestamp_ms") {
        Some(Value::Int(ms)) => *ms,
        _ => return Err(sig_err("DateTime missing _timestamp_ms")),
    };
    let tz_str = match fields.get("_tz") {
        Some(Value::String(s)) => s.as_str(),
        _ => return Err(sig_err("DateTime missing _tz")),
    };
    let tz: Tz = tz_str.parse().map_err(|_| sig_err(format!("invalid timezone: {}", tz_str)))?;
    let secs = ts_ms / 1000;
    let nanos = ((ts_ms % 1000) * 1_000_000) as u32;
    match chrono::DateTime::from_timestamp(secs, nanos) {
        Some(utc_dt) => Ok(utc_dt.with_timezone(&tz)),
        None => Err(sig_err(format!("invalid timestamp: {}", ts_ms))),
    }
}

/// Format a chrono DateTime as ISO 8601 / RFC 3339.
pub(crate) fn format_iso(fields: &BTreeMap<String, Value>) -> Result<String, Signal> {
    let dt = extract_chrono_dt(fields)?;
    Ok(dt.to_rfc3339())
}

/// Detect local IANA timezone: $TZ → /etc/timezone → /etc/localtime symlink
/// → a zone matching the OS's current UTC offset → UTC.
fn local_tz_name() -> String {
    // 1. $TZ environment variable
    if let Ok(tz_env) = std::env::var("TZ") {
        if !tz_env.is_empty() {
            if tz_env.parse::<Tz>().is_ok() {
                return tz_env;
            }
        }
    }
    // 2. /etc/timezone (Debian/Ubuntu)
    if let Ok(contents) = std::fs::read_to_string("/etc/timezone") {
        let name = contents.trim().to_string();
        if !name.is_empty() && name.parse::<Tz>().is_ok() {
            return name;
        }
    }
    // 3. /etc/localtime symlink (most Linux distros, macOS)
    if let Ok(target) = std::fs::read_link("/etc/localtime") {
        let path = target.to_string_lossy();
        if let Some(tz) = path.strip_prefix("/usr/share/zoneinfo/") {
            if !tz.is_empty() && tz.parse::<Tz>().is_ok() {
                return tz.to_string();
            }
        }
    }
    // 4. Neither file exists on Windows, and returning UTC there would be a
    //    silently wrong answer rather than a missing one. Ask the OS for its
    //    current offset and pick a zone that agrees with it. The zone name is
    //    a guess, but every arithmetic result for *now* is correct, which is
    //    what scripts actually use it for.
    if let Some(name) = tz_matching_local_offset() {
        return name;
    }
    "UTC".to_string()
}

/// The first IANA zone whose current UTC offset matches the OS's.
fn tz_matching_local_offset() -> Option<String> {
    use chrono::{Local, Offset, TimeZone, Utc};
    let now = Utc::now();
    let want = Local.from_utc_datetime(&now.naive_utc()).offset().fix();
    if want.local_minus_utc() == 0 {
        return None; // UTC is already the fallback; no need to guess a name.
    }
    chrono_tz::TZ_VARIANTS
        .iter()
        .find(|tz| {
            tz.from_utc_datetime(&now.naive_utc()).offset().fix() == want
        })
        .map(|tz| tz.name().to_string())
}

fn sig_err(msg: impl Into<String>) -> Signal {
    Signal::Error(QueError::new(ErrorKind::Runtime, msg.into()))
}

fn parse_tz(name: &str) -> Result<Tz, Signal> {
    name.parse::<Tz>().map_err(|_| sig_err(format!("invalid timezone: {}", name)))
}

impl Interpreter {
    pub(crate) fn call_time(&mut self, func: &str, args: &[Value]) -> IResult {
        match func {
            // time.now() -> DateTime in local timezone
            "now" => {
                let tz_name = local_tz_name();
                let tz = parse_tz(&tz_name)?;
                let now = chrono::Utc::now().with_timezone(&tz);
                Ok(make_datetime_value(now.timestamp_millis(), &tz_name))
            }
            // time.timestamp() -> Unix milliseconds, for measuring elapsed
            // time. `time.now()` answers "what is the date"; this answers
            // "how long did that take", and the two are different questions.
            "timestamp" => {
                use std::time::{SystemTime, UNIX_EPOCH};
                let millis = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64;
                Ok(Value::Int(millis))
            }
            // time.of(y, m, d, h?, min?, sec?, tz?) -> DateTime
            "of" => {
                let year = match args.first() {
                    Some(Value::Int(y)) => *y as i32,
                    _ => return Err(sig_err("time.of() requires year as first argument")),
                };
                let month = match args.get(1) {
                    Some(Value::Int(m)) => *m as u32,
                    _ => return Err(sig_err("time.of() requires month as second argument")),
                };
                let day = match args.get(2) {
                    Some(Value::Int(d)) => *d as u32,
                    _ => return Err(sig_err("time.of() requires day as third argument")),
                };
                let hour = match args.get(3) {
                    Some(Value::Int(h)) => *h as u32,
                    _ => 0,
                };
                let minute = match args.get(4) {
                    Some(Value::Int(m)) => *m as u32,
                    _ => 0,
                };
                let second = match args.get(5) {
                    Some(Value::Int(s)) => *s as u32,
                    _ => 0,
                };
                let tz_name = match args.get(6) {
                    Some(Value::String(s)) => s.clone(),
                    _ => "UTC".to_string(),
                };
                let tz = parse_tz(&tz_name)?;

                let naive = NaiveDate::from_ymd_opt(year, month, day)
                    .and_then(|d| d.and_hms_opt(hour, minute, second))
                    .ok_or_else(|| sig_err(format!(
                        "invalid date/time: {}-{:02}-{:02} {:02}:{:02}:{:02}",
                        year, month, day, hour, minute, second
                    )))?;

                let dt = tz.from_local_datetime(&naive)
                    .single()
                    .ok_or_else(|| sig_err(format!(
                        "ambiguous or invalid local time in timezone {}",
                        tz_name
                    )))?;

                Ok(make_datetime_value(dt.timestamp_millis(), &tz_name))
            }
            // time.parse(str, fmt, tz?) -> DateTime
            "parse" => {
                let input = match args.first() {
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(sig_err("time.parse() requires a string as first argument")),
                };
                let fmt = match args.get(1) {
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(sig_err("time.parse() requires a format string as second argument")),
                };
                let tz_name = match args.get(2) {
                    Some(Value::String(s)) => s.clone(),
                    _ => "UTC".to_string(),
                };
                let tz = parse_tz(&tz_name)?;

                let naive = chrono::NaiveDateTime::parse_from_str(input, fmt)
                    .map_err(|e| sig_err(format!("time.parse() failed: {}", e)))?;

                let dt = tz.from_local_datetime(&naive)
                    .single()
                    .ok_or_else(|| sig_err(format!(
                        "ambiguous or invalid local time in timezone {}",
                        tz_name
                    )))?;

                Ok(make_datetime_value(dt.timestamp_millis(), &tz_name))
            }
            // time.from_timestamp(ms, tz?) -> DateTime
            "from_timestamp" => {
                let ms = match args.first() {
                    Some(Value::Int(ms)) => *ms,
                    _ => return Err(sig_err("time.from_timestamp() requires an Int (milliseconds)")),
                };
                let tz_name = match args.get(1) {
                    Some(Value::String(s)) => s.clone(),
                    _ => "UTC".to_string(),
                };
                // Validate the timezone
                parse_tz(&tz_name)?;

                Ok(make_datetime_value(ms, &tz_name))
            }
            // time.timezone() -> String (detected system IANA timezone)
            "timezone" => {
                Ok(Value::String(local_tz_name()))
            }
            _ => Err(sig_err(format!("unknown function 'time.{}'", func))),
        }
    }

    /// Handle method calls on DateTime instances.
    pub(crate) fn datetime_method(
        &mut self,
        fields: &BTreeMap<String, Value>,
        method: &str,
        args: &[Value],
    ) -> IResult {
        match method {
            "year" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::Int(dt.year() as i64))
            }
            "month" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::Int(dt.month() as i64))
            }
            "day" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::Int(dt.day() as i64))
            }
            "hour" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::Int(dt.hour() as i64))
            }
            "minute" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::Int(dt.minute() as i64))
            }
            "second" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::Int(dt.second() as i64))
            }
            "weekday" => {
                let dt = extract_chrono_dt(fields)?;
                let name = match dt.weekday() {
                    chrono::Weekday::Mon => "Mon",
                    chrono::Weekday::Tue => "Tue",
                    chrono::Weekday::Wed => "Wed",
                    chrono::Weekday::Thu => "Thu",
                    chrono::Weekday::Fri => "Fri",
                    chrono::Weekday::Sat => "Sat",
                    chrono::Weekday::Sun => "Sun",
                };
                Ok(Value::String(name.to_string()))
            }
            "day_of_year" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::Int(dt.ordinal() as i64))
            }
            "format" => {
                let fmt = match args.first() {
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(sig_err("DateTime.format() requires a format string")),
                };
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::String(dt.format(fmt).to_string()))
            }
            "to_iso" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::String(dt.to_rfc3339()))
            }
            "in_tz" => {
                let tz_name = match args.first() {
                    Some(Value::String(s)) => s.as_str(),
                    _ => return Err(sig_err("DateTime.in_tz() requires a timezone name")),
                };
                let tz = parse_tz(tz_name)?;
                let dt = extract_chrono_dt(fields)?;
                let converted = dt.with_timezone(&tz);
                Ok(make_datetime_value(converted.timestamp_millis(), tz_name))
            }
            "utc" => {
                let ts_ms = match fields.get("_timestamp_ms") {
                    Some(Value::Int(ms)) => *ms,
                    _ => return Err(sig_err("DateTime missing _timestamp_ms")),
                };
                Ok(make_datetime_value(ts_ms, "UTC"))
            }
            "timezone" => {
                match fields.get("_tz") {
                    Some(Value::String(s)) => Ok(Value::String(s.clone())),
                    _ => Err(sig_err("DateTime missing _tz")),
                }
            }
            "add_days" => {
                let n = match args.first() {
                    Some(Value::Int(n)) => *n,
                    _ => return Err(sig_err("DateTime.add_days() requires an Int")),
                };
                let dt = extract_chrono_dt(fields)?;
                let new_dt = dt + Duration::days(n);
                let tz_name = fields.get("_tz").and_then(|v| match v {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                }).unwrap_or("UTC");
                Ok(make_datetime_value(new_dt.timestamp_millis(), tz_name))
            }
            "add_hours" => {
                let n = match args.first() {
                    Some(Value::Int(n)) => *n,
                    _ => return Err(sig_err("DateTime.add_hours() requires an Int")),
                };
                let dt = extract_chrono_dt(fields)?;
                let new_dt = dt + Duration::hours(n);
                let tz_name = fields.get("_tz").and_then(|v| match v {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                }).unwrap_or("UTC");
                Ok(make_datetime_value(new_dt.timestamp_millis(), tz_name))
            }
            "add_minutes" => {
                let n = match args.first() {
                    Some(Value::Int(n)) => *n,
                    _ => return Err(sig_err("DateTime.add_minutes() requires an Int")),
                };
                let dt = extract_chrono_dt(fields)?;
                let new_dt = dt + Duration::minutes(n);
                let tz_name = fields.get("_tz").and_then(|v| match v {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                }).unwrap_or("UTC");
                Ok(make_datetime_value(new_dt.timestamp_millis(), tz_name))
            }
            "add_seconds" => {
                let n = match args.first() {
                    Some(Value::Int(n)) => *n,
                    _ => return Err(sig_err("DateTime.add_seconds() requires an Int")),
                };
                let dt = extract_chrono_dt(fields)?;
                let new_dt = dt + Duration::seconds(n);
                let tz_name = fields.get("_tz").and_then(|v| match v {
                    Value::String(s) => Some(s.as_str()),
                    _ => None,
                }).unwrap_or("UTC");
                Ok(make_datetime_value(new_dt.timestamp_millis(), tz_name))
            }
            "timestamp" => {
                match fields.get("_timestamp_ms") {
                    Some(Value::Int(ms)) => Ok(Value::Int(*ms)),
                    _ => Err(sig_err("DateTime missing _timestamp_ms")),
                }
            }
            "to_string" => {
                let dt = extract_chrono_dt(fields)?;
                Ok(Value::String(dt.to_rfc3339()))
            }
            "iso" => Err(sig_err("`.iso()` was removed; use `.to_iso()` instead")),
            "tz" => Err(sig_err("`.tz()` was removed; use `.timezone()` instead")),
            _ => Err(sig_err(format!("DateTime has no method '{}'", method))),
        }
    }
}
