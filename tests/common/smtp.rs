//! Hamis SMTP kiszolgáló az e-mail tesztekhez: egy helyi TCP kiszolgáló (titkosítás nélkül), amely rögzíti a
//! beérkező leveleket. A levelezést az admin panel mentett beállítása köti hozzá (`security: none`).

#![allow(dead_code)]

use parking_lot::Mutex;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Egy beérkezett levél.
#[derive(Clone, Debug)]
pub struct Mail {
    pub from: String,
    pub to: Vec<String>,
    /// a teljes üzenet (fejlécek + törzs), ahogy érkezett
    pub raw: String,
}

impl Mail {
    pub fn header(&self, name: &str) -> Option<String> {
        let head = self.raw.split("\r\n\r\n").next().unwrap_or("");
        let mut value: Option<String> = None;
        for line in head.split("\r\n") {
            if let Some(v) = value.as_mut() {
                if line.starts_with(' ') || line.starts_with('\t') {
                    v.push(' ');
                    v.push_str(line.trim());
                    continue;
                }
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                if k.eq_ignore_ascii_case(name) {
                    value = Some(v.trim().to_string());
                }
            }
        }
        value
    }

    /// A törzs visszafejtve (7bit / quoted-printable / base64).
    pub fn body(&self) -> String {
        let body = self.raw.split_once("\r\n\r\n").map(|(_, b)| b).unwrap_or("");
        match self.header("Content-Transfer-Encoding").map(|e| e.to_lowercase()).as_deref() {
            Some("quoted-printable") => decode_qp(body),
            Some("base64") => {
                use base64::Engine;
                let joined: String = body.split("\r\n").collect();
                base64::engine::general_purpose::STANDARD.decode(joined.trim()).map(|b| String::from_utf8_lossy(&b).to_string()).unwrap_or_default()
            }
            _ => body.to_string(),
        }
    }

    /// A tárgy (a kódolt fejléc is visszafejtve, ha `=?utf-8?...?=` alakú).
    pub fn subject(&self) -> String {
        decode_header(&self.header("Subject").unwrap_or_default())
    }
}

fn decode_qp(text: &str) -> String {
    let mut out: Vec<u8> = Vec::new();
    let bytes: Vec<u8> = text.as_bytes().to_vec();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'=' {
            if bytes.get(i + 1) == Some(&b'\r') && bytes.get(i + 2) == Some(&b'\n') {
                i += 3;
                continue;
            }
            if let (Some(a), Some(b)) = (bytes.get(i + 1), bytes.get(i + 2)) {
                if let Ok(v) = u8::from_str_radix(&format!("{}{}", *a as char, *b as char), 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).replace("\r\n", "\n")
}

fn decode_header(value: &str) -> String {
    // =?utf-8?b?....?= vagy =?utf-8?q?....?= (több szó szóközzel elválasztva)
    let mut out = String::new();
    for part in value.split(' ') {
        if let Some(inner) = part.strip_prefix("=?").and_then(|p| p.strip_suffix("?=")) {
            let pieces: Vec<&str> = inner.splitn(3, '?').collect();
            if pieces.len() == 3 {
                match pieces[1].to_lowercase().as_str() {
                    "b" => {
                        use base64::Engine;
                        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(pieces[2]) {
                            out.push_str(&String::from_utf8_lossy(&bytes));
                            continue;
                        }
                    }
                    "q" => {
                        out.push_str(&decode_qp(&pieces[2].replace('_', " ")));
                        continue;
                    }
                    _ => {}
                }
            }
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

pub struct FakeSmtp {
    pub addr: SocketAddr,
    mails: Arc<Mutex<Vec<Mail>>>,
    stop: Arc<AtomicBool>,
    /// ha be van állítva, a RCPT TO erre a kóddal válaszol (pl. "550 no such user")
    reject_recipient: Arc<Mutex<Option<String>>>,
}

impl FakeSmtp {
    pub fn start() -> FakeSmtp {
        let listener = TcpListener::bind("127.0.0.1:0").expect("SMTP kiszolgáló");
        listener.set_nonblocking(true).unwrap();
        let addr = listener.local_addr().unwrap();
        let mails: Arc<Mutex<Vec<Mail>>> = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let reject: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
        let (m, s, r) = (mails.clone(), stop.clone(), reject.clone());
        std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                match listener.accept() {
                    Ok((stream, _)) => {
                        let (m, r) = (m.clone(), r.clone());
                        std::thread::spawn(move || serve(stream, m, r));
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(10)),
                }
            }
        });
        FakeSmtp { addr, mails, stop, reject_recipient: reject }
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn mails(&self) -> Vec<Mail> {
        self.mails.lock().clone()
    }

    pub fn clear(&self) {
        self.mails.lock().clear();
    }

    pub fn reject_recipients_with(&self, reply: Option<&str>) {
        *self.reject_recipient.lock() = reply.map(|r| r.to_string());
    }

    /// Várakozás, amíg legalább `n` levél beér.
    pub async fn wait_for(&self, n: usize, secs: u64) -> Vec<Mail> {
        let end = Instant::now() + Duration::from_secs(secs);
        loop {
            let got = self.mails();
            if got.len() >= n || Instant::now() >= end {
                return got;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
}

impl Drop for FakeSmtp {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

fn serve(stream: std::net::TcpStream, mails: Arc<Mutex<Vec<Mail>>>, reject: Arc<Mutex<Option<String>>>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(_) => return,
    };
    let mut reader = BufReader::new(stream);
    let say = |w: &mut std::net::TcpStream, line: &str| {
        let _ = w.write_all(format!("{line}\r\n").as_bytes());
        let _ = w.flush();
    };
    say(&mut writer, "220 hamis-smtp ESMTP kesz");
    let (mut from, mut to): (String, Vec<String>) = (String::new(), Vec::new());
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let command = line.trim_end().to_string();
        let upper = command.to_uppercase();
        if upper.starts_with("EHLO") {
            say(&mut writer, "250-hamis-smtp");
            say(&mut writer, "250 8BITMIME");
        } else if upper.starts_with("HELO") {
            say(&mut writer, "250 hamis-smtp");
        } else if upper.starts_with("MAIL FROM") {
            from = between_angle(&command);
            to.clear();
            say(&mut writer, "250 OK");
        } else if upper.starts_with("RCPT TO") {
            if let Some(reply) = reject.lock().clone() {
                say(&mut writer, &reply);
            } else {
                to.push(between_angle(&command));
                say(&mut writer, "250 OK");
            }
        } else if upper == "DATA" {
            say(&mut writer, "354 jöhet");
            let mut data = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => return,
                    Ok(_) => {}
                }
                if line == ".\r\n" {
                    break;
                }
                // a pont-kitöltés (dot-stuffing) visszafejtése
                data.push_str(line.strip_prefix('.').filter(|_| line.starts_with("..")).unwrap_or(&line));
            }
            let raw = data.strip_suffix("\r\n").unwrap_or(&data).to_string();
            mails.lock().push(Mail { from: from.clone(), to: to.clone(), raw });
            say(&mut writer, "250 OK elfogadva");
        } else if upper == "RSET" {
            say(&mut writer, "250 OK");
        } else if upper == "NOOP" {
            say(&mut writer, "250 OK");
        } else if upper == "QUIT" {
            say(&mut writer, "221 viszlát");
            return;
        } else {
            say(&mut writer, "502 nem támogatott");
        }
    }
}

fn between_angle(command: &str) -> String {
    match (command.find('<'), command.rfind('>')) {
        (Some(a), Some(b)) if b > a => command[a + 1..b].to_string(),
        _ => command.split(':').nth(1).unwrap_or("").trim().to_string(),
    }
}
