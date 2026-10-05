//! Közös segédek a kliens (JavaScript, HTML, fordítások) tesztjeihez: fájlolvasás, node futtatás, HTML bejárás,
//! a Rust forrás üzeneteinek kigyűjtése.

#![allow(dead_code)]

use regex::Regex;
use serde_json::Value;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn read(parts: &[&str]) -> String {
    let mut path = root();
    for part in parts {
        path.push(part);
    }
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| panic!("{pattern}: {e}"))
}

pub fn node_available() -> bool {
    Command::new("node").arg("--version").output().map(|o| o.status.success()).unwrap_or(false)
}

/// A node futtatása a gépen: ha nincs, a teszt kimarad (a Python `skipif` megfelelője).
#[macro_export]
macro_rules! require_node {
    () => {
        if !$crate::frontend_support::node_available() {
            eprintln!("node nem elérhető: a teszt kimarad");
            return;
        }
    };
}

/// `node --check` egy fájlra.
pub fn node_check_file(path: &std::path::Path) -> Result<(), String> {
    let out = Command::new("node").arg("--check").arg(path).output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).to_string()) }
}

/// `node --check` egy forrásszövegre (stdin).
pub fn node_check_source(source: &str) -> Result<(), String> {
    let mut child = Command::new("node").args(["--check", "-"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    child.stdin.take().unwrap().write_all(source.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).to_string()) }
}

/// `node -e HARNESS ARG...` futtatása; a bemenet JSON az stdin-en, a kimenet utolsó sora JSON.
pub fn run_node(harness: &str, args: &[&str], input: &Value) -> Value {
    let mut child = Command::new("node")
        .arg("-e")
        .arg(harness)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("node");
    let mut stdin = child.stdin.take().unwrap();
    let payload = input.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(payload.as_bytes());
    });
    let out = child.wait_with_output().expect("node kimenet");
    let _ = writer.join();
    assert!(out.status.success(), "node: {}", String::from_utf8_lossy(&out.stderr));
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    serde_json::from_str(text.trim().lines().last().unwrap_or("null")).unwrap_or_else(|e| panic!("node kimenet: {e}: {text}"))
}

/// A `start` jelölőtől az első `end` szövegig (azzal együtt) terjedő kódrészlet.
pub fn block(source: &str, start: &str, end: &str) -> String {
    let i = source.find(start).unwrap_or_else(|| panic!("nincs ilyen kezdet: {start}"));
    let j = source[i..].find(end).unwrap_or_else(|| panic!("nincs ilyen vég: {end:?}")) + i + end.len();
    source[i..j].to_string()
}

/// A `window.NAME = {...};` alakú adatfájl JSON-ja.
pub fn data_of(source: &str, global: &str) -> Value {
    let pattern = re(&format!(r"(?ms)^window\.{global} = (.*);\s*$"));
    let body = pattern.captures(source).unwrap_or_else(|| panic!("nincs window.{global}"))[1].to_string();
    serde_json::from_str(&body).unwrap_or_else(|e| panic!("{global}: {e}"))
}

pub fn ui_keys(translations: &Value) -> std::collections::BTreeSet<String> {
    translations.as_object().unwrap().keys().filter(|k| *k != "server").cloned().collect()
}

pub fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or_else(|| panic!("nincs szöveg: {key}"))
}

pub fn placeholders(text: &str) -> std::collections::BTreeSet<String> {
    re(r"\{(\w+)\}").captures_iter(text).map(|c| c[1].to_string()).collect()
}

// ---------------------------------------------------------------------------------------------------
// HTML bejárás
// ---------------------------------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum Node {
    /// kezdő címke: (név, attribútumok, önzáró-e)
    Start(String, Vec<(String, String)>, bool),
    End(String),
    Text(String),
}

fn unescape(text: &str) -> String {
    // az entitások (&amp;, &nbsp;, &#8211;…) nem betűk: egy nem betű jellel helyettesítjük
    re(r"&#?\w+;").replace_all(text, "·").to_string()
}

/// A HTML elemei sorrendben (megjegyzések nélkül; a `script` és `style` tartalma egyetlen szöveg).
pub fn html_nodes(html: &str) -> Vec<Node> {
    let tag = re(r#"(?s)<!--.*?-->|<!DOCTYPE[^>]*>|<(/?)([a-zA-Z][\w:-]*)((?:[^>"']|"[^"]*"|'[^']*')*?)(/?)>|([^<]+|<)"#);
    let attr = re(r#"([\w:@.-]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+)))?"#);
    let mut nodes = Vec::new();
    let mut pos = 0;
    while pos < html.len() {
        let Some(m) = tag.captures_at(html, pos) else { break };
        let whole = m.get(0).unwrap();
        pos = whole.end();
        if whole.as_str().starts_with("<!--") || whole.as_str().to_uppercase().starts_with("<!DOCTYPE") {
            continue;
        }
        if let Some(name) = m.get(2) {
            let name = name.as_str().to_lowercase();
            if m.get(1).map(|c| c.as_str()) == Some("/") {
                nodes.push(Node::End(name));
                continue;
            }
            let attrs: Vec<(String, String)> = attr
                .captures_iter(m.get(3).map(|a| a.as_str()).unwrap_or(""))
                .map(|a| (a[1].to_string(), a.get(2).or(a.get(3)).or(a.get(4)).map(|v| v.as_str().to_string()).unwrap_or_default()))
                .collect();
            let self_closing = m.get(4).map(|s| s.as_str() == "/").unwrap_or(false);
            nodes.push(Node::Start(name.clone(), attrs, self_closing));
            if (name == "script" || name == "style") && !self_closing {
                let close = format!("</{name}>");
                let end = html[pos..].find(&close).map(|i| pos + i).unwrap_or(html.len());
                if end > pos {
                    nodes.push(Node::Text(html[pos..end].to_string()));
                }
                pos = end;
            }
        } else if let Some(t) = m.get(5) {
            nodes.push(Node::Text(unescape(t.as_str())));
        }
    }
    nodes
}

pub fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

pub const VOID_TAGS: [&str; 14] = ["meta", "link", "input", "br", "hr", "img", "source", "use", "path", "circle", "line", "polyline", "polygon", "rect"];

// ---------------------------------------------------------------------------------------------------
// Rust forrás
// ---------------------------------------------------------------------------------------------------

/// A `src/` Rust forrásfájljai (útvonal, szöveg): a megjegyzések és a `#[cfg(test)]` utáni rész nélkül.
pub fn rust_sources() -> Vec<(String, String)> {
    fn walk(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    let mut files = Vec::new();
    walk(&root().join("src"), &mut files);
    files.sort();
    let comment = re(r"(?m)//[^\n]*");
    files
        .into_iter()
        .map(|path| {
            let source = strip_test_modules(&std::fs::read_to_string(&path).unwrap());
            let relative = path.strip_prefix(root()).unwrap().to_string_lossy().replace('\\', "/");
            (relative, comment.replace_all(&source, "").to_string())
        })
        .collect()
}

/// A forrás szöveges literáljai (a kettőskeresztes escape-ek visszafejtve).
pub fn string_literals(source: &str) -> Vec<String> {
    re(r#""((?:[^"\\\n]|\\.)*)""#).captures_iter(source).map(|c| c[1].replace("\\\"", "\"").replace("\\n", "\n").replace("\\\\", "\\")).collect()
}

/// A `#[cfg(test)]` jelölésű blokkok (egységteszt modulok) elhagyása: a kapcsos zárójelek párosítása a szöveges és
/// karakter literálokat átlépi.
pub fn strip_test_modules(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let marker: Vec<char> = "#[cfg(test)]".chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i..].starts_with(&marker) {
            // a következő `{`-ig (vagy `;`-ig: egysoros elem), majd a páros záró zárójelig
            let mut j = i + marker.len();
            while j < chars.len() && chars[j] != '{' && chars[j] != ';' {
                j += 1;
            }
            if j < chars.len() && chars[j] == '{' {
                let mut depth = 0i32;
                while j < chars.len() {
                    match chars[j] {
                        '"' => {
                            j += 1;
                            while j < chars.len() && chars[j] != '"' {
                                if chars[j] == '\\' {
                                    j += 1;
                                }
                                j += 1;
                            }
                        }
                        '\'' => {
                            // karakter literál ('x', '\n', '{') — az életidő jelölő (`'a`) nem zár
                            if chars.get(j + 2) == Some(&'\'') {
                                j += 2;
                            } else if chars.get(j + 1) == Some(&'\\') && chars.get(j + 3) == Some(&'\'') {
                                j += 3;
                            }
                        }
                        '{' => depth += 1,
                        '}' => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    j += 1;
                }
            }
            i = j + 1;
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}
