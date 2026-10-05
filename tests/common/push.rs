//! Hamis „push szolgáltatás” a Web Push tesztekhez: egy helyi HTTP kiszolgáló, amely rögzíti a beérkező kéréseket,
//! és az RFC 8291 szerint (aes128gcm) vissza is fejti a tartalmat a feliratkozás saját kulcsával.

#![allow(dead_code)]

use aes_gcm::aead::Aead;
use aes_gcm::{Aes128Gcm, KeyInit, Nonce};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hkdf::Hkdf;
use p256::SecretKey;
use p256::elliptic_curve::sec1::ToSec1Point;
use parking_lot::Mutex;
use rand::RngExt;
use serde_json::Value;
use sha2::Sha256;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::time::{Duration, Instant};

/// Egy beérkezett küldés.
#[derive(Clone, Debug)]
pub struct PushRequest {
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// a feliratkozás kulcsával visszafejtett JSON tartalom (hibás törzsnél `Null`)
    pub payload: Value,
}

impl PushRequest {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// A feliratkozás adatai (mint a böngésző `PushSubscription.toJSON()`-ja).
#[derive(Clone, Debug)]
pub struct Subscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}

pub struct PushSink {
    pub addr: SocketAddr,
    received: Arc<Mutex<Vec<PushRequest>>>,
    status: Arc<AtomicU16>,
    stop: Arc<AtomicBool>,
    secret: SecretKey,
    auth_secret: [u8; 16],
}

fn random_secret() -> SecretKey {
    loop {
        let mut bytes = [0u8; 32];
        rand::rng().fill(&mut bytes[..]);
        if let Ok(key) = SecretKey::from_slice(&bytes) {
            return key;
        }
    }
}

impl PushSink {
    /// Elindítja a kiszolgálót egy szabad porton; minden kérésre `201`-gyel válaszol (a `set_status` módosítja).
    pub fn start() -> PushSink {
        let listener = TcpListener::bind("127.0.0.1:0").expect("push kiszolgáló");
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let received: Arc<Mutex<Vec<PushRequest>>> = Arc::new(Mutex::new(Vec::new()));
        let status = Arc::new(AtomicU16::new(201));
        let stop = Arc::new(AtomicBool::new(false));
        let secret = random_secret();
        let mut auth_secret = [0u8; 16];
        rand::rng().fill(&mut auth_secret[..]);
        let (recv, st, sp, sec, auth) = (received.clone(), status.clone(), stop.clone(), secret.clone(), auth_secret);
        std::thread::spawn(move || {
            while !sp.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
                        if let Some(request) = read_request(&mut stream, &sec, &auth) {
                            recv.lock().push(request);
                        }
                        let code = st.load(Ordering::SeqCst);
                        let _ = write!(stream, "HTTP/1.1 {code} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        let _ = stream.flush();
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        PushSink { addr, received, status, stop, secret, auth_secret }
    }

    /// A válasz állapotkódja a következő kérésektől (pl. 410: a feliratkozás megszűnt, 500: átmeneti hiba).
    pub fn set_status(&self, code: u16) {
        self.status.store(code, Ordering::SeqCst);
    }

    /// Feliratkozás a kiszolgáló egy útvonalára, a kiszolgáló kulcsaival.
    pub fn subscription(&self, path: &str) -> Subscription {
        let public = self.secret.public_key().to_sec1_point(false);
        Subscription {
            endpoint: format!("http://{}/{path}", self.addr),
            p256dh: URL_SAFE_NO_PAD.encode(public.as_bytes()),
            auth: URL_SAFE_NO_PAD.encode(self.auth_secret),
        }
    }

    pub fn received(&self) -> Vec<PushRequest> {
        self.received.lock().clone()
    }

    pub fn payloads(&self) -> Vec<Value> {
        self.received().into_iter().map(|r| r.payload).collect()
    }

    pub fn bodies(&self) -> Vec<String> {
        self.payloads().iter().filter_map(|p| p["body"].as_str().map(|s| s.to_string())).collect()
    }

    pub fn clear(&self) {
        self.received.lock().clear();
    }

    /// Várakozás, amíg legalább `n` kérés beér (legfeljebb `secs` másodpercig). Visszatér: a beérkezettek.
    pub async fn wait_for(&self, n: usize, secs: u64) -> Vec<PushRequest> {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let got = self.received();
            if got.len() >= n || Instant::now() >= end {
                return got;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }

    /// Rövid várakozás után igaz, ha nem érkezett kérés (negatív ellenőrzéshez).
    pub async fn stays_silent(&self) -> bool {
        tokio::time::sleep(Duration::from_millis(400)).await;
        self.received.lock().is_empty()
    }
}

impl Drop for PushSink {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn read_request(stream: &mut std::net::TcpStream, secret: &SecretKey, auth: &[u8; 16]) -> Option<PushRequest> {
    let mut data: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    let header_end = loop {
        if let Some(pos) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos;
        }
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            return None;
        }
        data.extend_from_slice(&buf[..n]);
    };
    let head = String::from_utf8_lossy(&data[..header_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next()?;
    let path = request_line.split_whitespace().nth(1)?.to_string();
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect();
    let length: usize = headers.iter().find(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, v)| v.parse().ok()).unwrap_or(0);
    let mut body = data[header_end + 4..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&buf[..n]);
    }
    body.truncate(length);
    let payload = decrypt(&body, secret, auth).and_then(|plain| serde_json::from_slice(&plain).ok()).unwrap_or(Value::Null);
    Some(PushRequest { path, headers, body, payload })
}

/// RFC 8291 / RFC 8188 (aes128gcm) visszafejtés a feliratkozó titkos kulcsával és auth titkával.
pub fn decrypt(body: &[u8], ua_secret: &SecretKey, auth_secret: &[u8]) -> Option<Vec<u8>> {
    if body.len() < 21 {
        return None;
    }
    let salt = &body[..16];
    let id_len = body[20] as usize;
    if body.len() < 21 + id_len {
        return None;
    }
    let as_public_bytes = &body[21..21 + id_len];
    let ciphertext = &body[21 + id_len..];
    let as_public = p256::PublicKey::from_sec1_bytes(as_public_bytes).ok()?;
    let ua_public = ua_secret.public_key().to_sec1_point(false);
    let shared = p256::ecdh::diffie_hellman(ua_secret.to_nonzero_scalar(), as_public.as_affine());

    let mut key_info = b"WebPush: info\0".to_vec();
    key_info.extend_from_slice(ua_public.as_bytes());
    key_info.extend_from_slice(as_public_bytes);
    let mut ikm = [0u8; 32];
    Hkdf::<Sha256>::new(Some(auth_secret), shared.raw_secret_bytes()).expand(&key_info, &mut ikm).ok()?;
    let hk = Hkdf::<Sha256>::new(Some(salt), &ikm);
    let mut cek = [0u8; 16];
    hk.expand(b"Content-Encoding: aes128gcm\0", &mut cek).ok()?;
    let mut nonce = [0u8; 12];
    hk.expand(b"Content-Encoding: nonce\0", &mut nonce).ok()?;
    let cipher = Aes128Gcm::new_from_slice(&cek).ok()?;
    let mut plain = cipher.decrypt(&Nonce::try_from(&nonce[..]).ok()?, ciphertext).ok()?;
    // az utolsó rekord elválasztója (0x02) és az utána álló kitöltés
    while plain.last() == Some(&0) {
        plain.pop();
    }
    if plain.pop() != Some(0x02) {
        return None;
    }
    Some(plain)
}

/// A VAPID `Authorization` fejléc ellenőrzése: `vapid t=<jwt>, k=<nyilvános kulcs>`. Visszatér: a JWT
/// követelései, ha az aláírás érvényes a `k` kulccsal.
pub fn verify_vapid(header: &str) -> Option<Value> {
    use p256::ecdsa::signature::Verifier;
    use p256::ecdsa::{Signature, VerifyingKey};
    let rest = header.strip_prefix("vapid ")?;
    let (t_part, k_part) = rest.split_once(", ")?;
    let jwt = t_part.strip_prefix("t=")?;
    let key = k_part.strip_prefix("k=")?;
    let (signing_input, signature) = jwt.rsplit_once('.')?;
    let public = URL_SAFE_NO_PAD.decode(key).ok()?;
    let verifying = VerifyingKey::from_sec1_bytes(&public).ok()?;
    let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(signature).ok()?).ok()?;
    verifying.verify(signing_input.as_bytes(), &signature).ok()?;
    let claims = signing_input.split('.').nth(1)?;
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).ok()?).ok()
}
