//! Konfiguráció: helyi `.env` fájl, környezeti változók, konstansok.

use std::collections::HashSet;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Hálózat (IP / CIDR) az admin IP-listához.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct IpNet {
    pub addr: IpAddr,
    pub prefix: u8,
}

impl IpNet {
    /// Elfogadja az `a.b.c.d`, `a.b.c.d/n`, IPv6 és `::/n` alakokat (mint a Python `ip_network(strict=False)`).
    pub fn parse(text: &str) -> Option<IpNet> {
        let (addr_text, prefix_text) = match text.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (text, None),
        };
        let addr: IpAddr = addr_text.parse().ok()?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        let prefix = match prefix_text {
            Some(p) => p.parse::<u8>().ok().filter(|p| *p <= max)?,
            None => max,
        };
        Some(IpNet { addr, prefix })
    }

    pub fn contains(&self, ip: &IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - self.prefix as u32) };
                (u32::from(net) & mask) == (u32::from(*ip) & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - self.prefix as u32) };
                (u128::from(net) & mask) == (u128::from(*ip) & mask)
            }
            _ => false,
        }
    }
}

impl std::fmt::Display for IpNet {
    /// A hálózat szokásos alakja (a gazdabitek nullázva, mint a Python `str(ip_network(...))`).
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let masked = match self.addr {
            IpAddr::V4(a) => {
                let mask = if self.prefix == 0 { 0 } else { u32::MAX << (32 - self.prefix as u32) };
                IpAddr::V4((u32::from(a) & mask).into())
            }
            IpAddr::V6(a) => {
                let mask = if self.prefix == 0 { 0 } else { u128::MAX << (128 - self.prefix as u32) };
                IpAddr::V6((u128::from(a) & mask).into())
            }
        };
        write!(f, "{}/{}", masked, self.prefix)
    }
}

/// Vesszővel elválasztott IP-címek / CIDR-ek listája. Hibás elemnél hiba: az engedélylista elírása ne
/// kapcsolja ki csendben a korlátozást.
pub fn parse_ip_networks(raw: &str) -> Result<Vec<IpNet>, String> {
    let mut networks = Vec::new();
    for item in raw.split(',') {
        let item = item.trim();
        if item.is_empty() {
            continue;
        }
        match IpNet::parse(item) {
            Some(net) => networks.push(net),
            None => return Err(format!("ADMIN_IP_ALLOWLIST: érvénytelen IP-cím vagy hálózat: '{item}'")),
        }
    }
    Ok(networks)
}

/// A program mappája (ahol a `static/`, `templates/`, `dict/` van). A `SCRABBLE_BASE_DIR`, ennek híján a
/// futtatás mappája, a végrehajtható fájl környéke, végül a fordítás helye.
pub fn base_dir() -> &'static Path {
    static BASE: OnceLock<PathBuf> = OnceLock::new();
    BASE.get_or_init(|| {
        if let Ok(dir) = std::env::var("SCRABBLE_BASE_DIR") {
            return PathBuf::from(dir);
        }
        let mut candidates: Vec<PathBuf> = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd);
        }
        if let Ok(exe) = std::env::current_exe() {
            let mut dir = exe.parent().map(|p| p.to_path_buf());
            for _ in 0..4 {
                match dir {
                    Some(d) => {
                        candidates.push(d.clone());
                        dir = d.parent().map(|p| p.to_path_buf());
                    }
                    None => break,
                }
            }
        }
        candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        candidates
            .into_iter()
            .find(|dir| dir.join("static").is_dir() && dir.join("dict").is_dir())
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")))
    })
}

pub fn dict_dir() -> PathBuf {
    base_dir().join("dict")
}

/// A helyi `.env` fájl (soronként `KULCS=érték`, opcionális `export `) beolvasása a környezetbe.
///
/// A fájl nincs a git tárban, ezért a GitHubról frissítés sosem írja felül: ide kerül a gépre jellemző
/// beállítás (pl. `PORT`). A ténylegesen beállított környezeti változó erősebb a fájlnál.
/// Visszatér: a ténylegesen beállított kulcsok.
pub fn load_env_file(path: &Path) -> Vec<String> {
    let mut applied = Vec::new();
    let Ok(text) = std::fs::read_to_string(path) else { return applied };
    for raw in text.lines() {
        let mut line = raw.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("export ") {
            line = rest.trim_start();
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let key = key.trim();
        let mut value = value.trim();
        if value.len() >= 2 {
            let bytes = value.as_bytes();
            if bytes[0] == bytes[value.len() - 1] && (bytes[0] == b'"' || bytes[0] == b'\'') {
                value = &value[1..value.len() - 1];
            }
        }
        let valid_key = !key.is_empty()
            && key.replace('_', "").chars().all(|c| c.is_ascii_alphanumeric())
            && !key.chars().next().is_some_and(|c| c.is_ascii_digit());
        if valid_key && std::env::var_os(key).is_none() {
            // SAFETY: az indulás elején, a szálak elindítása előtt hívódik.
            unsafe { std::env::set_var(key, value) };
            applied.push(key.to_string());
        }
    }
    applied
}

fn env_string(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn env_int(key: &str, default: i64) -> i64 {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok()).unwrap_or(default)
}

/// Az IP-alapú forgalomkorlátok alapértékei: (kérések, ablak másodpercben).
pub const AUTH_RATE_LIMITS: &[(&str, (u32, u32))] = &[
    ("request_code", (3, 300)),
    ("login", (10, 300)),
    ("register", (3, 3600)),
    ("search_users", (20, 60)),
    ("leaderboard", (30, 60)),
    ("dictionary", (60, 60)),
    ("replay", (60, 60)),
    ("analysis", (30, 60)),
    ("daily", (60, 60)),
    ("practice", (120, 60)),
    ("push", (20, 60)),
    ("word_review", (240, 60)),
    ("announcements", (60, 60)),
    ("admin", (120, 60)),
    ("admin_danger", (20, 60)),
];

pub const SESSION_MAX_AGE_DAYS: i64 = 30;
pub const VERIFICATION_CODE_EXPIRY_MINUTES: i64 = 10;
pub const VERIFICATION_MAX_ATTEMPTS: i64 = 5;
pub const EMAIL_VERIFIED_WINDOW_MINUTES: i64 = 30;

#[derive(Clone, Debug)]
pub struct Config {
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_user: String,
    pub smtp_password: String,
    pub smtp_from: String,
    pub db_path: String,
    pub backup_dir: String,
    pub word_reject_threshold: i64,
    pub admin_emails: HashSet<String>,
    pub admin_session_idle_minutes: i64,
    pub admin_sudo_minutes: i64,
    pub admin_ip_allowlist: Vec<IpNet>,
    pub port: u16,
    pub secret_key: String,
}

impl Config {
    pub fn smtp_configured(&self) -> bool {
        !self.smtp_user.is_empty() && !self.smtp_password.is_empty() && !self.smtp_from.is_empty()
    }

    /// A környezeti változókból (a `.env` betöltése után). Hibás `ADMIN_IP_ALLOWLIST`-nél hiba.
    pub fn from_env() -> Result<Config, String> {
        let admin_emails = env_string("ADMIN_EMAILS", "")
            .split(',')
            .map(|e| e.trim().to_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        let secret_key = std::env::var("SECRET_KEY").ok().filter(|k| !k.is_empty()).unwrap_or_else(|| {
            let mut bytes = [0u8; 32];
            use rand::RngExt;
            rand::rng().fill(&mut bytes[..]);
            hex::encode(bytes)
        });
        Ok(Config {
            smtp_host: env_string("SMTP_HOST", "smtp.gmail.com"),
            smtp_port: env_int("SMTP_PORT", 587).clamp(1, 65535) as u16,
            smtp_user: env_string("SMTP_USER", ""),
            smtp_password: env_string("SMTP_PASSWORD", ""),
            smtp_from: env_string("SMTP_FROM", ""),
            db_path: env_string("SCRABBLE_DB_PATH", "scrabble.db"),
            backup_dir: env_string("SCRABBLE_BACKUP_DIR", "backups"),
            word_reject_threshold: env_int("WORD_REJECT_THRESHOLD", 1).max(1),
            admin_emails,
            admin_session_idle_minutes: env_int("ADMIN_SESSION_IDLE_MINUTES", 30).max(1),
            admin_sudo_minutes: env_int("ADMIN_SUDO_MINUTES", 10).max(1),
            admin_ip_allowlist: parse_ip_networks(&env_string("ADMIN_IP_ALLOWLIST", ""))?,
            port: env_int("PORT", 5000).clamp(1, 65535) as u16,
            secret_key,
        })
    }
}

static CONFIG: OnceLock<Config> = OnceLock::new();

/// A folyamat konfigurációja (az első híváskor épül fel; hibás beállításnál a program leáll).
pub fn get() -> &'static Config {
    CONFIG.get_or_init(|| {
        load_env_file(&base_dir().join(".env"));
        match Config::from_env() {
            Ok(config) => config,
            Err(message) => {
                eprintln!("{message}");
                std::process::exit(1);
            }
        }
    })
}

/// A konfiguráció előre beállítása (tesztekhez; csak az első hívás hat).
pub fn set_for_tests(config: Config) {
    let _ = CONFIG.set(config);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ip_nets_contain_addresses() {
        let net = IpNet::parse("192.168.1.0/24").unwrap();
        assert!(net.contains(&"192.168.1.77".parse().unwrap()));
        assert!(!net.contains(&"192.168.2.1".parse().unwrap()));
        let single = IpNet::parse("10.0.0.5").unwrap();
        assert!(single.contains(&"10.0.0.5".parse().unwrap()));
        assert!(!single.contains(&"10.0.0.6".parse().unwrap()));
        let v6 = IpNet::parse("::1").unwrap();
        assert!(v6.contains(&"::1".parse().unwrap()));
    }

    #[test]
    fn bad_allowlist_is_an_error() {
        assert!(parse_ip_networks("10.0.0.0/8, nonsense").is_err());
        assert_eq!(parse_ip_networks("").unwrap().len(), 0);
        assert_eq!(parse_ip_networks("10.0.0.0/8, 127.0.0.1").unwrap().len(), 2);
    }

    #[test]
    fn env_file_values_are_parsed() {
        let dir = std::env::temp_dir().join(format!("scrabble-env-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".env");
        std::fs::write(&path, "# megjegyzés\nSCRABBLE_TEST_A=12\nexport SCRABBLE_TEST_B=\"hello\"\nbad line\n1X=3\n").unwrap();
        let applied = load_env_file(&path);
        assert!(applied.contains(&"SCRABBLE_TEST_A".to_string()));
        assert_eq!(std::env::var("SCRABBLE_TEST_B").unwrap(), "hello");
        assert!(!applied.contains(&"1X".to_string()));
        assert!(load_env_file(&dir.join("missing")).is_empty());
    }
}
