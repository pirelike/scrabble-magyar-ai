//! Admin panel kliens: JS szintaxis, fordítások teljessége, a hívott API útvonalak és elem-azonosítók léte (a Python
//! `test_admin_client.py` megfelelője).
//!
//! A nyilvános kliens tesztjeinek mintájára, de az admin fájlokra: azok nem a nyilvános `static/` mappában vannak,
//! és a fordításaik sem a nyilvános `i18n-data.js`-ben. A kliens több fájlból áll (mag, komponensek, három nézetfájl,
//! indítás): a tesztek az összes fájlt együtt vizsgálják. Az útvonaltérkép a futó alkalmazásból jön.

#[macro_use]
mod frontend_support;
mod common;
use frontend_support::*;
use regex::Regex;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// A betöltés sorrendje a HTML-ben (a későbbi fájlok az előzők globális neveit használják)
const JS_FILES: [&str; 6] = ["admin.js", "admin-ui.js", "admin-views-a.js", "admin-views-b.js", "admin-views-c.js", "admin-main.js"];
const ALL_JS: [&str; 8] =
    ["admin.js", "admin-ui.js", "admin-views-a.js", "admin-views-b.js", "admin-views-c.js", "admin-main.js", "admin-boot.js", "admin-i18n.js"];
/// Az összes menüpont (a nézetfájlok `registerSection` hívásai): azonosító → sorrend
const SECTIONS: [(&str, i64); 15] = [
    ("overview", 10),
    ("users", 20),
    ("rooms", 30),
    ("games", 40),
    ("async", 50),
    ("ratings", 60),
    ("dictionary", 70),
    ("daily", 80),
    ("moderation", 90),
    ("comm", 100),
    ("stats", 110),
    ("security", 120),
    ("system", 130),
    ("settings", 140),
    ("audit", 150),
];
/// Implicit használat: az I18N.apply() a dokumentum címét az `app.title` kulcsból veszi
const IMPLICIT_KEYS: [&str; 1] = ["app.title"];

fn admin_js() -> String {
    JS_FILES.iter().map(|name| read(&["web", "admin", name])).collect::<Vec<_>>().join("\n")
}

fn admin_html() -> String {
    read(&["web", "templates", "admin.html"])
}

fn admin_data() -> Value {
    data_of(&read(&["web", "admin", "admin-i18n.js"]), "ADMIN_I18N")
}

fn public_data() -> Value {
    data_of(&read(&["web", "static", "i18n-data.js"]), "I18N_DATA")
}

fn ui_keys_without_implicit(translations: &Value) -> BTreeSet<String> {
    ui_keys(translations).into_iter().filter(|k| !IMPLICIT_KEYS.contains(&k.as_str())).collect()
}

// ===================================================================================================
// JavaScript szintaxis
// ===================================================================================================

#[test]
fn admin_files_parse() {
    require_node!();
    for name in ALL_JS {
        if let Err(e) = node_check_file(&root().join("web/admin").join(name)) {
            panic!("{name}: {e}");
        }
    }
}

#[test]
fn app_js_still_parses_with_the_entry_point() {
    require_node!();
    node_check_file(&root().join("web/static").join("app.js")).unwrap();
}

#[test]
fn no_stray_script_in_the_assets_folder() {
    let mut names: Vec<String> = std::fs::read_dir(root().join("web/admin"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.ends_with(".js"))
        .collect();
    names.sort();
    let mut expected: Vec<String> = ALL_JS.iter().map(|s| s.to_string()).collect();
    expected.sort();
    assert_eq!(names, expected);
}

// ===================================================================================================
// Fordítások
// ===================================================================================================

#[test]
fn both_languages_have_the_same_keys() {
    let data = admin_data();
    assert_eq!(ui_keys(&data["hu"]), ui_keys(&data["en"]));
}

#[test]
fn no_empty_translations() {
    let data = admin_data();
    for lang in ["hu", "en"] {
        let empty: Vec<String> = ui_keys(&data[lang]).into_iter().filter(|k| text(&data[lang], k).trim().is_empty()).collect();
        assert!(empty.is_empty(), "{lang}: {empty:?}");
    }
}

#[test]
fn placeholders_match_between_languages() {
    let data = admin_data();
    for key in ui_keys(&data["hu"]) {
        assert_eq!(placeholders(text(&data["hu"], &key)), placeholders(text(&data["en"], &key)), "{key}");
    }
}

#[test]
fn keys_are_plain_lowercase_names() {
    // a kulcsok csak kisbetűt és aláhúzást tartalmaznak: a kódban szó szerint kereshetők (nincs összefűzés)
    let (data, pattern) = (admin_data(), re(r"^admin\.[a-z_]+$"));
    for key in ui_keys_without_implicit(&data["hu"]) {
        assert!(pattern.is_match(&key), "{key}");
    }
}

#[test]
fn admin_text_is_not_in_the_public_translations() {
    let (admin, public) = (admin_data(), public_data());
    for lang in ["hu", "en"] {
        let leaked: Vec<&String> = public[lang].as_object().unwrap().keys().filter(|k| k.starts_with("admin.")).collect();
        assert!(leaked.is_empty());
        let overlap: Vec<String> = ui_keys_without_implicit(&admin[lang]).intersection(&ui_keys(&public[lang])).cloned().collect();
        assert!(overlap.is_empty(), "{overlap:?}");
    }
}

#[test]
fn every_key_used_in_js_and_html_exists() {
    let (admin, public, js, html) = (admin_data(), public_data(), admin_js(), admin_html());
    let used_js: BTreeSet<String> = re(r#"['"]((?:admin|common)\.[a-z_]+)['"]"#).captures_iter(&js).map(|c| c[1].to_string()).collect();
    let used_html: BTreeSet<String> = re(r#"data-i18n(?:-[a-z]+)?="([^"]+)""#).captures_iter(&html).map(|c| c[1].to_string()).collect();
    let known: BTreeSet<String> = ui_keys(&admin["hu"]).union(&ui_keys(&public["hu"])).cloned().collect();
    let missing: Vec<&String> = used_js.union(&used_html).filter(|k| !known.contains(*k)).collect();
    assert!(missing.is_empty(), "{missing:?}");
    for key in &used_html {
        // a közös (nyilvános) kulcsok mindkét nyelven megvannak
        assert!(admin["hu"].get(key.as_str()).is_some() || public["en"].get(key.as_str()).is_some(), "{key}");
    }
}

#[test]
fn every_translation_key_literal_in_js_is_well_formed() {
    // minden `'admin.…'` szövegkonstans érvényes kulcs (elgépelt vagy számjegyes kulcs nem maradhat észrevétlen)
    let (js, pattern) = (admin_js(), re(r"^admin\.[a-z_]+$"));
    for m in re(r#"['"](admin\.[^'"\s]*)['"]"#).captures_iter(&js) {
        assert!(pattern.is_match(&m[1]), "{}", &m[1]);
    }
}

#[test]
fn no_unused_admin_keys() {
    let (data, js, html) = (admin_data(), admin_js(), admin_html());
    let mut used: BTreeSet<String> = re(r#"['"](admin\.[a-z_]+)['"]"#).captures_iter(&js).map(|c| c[1].to_string()).collect();
    used.extend(re(r#"data-i18n(?:-[a-z]+)?="(admin\.[a-z_]+)""#).captures_iter(&html).map(|c| c[1].to_string()));
    let unused: Vec<String> = ui_keys_without_implicit(&data["hu"]).into_iter().filter(|k| !used.contains(k)).collect();
    assert!(unused.is_empty(), "{unused:?}");
}

#[test]
fn parameters_passed_in_js_match_the_placeholders() {
    // `t('kulcs', {a, b})`: a hívás pontosan a fordításban szereplő paramétereket adja át
    let (data, js) = (admin_data(), admin_js());
    let mut checked = 0;
    for m in re(r"\bt\(\s*'(admin\.[a-z_]+)'\s*,\s*\{([^{}]*)\}").captures_iter(&js) {
        let key = m[1].to_string();
        let Some(translation) = data["hu"].get(&key).and_then(|v| v.as_str()) else { continue };
        let expected = placeholders(translation);
        let mut names = BTreeSet::new();
        let (mut level, mut current, mut parts) = (0i32, String::new(), Vec::new());
        for ch in m[2].chars() {
            match ch {
                '(' => level += 1,
                ')' => level -= 1,
                _ => {}
            }
            if ch == ',' && level == 0 {
                parts.push(std::mem::take(&mut current));
            } else {
                current.push(ch);
            }
        }
        parts.push(current);
        for part in parts {
            if !part.trim().is_empty() {
                names.insert(part.split(':').next().unwrap().trim().to_string());
            }
        }
        assert!(expected.is_subset(&names), "{key}: hiányzó paraméter {:?}", expected.difference(&names).collect::<Vec<_>>());
        checked += 1;
    }
    assert!(checked > 60, "{checked}");
}

#[test]
fn the_document_title_is_overridden_for_the_admin_page() {
    let data = admin_data();
    assert!(text(&data["hu"], "app.title").starts_with("Admin"));
    assert!(text(&data["en"], "app.title").starts_with("Admin"));
}

#[test]
fn the_translation_file_is_loadable_by_the_browser() {
    let source = read(&["web", "admin", "admin-i18n.js"]);
    assert!(source.trim_end().ends_with("};"));
    assert_eq!(source.matches("window.ADMIN_I18N").count(), 1);
}

// ===================================================================================================
// A szerver üzenetei
// ===================================================================================================

/// Az admin végpontok magyar üzenetei: `AdminError::new("üzenet", ..)`, `AdminError::field(..)` és a beállítások
/// `err("üzenet")` hívásai (a nem konstans üzenetet nem vizsgáljuk).
fn server_messages() -> BTreeSet<String> {
    let call = re(r#"(?:AdminError::(?:new|field)|\berr|SettingError)\(\s*"((?:[^"\\]|\\.)*)""#);
    let mut messages = BTreeSet::new();
    for (path, source) in rust_sources() {
        if path.starts_with("src/admin/") || path == "src/settings.rs" {
            for m in call.captures_iter(&source) {
                messages.insert(m[1].replace("\\\"", "\"").replace("\\n", "\n"));
            }
        }
    }
    messages
}

#[test]
fn messages_were_found() {
    assert!(server_messages().len() >= 100);
}

#[test]
fn every_message_has_an_english_translation() {
    let data = admin_data();
    let exact = data["en"]["server"]["exact"].as_object().unwrap();
    let missing: Vec<String> = server_messages().into_iter().filter(|m| !exact.contains_key(m)).collect();
    assert!(missing.is_empty(), "{missing:#?}");
}

#[test]
fn no_stale_translations() {
    let data = admin_data();
    let messages = server_messages();
    let stale: Vec<&String> = data["en"]["server"]["exact"].as_object().unwrap().keys().filter(|m| !messages.contains(*m)).collect();
    assert!(stale.is_empty(), "{stale:#?}");
}

#[test]
fn translations_are_not_empty_and_differ_from_the_source() {
    let data = admin_data();
    for (hungarian, english) in data["en"]["server"]["exact"].as_object().unwrap() {
        let english = english.as_str().unwrap();
        assert!(!english.trim().is_empty() && english != hungarian, "{hungarian}");
    }
}

// ===================================================================================================
// API útvonalak és elemek
// ===================================================================================================

/// Egy JS útvonal-konstans (sablonbehelyettesítéssel és lekérdezéssel) → illesztésre alkalmas minta útvonal.
fn normalise(path: &str) -> String {
    re(r"\$\{[^}]*\}").replace_all(path, "1").split('?').next().unwrap().to_string()
}

/// Az admin útvonalak (metódus, illesztő regex, minta) a futó alkalmazás útvonaltérképéből.
async fn route_matchers() -> Vec<(String, Regex, String)> {
    let server = common::TestServer::start().await;
    let param = re(r"\{[^}]+\}");
    scrabble::admin::routes::route_table(&server.app)
        .into_iter()
        .map(|(method, path)| {
            let path = if path.starts_with("/api/admin") || path.starts_with("/admin") { path } else { format!("/api/admin{path}") };
            let mut pattern = String::from("^");
            let mut last = 0;
            for m in param.find_iter(&path) {
                pattern.push_str(&regex::escape(&path[last..m.start()]));
                pattern.push_str("[^/]+");
                last = m.end();
            }
            pattern.push_str(&regex::escape(&path[last..]));
            pattern.push('$');
            (method.to_string(), re(&pattern), path)
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_api_path_called_by_admin_js_exists() {
    let (js, rules) = (admin_js(), route_matchers().await);
    let paths: BTreeSet<String> = re(r#"[`'"](/api/admin[^`'"\s?]*)"#).captures_iter(&js).map(|c| normalise(&c[1])).collect();
    assert!(paths.len() > 100, "{}", paths.len());
    for path in paths {
        assert!(rules.iter().any(|(_, pattern, _)| pattern.is_match(&path)), "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_api_call_uses_a_method_the_route_allows() {
    let (js, rules) = (admin_js(), route_matchers().await);
    let verbs: BTreeMap<&str, &str> = [("get", "GET"), ("post", "POST"), ("patch", "PATCH"), ("del", "DELETE")].into_iter().collect();
    let mut calls: Vec<(String, String)> = Vec::new();
    for m in re(r#"Api\.(get|post|patch|del)\(\s*[`'"](/api/admin[^`'"]*)[`'"]"#).captures_iter(&js) {
        calls.push((verbs[&m[1]].to_string(), normalise(&m[2])));
    }
    for m in re(r#"Api\.(?:request|call)\(\s*'([A-Z]+)'\s*,\s*[`'"](/api/admin[^`'"]*)[`'"]"#).captures_iter(&js) {
        calls.push((m[1].to_string(), normalise(&m[2])));
    }
    assert!(calls.len() > 120, "{}", calls.len());
    for (method, path) in calls {
        assert!(rules.iter().any(|(m, pattern, _)| *m == method && pattern.is_match(&path)), "{method} {path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_downloaded_path_is_a_get_route() {
    let (js, rules) = (admin_js(), route_matchers().await);
    for m in re(r#"(?:UI\.download\([^,]+,\s*|location\.href\s*=\s*|downloadWithSudo\()[`'"](/api/admin[^`'"]*)"#).captures_iter(&js) {
        let path = normalise(&m[1]);
        assert!(rules.iter().any(|(method, pattern, _)| method == "GET" && pattern.is_match(&path)), "{path}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn every_admin_route_with_a_ui_is_called_somewhere() {
    // minden admin végpontnak van felülete (kivéve a munkamenet-kezelést és a naplót, amelyet a mag hív): új végpont
    // nem maradhat gomb nélkül
    let (js, rules) = (admin_js(), route_matchers().await);
    let called: BTreeSet<String> = re(r#"[`'"](/api/admin[^`'"\s?]*)"#).captures_iter(&js).map(|c| normalise(&c[1])).collect();
    let param = re(r"\{[^}]+\}");
    let mut uncovered: BTreeSet<String> = BTreeSet::new();
    for (_, pattern, path) in &rules {
        if !path.starts_with("/api/admin") {
            continue;
        }
        let sample = param.replace_all(path, "1").to_string();
        if !called.iter().any(|c| pattern.is_match(c) || *c == sample) {
            uncovered.insert(path.clone());
        }
    }
    assert!(uncovered.is_empty(), "{uncovered:?}");
}

#[test]
fn every_element_id_used_by_admin_js_exists_in_the_html() {
    let (js, html) = (admin_js(), admin_html());
    let mut used: BTreeSet<String> = re(r#"getElementById\(['"]([\w-]+)['"]\)"#).captures_iter(&js).map(|c| c[1].to_string()).collect();
    used.extend(re(r#"(?:Dialogs\.(?:open|close|isOpen)|showFormError|hideFormError)\(['"]([\w-]+)['"]"#).captures_iter(&js).map(|c| c[1].to_string()));
    assert!(used.len() > 20, "{}", used.len());
    let defined: BTreeSet<String> = re(r#"\bid="([\w-]+)""#).captures_iter(&html).map(|c| c[1].to_string()).collect();
    let missing: Vec<&String> = used.difference(&defined).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn every_static_reference_in_the_page_resolves() {
    let html = admin_html();
    let paths: Vec<String> = re(r#"(?:href|src)="(/(?:static|admin/assets)/[^"?{]+)"#).captures_iter(&html).map(|c| c[1].to_string()).collect();
    assert!(!paths.is_empty());
    for path in paths {
        let file = if let Some(rest) = path.strip_prefix("/static/") {
            root().join("web/static").join(rest)
        } else {
            root().join("web/admin").join(path.split("/admin/assets/").nth(1).unwrap())
        };
        assert!(file.is_file(), "{path}");
    }
}

#[test]
fn scripts_load_in_dependency_order() {
    let order: Vec<String> = re(r#"<script src="/(?:static|admin/assets)/([^"?]+)"#).captures_iter(&admin_html()).map(|c| c[1].to_string()).collect();
    let mut expected: Vec<String> = ["admin-boot.js", "i18n-data.js", "admin-i18n.js", "i18n.js"].iter().map(|s| s.to_string()).collect();
    expected.extend(JS_FILES.iter().map(|s| s.to_string()));
    assert_eq!(order, expected);
}

#[test]
fn the_only_external_script_is_the_socket_io_client() {
    let external: Vec<String> = re(r#"<script src="(https?://[^"]+)""#).captures_iter(&admin_html()).map(|c| c[1].to_string()).collect();
    assert_eq!(external, vec!["https://cdnjs.cloudflare.com/ajax/libs/socket.io/4.7.4/socket.io.min.js"]);
}

#[test]
fn every_menu_section_is_registered_in_order() {
    let registered: BTreeMap<String, i64> =
        re(r"registerSection\(\{\s*id:\s*'(\w+)',\s*order:\s*(\d+)").captures_iter(&admin_js()).map(|m| (m[1].to_string(), m[2].parse().unwrap())).collect();
    let expected: BTreeMap<String, i64> = SECTIONS.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    assert_eq!(registered, expected);
}

#[test]
fn every_section_has_a_translated_menu_label() {
    let (data, js) = (admin_data(), admin_js());
    for m in re(r"registerSection\(\{[^}]*labelKey:\s*'(admin\.[a-z_]+)'").captures_iter(&js) {
        assert!(data["hu"].get(&m[1]).is_some(), "{}", &m[1]);
    }
}

#[test]
fn every_icon_used_in_the_menu_is_in_the_sprite() {
    let (js, html) = (admin_js(), admin_html());
    let mut icons: BTreeSet<String> = re(r"icon:\s*'([a-z]+)'").captures_iter(&js).map(|c| c[1].to_string()).collect();
    icons.extend(re(r"makeIcon\('([a-z]+)'\)").captures_iter(&js).map(|c| c[1].to_string()));
    let sprite: BTreeSet<String> = re(r#"<symbol id="i-([a-z]+)""#).captures_iter(&html).map(|c| c[1].to_string()).collect();
    let missing: Vec<&String> = icons.difference(&sprite).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

// ===================================================================================================
// Kódolási szabályok
// ===================================================================================================

#[test]
fn no_innerhtml() {
    let js = admin_js();
    for forbidden in ["innerHTML", "outerHTML", "insertAdjacentHTML", "document.write"] {
        assert!(!js.contains(forbidden), "{forbidden}");
    }
}

#[test]
fn no_dynamic_code() {
    assert!(!re(r"\beval\(|new Function\(").is_match(&admin_js()));
}

#[test]
fn no_inline_style_attributes_from_js() {
    // a szigorú CSP (`style-src 'self'`) a `style` attribútumot nem engedi; a CSSOM (`el.style.x`) igen
    let js = admin_js();
    assert!(!re(r#"setAttribute\(\s*['"]style['"]"#).is_match(&js));
    assert!(!js.contains("cssText"));
}

#[test]
fn every_request_carries_the_csrf_header() {
    let js = admin_js();
    assert!(js.contains("'X-Admin-Request': '1'"));
    // minden hálózati hívás az `Api.request`-en megy át
    assert_eq!(re(r"\bfetch\(").find_iter(&js).count(), 1);
}

#[test]
fn no_other_network_apis() {
    assert!(!re(r"XMLHttpRequest|sendBeacon|new WebSocket|new EventSource").is_match(&admin_js()));
}

#[test]
fn user_text_is_never_interpreted_as_html() {
    // a szöveg csak szövegcsomópontként kerül be (`h()` / `textContent`): `Node` nélküli gyerek is szöveg
    assert!(admin_js().contains("document.createTextNode(String(child))"));
}

#[test]
fn the_admin_page_has_no_inline_code() {
    let html = admin_html();
    assert!(!re(r"<script\b[^>]*>").find_iter(&html).any(|m| !m.as_str().contains("src=")));
    assert!(!re(r"\sstyle=").is_match(&html));
    assert!(!re(r"\son[a-z]+=").is_match(&html));
}

#[test]
fn the_page_is_not_indexable() {
    assert!(admin_html().contains(r#"<meta name="robots" content="noindex,nofollow">"#));
}

#[test]
fn public_sources_do_not_know_the_admin_client() {
    for parts in [
        ["web", "static", "app.js"],
        ["web", "static", "i18n-data.js"],
        ["web", "static", "i18n.js"],
        ["web", "templates", "index.html"],
        ["web", "templates", "sw.js"],
    ] {
        let source = read(&parts);
        assert!(!source.contains("/api/admin") && !source.contains("web/admin") && !source.contains("admin-i18n"), "{parts:?}");
    }
}

#[test]
fn visible_text_is_translatable() {
    let mut stack: Vec<(String, bool)> = Vec::new();
    let mut untranslated: Vec<String> = Vec::new();
    let skipped = ["br", "input", "meta", "link", "use", "path", "line", "circle", "rect", "polyline", "polygon"];
    for node in html_nodes(&admin_html()) {
        match node {
            Node::Start(tag, attrs, self_closing) => {
                if skipped.contains(&tag.as_str()) || self_closing {
                    continue;
                }
                let flagged =
                    attrs.iter().any(|(k, _)| k.starts_with("data-i18n") && k != "data-i18n-placeholder" && k != "data-i18n-title" && k != "data-i18n-aria");
                stack.push((tag, flagged));
            }
            Node::End(tag) => {
                if stack.last().is_some_and(|(t, _)| *t == tag) {
                    stack.pop();
                }
            }
            Node::Text(data) => {
                let text = data.trim();
                if text.is_empty() || stack.last().is_none_or(|(t, _)| ["script", "style", "title", "symbol"].contains(&t.as_str())) {
                    continue;
                }
                if text == "HU" {
                    continue; // a nyelvváltó gomb kódját az I18N.apply() tölti ki
                }
                if !stack.iter().any(|(_, flagged)| *flagged) {
                    untranslated.push(text.to_string());
                }
            }
        }
    }
    assert!(untranslated.is_empty(), "{untranslated:?}");
}

// ===================================================================================================
// A kliens magja és a fordítás valódi futtatása node-ban
// ===================================================================================================

const HARNESS: &str = r#"
const fs = require('fs');
const path = require('path');
const root = process.argv[1];
const store = {};
global.window = { dispatchEvent() {} };
global.CustomEvent = class { constructor(type, init) { this.type = type; this.detail = init && init.detail; } };
global.localStorage = { getItem: (k) => store[k] || null, setItem: (k, v) => { store[k] = v; } };
global.document = {
    title: '',
    documentElement: { _lang: 'hu', getAttribute() { return this._lang; }, setAttribute(k, v) { if (k === 'lang') this._lang = v; } },
    querySelectorAll() { return []; },
    querySelector() { return null; },
};
const read = (...p) => fs.readFileSync(path.join(root, ...p), 'utf8');
const code = read('web', 'static', 'i18n-data.js') + '\n' + read('web', 'admin', 'admin-i18n.js') + '\n' + read('web', 'static', 'i18n.js') +
             '\n' + read('web', 'admin', 'admin.js') +
             '\nmodule.exports = { I18N, t, tServer, queryString, fmtNum, fmtPercent, fmtBytes, fmtDuration, formatCountdown };';
const m = { exports: {} };
new Function('module', 'exports', 'window', 'document', 'localStorage', 'CustomEvent', code)(
    m, m.exports, global.window, global.document, global.localStorage, global.CustomEvent);
const api = m.exports;
const cases = JSON.parse(fs.readFileSync(0, 'utf8'));
const out = cases.map((c) => {
    if (c.op === 'lang') { api.I18N.setLang(c.lang); return api.I18N.lang; }
    if (c.op === 't') return api.t(c.key, c.params);
    if (c.op === 'server') return api.tServer(c.msg);
    return api[c.op].apply(null, c.args || []);
});
process.stdout.write(JSON.stringify(out));
"#;

fn run(cases: Value) -> Vec<Value> {
    let root = root();
    run_node(HARNESS, &[root.to_str().unwrap()], &cases).as_array().unwrap().clone()
}

#[test]
fn pure_helpers_work_without_a_dom() {
    require_node!();
    let out = run(json!([
        {"op": "queryString", "args": [{"a": 1, "b": "", "c": null, "d": true, "e": "x y"}]},
        {"op": "queryString", "args": [{}]},
        {"op": "fmtBytes", "args": [1536]},
        {"op": "fmtBytes", "args": [null]},
        {"op": "fmtPercent", "args": [12.34]},
        {"op": "fmtNum", "args": [null]},
        {"op": "formatCountdown", "args": [75]},
        {"op": "fmtDuration", "args": [90061]},
        {"op": "fmtDuration", "args": [125]},
        {"op": "fmtDuration", "args": [42]},
    ]));
    assert_eq!(out[0], json!("?a=1&d=1&e=x+y"));
    assert_eq!(out[1], json!(""));
    assert!(out[2].as_str().unwrap().ends_with("KB") && out[3] == json!("–"));
    assert!(out[4].as_str().unwrap().ends_with('%') && out[5] == json!("–"));
    assert_eq!(out[6], json!("1:15"));
    assert_eq!((out[7].clone(), out[8].clone(), out[9].clone()), (json!("1 n 1 ó"), json!("2 p"), json!("42 mp")));
}

#[test]
fn translations_are_merged_into_the_shared_translator() {
    require_node!();
    let data = admin_data();
    let out = run(json!([
        {"op": "t", "key": "admin.nav_users"},
        {"op": "server", "msg": "Hibás jelszó."},
        {"op": "lang", "lang": "en"},
        {"op": "t", "key": "admin.nav_users"},
        {"op": "server", "msg": "Hibás jelszó."},
        {"op": "server", "msg": "A felhasználó nincs kitiltva."},
        {"op": "fmtDuration", "args": [90061]},
        {"op": "t", "key": "admin.audit_range", "params": {"from": 1, "to": 50, "total": 120}},
    ]));
    assert_eq!(out[0], data["hu"]["admin.nav_users"]);
    assert_eq!(out[1], json!("Hibás jelszó.")); // magyarul az eredeti marad
    assert_eq!(out[3], data["en"]["admin.nav_users"]);
    assert_eq!(out[4], json!("Wrong password."));
    assert_eq!(out[5], json!("The user is not banned."));
    assert_eq!(out[6], json!("1 d 1 h"));
    assert_eq!(out[7], json!("1–50 / 120"));
}

#[test]
fn every_admin_text_formats_in_both_languages() {
    require_node!();
    let data = admin_data();
    let mut cases: Vec<Value> = vec![json!({"op": "lang", "lang": "hu"})];
    let mut expected: Vec<(String, String)> = Vec::new();
    for lang in ["hu", "en"] {
        if lang == "en" {
            cases.push(json!({"op": "lang", "lang": "en"}));
        }
        for key in ui_keys(&data[lang]) {
            let params: serde_json::Map<String, Value> = placeholders(text(&data[lang], &key)).into_iter().map(|n| (n, json!("X"))).collect();
            cases.push(json!({"op": "t", "key": key, "params": params}));
            expected.push((lang.to_string(), key));
        }
    }
    let out = run(Value::Array(cases.clone()));
    let texts: Vec<&Value> = out.iter().zip(&cases).filter(|(_, c)| c["op"] == "t").map(|(o, _)| o).collect();
    assert_eq!(texts.len(), expected.len());
    for ((lang, key), text) in expected.iter().zip(texts) {
        let text = text.as_str().unwrap();
        assert!(!text.contains('{') && !text.contains('}'), "{lang} {key}: {text}");
    }
}
