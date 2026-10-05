//! Generikus forgalomkorlátozó Socket.IO (SID) és HTTP (IP) kérésekhez.

use crate::util;
use parking_lot::Mutex;
use serde_json::{Value, json};
use std::collections::{HashMap, VecDeque};

const IP_PRUNE_THRESHOLD: usize = 1000;

type Limit = (u32, u32);

#[derive(Default)]
struct Inner {
    socket_history: HashMap<String, HashMap<String, VecDeque<f64>>>,
    ip_history: HashMap<String, HashMap<String, VecDeque<f64>>>,
    /// Futásidejű felülbírálatok (admin panel): az eredeti korlát-táblákat nem írják át
    socket_overrides: HashMap<String, Limit>,
    ip_overrides: HashMap<String, Limit>,
}

pub struct RateLimiter {
    socket_limits: HashMap<String, Limit>,
    ip_limits: HashMap<String, Limit>,
    inner: Mutex<Inner>,
}

fn trim(stamps: &mut VecDeque<f64>, now: f64, window: f64) {
    while stamps.front().is_some_and(|t| now - t >= window) {
        stamps.pop_front();
    }
}

impl RateLimiter {
    pub fn new(socket_limits: &[(&str, Limit)], ip_limits: &[(&str, Limit)]) -> RateLimiter {
        RateLimiter {
            socket_limits: socket_limits.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            ip_limits: ip_limits.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
            inner: Mutex::new(Inner::default()),
        }
    }

    /// A forgalomkorlátok felülbírálata ({név: (max, ablak_mp)}); üres → az eredeti értékek.
    pub fn set_overrides(&self, ip: HashMap<String, Limit>, socket: HashMap<String, Limit>) {
        let mut inner = self.inner.lock();
        inner.ip_overrides = ip;
        inner.socket_overrides = socket;
    }

    pub fn ip_limit(&self, action: &str) -> Option<Limit> {
        self.inner.lock().ip_overrides.get(action).copied().or_else(|| self.ip_limits.get(action).copied())
    }

    pub fn socket_limit(&self, event: &str) -> Option<Limit> {
        self.inner.lock().socket_overrides.get(event).copied().or_else(|| self.socket_limits.get(event).copied())
    }

    /// A jelenleg érvényes korlátok és az alapértelmezettek: {'http': {név: {...}}, 'socket': {...}}.
    pub fn effective_limits(&self) -> Value {
        let inner = self.inner.lock();
        let table = |defaults: &HashMap<String, Limit>, overrides: &HashMap<String, Limit>| {
            let mut names: Vec<&String> = defaults.keys().chain(overrides.keys()).collect();
            names.sort();
            names.dedup();
            let mut map = serde_json::Map::new();
            for name in names {
                let default = defaults.get(name).map(|l| json!([l.0, l.1]));
                let current = overrides.get(name).or_else(|| defaults.get(name)).map(|l| json!([l.0, l.1]));
                map.insert(
                    name.clone(),
                    json!({"default": default, "current": current, "overridden": overrides.contains_key(name)}),
                );
            }
            Value::Object(map)
        };
        json!({
            "http": table(&self.ip_limits, &inner.ip_overrides),
            "socket": table(&self.socket_limits, &inner.socket_overrides),
        })
    }

    fn limited(history: &HashMap<String, HashMap<String, VecDeque<f64>>>, limit_of: &dyn Fn(&str) -> Option<Limit>, key_name: &str, now: f64) -> Vec<Value> {
        let mut rows = Vec::new();
        for (key, actions) in history {
            for (action, stamps) in actions {
                let Some((maximum, window)) = limit_of(action) else { continue };
                let recent: Vec<f64> = stamps.iter().copied().filter(|t| now - t < window as f64).collect();
                if recent.len() >= maximum as usize {
                    let oldest = recent.iter().cloned().fold(f64::INFINITY, f64::min);
                    let mut row = serde_json::Map::new();
                    row.insert(key_name.to_string(), json!(key));
                    row.insert(if key_name == "ip" { "action" } else { "event" }.to_string(), json!(action));
                    row.insert("count".into(), json!(recent.len()));
                    row.insert("max".into(), json!(maximum));
                    row.insert("window".into(), json!(window));
                    row.insert("retry_in".into(), json!(((window as f64 - (now - oldest)) as i64).max(0)));
                    rows.push(Value::Object(row));
                }
            }
        }
        rows
    }

    /// A jelenleg korlátozott IP-k: [{ip, action, count, max, window, retry_in}].
    pub fn limited_ips(&self) -> Vec<Value> {
        let now = util::now();
        let limits_ip = |a: &str| self.ip_limit(a);
        let inner = self.inner.lock();
        // a határérték-lekérdezés külön zárat vesz: másolat az előzményekről
        let history = inner.ip_history.clone();
        drop(inner);
        Self::limited(&history, &limits_ip, "ip", now)
    }

    /// A jelenleg korlátozott socket kapcsolatok: [{sid, event, count, max, window, retry_in}].
    pub fn limited_sids(&self) -> Vec<Value> {
        let now = util::now();
        let limits_socket = |e: &str| self.socket_limit(e);
        let history = self.inner.lock().socket_history.clone();
        Self::limited(&history, &limits_socket, "sid", now)
    }

    /// Egy IP korlátozásának feloldása (az előzmények törlése). Visszatér: igaz, ha volt mit törölni.
    pub fn clear_ip(&self, ip: &str) -> bool {
        self.inner.lock().ip_history.remove(ip).is_some()
    }

    /// Socket.IO event rate limit ellenőrzés. True = engedélyezve.
    pub fn check_socket(&self, sid: &str, event: &str) -> bool {
        let Some((max_requests, window)) = self.socket_limit(event) else { return true };
        let now = util::now();
        let mut inner = self.inner.lock();
        let stamps = inner.socket_history.entry(sid.to_string()).or_default().entry(event.to_string()).or_default();
        trim(stamps, now, window as f64);
        if stamps.len() >= max_requests as usize {
            return false;
        }
        stamps.push_back(now);
        true
    }

    /// IP-alapú rate limit ellenőrzés (auth endpointokra). True = engedélyezve.
    pub fn check_ip(&self, ip: &str, action: &str) -> bool {
        let Some((max_requests, window)) = self.ip_limit(action) else { return true };
        let now = util::now();
        let mut inner = self.inner.lock();
        if inner.ip_history.len() > IP_PRUNE_THRESHOLD {
            let longest = self.ip_limits.values().chain(inner.ip_overrides.values()).map(|l| l.1).max().unwrap_or(0) as f64;
            inner.ip_history.retain(|_, actions| actions.values().any(|stamps| stamps.iter().any(|t| now - t < longest)));
        }
        let stamps = inner.ip_history.entry(ip.to_string()).or_default().entry(action.to_string()).or_default();
        trim(stamps, now, window as f64);
        if stamps.len() >= max_requests as usize {
            return false;
        }
        stamps.push_back(now);
        true
    }

    /// SID törlése disconnect-kor.
    pub fn clear_sid(&self, sid: &str) {
        self.inner.lock().socket_history.remove(sid);
    }

    /// A memóriában tartott IP-előzmények száma (admin rendszer-nézet).
    pub fn ip_history_len(&self) -> usize {
        self.inner.lock().ip_history.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limiter() -> RateLimiter {
        RateLimiter::new(&[("chat", (3, 10))], &[("login", (2, 60))])
    }

    #[test]
    fn socket_limit_blocks_after_max() {
        let l = limiter();
        assert!(l.check_socket("a", "chat"));
        assert!(l.check_socket("a", "chat"));
        assert!(l.check_socket("a", "chat"));
        assert!(!l.check_socket("a", "chat"));
        assert!(l.check_socket("b", "chat"), "másik kapcsolat külön számlálót kap");
        assert!(l.check_socket("a", "ismeretlen"), "a nem korlátozott esemény szabad");
    }

    #[test]
    fn ip_limit_and_clear() {
        let l = limiter();
        assert!(l.check_ip("1.1.1.1", "login"));
        assert!(l.check_ip("1.1.1.1", "login"));
        assert!(!l.check_ip("1.1.1.1", "login"));
        assert_eq!(l.limited_ips().len(), 1);
        assert_eq!(l.limited_ips()[0]["action"], "login");
        assert!(l.clear_ip("1.1.1.1"));
        assert!(!l.clear_ip("1.1.1.1"));
        assert!(l.check_ip("1.1.1.1", "login"));
        assert!(l.limited_ips().is_empty());
    }

    #[test]
    fn overrides_replace_defaults() {
        let l = limiter();
        l.set_overrides(HashMap::from([("login".to_string(), (1, 60))]), HashMap::new());
        assert!(l.check_ip("2.2.2.2", "login"));
        assert!(!l.check_ip("2.2.2.2", "login"));
        let effective = l.effective_limits();
        assert_eq!(effective["http"]["login"]["overridden"], true);
        assert_eq!(effective["http"]["login"]["default"], json!([2, 60]));
        assert_eq!(effective["http"]["login"]["current"], json!([1, 60]));
        assert_eq!(effective["socket"]["chat"]["overridden"], false);
        l.set_overrides(HashMap::new(), HashMap::new());
        assert_eq!(l.ip_limit("login"), Some((2, 60)));
    }

    #[test]
    fn clearing_sid_resets_its_history() {
        let l = limiter();
        for _ in 0..3 {
            l.check_socket("a", "chat");
        }
        assert!(!l.check_socket("a", "chat"));
        assert_eq!(l.limited_sids().len(), 1);
        l.clear_sid("a");
        assert!(l.check_socket("a", "chat"));
    }
}
