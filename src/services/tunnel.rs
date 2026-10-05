//! Cloudflare tunnel (a `cloudflared` folyamat kezelése): indítás, leállítás, a publikus cím kiolvasása.

use parking_lot::Mutex;
use regex::Regex;
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;

#[derive(Default)]
struct Inner {
    process: Option<Child>,
    public_url: Option<String>,
    port: Option<u16>,
}

#[derive(Default)]
pub struct Tunnel {
    inner: Arc<Mutex<Inner>>,
}

/// A `cloudflared` programot a PATH-ban keresi.
fn find_cloudflared() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join("cloudflared")).find(|candidate| candidate.is_file())
}

impl Tunnel {
    pub fn new() -> Tunnel {
        Tunnel::default()
    }

    /// A tunnel állapota az admin panelnek: {'state': 'running' | 'starting' | 'stopped' | 'unavailable', 'url'}.
    pub fn status(&self) -> Value {
        let mut inner = self.inner.lock();
        if find_cloudflared().is_none() && inner.process.is_none() {
            return json!({"state": "unavailable", "url": null});
        }
        let alive = match inner.process.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        };
        if !alive {
            return json!({"state": "stopped", "url": null});
        }
        match &inner.public_url {
            Some(url) => json!({"state": "running", "url": url}),
            None => json!({"state": "starting", "url": null}),
        }
    }

    /// A tunnel újraindítása a korábban használt porton (az új publikus cím más lesz).
    pub fn restart(&self) -> bool {
        let Some(port) = self.inner.lock().port else { return false };
        self.stop();
        self.start(port);
        self.inner.lock().process.is_some()
    }

    /// Cloudflare tunnel indítása háttérben.
    pub fn start(&self, port: u16) {
        {
            let mut inner = self.inner.lock();
            inner.port = Some(port);
            inner.public_url = None;
        }
        let Some(cloudflared) = find_cloudflared() else {
            println!("\n  [!] cloudflared nincs telepítve - tunnel nem elérhető");
            println!("      Telepítés: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/\n");
            return;
        };
        println!("\n  [*] Cloudflare tunnel indítása...");
        let spawned =
            Command::new(cloudflared).args(["tunnel", "--url", &format!("http://localhost:{port}")]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let mut child = match spawned {
            Ok(child) => child,
            Err(e) => {
                println!("  [!] A tunnel nem indítható: {e}");
                return;
            }
        };
        // a cloudflared a naplóját a hibakimenetre írja: mindkettőt figyeljük
        let pipes: Vec<Box<dyn std::io::Read + Send>> = vec![
            child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>).unwrap(),
            child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>).unwrap(),
        ];
        self.inner.lock().process = Some(child);
        let url_re = Regex::new(r"(https://[a-z0-9-]+\.trycloudflare\.com)").expect("érvényes regex");
        for pipe in pipes {
            let inner = self.inner.clone();
            let url_re = url_re.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    if let Some(found) = url_re.captures(&line) {
                        let url = found[1].to_string();
                        inner.lock().public_url = Some(url.clone());
                        println!("\n{}", "=".repeat(50));
                        println!("  PUBLIKUS URL: {url}");
                        println!("  Oszd meg ezt a linket a barátaiddal!");
                        println!("{}\n", "=".repeat(50));
                    }
                }
            });
        }
    }

    /// Cloudflare tunnel leállítása.
    pub fn stop(&self) {
        let mut inner = self.inner.lock();
        if let Some(mut child) = inner.process.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        inner.public_url = None;
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.stop();
    }
}
