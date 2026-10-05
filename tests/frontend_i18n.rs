//! Többnyelvű felület: a fordítások teljessége és a kliens fordító működése (a Python `test_i18n.py` megfelelője).
//! A szerver üzeneteit a Rust forrásból gyűjti ki.

#[macro_use]
mod frontend_support;
use frontend_support::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

fn data() -> Value {
    data_of(&read(&["static", "i18n-data.js"]), "I18N_DATA")
}

fn app_js() -> String {
    read(&["static", "app.js"]) + &read(&["static", "i18n.js"])
}

fn index_html() -> String {
    read(&["templates", "index.html"])
}

/// Dinamikusan összerakott kulcscsaládok (a kódban `t('előtag.' + ...)` formában szerepelnek)
const DYNAMIC_PREFIXES: [&str; 9] = ["ai.", "history.", "dict.reason_", "sound.cat_", "board.", "last.vote_", "badge.", "hunt.reason_", "hunt.rank_"];

// ===================================================================================================
// Az adatfájl
// ===================================================================================================

#[test]
fn both_languages_have_the_same_keys() {
    let data = data();
    assert_eq!(ui_keys(&data["hu"]), ui_keys(&data["en"]));
}

#[test]
fn no_empty_translations() {
    let data = data();
    for lang in ["hu", "en"] {
        for key in ui_keys(&data[lang]) {
            assert!(!text(&data[lang], &key).trim().is_empty() || key == "lobby.guest_suffix", "{lang} {key}");
        }
    }
}

#[test]
fn placeholders_match_between_languages() {
    let data = data();
    for key in ui_keys(&data["hu"]) {
        let (hu, en) = (placeholders(text(&data["hu"], &key)), placeholders(text(&data["en"], &key)));
        if key.ends_with("_one") {
            // az angol egyes szám elhagyhatja a számot ("1 tile"), de újat nem vezethet be
            assert!(en.is_subset(&hu), "{key}: {hu:?} != {en:?}");
        } else {
            assert_eq!(hu, en, "{key}");
        }
    }
}

#[test]
fn plural_keys_have_a_base_key() {
    let data = data();
    for key in ui_keys(&data["en"]) {
        if let Some(base) = key.strip_suffix("_one") {
            assert!(data["en"].get(base).is_some(), "{key}");
        }
    }
}

#[test]
fn english_differs_from_hungarian_for_real_text() {
    // a nyelvfüggetlen szavak kivételével az angol szöveg nem lehet magyar másolat
    let data = data();
    let allowed = ["game.chat", "game.offline", "history.vs", "sound.max", "create.ai_1", "room.badge_bots_one", "create.limit_none", "quiz.progress"];
    let same: Vec<String> = ui_keys(&data["hu"]).into_iter().filter(|k| data["hu"][k] == data["en"][k]).collect();
    let bad: Vec<&String> = same.iter().filter(|k| !allowed.contains(&k.as_str()) && text(&data["hu"], k).chars().count() > 12).collect();
    assert!(bad.is_empty(), "{same:?}");
}

#[test]
fn the_file_is_loadable_by_the_browser() {
    let source = read(&["static", "i18n-data.js"]);
    assert!(source.starts_with("//"));
    assert!(source.contains("window.I18N_DATA = {"));
}

// ===================================================================================================
// A kulcsok használata
// ===================================================================================================

#[test]
fn every_key_used_in_js_exists() {
    let (data, js) = (data(), app_js());
    let used: BTreeSet<String> = re(r"\bt\('([a-z_]+\.[a-z0-9_]+)'").captures_iter(&js).map(|c| c[1].to_string()).filter(|k| !k.ends_with('_')).collect();
    let missing: Vec<&String> = used.iter().filter(|k| data["hu"].get(k.as_str()).is_none()).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn every_html_key_exists() {
    let (data, html) = (data(), index_html());
    let used: BTreeSet<String> = re(r#"data-i18n(?:-placeholder|-title|-aria)?="([^"]+)""#).captures_iter(&html).map(|c| c[1].to_string()).collect();
    assert!(!used.is_empty());
    let missing: Vec<&String> = used.iter().filter(|k| data["hu"].get(k.as_str()).is_none() || data["en"].get(k.as_str()).is_none()).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn dynamic_key_families_are_complete() {
    let data = data();
    let has = |key: &str| data["en"].get(key).is_some();
    let has_both = |key: &str| data["en"].get(key).is_some() && data["hu"].get(key).is_some();
    for level in 1..=10 {
        assert!(has_both(&format!("ai.level_{level}")), "ai.level_{level}");
    }
    for kind in ["exchange", "pass", "challenge_reject", "other"] {
        assert!(has(&format!("history.{kind}")), "history.{kind}");
    }
    for reason in ["not_in_dictionary", "invalid_chars", "too_long", "too_short"] {
        assert!(has(&format!("dict.reason_{reason}")), "dict.reason_{reason}");
    }
    for reason in ["too_short", "invalid_chars", "not_in_rack", "split_digraph", "not_a_word"] {
        assert!(has_both(&format!("hunt.reason_{reason}")), "hunt.reason_{reason}"); // practice::check_rack_word
    }
    for rank in ["master", "advanced", "solid", "beginner"] {
        assert!(has_both(&format!("hunt.rank_{rank}")), "hunt.rank_{rank}");
    }
    for cat in ["tile_place", "vote", "challenge_result", "your_turn", "chat", "game_events"] {
        assert!(has(&format!("sound.cat_{cat}")) && has(&format!("sound.cat_{cat}_desc")), "sound.cat_{cat}");
    }
    for prefix in ["dl", "tl", "dw", "tw"] {
        assert!(has(&format!("board.{prefix}_long")) && has(&format!("board.{prefix}_short")), "board.{prefix}");
    }
    for badge in scrabble::achievements::BADGES {
        for lang in ["hu", "en"] {
            assert!(data[lang].get(format!("badge.{badge}")).is_some() && data[lang].get(format!("badge.{badge}_desc")).is_some(), "{lang} {badge}");
        }
    }
    for kind in ["exchange", "pass", "rejected", "skip", "vote_accept", "vote_reject", "game_over", "game_over_draw", "save_revert", "place", "pending", "withdrawn", "timeout", "resigned"] {
        assert!(has(&format!("last.{kind}")), "last.{kind}");
    }
}

#[test]
fn no_unused_keys() {
    let data = data();
    let corpus = app_js() + &index_html();
    let unused: Vec<String> = ui_keys(&data["hu"])
        .into_iter()
        .filter(|key| !key.ends_with("_one") && !DYNAMIC_PREFIXES.iter().any(|p| key.starts_with(p)))
        .filter(|key| !corpus.contains(&format!("'{key}'")) && !corpus.contains(&format!("\"{key}\"")))
        .collect();
    assert!(unused.is_empty(), "{unused:?}");
}

/// A `t('kulcs', {..})` hívás törzse a nyitó kapcsos zárójeltől a párjáig, felső szintű vesszők mentén szétvágva.
fn call_params(js: &str, open_end: usize) -> BTreeSet<String> {
    let bytes: Vec<char> = js[open_end..].chars().collect();
    let (mut depth, mut i) = (1, 0);
    while depth > 0 && i < bytes.len() {
        match bytes[i] {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        i += 1;
    }
    let body: String = bytes[..i.saturating_sub(1)].iter().collect();
    let (mut parts, mut level, mut current) = (Vec::new(), 0, String::new());
    for ch in body.chars() {
        match ch {
            '(' | '[' | '{' => level += 1,
            ')' | ']' | '}' => level -= 1,
            _ => {}
        }
        if ch == ',' && level == 0 {
            parts.push(std::mem::take(&mut current));
        } else {
            current.push(ch);
        }
    }
    parts.push(current);
    parts.iter().filter(|p| !p.trim().is_empty()).map(|p| p.split(':').next().unwrap().trim().to_string()).collect()
}

#[test]
fn js_params_cover_the_placeholders() {
    let (data, js) = (data(), app_js());
    for m in re(r"\bt\('([a-z_]+\.[a-z0-9_]+)',\s*\{").captures_iter(&js) {
        let key = m[1].to_string();
        let names = call_params(&js, m.get(0).unwrap().end());
        let needed = placeholders(text(&data["hu"], &key));
        assert!(needed.is_subset(&names), "{key}: hiányzó paraméter {:?}", needed.difference(&names).collect::<Vec<_>>());
    }
}

// ===================================================================================================
// A HTML lefedettsége
// ===================================================================================================

fn untranslated(html: &str) -> (Vec<String>, Vec<(String, String)>) {
    let mut stack: Vec<(String, bool)> = Vec::new(); // (címke, van-e data-i18n*)
    let mut classes: Vec<String> = Vec::new();
    let (mut texts, mut attrs_found) = (Vec::new(), Vec::new());
    let letters = re(r"[A-Za-zÁ-űá-ű]{2,}");
    for node in html_nodes(html) {
        match node {
            Node::Start(tag, attrs, self_closing) => {
                let has_attr = |n: &str| attr(&attrs, n).is_some();
                if has_attr("placeholder") && !has_attr("data-i18n-placeholder") {
                    attrs_found.push(("placeholder".to_string(), attr(&attrs, "placeholder").unwrap().to_string()));
                }
                if has_attr("title") && tag != "title" && !has_attr("data-i18n-title") {
                    attrs_found.push(("title".to_string(), attr(&attrs, "title").unwrap().to_string()));
                }
                if has_attr("aria-label") && !has_attr("data-i18n-title") && !has_attr("data-i18n-aria") {
                    attrs_found.push(("aria-label".to_string(), attr(&attrs, "aria-label").unwrap().to_string()));
                }
                if !VOID_TAGS.contains(&tag.as_str()) && !self_closing {
                    stack.push((tag, attrs.iter().any(|(k, _)| k.contains("data-i18n"))));
                    classes.push(attr(&attrs, "class").unwrap_or("").to_string());
                }
            }
            Node::End(tag) => {
                if let Some(i) = stack.iter().rposition(|(t, _)| *t == tag) {
                    stack.truncate(i);
                    classes.truncate(i);
                }
            }
            Node::Text(data) => {
                let text = data.trim();
                if text.is_empty() || !letters.is_match(text) {
                    continue;
                }
                if stack.iter().any(|(t, _)| ["script", "style", "title", "svg", "symbol"].contains(&t.as_str())) {
                    continue;
                }
                if stack.iter().any(|(_, flagged)| *flagged) {
                    continue;
                }
                let joined = classes.join(" ");
                if joined.contains("brand-mark") || joined.contains("lang-code") {
                    continue;
                }
                texts.push(text.to_string());
            }
        }
    }
    (texts, attrs_found)
}

#[test]
fn all_visible_text_is_translatable() {
    let (texts, _) = untranslated(&index_html());
    assert!(texts.is_empty(), "{texts:?}");
}

#[test]
fn all_attributes_are_translatable() {
    let (_, attrs) = untranslated(&index_html());
    assert!(attrs.is_empty(), "{attrs:?}");
}

#[test]
fn language_toggle_buttons_exist_on_main_screens() {
    assert!(index_html().matches("btn-lang-toggle").count() >= 5);
}

#[test]
fn the_html_lang_attribute_is_set_before_paint() {
    let html = index_html();
    assert!(html.contains("document.documentElement.setAttribute('lang', lang)"));
    assert!(html.contains("scrabble-lang"));
}

// ===================================================================================================
// A szerver üzenetei
// ===================================================================================================

/// A szerver forrásában szereplő, a kliensnek szánt üzenetek: mondatszerű szöveges literálok (a formázó
/// helyőrzők `7`-tel helyettesítve). Az admin panel, a levelezés, a push és a beállítások üzenetei nem ide tartoznak.
fn server_messages() -> std::collections::BTreeMap<String, String> {
    let skipped = ["src/mail.rs", "src/push.rs", "src/settings.rs", "src/tunnel.rs", "src/main.rs"];
    let (word, placeholder) = (re(r"[a-záéíóöőúüű]{3}"), re(r"\{[^}]*\}"));
    let skipped_context = re(r"(?:println|eprintln|panic|assert\w*|write|writeln|unreachable)!\(\s*$|\.expect\(\s*$");
    let mut found = std::collections::BTreeMap::new();
    for (path, source) in rust_sources() {
        if path.starts_with("src/admin/") || skipped.contains(&path.as_str()) {
            continue;
        }
        for m in re(r#""((?:[^"\\\n]|\\.)*)""#).captures_iter(&source) {
            let start = m.get(0).unwrap().start();
            let mut from = start.saturating_sub(40);
            while !source.is_char_boundary(from) {
                from += 1;
            }
            let before = &source[from..start];
            if skipped_context.is_match(before) {
                continue;
            }
            let literal = m[1].replace("\\\"", "\"").replace("\\n", "\n");
            let message = placeholder.replace_all(&literal, "7").to_string();
            if word.is_match(&message) && message.contains(' ') && (message.ends_with('.') || message.ends_with('!')) {
                found.entry(message).or_insert(path.clone());
            }
        }
    }
    found
}

/// A kliens ezeket a szerkezetes adatból maga formázza, vagy sosem jelennek meg a felhasználónak
const NOT_SHOWN: [&str; 6] = [
    "7 szavai elutasítva: 7. Betűk visszavéve, újra ő következik.",
    "7 elfogadta.",
    "7 elutasította.",
    "7 lerakása a mentés miatt visszavonva.",
    "7 visszavonta a lerakását.",
    "7 feladta a játékot.",
];

fn server_patterns(data: &Value) -> Vec<regex::Regex> {
    data["en"]["server"]["patterns"].as_array().unwrap().iter().map(|p| re(p[0].as_str().unwrap())).collect()
}

#[test]
fn messages_were_found() {
    assert!(server_messages().len() >= 100);
}

#[test]
fn every_server_message_has_an_english_translation() {
    let data = data();
    let exact = data["en"]["server"]["exact"].as_object().unwrap();
    let patterns = server_patterns(&data);
    let missing: Vec<(String, String)> = server_messages()
        .into_iter()
        .filter(|(msg, _)| !exact.contains_key(msg) && !NOT_SHOWN.contains(&msg.as_str()) && !patterns.iter().any(|p| p.find(msg).is_some_and(|m| m.start() == 0)))
        .collect();
    assert!(missing.is_empty(), "{missing:#?}");
}

#[test]
fn exact_entries_exist_in_the_server_sources() {
    let data = data();
    let all: String = rust_sources().iter().flat_map(|(_, source)| string_literals(source)).collect::<Vec<_>>().join("\n");
    let stale: Vec<&String> = data["en"]["server"]["exact"].as_object().unwrap().keys().filter(|m| !all.contains(m.as_str())).collect();
    assert!(stale.is_empty(), "{stale:?}");
}

#[test]
fn patterns_compile_and_have_templates() {
    let data = data();
    for pair in data["en"]["server"]["patterns"].as_array().unwrap() {
        let (pattern, template) = (pair[0].as_str().unwrap(), pair[1].as_str().unwrap());
        let compiled = regex::Regex::new(pattern).unwrap_or_else(|e| panic!("{pattern}: {e}"));
        let groups = compiled.captures_len() - 1;
        for reference in re(r"\$(\d)").captures_iter(template) {
            assert!(reference[1].parse::<usize>().unwrap() <= groups, "{pattern} → {template}");
        }
    }
}

// ===================================================================================================
// A böngészős fordító valódi futtatása node-ban
// ===================================================================================================

const HARNESS: &str = r#"
const fs = require('fs');
const path = require('path');
const root = process.argv[1];
const lang = process.argv[2];
const store = {};
global.window = { dispatchEvent() {} };
global.CustomEvent = class { constructor(type, init) { this.type = type; this.detail = init && init.detail; } };
global.localStorage = { getItem: (k) => store[k] || null, setItem: (k, v) => { store[k] = v; } };
global.document = {
    title: '',
    documentElement: { _lang: lang, getAttribute() { return this._lang; }, setAttribute(k, v) { if (k === 'lang') this._lang = v; } },
    querySelectorAll() { return []; },
    querySelector() { return null; },
};
const code = fs.readFileSync(path.join(root, 'static', 'i18n-data.js'), 'utf8') + '\n' +
             fs.readFileSync(path.join(root, 'static', 'i18n.js'), 'utf8') +
             '\nmodule.exports = { I18N, t, tServer };';
const m = { exports: {} };
new Function('module', 'exports', 'window', 'document', 'localStorage', 'CustomEvent', code)(
    m, m.exports, global.window, global.document, global.localStorage, global.CustomEvent);
const { I18N, t, tServer } = m.exports;
const cases = JSON.parse(fs.readFileSync(0, 'utf8'));
const out = cases.map((c) => {
    if (c.op === 't') return t(c.key, c.params);
    if (c.op === 'server') return tServer(c.msg);
    if (c.op === 'set') { I18N.setLang(c.lang); return I18N.lang; }
    if (c.op === 'title') return document.title;
    return null;
});
console.log(JSON.stringify(out));
"#;

fn translate(lang: &str, cases: Value) -> Vec<Value> {
    let root = root();
    run_node(HARNESS, &[root.to_str().unwrap(), lang], &cases).as_array().unwrap().clone()
}

#[test]
fn translates_with_parameters() {
    require_node!();
    assert_eq!(translate("hu", json!([{"op": "t", "key": "room.players_owner", "params": {"n": 2, "max": 4, "owner": "Anna"}}])), vec![json!("2/4 játékos · Anna")]);
}

#[test]
fn english() {
    require_node!();
    let out = translate("en", json!([{"op": "t", "key": "room.players_owner", "params": {"n": 2, "max": 4, "owner": "Anna"}}, {"op": "t", "key": "lobby.join"}]));
    assert_eq!(out, vec![json!("2/4 players · Anna"), json!("Join")]);
}

#[test]
fn the_singular_form_is_used_for_one() {
    require_node!();
    let out = translate("en", json!([{"op": "t", "key": "common.points", "params": {"n": 1}}, {"op": "t", "key": "common.points", "params": {"n": 5}}, {"op": "t", "key": "game.bag", "params": {"n": 1}}]));
    assert_eq!(out, vec![json!("1 pt"), json!("5 pts"), json!("Bag: 1 tile")]);
}

#[test]
fn an_unknown_key_returns_the_key_and_a_missing_param_keeps_the_placeholder() {
    require_node!();
    assert_eq!(translate("en", json!([{"op": "t", "key": "no.such.key"}])), vec![json!("no.such.key")]);
    assert_eq!(translate("hu", json!([{"op": "t", "key": "common.points", "params": {}}])), vec![json!("{n} pont")]);
}

#[test]
fn server_messages_are_translated_in_english_only() {
    require_node!();
    let cases = json!([
        {"op": "server", "msg": "Nem te következel."},
        {"op": "server", "msg": "Érvénytelen szó(k): XQZ"},
        {"op": "server", "msg": "Nincs 'K' betűd."},
        {"op": "server", "msg": "A (3,4) mező már foglalt."},
        {"op": "server", "msg": "Hibás kód. Még 3 próbálkozásod van."},
        {"op": "server", "msg": "Ismeretlen üzenet, amit nem fordítunk."},
    ]);
    assert_eq!(
        translate("en", cases.clone()),
        vec![
            json!("It's not your turn."),
            json!("Invalid word(s): XQZ"),
            json!("You don't have the letter 'K'."),
            json!("Square (3,4) is already occupied."),
            json!("Wrong code. You have 3 attempts left."),
            json!("Ismeretlen üzenet, amit nem fordítunk."),
        ]
    );
    assert_eq!(translate("hu", json!([cases[0].clone()])), vec![json!("Nem te következel.")]);
}

#[test]
fn the_exact_lookup_ignores_object_prototype_names() {
    require_node!();
    assert_eq!(translate("en", json!([{"op": "server", "msg": "constructor"}])), vec![json!("constructor")]);
}

#[test]
fn switching_language() {
    require_node!();
    let out = translate("hu", json!([{"op": "t", "key": "lobby.join"}, {"op": "set", "lang": "en"}, {"op": "t", "key": "lobby.join"}, {"op": "title"}, {"op": "set", "lang": "xx"}]));
    assert_eq!(out, vec![json!("Csatlakozás"), json!("en"), json!("Join"), json!("Hungarian Scrabble"), json!("en")]);
}

#[test]
fn every_key_formats_without_leftover_placeholders() {
    require_node!();
    // ha minden paramétert megadunk, nem marad feldolgozatlan {helyőrző}
    let data = data();
    let keys: Vec<String> = ui_keys(&data["en"]).into_iter().collect();
    let names = [
        "n", "max", "owner", "name", "names", "players", "words", "score", "player", "room", "word", "total", "bag", "hands", "vowels", "consonants", "blanks", "played", "won", "rate", "rating", "change", "done",
        "lost", "missed", "optimal", "turns", "rank", "best", "date", "correct", "d", "h", "m", "time", "letters", "len", "got", "tiles", "pct", "solved", "streak", "reason", "until",
    ];
    let params: Value = names.iter().map(|n| (n.to_string(), json!("X"))).collect::<serde_json::Map<_, _>>().into();
    let cases: Vec<Value> = keys.iter().map(|k| json!({"op": "t", "key": k, "params": params})).collect();
    for lang in ["hu", "en"] {
        let out = translate(lang, Value::Array(cases.clone()));
        let leftovers: Vec<(&String, &Value)> = keys.iter().zip(out.iter()).filter(|(_, v)| v.as_str().unwrap_or("").contains('{')).collect();
        assert!(leftovers.is_empty(), "{lang}: {leftovers:?}");
    }
}
