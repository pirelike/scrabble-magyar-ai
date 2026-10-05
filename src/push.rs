//! Web Push értesítések: VAPID kulcsok, feliratkozások és a „Te jössz!” üzenet.
//!
//! A kulcspár a `VAPID_PRIVATE_KEY` környezeti változóból jön (PEM, vagy a `web-push` / `vapid` eszközök base64url
//! formája; a nyilvános kulcs ebből számolódik), ennek híján az első induláskor készül és az adatbázisban
//! (`app_settings`) marad, hogy a feliratkozások újraindítás után is érvényesek legyenek. A Python változattal
//! létrehozott (PEM) kulcs és a meglévő feliratkozások változatlanul használhatók.
//!
//! A titkosítás RFC 8291 (aes128gcm), a feladó azonosítása RFC 8292 (VAPID, ES256 JWT).

use crate::db::Db;
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes128Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
use p256::{PublicKey, SecretKey};
use serde_json::{Value, json};
use sha2::Sha256;
use std::sync::{Arc, OnceLock};

/// ennyi ideig őrzi a push szolgáltatás a kézbesítetlen üzenetet
pub const TTL_SECONDS: u32 = 6 * 3600;
const RECORD_SIZE: u32 = 4096;

pub const DEFAULT_LANG: &str = "hu";

/// (nyelv, kulcs) → szöveg
pub fn message_template(lang: &str, kind: &str) -> &'static str {
    match (lang, kind) {
        ("en", "title") => "Hungarian Scrabble",
        ("en", "turn") => "It's your turn! · {room}",
        ("en", "invite") => "You've been invited to a correspondence game: {room}",
        ("en", "report") => "A new report has arrived · {room}",
        ("en", "reminder") => "Reminder: your move is waiting · {room}",
        ("en", "test") => "Test notification: notifications are working.",
        (_, "title") => "Magyar Scrabble",
        (_, "turn") => "Te jössz! · {room}",
        (_, "invite") => "Meghívtak egy levelezős játékba: {room}",
        (_, "report") => "Új bejelentés érkezett · {room}",
        (_, "reminder") => "Emlékeztető: rád vár a lépés · {room}",
        (_, "test") => "Teszt értesítés: az értesítések működnek.",
        _ => "",
    }
}

pub fn is_supported_lang(lang: &str) -> bool {
    lang == "hu" || lang == "en"
}

/// A push üzenet tartalma a felhasználó nyelvén: {'title', 'body', 'tag', 'url'}.
pub fn build_message(kind: &str, lang: Option<&str>, room: &str, room_id: &str) -> Value {
    let lang = lang.filter(|l| is_supported_lang(l)).unwrap_or(DEFAULT_LANG);
    json!({
        "title": message_template(lang, "title"),
        "body": message_template(lang, kind).replace("{room}", room),
        "tag": format!("{kind}-{room_id}"),
        "url": "/",
    })
}

struct Keys {
    signing: SigningKey,
    public_b64: String,
}

/// Egy küldés eredménye.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Outcome {
    Ok,
    /// a feliratkozás megszűnt
    Gone,
    Error,
}

pub struct Push {
    db: Arc<Db>,
    keys: OnceLock<Option<Keys>>,
    subject_override: Option<String>,
    from_address: Box<dyn Fn() -> String + Send + Sync>,
}

fn b64url(raw: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(raw)
}

fn public_b64(secret: &SecretKey) -> String {
    use p256::elliptic_curve::sec1::ToSec1Point;
    b64url(secret.public_key().to_sec1_point(false).as_bytes())
}

/// A `VAPID_PRIVATE_KEY` értéke: PEM, base64url nyers (32 bájtos) kulcs vagy base64url DER.
pub fn parse_private_key(configured: &str) -> Option<SecretKey> {
    let text = configured.replace("\\n", "\n");
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    if text.starts_with("-----") {
        return SecretKey::from_pkcs8_pem(text).ok().or_else(|| SecretKey::from_sec1_pem(text).ok());
    }
    let raw = URL_SAFE_NO_PAD.decode(text.replace('\n', "").trim_end_matches('=')).ok()?;
    if raw.len() == 32 {
        return SecretKey::from_slice(&raw).ok();
    }
    SecretKey::from_pkcs8_der(&raw).ok().or_else(|| SecretKey::from_sec1_der(&raw).ok())
}

fn random_secret() -> SecretKey {
    use rand::RngExt;
    loop {
        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes[..]);
        if let Ok(key) = SecretKey::from_slice(&bytes) {
            return key;
        }
    }
}

impl Push {
    pub fn new(db: Arc<Db>, from_address: Box<dyn Fn() -> String + Send + Sync>) -> Push {
        Push {
            db,
            keys: OnceLock::new(),
            subject_override: std::env::var("VAPID_SUBJECT").ok().filter(|s| !s.is_empty()),
            from_address,
        }
    }

    pub fn is_available(&self) -> bool {
        true
    }

    /// A VAPID `sub` mezője (mailto: vagy https: cím).
    pub fn subject(&self) -> String {
        if let Some(configured) = &self.subject_override {
            return configured.clone();
        }
        let sender = (self.from_address)();
        if sender.is_empty() { "mailto:admin@localhost.localdomain".to_string() } else { format!("mailto:{sender}") }
    }

    /// (aláíró kulcs, nyilvános kulcs): környezetből, az adatbázisból, vagy újonnan generálva. A generált kulcs az
    /// adatbázisba kerül (a környezetből jövő sosem írja felül).
    fn load_keys(&self) -> Keys {
        let from_env = std::env::var("VAPID_PRIVATE_KEY").ok().and_then(|v| {
            let parsed = parse_private_key(&v);
            if parsed.is_none() && !v.trim().is_empty() {
                println!("[push] A VAPID_PRIVATE_KEY nem olvasható, a tárolt kulcsot használom");
            }
            parsed
        });
        let secret = from_env.unwrap_or_else(|| {
            let stored = self.db.get_setting("vapid_private_pem").unwrap_or_default();
            if stored.starts_with("-----") {
                if let Some(key) = parse_private_key(&stored) {
                    return key;
                }
            }
            let key = random_secret();
            if let Ok(pem) = key.to_pkcs8_pem(LineEnding::LF) {
                let _ = self.db.set_setting("vapid_private_pem", pem.as_str());
            }
            key
        });
        Keys { public_b64: public_b64(&secret), signing: SigningKey::from(&secret) }
    }

    fn keys(&self) -> &Keys {
        self.keys.get_or_init(|| Some(self.load_keys())).as_ref().expect("a kulcsok betöltődtek")
    }

    /// A böngészőnek szóló nyilvános kulcs (applicationServerKey, base64url).
    pub fn public_key(&self) -> String {
        self.keys().public_b64.clone()
    }

    /// VAPID fejléc (`vapid t=<jwt>, k=<kulcs>`) az adott végponthoz.
    pub fn authorization(&self, endpoint: &str, exp: u64) -> Option<String> {
        let audience = origin_of(endpoint)?;
        let keys = self.keys();
        let header = b64url(br#"{"typ":"JWT","alg":"ES256"}"#);
        let claims = b64url(json!({"aud": audience, "exp": exp, "sub": self.subject()}).to_string().as_bytes());
        let signing_input = format!("{header}.{claims}");
        let signature: Signature = keys.signing.sign(signing_input.as_bytes());
        Some(format!("vapid t={signing_input}.{}, k={}", b64url(&signature.to_bytes()), keys.public_b64))
    }

    /// Egy feliratkozás értesítése. Visszatér: Ok | Gone (a feliratkozás megszűnt) | Error.
    fn send_one(&self, endpoint: &str, p256dh: &str, auth: &str, payload: &Value) -> Outcome {
        let body = match encrypt(payload.to_string().as_bytes(), p256dh, auth, None) {
            Ok(body) => body,
            Err(e) => {
                println!("[push] A titkosítás nem sikerült: {e}");
                return Outcome::Error;
            }
        };
        let exp = crate::util::now() as u64 + 12 * 3600;
        let Some(authorization) = self.authorization(endpoint, exp) else { return Outcome::Error };
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .into();
        let response = agent
            .post(endpoint)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("TTL", &TTL_SECONDS.to_string())
            .header("Authorization", &authorization)
            .send(&body[..]);
        match response {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    Outcome::Ok
                } else if status == 404 || status == 410 {
                    Outcome::Gone
                } else {
                    println!("[push] Sikertelen küldés ({status})");
                    Outcome::Error
                }
            }
            Err(e) => {
                println!("[push] Hiba a küldésnél: {e}");
                Outcome::Error
            }
        }
    }

    /// A felhasználó minden feliratkozott eszközének értesítése; a megszűnt feliratkozások törlődnek.
    /// Visszatér: a sikeresen küldött értesítések száma.
    pub fn notify_user(&self, user_id: i64, kind: &str, room: &str, room_id: &str) -> usize {
        let mut sent = 0;
        for sub in self.db.get_push_subscriptions(user_id) {
            let get = |k: &str| sub.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let payload = build_message(kind, Some(&get("lang")), room, room_id);
            match self.send_one(&get("endpoint"), &get("p256dh"), &get("auth"), &payload) {
                Outcome::Ok => sent += 1,
                Outcome::Gone => {
                    self.db.delete_push_subscription(&get("endpoint"), None);
                }
                Outcome::Error => {}
            }
        }
        sent
    }

    /// „Te jössz!” (vagy meghívó) értesítés háttérben (nem blokkolja a játékmenetet). Igaz, ha volt feliratkozás.
    pub fn notify_turn(self: &Arc<Self>, user_id: i64, room_name: &str, room_id: &str, kind: &str) -> bool {
        if self.db.count_push_subscriptions(user_id) == 0 {
            return false;
        }
        let push = self.clone();
        let (room_name, room_id, kind) = (room_name.to_string(), room_id.to_string(), kind.to_string());
        std::thread::spawn(move || {
            push.notify_user(user_id, &kind, &room_name, &room_id);
        });
        true
    }

    /// Értesítés a felhasználó eszközeinek, háttérben (nem blokkol). Visszatér: igaz, ha volt feliratkozás.
    pub fn notify_background(self: &Arc<Self>, user_id: i64, kind: &str, room: &str) -> bool {
        self.notify_turn(user_id, room, "", kind)
    }

    /// Egyedi (admin által írt) értesítés a felhasználó minden eszközére. Visszatér: {'sent', 'removed', 'failed'}
    /// (a megszűnt feliratkozások törlődnek).
    pub fn send_custom(&self, user_id: i64, title: &str, body: &str, url: &str) -> Value {
        let (mut sent, mut removed, mut failed) = (0, 0, 0);
        for sub in self.db.get_push_subscriptions(user_id) {
            let get = |k: &str| sub.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
            let payload = json!({"title": title, "body": body, "tag": "broadcast", "url": url});
            match self.send_one(&get("endpoint"), &get("p256dh"), &get("auth"), &payload) {
                Outcome::Ok => sent += 1,
                Outcome::Gone => {
                    self.db.delete_push_subscription(&get("endpoint"), None);
                    removed += 1;
                }
                Outcome::Error => failed += 1,
            }
        }
        json!({"sent": sent, "removed": removed, "failed": failed})
    }
}

/// A végpont origin-je (séma + host[:port]) a VAPID `aud` mezőjéhez.
pub fn origin_of(endpoint: &str) -> Option<String> {
    let (scheme, rest) = endpoint.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    if host.is_empty() { None } else { Some(format!("{scheme}://{host}")) }
}

/// RFC 8291 (aes128gcm): a tartalom titkosítása a feliratkozó kulcsára. `fixed` (tesztekhez): (só, kiszolgálói
/// privát kulcs). Visszatér: a küldendő törzs (fejléc + titkosított rekord).
pub fn encrypt(plaintext: &[u8], p256dh_b64: &str, auth_b64: &str, fixed: Option<([u8; 16], SecretKey)>) -> Result<Vec<u8>, String> {
    let ua_public_bytes = URL_SAFE_NO_PAD.decode(p256dh_b64.trim_end_matches('=')).map_err(|_| "érvénytelen p256dh".to_string())?;
    let auth_secret = URL_SAFE_NO_PAD.decode(auth_b64.trim_end_matches('=')).map_err(|_| "érvénytelen auth".to_string())?;
    let ua_public = PublicKey::from_sec1_bytes(&ua_public_bytes).map_err(|_| "érvénytelen feliratkozói kulcs".to_string())?;

    let (salt, as_secret) = match fixed {
        Some(f) => f,
        None => {
            use rand::RngExt;
            let mut salt = [0u8; 16];
            rand::rng().fill(&mut salt[..]);
            (salt, random_secret())
        }
    };
    use p256::elliptic_curve::sec1::ToSec1Point;
    let as_public = as_secret.public_key().to_sec1_point(false);
    let shared = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua_public.as_affine());

    // IKM = HKDF(auth_secret, ecdh_secret, "WebPush: info\0" || ua_public || as_public)
    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(&ua_public_bytes);
    key_info.extend_from_slice(as_public.as_bytes());
    let hk_ikm = Hkdf::<Sha256>::new(Some(&auth_secret), shared.raw_secret_bytes());
    let mut ikm = [0u8; 32];
    hk_ikm.expand(&key_info, &mut ikm).map_err(|_| "hkdf".to_string())?;

    let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
    let mut cek = [0u8; 16];
    hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).map_err(|_| "hkdf".to_string())?;
    let mut nonce = [0u8; 12];
    hk.expand(b"Content-Encoding: nonce\0", &mut nonce).map_err(|_| "hkdf".to_string())?;

    let mut padded = plaintext.to_vec();
    padded.push(0x02); // az utolsó rekord elválasztója
    if padded.len() > RECORD_SIZE as usize - 16 {
        return Err("túl hosszú üzenet".to_string());
    }
    let cipher = Aes128Gcm::new_from_slice(&cek).map_err(|_| "kulcs".to_string())?;
    let ciphertext = cipher.encrypt(&Nonce::try_from(&nonce[..]).map_err(|_| "nonce".to_string())?, padded.as_slice()).map_err(|_| "titkosítás".to_string())?;

    let mut body = Vec::with_capacity(21 + as_public.len() + ciphertext.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    body.push(as_public.len() as u8);
    body.extend_from_slice(as_public.as_bytes());
    body.extend_from_slice(&ciphertext);
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_extraction() {
        assert_eq!(origin_of("https://fcm.googleapis.com/fcm/send/abc").as_deref(), Some("https://fcm.googleapis.com"));
        assert_eq!(origin_of("https://push.example:8443/x?y=1").as_deref(), Some("https://push.example:8443"));
        assert_eq!(origin_of("nem url"), None);
        assert_eq!(origin_of("https:///x"), None);
    }

    #[test]
    fn messages_are_localized() {
        let hu = build_message("turn", Some("hu"), "Szoba", "r1");
        assert_eq!(hu["body"], "Te jössz! · Szoba");
        assert_eq!(hu["tag"], "turn-r1");
        let en = build_message("turn", Some("en"), "Room", "r1");
        assert_eq!(en["body"], "It's your turn! · Room");
        assert_eq!(en["title"], "Hungarian Scrabble");
        assert_eq!(build_message("invite", Some("zz"), "X", "")["title"], "Magyar Scrabble");
        assert_eq!(build_message("test", None, "", "")["body"], "Teszt értesítés: az értesítések működnek.");
    }

    #[test]
    fn private_key_formats_are_accepted() {
        let secret = random_secret();
        let pem = secret.to_pkcs8_pem(LineEnding::LF).unwrap();
        assert_eq!(parse_private_key(pem.as_str()).unwrap(), secret);
        let escaped = pem.as_str().replace('\n', "\\n");
        assert_eq!(parse_private_key(&escaped).unwrap(), secret);
        let raw = b64url(&secret.to_bytes());
        assert_eq!(parse_private_key(&raw).unwrap(), secret);
        assert!(parse_private_key("").is_none());
        assert!(parse_private_key("nem kulcs").is_none());
    }

    #[test]
    fn rfc8291_example_matches() {
        // RFC 8291, 5. fejezet / A. függelék példa
        let plaintext = b"When I grow up, I want to be a watermelon";
        let as_private = URL_SAFE_NO_PAD.decode("yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw").unwrap();
        let salt: [u8; 16] = URL_SAFE_NO_PAD.decode("DGv6ra1nlYgDCS1FRnbzlw").unwrap().try_into().unwrap();
        let body = encrypt(
            plaintext,
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4",
            "BTBZMqHH6r4Tts7J_aSIgg",
            Some((salt, SecretKey::from_slice(&as_private).unwrap())),
        )
        .unwrap();
        assert_eq!(
            b64url(&body),
            "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN"
        );
    }

    #[test]
    fn vapid_header_has_valid_structure() {
        let db = Arc::new(Db::open_temp().unwrap());
        let push = Push::new(db.clone(), Box::new(|| "admin@example.com".to_string()));
        let header = push.authorization("https://push.example/abc", 2_000_000_000).unwrap();
        assert!(header.starts_with("vapid t="));
        let (token, key) = header.trim_start_matches("vapid t=").split_once(", k=").unwrap();
        assert_eq!(key, push.public_key());
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let claims: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
        assert_eq!(claims["aud"], "https://push.example");
        assert_eq!(claims["sub"], "mailto:admin@example.com");
        assert_eq!(claims["exp"], 2_000_000_000u64);
        // az aláírás ellenőrizhető a nyilvános kulccsal
        use p256::ecdsa::signature::Verifier;
        use p256::ecdsa::VerifyingKey;
        let public = URL_SAFE_NO_PAD.decode(key).unwrap();
        let verifying = VerifyingKey::from_sec1_bytes(&public).unwrap();
        let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        verifying.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature).unwrap();
        // a kulcs az adatbázisban marad: új példány ugyanazt a nyilvános kulcsot adja
        let again = Push::new(db, Box::new(String::new));
        assert_eq!(again.public_key(), push.public_key());
    }
}
