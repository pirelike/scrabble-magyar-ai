//! Jelszó-hash a werkzeug formátumában (`pbkdf2:sha256:260000$só$hex`): a Python változattal létrehozott
//! fiókok jelszavai is érvényesek maradnak.

use hmac::Hmac;
use rand::RngExt;
use sha2::{Sha256, Sha512};
use subtle::ConstantTimeEq;

pub const DEFAULT_ITERATIONS: u32 = 260_000;
const SALT_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

fn gen_salt(length: usize) -> String {
    let mut rng = rand::rng();
    (0..length).map(|_| SALT_CHARS[rng.random_range(0..SALT_CHARS.len())] as char).collect()
}

fn derive(hash_name: &str, password: &[u8], salt: &[u8], iterations: u32) -> Option<String> {
    match hash_name {
        "sha256" => {
            let mut out = [0u8; 32];
            pbkdf2::pbkdf2::<Hmac<Sha256>>(password, salt, iterations, &mut out).ok()?;
            Some(hex::encode(out))
        }
        "sha512" => {
            let mut out = [0u8; 64];
            pbkdf2::pbkdf2::<Hmac<Sha512>>(password, salt, iterations, &mut out).ok()?;
            Some(hex::encode(out))
        }
        _ => None,
    }
}

/// Új jelszó-hash `pbkdf2:sha256:260000` móddal.
pub fn generate_password_hash(password: &str) -> String {
    let salt = gen_salt(16);
    let hash = derive("sha256", password.as_bytes(), salt.as_bytes(), DEFAULT_ITERATIONS).expect("sha256 mindig elérhető");
    format!("pbkdf2:sha256:{DEFAULT_ITERATIONS}${salt}${hash}")
}

/// A jelszó egyezik-e a tárolt hash-sel. Ismeretlen vagy hibás alakú hash-nél hamis.
pub fn check_password_hash(stored: &str, password: &str) -> bool {
    let mut parts = stored.splitn(3, '$');
    let (Some(method), Some(salt), Some(expected)) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    let mut method_parts = method.split(':');
    if method_parts.next() != Some("pbkdf2") {
        return false;
    }
    let hash_name = method_parts.next().unwrap_or("sha256");
    let iterations = match method_parts.next() {
        Some(text) => match text.parse::<u32>() {
            Ok(n) if n > 0 => n,
            _ => return false,
        },
        None => 600_000,
    };
    match derive(hash_name, password.as_bytes(), salt.as_bytes(), iterations) {
        Some(actual) => actual.as_bytes().ct_eq(expected.as_bytes()).into(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let hash = generate_password_hash("titok123");
        assert!(hash.starts_with("pbkdf2:sha256:260000$"));
        assert!(check_password_hash(&hash, "titok123"));
        assert!(!check_password_hash(&hash, "titok124"));
        assert!(!check_password_hash(&hash, ""));
    }

    #[test]
    fn salts_differ() {
        assert_ne!(generate_password_hash("x"), generate_password_hash("x"));
    }

    #[test]
    fn verifies_hashes_made_by_werkzeug() {
        // a Python werkzeug `generate_password_hash` valódi kimenete
        assert!(check_password_hash("pbkdf2:sha256:1000$3l7nq5hS$ee316dfe3830406c728991501fdfdde0f5189df2f31faacfe5931fcef4eb0ab7", "titok123"));
        let hash = "pbkdf2:sha256:260000$PxxF4QrUob13H5pC$74fe0a317a0dbf08a0d4552646d061224455d9e4fe1bde5a947cade772c6b3f2";
        assert!(check_password_hash(hash, "Jelszó-ékezet ő"));
        assert!(!check_password_hash(hash, "Jelszó-ékezet o"));
    }

    #[test]
    fn rejects_garbage() {
        assert!(!check_password_hash("nonsense", "x"));
        assert!(!check_password_hash("plain$salt$hash", "x"));
        assert!(!check_password_hash("pbkdf2:md5:10$salt$hash", "x"));
        assert!(!check_password_hash("pbkdf2:sha256:abc$salt$hash", "x"));
        assert!(!check_password_hash("", ""));
    }
}
