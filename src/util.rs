//! Közös segédek: idő, véletlen, tokenek, időbélyegek, JSON-kezelés.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{NaiveDateTime, Utc};
use rand::RngExt;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// Az adatbázis időbélyegeinek formátuma (UTC, időzóna nélkül).
pub const TS_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

/// Unix idő másodpercben (törtrésszel), mint a Python `time.time()`.
pub fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Az aktuális UTC idő időzóna nélkül.
pub fn utcnow() -> NaiveDateTime {
    Utc::now().naive_utc()
}

pub fn format_ts(moment: NaiveDateTime) -> String {
    moment.format(TS_FORMAT).to_string()
}

/// Adatbázis-időbélyeg → idő (hibás vagy hiányzó érték → None).
pub fn parse_ts(value: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(value, TS_FORMAT).ok()
}

pub fn now_ts() -> String {
    format_ts(utcnow())
}

/// Mint a Python `secrets.token_urlsafe(nbytes)`: URL-barát, kitöltés nélküli base64.
pub fn token_urlsafe(nbytes: usize) -> String {
    let mut bytes = vec![0u8; nbytes];
    rand::rng().fill(&mut bytes[..]);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Mint a Python `secrets.randbelow(n)`.
pub fn random_below(n: u32) -> u32 {
    rand::rng().random_range(0..n)
}

/// Rövid (8 karakteres) véletlen azonosító, mint a Python `str(uuid.uuid4())[:8]`.
pub fn short_id() -> String {
    let mut bytes = [0u8; 4];
    rand::rng().fill(&mut bytes[..]);
    hex::encode(bytes)
}

/// Egy JSON érték egész számként (a Python `isinstance(x, int) and not bool` megfelelője).
pub fn as_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
        _ => None,
    }
}

/// Szigorúan egész szám (a lebegőpontos érték nem számít, mint a Python `isinstance(x, int)`).
pub fn strict_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) if n.is_i64() || n.is_u64() => n.as_i64(),
        _ => None,
    }
}

/// A Python `int(x)` megfelelője JSON értékre (szám, szám szövegként; a bool is számít, mint Pythonban).
pub fn py_int(value: &Value) -> Option<i64> {
    match value {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::Bool(b) => Some(*b as i64),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// A Python `bool(x)` megfelelője JSON értékre.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

pub fn str_of(value: &Value) -> Option<&str> {
    value.as_str()
}

/// Egy objektum mezője (vagy Null).
pub fn field<'a>(value: &'a Value, key: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    value.get(key).unwrap_or(&NULL)
}

/// A Python `str.casefold()` közelítése (a magyar betűkhöz elegendő a kisbetűsítés).
pub fn casefold(text: &str) -> String {
    text.to_lowercase()
}

/// A Python `\w` megfelelője: betű, szám vagy aláhúzás.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
