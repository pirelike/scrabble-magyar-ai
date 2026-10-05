//! Levelező szerver (SMTP): a beállítás tárolása (admin panelen mentett felülbírálat vagy környezeti változók) és az
//! e-mail küldés (verifikációs kód, értesítések, admin üzenetek, kapcsolat-próba).
//!
//! A ténylegesen használt beállítás kétféle forrásból jöhet:
//! * az admin panelen mentett felülbírálat (`app_settings`, `mail.smtp` kulcs, JSON) — ez az erősebb, újraindítás
//!   nélkül hat;
//! * ennek híján a környezeti változókból olvasott `SMTP_*` értékek (a korábbi viselkedés: STARTTLS).
//!
//! A jelszó a felülbírálatban az adatbázisban van, de sosem kerül ki a szerverről: a `public_view()` csak azt mondja
//! meg, hogy be van-e állítva.

use crate::config::Config;
use crate::db::Db;
use crate::util;
use lettre::message::header::ContentType;
use lettre::message::{Mailbox, MultiPart, SinglePart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::transport::smtp::client::{Tls, TlsParameters};
use lettre::{Message, SmtpTransport, Transport};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

pub const KEY: &str = "mail.smtp";
pub const SECURITY_MODES: [&str; 3] = ["starttls", "ssl", "none"];
pub const STORED_FIELDS: [&str; 8] = ["host", "port", "security", "username", "password", "from_address", "from_name", "verify_tls"];
/// mp: küldés
pub const SEND_TIMEOUT: u64 = 15;
/// mp: az admin panel kapcsolat-próbája
pub const CHECK_TIMEOUT: u64 = 10;

pub fn default_port(security: &str) -> u16 {
    match security {
        "ssl" => 465,
        "none" => 25,
        _ => 587,
    }
}

/// A most érvényes levelező beállítás (jelszóval — csak szerveroldali használatra).
#[derive(Clone, Debug, PartialEq)]
pub struct MailConfig {
    pub host: String,
    pub port: u16,
    pub security: String,
    pub username: String,
    pub password: String,
    pub from_address: String,
    pub from_name: String,
    pub verify_tls: bool,
    /// "database" | "environment"
    pub source: &'static str,
    pub configured: bool,
}

impl MailConfig {
    /// A beállítás jelszó nélkül, a felületnek.
    pub fn public(&self) -> Value {
        json!({
            "host": self.host, "port": self.port, "security": self.security, "username": self.username,
            "password_set": !self.password.is_empty(), "from_address": self.from_address, "from_name": self.from_name,
            "verify_tls": self.verify_tls,
        })
    }
}

/// Elég-e a beállítás a küldéshez: kiszolgáló, feladó, és ha van felhasználónév, jelszó is.
fn complete(cfg: &MailConfig) -> bool {
    !cfg.host.is_empty() && cfg.port != 0 && !cfg.from_address.is_empty() && (cfg.username.is_empty() || !cfg.password.is_empty())
}

pub struct Mailer {
    db: Arc<Db>,
    config: Arc<Config>,
}

impl Mailer {
    pub fn new(db: Arc<Db>, config: Arc<Config>) -> Mailer {
        Mailer { db, config }
    }

    /// A környezeti változókból jövő beállítás (a jelszóval együtt).
    pub fn environment(&self) -> MailConfig {
        let c = &self.config;
        MailConfig {
            host: c.smtp_host.clone(),
            port: c.smtp_port,
            security: "starttls".to_string(),
            username: c.smtp_user.clone(),
            password: c.smtp_password.clone(),
            from_address: c.smtp_from.clone(),
            from_name: String::new(),
            verify_tls: true,
            source: "environment",
            configured: c.smtp_configured(),
        }
    }

    /// A mentett felülbírálat (JSON objektum) vagy None (nincs, nem olvasható, vagy hiányos).
    pub fn stored(&self) -> Option<Value> {
        let raw = self.db.get_setting(KEY).filter(|r| !r.is_empty())?;
        let data: Value = serde_json::from_str(&raw).ok()?;
        let object = data.as_object()?;
        if STORED_FIELDS.iter().any(|f| !object.contains_key(*f)) {
            return None;
        }
        let security = data["security"].as_str()?;
        if !SECURITY_MODES.contains(&security) || !data["port"].is_i64() {
            return None;
        }
        Some(data)
    }

    /// A most érvényes beállítás (jelszóval!), a forrással és a `configured` jelzővel.
    pub fn current(&self) -> MailConfig {
        let Some(saved) = self.stored() else { return self.environment() };
        let text = |k: &str| saved[k].as_str().unwrap_or("").to_string();
        let mut cfg = MailConfig {
            host: text("host"),
            port: saved["port"].as_i64().unwrap_or(0).clamp(0, 65535) as u16,
            security: text("security"),
            username: text("username"),
            password: text("password"),
            from_address: text("from_address"),
            from_name: text("from_name"),
            verify_tls: saved["verify_tls"].as_bool().unwrap_or(true),
            source: "database",
            configured: false,
        };
        cfg.configured = complete(&cfg);
        cfg
    }

    pub fn is_configured(&self) -> bool {
        self.current().configured
    }

    /// A feladó címe (pl. a push VAPID `mailto:` címéhez), vagy üres szöveg.
    pub fn from_address(&self) -> String {
        self.current().from_address
    }

    /// A felülbírálat beírása a hívó tranzakciójában. `cfg`: a `STORED_FIELDS` mezőit tartalmazó (ellenőrzött) objektum.
    pub fn save(&self, tx: &rusqlite::Transaction, cfg: &Value, admin_id: i64) -> rusqlite::Result<()> {
        let mut payload = serde_json::Map::new();
        for field in STORED_FIELDS {
            payload.insert(field.to_string(), cfg[field].clone());
        }
        payload.insert("updated_by".into(), json!(admin_id));
        payload.insert("updated_at".into(), json!(util::now_ts()));
        tx.execute(
            "INSERT INTO app_settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![KEY, Value::Object(payload).to_string()],
        )?;
        Ok(())
    }

    /// A felülbírálat törlése: a környezeti beállítás lép érvénybe. Visszatér: volt-e mit törölni.
    pub fn clear(&self, tx: &rusqlite::Transaction) -> rusqlite::Result<bool> {
        Ok(tx.execute("DELETE FROM app_settings WHERE key = ?", [KEY])? > 0)
    }

    /// A beállítás a felületnek: jelszó nélkül. `environment`: a környezeti alapérték (a „visszaállítás” célja).
    pub fn public_view(&self) -> Value {
        let cfg = self.current();
        let saved = self.stored();
        let mut view = cfg.public();
        view["source"] = json!(cfg.source);
        view["configured"] = json!(cfg.configured);
        view["updated_by"] = saved.as_ref().map(|s| s["updated_by"].clone()).unwrap_or(Value::Null);
        view["updated_at"] = saved.as_ref().map(|s| s["updated_at"].clone()).unwrap_or(Value::Null);
        let env = self.environment();
        let mut env_view = env.public();
        env_view["configured"] = json!(self.config.smtp_configured());
        view["environment"] = env_view;
        view["defaults"] = json!({"starttls": 587, "ssl": 465, "none": 25});
        view
    }

    // ===== Küldés =====

    /// Verifikációs kód küldése emailben. Ha SMTP nincs konfigurálva, konzolra ír.
    pub fn send_verification_email(self: &Arc<Self>, to_email: &str, code: &str) {
        if !self.is_configured() {
            println!("\n  [VERIFIKÁCIÓ] Email: {to_email} | Kód: {code}\n");
            return;
        }
        // Háttérszálon küldés, hogy ne blokkolja a requestet
        let mailer = self.clone();
        let (to, code) = (to_email.to_string(), code.to_string());
        std::thread::spawn(move || {
            let cfg = mailer.current();
            let text = format!("A verifikációs kódod: {code}\n\nEz a kód 10 percig érvényes.\nHa nem te kérted, hagyd figyelmen kívül ezt az emailt.");
            let html = format!(
                "<div style=\"font-family: Arial, sans-serif; max-width: 400px; margin: 0 auto; padding: 20px; background: #1a1a2e; color: #eee; border-radius: 12px;\">\
                 <h2 style=\"color: #e8b930; text-align: center;\">Magyar Scrabble</h2>\
                 <p style=\"text-align: center;\">A verifikációs kódod:</p>\
                 <div style=\"text-align: center; font-size: 32px; font-weight: bold; letter-spacing: 8px; color: #e8b930; padding: 20px; background: #16213e; border-radius: 8px; margin: 16px 0;\">{code}</div>\
                 <p style=\"text-align: center; color: #aaa; font-size: 14px;\">Ez a kód 10 percig érvényes.</p></div>"
            );
            let result =
                build_message(&cfg, &to, "Magyar Scrabble - Verifikációs kód", Body::Alternative(text, html)).and_then(|msg| send(&cfg, &msg, SEND_TIMEOUT));
            match result {
                Ok(()) => println!("  [EMAIL] Verifikációs kód elküldve: {to}"),
                Err(e) => {
                    println!("  [EMAIL HIBA] {e}");
                    println!("  [VERIFIKÁCIÓ] Email: {to} | Kód: {code}");
                }
            }
        });
    }

    /// Egyszerű szöveges e-mail (értesítések, admin üzenetek). Visszatér: (elküldve-e, hibakód: `classify_error`).
    ///
    /// SMTP nélkül a konzolra ír és (false, "smtp_not_configured")-et ad. `wait`: a küldés megvárása (a hibát
    /// visszaadja); különben háttérszálon megy, és a visszatérés csak azt jelzi, hogy elindult.
    pub fn send_plain_email(self: &Arc<Self>, to_email: &str, subject: &str, body: &str, wait: bool) -> (bool, Option<String>) {
        if !self.is_configured() {
            println!("\n  [EMAIL] Címzett: {to_email} | Tárgy: {subject}\n  {body}\n");
            return (false, Some("smtp_not_configured".to_string()));
        }
        let cfg = self.current();
        let deliver = move |cfg: MailConfig, to: String, subject: String, body: String| -> Result<(), MailError> {
            let msg = build_message(&cfg, &to, &subject, Body::Plain(body))?;
            send(&cfg, &msg, SEND_TIMEOUT)
        };
        if wait {
            return match deliver(cfg, to_email.to_string(), subject.to_string(), body.to_string()) {
                Ok(()) => (true, None),
                Err(e) => {
                    println!("  [EMAIL HIBA] {e}");
                    (false, Some(e.code().to_string()))
                }
            };
        }
        let (to, subject, body) = (to_email.to_string(), subject.to_string(), body.to_string());
        std::thread::spawn(move || {
            if let Err(e) = deliver(cfg, to, subject, body) {
                println!("  [EMAIL HIBA] {e}");
            }
        });
        (true, None)
    }
}

enum Body {
    Plain(String),
    Alternative(String, String),
}

/// Egy SMTP / hálózati hiba rövid, biztonságos kódja. A kiszolgáló szövege szándékosan nem kerül a felületre (egy
/// tetszőleges címre mutató próbával különben belső szolgáltatások üdvözlőszövege szivároghatna ki).
#[derive(Debug)]
pub struct MailError {
    code: &'static str,
    detail: String,
}

impl MailError {
    pub fn code(&self) -> &'static str {
        self.code
    }

    fn new(code: &'static str, detail: impl Into<String>) -> MailError {
        MailError { code, detail: detail.into() }
    }
}

impl std::fmt::Display for MailError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

/// A hiba kódja a lettre hibájából (a szöveges leírás és a hibalánc alapján).
pub fn classify_error(err: &lettre::transport::smtp::Error) -> &'static str {
    use std::error::Error as _;
    let mut text = err.to_string().to_lowercase();
    let mut source = err.source();
    let mut io_kind = None;
    while let Some(inner) = source {
        text.push(' ');
        text.push_str(&inner.to_string().to_lowercase());
        if let Some(io) = inner.downcast_ref::<std::io::Error>() {
            io_kind = Some(io.kind());
        }
        source = inner.source();
    }
    let tls_protocol = ["corrupt message", "invalidcontenttype", "handshake", "invalidmessage"].iter().any(|w| text.contains(w));
    if err.is_tls() || tls_protocol || text.contains("tls") || text.contains("certificate") {
        if text.contains("certificate") || text.contains("unknownissuer") || text.contains("notvalid") {
            return "certificate";
        }
        return "tls";
    }
    if err.is_timeout() || io_kind == Some(std::io::ErrorKind::TimedOut) || text.contains("timed out") {
        return "timeout";
    }
    if text.contains("failed to lookup")
        || text.contains("name or service not known")
        || text.contains("nodename nor servname")
        || text.contains("no address associated")
    {
        return "dns";
    }
    if io_kind == Some(std::io::ErrorKind::ConnectionRefused) || text.contains("connection refused") {
        return "refused";
    }
    if text.contains("authentication") || text.contains("535") || text.contains("534") || text.contains("credentials") {
        return "auth";
    }
    if text.contains("not supported") || text.contains("unsupported") {
        return "unsupported";
    }
    // a lettre nem árulja el, melyik SMTP parancs bukott el: a szöveg és a kiterjesztett állapotkód (RFC 3463:
    // 5.1.1–5.1.3, 5.1.6 a címzett, 5.1.7–5.1.8 a feladó címe) alapján döntünk
    if ["5.1.7", "5.1.8"].iter().any(|c| text.contains(c)) || text.contains("sender") || text.contains("mail from") {
        return "sender";
    }
    if ["5.1.1", "5.1.2", "5.1.3", "5.1.6"].iter().any(|c| text.contains(c))
        || ["recipient", "rcpt", "user unknown", "no such user", "mailbox unavailable", "relay"].iter().any(|w| text.contains(w))
    {
        return "recipient";
    }
    if io_kind == Some(std::io::ErrorKind::UnexpectedEof)
        || text.contains("connection closed")
        || text.contains("disconnected")
        || text.contains("incomplete response")
    {
        return "disconnected";
    }
    if err.is_response() || err.is_permanent() || err.is_transient() || err.is_client() {
        return "protocol";
    }
    if io_kind.is_some() || text.contains("connection") || text.contains("io error") {
        return "connect";
    }
    "error"
}

fn mailbox(address: &str, name: &str) -> Result<Mailbox, MailError> {
    let addr = address.parse().map_err(|_| MailError::new("sender", format!("érvénytelen cím: {address}")))?;
    Ok(Mailbox::new(if name.is_empty() { None } else { Some(name.replace(['\r', '\n'], " ")) }, addr))
}

fn build_message(cfg: &MailConfig, to: &str, subject: &str, body: Body) -> Result<Message, MailError> {
    let to_box = to.parse::<Mailbox>().map_err(|_| MailError::new("recipient", format!("érvénytelen címzett: {to}")))?;
    let domain = cfg.from_address.rsplit_once('@').map(|(_, d)| d.trim()).filter(|d| !d.is_empty()).unwrap_or("localhost");
    let message_id = format!("<{}@{}>", crate::util::token_urlsafe(18), domain);
    let builder = Message::builder()
        .from(mailbox(&cfg.from_address, &cfg.from_name)?)
        .to(to_box)
        .subject(subject.replace(['\r', '\n'], " "))
        .message_id(Some(message_id));
    let message = match body {
        Body::Plain(text) => builder.header(ContentType::TEXT_PLAIN).body(text),
        Body::Alternative(text, html) => builder.multipart(
            MultiPart::alternative()
                .singlepart(SinglePart::builder().header(ContentType::TEXT_PLAIN).body(text))
                .singlepart(SinglePart::builder().header(ContentType::TEXT_HTML).body(html)),
        ),
    };
    message.map_err(|e| MailError::new("error", e.to_string()))
}

fn transport(cfg: &MailConfig, timeout: u64) -> Result<SmtpTransport, MailError> {
    let tls = |_: ()| -> Result<TlsParameters, MailError> {
        TlsParameters::builder(cfg.host.clone())
            .dangerous_accept_invalid_certs(!cfg.verify_tls) // saját / önaláírt tanúsítványú levelezőhöz (admin döntés)
            .dangerous_accept_invalid_hostnames(!cfg.verify_tls)
            .build_rustls()
            .map_err(|e| MailError::new("tls", e.to_string()))
    };
    let mut builder = SmtpTransport::builder_dangerous(cfg.host.clone()).port(cfg.port).timeout(Some(Duration::from_secs(timeout)));
    builder = match cfg.security.as_str() {
        "ssl" => builder.tls(Tls::Wrapper(tls(())?)),
        "none" => builder.tls(Tls::None),
        _ => builder.tls(Tls::Required(tls(())?)),
    };
    if !cfg.username.is_empty() {
        builder = builder.credentials(Credentials::new(cfg.username.clone(), cfg.password.clone()));
    }
    Ok(builder.build())
}

fn send(cfg: &MailConfig, message: &Message, timeout: u64) -> Result<(), MailError> {
    let transport = transport(cfg, timeout)?;
    transport.send(message).map(|_| ()).map_err(|e| MailError::new(classify_error(&e), e.to_string()))
}

/// Kapcsolat-próba a megadott beállítással: csatlakozás, titkosítás, bejelentkezés; `recipient` megadásakor teszt
/// levél is megy. Visszatér: (sikerült-e, kód) — a kód `ok` vagy a `classify_error` értéke.
pub fn check_connection(cfg: &MailConfig, recipient: Option<&str>) -> (bool, &'static str) {
    let result = (|| -> Result<(), MailError> {
        match recipient {
            Some(to) => {
                let message = build_message(
                    cfg,
                    to,
                    "Magyar Scrabble — teszt e-mail",
                    Body::Plain("Ez egy teszt e-mail az admin panelről. A levelező szerver beállítása működik.".to_string()),
                )?;
                send(cfg, &message, CHECK_TIMEOUT)
            }
            None => {
                let transport = transport(cfg, CHECK_TIMEOUT)?;
                match transport.test_connection() {
                    Ok(true) => Ok(()),
                    Ok(false) => Err(MailError::new("disconnected", "a kiszolgáló nem válaszol")),
                    Err(e) => Err(MailError::new(classify_error(&e), e.to_string())),
                }
            }
        }
    })();
    match result {
        Ok(()) => (true, "ok"),
        Err(e) => {
            let detail: String = e.detail.chars().take(200).collect();
            println!("  [EMAIL PRÓBA] {}: {detail}", e.code);
            (false, e.code)
        }
    }
}
