//! Rövid életű, aláírt token a Socket.IO identitás igazolásához.
//!
//! A socket a bejelentkezés előtt csatlakozik, ezért a handshake cookie-jai nem tartalmazzák a session sütit. A
//! kliens ezért a (cookie-val védett) HTTP végponton kér egy tokent, és azt küldi a `set_name` eseményben — így a
//! szerver nem a kliens által megadott `user_id`-ban bízik.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

const SALT: &str = "socket-auth";
/// másodperc
pub const TOKEN_MAX_AGE: u64 = 300;

fn sign(secret_key: &str, message: &str) -> Vec<u8> {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(format!("{SALT}:{secret_key}").as_bytes()).expect("a HMAC bármilyen kulcshosszt elfogad");
    mac.update(message.as_bytes());
    mac.finalize().into_bytes().to_vec()
}

fn token_at(secret_key: &str, user_id: i64, timestamp: u64) -> String {
    let payload = URL_SAFE_NO_PAD.encode(format!("{{\"uid\":{user_id}}}"));
    let message = format!("{payload}.{timestamp}");
    format!("{message}.{}", URL_SAFE_NO_PAD.encode(sign(secret_key, &message)))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Aláírt token a megadott felhasználóhoz.
pub fn create_socket_token(secret_key: &str, user_id: i64) -> String {
    token_at(secret_key, user_id, now_secs())
}

/// Visszaadja a token user_id-ját, vagy None-t, ha érvénytelen / lejárt.
pub fn verify_socket_token(secret_key: &str, token: Option<&str>, max_age: u64) -> Option<i64> {
    let token = token.filter(|t| !t.is_empty())?;
    let mut parts = token.splitn(3, '.');
    let (payload, timestamp, signature) = (parts.next()?, parts.next()?, parts.next()?);
    let message = format!("{payload}.{timestamp}");
    let given = URL_SAFE_NO_PAD.decode(signature).ok()?;
    if !bool::from(given.ct_eq(&sign(secret_key, &message))) {
        return None;
    }
    let issued: u64 = timestamp.parse().ok()?;
    if now_secs().saturating_sub(issued) > max_age || issued > now_secs() + 5 {
        return None;
    }
    let body: serde_json::Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    body.get("uid").and_then(|u| u.as_i64())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let token = create_socket_token("titok", 42);
        assert_eq!(verify_socket_token("titok", Some(&token), TOKEN_MAX_AGE), Some(42));
    }

    #[test]
    fn wrong_key_or_tampering_fails() {
        let token = create_socket_token("titok", 42);
        assert_eq!(verify_socket_token("másik", Some(&token), TOKEN_MAX_AGE), None);
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged_payload = URL_SAFE_NO_PAD.encode("{\"uid\":1}");
        parts[0] = &forged_payload;
        assert_eq!(verify_socket_token("titok", Some(&parts.join(".")), TOKEN_MAX_AGE), None);
    }

    #[test]
    fn expired_and_garbage_fail() {
        let old = token_at("titok", 7, now_secs() - 400);
        assert_eq!(verify_socket_token("titok", Some(&old), TOKEN_MAX_AGE), None);
        assert_eq!(verify_socket_token("titok", Some(&old), 1000), Some(7));
        assert_eq!(verify_socket_token("titok", Some("nem token"), TOKEN_MAX_AGE), None);
        assert_eq!(verify_socket_token("titok", Some(""), TOKEN_MAX_AGE), None);
        assert_eq!(verify_socket_token("titok", None, TOKEN_MAX_AGE), None);
    }
}
