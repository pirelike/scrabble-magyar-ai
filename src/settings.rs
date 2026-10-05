//! Futásidejű beállítások (funkciókapcsolók), újraindítás nélkül módosíthatók az admin panelről.
//!
//! A beállítások az `app_settings` táblában élnek (`cfg.<kulcs>` → JSON érték, `cfgmeta.<kulcs>` → ki és mikor
//! módosította), a szerver gyorsítótárazott `get(kulcs)` hívással olvassa őket. Nincs felülbírálat → az alapérték
//! számít (a környezeti változó vagy a kódban lévő konstans). A módosítás (`store`) a hívó tranzakciójában íródik,
//! a gyorsítótárat utána kell érvényteleníteni (`invalidate`).

use crate::db::Db;
use crate::util;
use parking_lot::RwLock;
use rusqlite::{Transaction, params};
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use std::sync::Arc;

const PREFIX: &str = "cfg.";
const META_PREFIX: &str = "cfgmeta.";

pub const MAINTENANCE_KEY: &str = "maintenance";

/// A beállítás típusa.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Bool,
    Int { min: i64, max: i64 },
    Float { min: f64, max: f64 },
    Choice(Vec<Value>),
    Limits,
    Json,
}

impl Kind {
    pub fn name(&self) -> &'static str {
        match self {
            Kind::Bool => "bool",
            Kind::Int { .. } => "int",
            Kind::Float { .. } => "float",
            Kind::Choice(_) => "choice",
            Kind::Limits => "limits",
            Kind::Json => "json",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Definition {
    pub key: &'static str,
    pub group: &'static str,
    pub kind: Kind,
    pub default: Value,
    pub hidden: bool,
}

pub const GROUPS: [&str; 9] = ["access", "rooms", "bots", "features", "dictionary", "timing", "chat", "limits", "logging"];

fn def(key: &'static str, group: &'static str, kind: Kind, default: Value) -> Definition {
    Definition { key, group, kind, default, hidden: false }
}

/// A beállítások leírása. `word_reject_threshold` alapértéke a konfigurációból jön.
pub fn definitions(config_threshold: i64) -> Vec<Definition> {
    let int = |min, max| Kind::Int { min, max };
    let mut list = vec![
        def("registration_open", "access", Kind::Bool, json!(true)),
        def("guest_allowed", "access", Kind::Bool, json!(true)),
        def("room_creation", "rooms", Kind::Choice(vec![json!("everyone"), json!("registered"), json!("none")]), json!("everyone")),
        def("max_spectators", "rooms", int(1, 200), json!(30)),
        def("default_hint_limit", "rooms", Kind::Choice(vec![json!(0), json!(1), json!(3), json!(5), json!(10)]), json!(3)),
        def("bots_enabled", "bots", Kind::Bool, json!(true)),
        def("max_bots", "bots", int(0, 3), json!(3)),
        def("default_bot_level", "bots", int(1, 10), json!(6)),
        def("bot_think_multiplier", "bots", Kind::Float { min: 0.0, max: 5.0 }, json!(1.0)),
        def("feature_daily", "features", Kind::Bool, json!(true)),
        def("feature_async", "features", Kind::Bool, json!(true)),
        def("feature_practice", "features", Kind::Bool, json!(true)),
        def("feature_word_review", "features", Kind::Bool, json!(true)),
        def("word_reject_threshold", "dictionary", int(1, 50), json!(config_threshold)),
        def("grace_disconnect", "timing", int(15, 1800), json!(120)),
        def("grace_waiting_owner", "timing", int(60, 7200), json!(600)),
        def("chat_max_length", "chat", int(20, 500), json!(200)),
        def("chat_rate_count", "chat", int(1, 60), json!(10)),
        def("chat_rate_window", "chat", int(1, 120), json!(10)),
        def("banned_word_action", "chat", Kind::Choice(vec![json!("mask"), json!("drop")]), json!("mask")),
        def("chat_log_enabled", "logging", Kind::Bool, json!(false)),
        def("chat_log_days", "logging", int(1, 365), json!(14)),
        def("backup_daily", "logging", Kind::Bool, json!(false)),
        def("backup_keep", "logging", int(1, 60), json!(7)),
        def("rate_limits_http", "limits", Kind::Limits, json!({})),
        def("rate_limits_socket", "limits", Kind::Limits, json!({})),
    ];
    // A karbantartási mód külön tárolódik (nem a beállítások oldalán szerkeszthető, hanem a Kommunikáció alatt)
    let mut maintenance = def(MAINTENANCE_KEY, "access", Kind::Json, json!({}));
    maintenance.hidden = true;
    list.push(maintenance);
    list
}

/// Érvénytelen beállítás (a HTTP réteg 400-zal válaszol).
#[derive(Debug, Clone, PartialEq)]
pub struct SettingError(pub String);

impl std::fmt::Display for SettingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SettingError {}

fn err(message: &str) -> SettingError {
    SettingError(message.to_string())
}

#[derive(Default)]
struct Cache {
    loaded: bool,
    values: HashMap<String, Value>,
    meta: HashMap<String, Value>,
}

pub struct Settings {
    db: Arc<Db>,
    definitions: Vec<Definition>,
    cache: RwLock<Cache>,
}

/// Forgalomkorlát-felülbírálat: {név: [max_kérés, ablak_mp]} (csak ismert nevek, értelmes határok).
pub fn validate_limits(value: &Value) -> Result<Value, SettingError> {
    let Some(map) = value.as_object() else { return Err(err("Érvénytelen forgalomkorlát-lista.")) };
    let mut clean = Map::new();
    for (name, pair) in map {
        let Some(pair) = pair.as_array().filter(|p| p.len() == 2) else { return Err(err("Érvénytelen forgalomkorlát-lista.")) };
        let (Some(count), Some(window)) = (util::strict_int(&pair[0]), util::strict_int(&pair[1])) else {
            return Err(err("Érvénytelen forgalomkorlát-lista."));
        };
        if !((1..=100_000).contains(&count) && (1..=86_400).contains(&window)) {
            return Err(err("A forgalomkorlát értéke tartományon kívül van."));
        }
        clean.insert(name.clone(), json!([count, window]));
    }
    Ok(Value::Object(clean))
}

impl Settings {
    pub fn new(db: Arc<Db>, config_threshold: i64) -> Settings {
        Settings { db, definitions: definitions(config_threshold), cache: RwLock::new(Cache::default()) }
    }

    /// A gyorsítótár eldobása: a következő olvasás újratölti az adatbázisból.
    pub fn invalidate(&self) {
        self.cache.write().loaded = false;
    }

    fn load(&self) {
        if self.cache.read().loaded {
            return;
        }
        let rows = self
            .db
            .with(|tx| crate::db::fetch_all(tx, "SELECT key, value FROM app_settings WHERE key LIKE 'cfg%'", []))
            .unwrap_or_default(); // nincs még adatbázis / séma: minden alapértelmezett
        let mut cache = self.cache.write();
        cache.values.clear();
        cache.meta.clear();
        for row in rows {
            let key = row.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let Some(parsed) = row.get("value").and_then(|v| v.as_str()).and_then(|raw| serde_json::from_str::<Value>(raw).ok()) else { continue };
            if let Some(name) = key.strip_prefix(PREFIX) {
                cache.values.insert(name.to_string(), parsed);
            } else if let Some(name) = key.strip_prefix(META_PREFIX) {
                cache.meta.insert(name.to_string(), parsed);
            }
        }
        cache.loaded = true;
    }

    pub fn definition(&self, key: &str) -> Option<&Definition> {
        self.definitions.iter().find(|d| d.key == key)
    }

    pub fn definitions(&self) -> &[Definition] {
        &self.definitions
    }

    pub fn default_of(&self, key: &str) -> Value {
        self.definition(key).map(|d| d.default.clone()).unwrap_or(Value::Null)
    }

    pub fn is_overridden(&self, key: &str) -> bool {
        self.load();
        self.cache.read().values.contains_key(key)
    }

    /// A beállítás értéke: a felülbírálat, ennek híján az alapérték.
    pub fn get(&self, key: &str) -> Value {
        self.load();
        if let Some(value) = self.cache.read().values.get(key) {
            return value.clone();
        }
        self.default_of(key)
    }

    pub fn get_bool(&self, key: &str) -> bool {
        self.get(key).as_bool().unwrap_or(false)
    }

    pub fn get_i64(&self, key: &str) -> i64 {
        self.get(key).as_i64().unwrap_or(0)
    }

    pub fn get_f64(&self, key: &str) -> f64 {
        self.get(key).as_f64().unwrap_or(0.0)
    }

    pub fn get_string(&self, key: &str) -> String {
        self.get(key).as_str().unwrap_or("").to_string()
    }

    /// A megadott érték ellenőrzése és normalizálása a beállítás típusa szerint.
    pub fn validate(&self, key: &str, value: &Value) -> Result<Value, SettingError> {
        let Some(spec) = self.definition(key) else { return Err(err("Ismeretlen beállítás.")) };
        match &spec.kind {
            Kind::Bool => match value {
                Value::Bool(_) => Ok(value.clone()),
                _ => Err(err("Érvénytelen érték.")),
            },
            Kind::Int { min, max } => {
                let Some(n) = util::strict_int(value) else { return Err(err("Érvénytelen érték.")) };
                if !(*min..=*max).contains(&n) {
                    return Err(err("Az érték a megengedett tartományon kívül van."));
                }
                Ok(json!(n))
            }
            Kind::Float { min, max } => {
                let n = match value {
                    Value::Number(n) => n.as_f64(),
                    _ => None,
                };
                let Some(n) = n else { return Err(err("Érvénytelen érték.")) };
                if !(*min..=*max).contains(&n) {
                    return Err(err("Az érték a megengedett tartományon kívül van."));
                }
                Ok(json!((n * 100.0).round_ties_even() / 100.0))
            }
            Kind::Choice(choices) => {
                if value.is_boolean() || !choices.contains(value) {
                    return Err(err("Érvénytelen érték."));
                }
                Ok(value.clone())
            }
            Kind::Limits => validate_limits(value),
            Kind::Json => {
                if value.is_object() { Ok(value.clone()) } else { Err(err("Érvénytelen érték.")) }
            }
        }
    }

    /// Egy beállítás felülbírálatának beírása a megadott tranzakcióban. `value` Null → a felülbírálat törlése
    /// (visszaállítás az alapértékre). A hívó `invalidate()`-t hív a tranzakció után.
    pub fn store(&self, tx: &Transaction, key: &str, value: &Value, admin_id: Option<i64>) -> Result<(), SettingError> {
        if self.definition(key).is_none() {
            return Err(err("Ismeretlen beállítás."));
        }
        let to_err = |e: rusqlite::Error| SettingError(e.to_string());
        if value.is_null() {
            tx.execute("DELETE FROM app_settings WHERE key = ?", [format!("{PREFIX}{key}")]).map_err(to_err)?;
            tx.execute("DELETE FROM app_settings WHERE key = ?", [format!("{META_PREFIX}{key}")]).map_err(to_err)?;
            return Ok(());
        }
        let clean = self.validate(key, value)?;
        let stamp = util::now_ts();
        for (name, payload) in [(format!("{PREFIX}{key}"), clean), (format!("{META_PREFIX}{key}"), json!({"by": admin_id, "at": stamp}))] {
            tx.execute(
                "INSERT INTO app_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![name, serde_json::to_string(&payload).unwrap_or_default()],
            )
            .map_err(to_err)?;
        }
        Ok(())
    }

    /// Az érvényben lévő karbantartási mód ({enabled, message_hu, message_en, until}), vagy None. A lejárt
    /// (`until` elmúlt) karbantartás magától véget ér.
    pub fn maintenance(&self) -> Option<Value> {
        let value = self.get(MAINTENANCE_KEY);
        if !value.is_object() || !value.get("enabled").is_some_and(util::truthy) {
            return None;
        }
        let until = value.get("until").and_then(|u| u.as_str()).filter(|u| !u.is_empty()).and_then(util::parse_ts);
        if until.is_some_and(|u| u <= util::utcnow()) {
            return None;
        }
        Some(value)
    }

    /// Az összes (nem rejtett) beállítás: érték, alapérték, módosítva-e, ki és mikor módosította.
    pub fn describe(&self, names: Option<&[String]>) -> Vec<Value> {
        self.load();
        let cache = self.cache.read();
        let mut items = Vec::new();
        for spec in &self.definitions {
            if spec.hidden || names.is_some_and(|n| !n.iter().any(|k| k == spec.key)) {
                continue;
            }
            let meta = cache.meta.get(spec.key).cloned().unwrap_or(Value::Null);
            let value = cache.values.get(spec.key).cloned().unwrap_or_else(|| spec.default.clone());
            let mut item = Map::new();
            item.insert("key".into(), json!(spec.key));
            item.insert("group".into(), json!(spec.group));
            item.insert("type".into(), json!(spec.kind.name()));
            item.insert("value".into(), value);
            item.insert("default".into(), spec.default.clone());
            item.insert("overridden".into(), json!(cache.values.contains_key(spec.key)));
            item.insert("changed_by".into(), meta.get("by").cloned().unwrap_or(Value::Null));
            item.insert("changed_at".into(), meta.get("at").cloned().unwrap_or(Value::Null));
            match &spec.kind {
                Kind::Int { min, max } => {
                    item.insert("min".into(), json!(min));
                    item.insert("max".into(), json!(max));
                }
                Kind::Float { min, max } => {
                    item.insert("min".into(), json!(min));
                    item.insert("max".into(), json!(max));
                }
                Kind::Choice(choices) => {
                    item.insert("choices".into(), Value::Array(choices.clone()));
                }
                _ => {}
            }
            items.push(Value::Object(item));
        }
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> Settings {
        Settings::new(Arc::new(Db::open_temp().unwrap()), 1)
    }

    #[test]
    fn defaults_apply_without_overrides() {
        let s = settings();
        assert!(s.get_bool("registration_open"));
        assert_eq!(s.get_i64("grace_disconnect"), 120);
        assert_eq!(s.get_string("room_creation"), "everyone");
        assert_eq!(s.get_f64("bot_think_multiplier"), 1.0);
        assert!(!s.is_overridden("grace_disconnect"));
        assert!(s.maintenance().is_none());
    }

    #[test]
    fn store_validates_and_persists() {
        let s = settings();
        s.db.with(|tx| Ok(s.store(tx, "grace_disconnect", &json!(300), Some(7)))).unwrap().unwrap();
        s.invalidate();
        assert_eq!(s.get_i64("grace_disconnect"), 300);
        assert!(s.is_overridden("grace_disconnect"));
        let item = s.describe(Some(&["grace_disconnect".to_string()])).remove(0);
        assert_eq!(item["changed_by"], 7);
        assert_eq!(item["default"], 120);
        // visszaállítás az alapra
        s.db.with(|tx| Ok(s.store(tx, "grace_disconnect", &Value::Null, None))).unwrap().unwrap();
        s.invalidate();
        assert_eq!(s.get_i64("grace_disconnect"), 120);
    }

    #[test]
    fn validation_rules() {
        let s = settings();
        assert!(s.validate("registration_open", &json!("igen")).is_err());
        assert!(s.validate("grace_disconnect", &json!(5)).is_err());
        assert!(s.validate("grace_disconnect", &json!(true)).is_err());
        assert!(s.validate("grace_disconnect", &json!(60.5)).is_err());
        assert_eq!(s.validate("grace_disconnect", &json!(60)).unwrap(), json!(60));
        assert!(s.validate("room_creation", &json!("senki")).is_err());
        assert!(s.validate("default_hint_limit", &json!(4)).is_err());
        assert_eq!(s.validate("default_hint_limit", &json!(5)).unwrap(), json!(5));
        assert_eq!(s.validate("bot_think_multiplier", &json!(1.2345)).unwrap(), json!(1.23));
        assert!(s.validate("bot_think_multiplier", &json!(9)).is_err());
        assert!(s.validate("nincs_ilyen", &json!(1)).is_err());
        assert!(s.validate("rate_limits_http", &json!({"login": [5, 60]})).is_ok());
        assert!(s.validate("rate_limits_http", &json!({"login": [0, 60]})).is_err());
        assert!(s.validate("rate_limits_http", &json!({"login": [5]})).is_err());
        assert!(s.validate("rate_limits_http", &json!({"login": [true, 60]})).is_err());
        assert!(s.validate("rate_limits_http", &json!([1, 2])).is_err());
    }

    #[test]
    fn maintenance_expires() {
        let s = settings();
        let future = util::format_ts(util::utcnow() + chrono::Duration::hours(1));
        let past = util::format_ts(util::utcnow() - chrono::Duration::hours(1));
        s.db.with(|tx| Ok(s.store(tx, MAINTENANCE_KEY, &json!({"enabled": true, "until": future}), None))).unwrap().unwrap();
        s.invalidate();
        assert!(s.maintenance().is_some());
        s.db.with(|tx| Ok(s.store(tx, MAINTENANCE_KEY, &json!({"enabled": true, "until": past}), None))).unwrap().unwrap();
        s.invalidate();
        assert!(s.maintenance().is_none());
        s.db.with(|tx| Ok(s.store(tx, MAINTENANCE_KEY, &json!({"enabled": false}), None))).unwrap().unwrap();
        s.invalidate();
        assert!(s.maintenance().is_none());
    }

    #[test]
    fn hidden_settings_are_not_described() {
        let s = settings();
        assert!(s.describe(None).iter().all(|i| i["key"] != MAINTENANCE_KEY));
        assert!(s.describe(None).len() >= 20);
    }
}
