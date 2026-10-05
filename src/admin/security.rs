//! Admin panel: biztonság — IP tiltás (a játékszerver őre is használja).

use crate::admin::candidates;
use crate::config::IpNet;
use crate::db::Db;
use crate::util;
use parking_lot::Mutex;
use std::net::IpAddr;

const CACHE_SECONDS: f64 = 30.0;

/// Az érvényes IP-tiltások gyorsítótára (a változás azonnal érvényesül: `invalidate`).
#[derive(Default)]
pub struct IpBans {
    cache: Mutex<Option<(f64, Vec<(IpNet, i64, String)>)>>,
}

impl IpBans {
    pub fn invalidate(&self) {
        *self.cache.lock() = None;
    }

    fn load(&self, db: &Db) -> Vec<(IpNet, i64, String)> {
        let now = util::now();
        let mut cache = self.cache.lock();
        if let Some((loaded, bans)) = cache.as_ref() {
            if now - loaded < CACHE_SECONDS {
                return bans.clone();
            }
        }
        let stamp = util::now_ts();
        let rows = db
            .with(|tx| crate::db::fetch_all(tx, "SELECT id, ip, reason, expires_at FROM ip_bans WHERE expires_at IS NULL OR expires_at > ?", [&stamp]))
            .unwrap_or_default(); // séma nélküli adatbázis: nincs tiltás
        let bans: Vec<(IpNet, i64, String)> = rows
            .iter()
            .filter_map(|r| {
                let net = IpNet::parse(r.get("ip")?.as_str()?)?;
                Some((net, r.get("id")?.as_i64()?, r.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string()))
            })
            .collect();
        *cache = Some((now, bans.clone()));
        bans
    }

    /// Igaz, ha a kliens IP-je egy érvényes tiltás alá esik.
    pub fn is_banned(&self, db: &Db, ip: &str) -> bool {
        let bans = self.load(db);
        if bans.is_empty() {
            return false;
        }
        let Ok(address) = ip.trim().parse::<IpAddr>() else { return false };
        let options = candidates(address);
        bans.iter().any(|(net, _, _)| options.iter().any(|c| net.contains(c)))
    }
}
