//! A kliens (app.js, index.html) és a szerver összhangja, valamint a kliens állapotkezelő és gyakorló logikája
//! valódi futtatással node-ban (a Python `test_frontend_consistency.py`, `test_client_logic.py` és
//! `test_practice_client.py` megfelelője).

#[macro_use]
mod frontend_support;
mod common;
use frontend_support::*;
use regex::Regex;
use scrabble::board::{Premium, premium_at};
use scrabble::tiles::{TILE_DISTRIBUTION, Tile, forms_digraph, letters, tokenize_word};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn app_js() -> String {
    read(&["web", "static", "app.js"])
}

fn index_html() -> String {
    read(&["web", "templates", "index.html"])
}

/// A `const NAME = { 'A': 1, ... };` objektum literál kiolvasása.
fn js_object(source: &str, name: &str) -> BTreeMap<String, i64> {
    let body = re(&format!(r"(?s)const {name} = \{{(.*?)\}};")).captures(source).unwrap_or_else(|| panic!("nincs {name}"))[1].to_string();
    re(r"'([^']*)':\s*(\d+)").captures_iter(&body).map(|c| (c[1].to_string(), c[2].parse().unwrap())).collect()
}

fn html_ids(html: &str) -> BTreeSet<String> {
    re(r#"\bid="([^"]+)""#).captures_iter(html).map(|c| c[1].to_string()).collect()
}

// ===================================================================================================
// Konstansok
// ===================================================================================================

#[test]
fn tile_values_and_counts_match_the_server() {
    let js = app_js();
    let values: BTreeMap<String, i64> = TILE_DISTRIBUTION.iter().map(|(l, v, _)| (l.to_string(), *v as i64)).collect();
    let counts: BTreeMap<String, i64> = TILE_DISTRIBUTION.iter().map(|(l, _, c)| (l.to_string(), *c as i64)).collect();
    assert_eq!(js_object(&js, "TILE_VALUES"), values);
    assert_eq!(js_object(&js, "TILE_COUNTS"), counts);
}

#[test]
fn all_letters_match_the_server() {
    let body = re(r"(?s)const ALL_LETTERS = \[(.*?)\];").captures(&app_js()).unwrap()[1].to_string();
    let mut js: Vec<String> = re(r"'([^']+)'").captures_iter(&body).map(|c| c[1].to_string()).collect();
    let mut server: Vec<String> = letters().map(|t| t.as_str().to_string()).collect();
    js.sort();
    server.sort();
    assert_eq!(js, server);
}

#[test]
fn premium_squares_match_the_server() {
    let body = re(r"(?s)const PREMIUM_QUARTER = \[(.*?)\n\];").captures(&app_js()).unwrap()[1].to_string();
    let mut js: BTreeMap<(usize, usize), String> = BTreeMap::new();
    for m in re(r"\[(\d+),\s*(\d+),\s*'(\w+)'\]").captures_iter(&body) {
        let (r, c): (usize, usize) = (m[1].parse().unwrap(), m[2].parse().unwrap());
        for cell in [(r, c), (r, 14 - c), (14 - r, c), (14 - r, 14 - c)] {
            js.insert(cell, m[3].to_string());
        }
    }
    for row in 0..15 {
        for col in 0..15 {
            let expected = match premium_at(row, col) {
                Premium::None => None,
                Premium::Dl => Some("DL"),
                Premium::Tl => Some("TL"),
                Premium::Dw => Some("DW"),
                Premium::Tw => Some("TW"),
                Premium::Star => Some("ST"),
            };
            assert_eq!(js.get(&(row, col)).map(|s| s.as_str()), expected, "({row},{col})");
        }
    }
}

#[test]
fn the_distribution_is_the_standard_one() {
    assert_eq!(TILE_DISTRIBUTION.len(), 39);
    assert_eq!(TILE_DISTRIBUTION.iter().map(|(_, _, c)| *c as u32).sum::<u32>(), 100);
}

// ===================================================================================================
// HTML azonosítók
// ===================================================================================================

#[test]
fn every_element_id_used_in_js_exists_in_html() {
    let (js, html) = (app_js(), index_html());
    let ids = html_ids(&html);
    let missing: Vec<String> = re(r"getElementById\('([^']+)'\)")
        .captures_iter(&js)
        .map(|c| c[1].to_string())
        .filter(|i| !ids.contains(i))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn no_duplicate_ids_in_html() {
    let ids: Vec<String> = re(r#"\bid="([^"]+)""#).captures_iter(&index_html()).map(|c| c[1].to_string()).collect();
    let duplicates: BTreeSet<&String> = ids.iter().filter(|i| ids.iter().filter(|j| j == i).count() > 1).collect();
    assert!(duplicates.is_empty(), "{duplicates:?}");
}

#[test]
fn scripts_are_loaded_in_dependency_order() {
    let names: Vec<String> =
        re(r#"<script src="([^"]+)""#).captures_iter(&index_html()).map(|c| c[1].split('?').next().unwrap().rsplit('/').next().unwrap().to_string()).collect();
    let pos = |n: &str| names.iter().position(|x| x == n).unwrap_or_else(|| panic!("nincs {n}: {names:?}"));
    assert!(pos("i18n-data.js") < pos("i18n.js") && pos("i18n.js") < pos("app.js"));
    assert!(pos("socket.io.min.js") < pos("app.js"));
}

#[test]
fn id_selectors_in_js_exist() {
    // felépítés közben létrehozott elemek azonosítói nem szerepelhetnek a HTML-ben
    let (js, html) = (app_js(), index_html());
    let ids = html_ids(&html);
    let missing: Vec<String> =
        re(r"querySelector\('#([\w-]+)").captures_iter(&js).map(|c| c[1].to_string()).filter(|i| i != "game-screen" && !ids.contains(i)).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

// ===================================================================================================
// Robot-fokozat választó
// ===================================================================================================

fn difficulty_select() -> String {
    re(r#"(?s)<select id="room-ai-difficulty"[^>]*>(.*?)</select>"#).captures(&index_html()).unwrap()[1].to_string()
}

#[test]
fn the_difficulty_select_offers_exactly_the_server_levels() {
    let values: Vec<String> = re(r#"<option value="(\w+)""#).captures_iter(&difficulty_select()).map(|c| c[1].to_string()).collect();
    let mut expected: Vec<String> = (scrabble::ai::MIN_LEVEL..=scrabble::ai::MAX_LEVEL).map(|l| l.to_string()).collect();
    expected.push("auto".to_string());
    assert_eq!(values, expected);
}

#[test]
fn the_default_option_is_the_default_level() {
    let selected: Vec<String> = re(r#"<option value="(\d+)" selected"#).captures_iter(&difficulty_select()).map(|c| c[1].to_string()).collect();
    assert_eq!(selected, vec![scrabble::ai::DEFAULT_LEVEL.to_string()]);
}

#[test]
fn every_option_has_a_matching_translation_key() {
    for m in re(r#"<option value="(\w+)"[^>]*data-i18n="([^"]+)""#).captures_iter(&difficulty_select()) {
        assert_eq!(&m[2], format!("ai.level_{}", &m[1]));
    }
}

// ===================================================================================================
// Socket.IO események
// ===================================================================================================

/// A szerver eseménykezelői: a szinkron esemény-táblázat és a külön regisztrált (`socket.on`) kezelők.
fn server_handlers() -> BTreeSet<String> {
    let source = read(&["src", "server", "mod.rs"]);
    let mut handlers: BTreeSet<String> = re(r#"\(\s*"([a-z_]+)",\s*\w+::\w+\s*\)"#).captures_iter(&source).map(|c| c[1].to_string()).collect();
    handlers.extend(re(r#"socket\.on\(\s*"([a-z_]+)""#).captures_iter(&source).map(|c| c[1].to_string()));
    handlers
}

/// Minden szöveges literál a szerver forrásában (az eseménynevek ellenőrzéséhez).
fn server_literals() -> BTreeSet<String> {
    rust_sources().iter().flat_map(|(_, source)| string_literals(source)).collect()
}

#[test]
fn events_emitted_by_the_client_have_server_handlers() {
    let emitted: BTreeSet<String> = re(r"socket\.emit\('(\w+)'").captures_iter(&app_js()).map(|c| c[1].to_string()).collect();
    assert!(!emitted.is_empty(), "nem találtam socket.emit hívást");
    let handlers = server_handlers();
    let missing: Vec<&String> = emitted.iter().filter(|e| !handlers.contains(*e)).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn events_listened_by_the_client_are_sent_by_the_server() {
    let literals = server_literals();
    let listened: BTreeSet<String> = re(r"socket\.on\('(\w+)'").captures_iter(&app_js()).map(|c| c[1].to_string()).collect();
    let builtin = ["connect", "disconnect", "connect_error"]; // a Socket.IO saját eseményei
    let missing: Vec<&String> = listened.iter().filter(|e| !builtin.contains(&e.as_str()) && !literals.contains(*e)).collect();
    assert!(missing.is_empty(), "{missing:?}");
}

#[test]
fn new_features_are_wired_on_both_sides() {
    let (js, handlers, literals) = (app_js(), server_handlers(), server_literals());
    for event in ["spectate_room", "leave_spectate", "preview_move", "request_hint"] {
        assert!(handlers.contains(event), "{event}");
        assert!(js.contains(&format!("'{event}'")), "{event}"); // a kilépés feltételes: socket.emit(cond ? 'a' : 'b')
    }
    for event in ["spectate_joined", "spectate_left", "move_preview", "hint_result", "live_games"] {
        assert!(literals.contains(event), "{event}");
        assert!(js.contains(&format!("socket.on('{event}'")), "{event}");
    }
}

// ===================================================================================================
// API útvonalak
// ===================================================================================================

fn public_routes() -> Vec<Regex> {
    let param = re(r"\{[^}]+\}");
    re(r#"\.route\("([^"]+)""#)
        .captures_iter(&read(&["src", "server", "http.rs"]))
        .map(|c| {
            let path = c[1].to_string();
            let (mut pattern, mut last) = (String::from("^"), 0);
            for m in param.find_iter(&path) {
                pattern.push_str(&regex::escape(&path[last..m.start()]));
                pattern.push_str("[^/]+");
                last = m.end();
            }
            pattern.push_str(&regex::escape(&path[last..]));
            pattern.push('$');
            re(&pattern)
        })
        .collect()
}

fn route_paths() -> Vec<String> {
    re(r#"\.route\("([^"]+)""#).captures_iter(&read(&["src", "server", "http.rs"])).map(|c| c[1].to_string()).collect()
}

#[test]
fn fetched_api_paths_exist_on_the_server() {
    let (js, rules, paths) = (app_js(), public_routes(), route_paths());
    let fetched: BTreeSet<String> = re(r"fetch\(`?'?(/api/[\w/-]+)").captures_iter(&js).map(|c| c[1].to_string()).collect();
    assert!(!fetched.is_empty());
    for path in fetched {
        let known =
            rules.iter().any(|r| r.is_match(&path)) || paths.iter().any(|p| p.starts_with(&path) || p.trim_end_matches('/') == path.trim_end_matches('/'));
        assert!(known, "{path}");
    }
}

// ===================================================================================================
// JavaScript szintaxis és statikus fájlok
// ===================================================================================================

#[test]
fn client_files_parse() {
    require_node!();
    for name in ["app.js", "i18n.js", "i18n-data.js"] {
        if let Err(e) = node_check_file(&root().join("web/static").join(name)) {
            panic!("{name}: {e}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_service_worker_template_parses() {
    require_node!();
    let server = common::TestServer::start().await;
    let body = server.http().get("/sw.js").await.text();
    node_check_source(&body).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn every_static_reference_in_html_resolves() {
    let server = common::TestServer::start().await;
    let paths: BTreeSet<String> = re(r#"(?:href|src)="(/static/[^"?]+)"#).captures_iter(&index_html()).map(|c| c[1].to_string()).collect();
    assert!(!paths.is_empty());
    let http = server.http();
    for path in paths {
        assert_eq!(http.get(&path).await.status, 200, "{path}");
    }
}

#[test]
fn the_language_toggle_has_an_aria_label() {
    for m in re(r"<button[^>]*btn-lang-toggle[^>]*>").find_iter(&index_html()) {
        assert!(m.as_str().contains("aria-label"), "{}", m.as_str());
    }
}

// ===================================================================================================
// Meghívó link, profil navigáció, betűtartó CSS
// ===================================================================================================

#[test]
fn url_actions_run_only_after_identity_is_sent() {
    // a meghívó linkes csatlakozás előtt a szervernek ismernie kell a nevet (set_name)
    let js = app_js();
    assert!(js.contains("const identityReady = this.sendIdentity();"));
    assert!(js.contains("identityReady.then(() => this.handleUrlActions());"));
    assert_eq!(js.matches("this.handleUrlActions()").count(), 1);
}

fn tabs(html: &str, nav_id: &str) -> Vec<String> {
    let nav = re(&format!(r#"(?s)<nav[^>]*id="{nav_id}".*?</nav>"#)).find(html).unwrap().as_str().to_string();
    re(r#"data-lobby-tab="(\w+)""#).captures_iter(&nav).map(|c| c[1].to_string()).collect()
}

#[test]
fn the_profile_has_the_same_tabs_in_the_same_order() {
    let html = index_html();
    assert_eq!(tabs(&html, "profile-nav"), tabs(&html, "lobby-nav"));
}

#[test]
fn the_profile_topbar_uses_the_lobby_layout() {
    let header = re(r#"(?s)<div id="profile-screen".*?</header>"#).find(&index_html()).unwrap().as_str().to_string();
    assert!(header.contains("app-topbar app-topbar-lobby") && header.contains(r#"id="profile-nav""#));
}

#[test]
fn lobby_tab_handlers_are_scoped_to_the_lobby_row() {
    // különben a profil fülei is a lobby paneljét váltanák, képernyőváltás nélkül
    let js = app_js();
    assert!(!js.contains("document.querySelectorAll('.lobby-nav-tab')"));
    assert!(js.contains("'#lobby-nav .lobby-nav-tab'") && js.contains("'#profile-nav .lobby-nav-tab'"));
}

fn css_rule(css: &str, selector: &str) -> String {
    re(&format!(r"(?s){}\s*\{{(.*?)\n    \}}", regex::escape(selector))).captures(css).unwrap_or_else(|| panic!("{selector}"))[1].to_string()
}

#[test]
fn the_tile_size_does_not_depend_on_the_board_size() {
    // a zseton mérete a betűtartó szélességét adja, az pedig a tábla méretét: ha a zseton a tábla méretéből
    // számolódna, a CSS változók körkörösen függnének egymástól (érvénytelen érték)
    let css = read(&["web", "static", "style.css"]);
    let rule = css_rule(&css, r#":root[data-hand="right"] .game-layout"#);
    let tile = re(r"(?s)--tile:(.*?);\s*--hand-w").captures(&rule).unwrap()[1].to_string();
    assert!(!tile.contains("--board-size") && !tile.contains("--hand-w"));
    let board = re(r"(?s)--board-size:(.*?);\s*$").captures(rule.trim()).unwrap()[1].to_string();
    assert!(board.contains("--hand-w"));
}

#[test]
fn the_right_layout_is_limited_to_wide_landscape_windows() {
    let css = read(&["web", "static", "style.css"]);
    let marker = r#":root[data-hand="right"] .game-layout"#;
    let at = css.find(marker).unwrap();
    let start = css[..at].rfind("@media").unwrap();
    let brace = css[start..].find('{').unwrap() + start;
    assert_eq!(css[start..brace].trim(), "@media (orientation: landscape) and (min-height: 541px) and (min-width: 820px)");
}

#[test]
fn the_selector_and_the_toolbar_button_exist() {
    let html = index_html();
    assert!(html.contains(r#"id="hand-position""#) && html.contains(r#"data-position="right""#));
    assert!(html.contains(r#"data-position="bottom""#) && html.contains(r#"id="btn-hand-position""#));
}

// ===================================================================================================
// Kétjegyű betűk: a kliens szabálya a szerverével egyezik
// ===================================================================================================

const DIGRAPH_HARNESS: &str = r#"
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const m = { exports: {} };
new Function('module', process.argv[1] + '\nmodule.exports = { formsDigraph };')(m);
const { formsDigraph } = m.exports;
console.log(JSON.stringify(input.pairs.map(([a, b]) => formsDigraph(a, b))));
"#;

const HUNT_TYPE_HARNESS: &str = r#"
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const m = { exports: {} };
new Function('module', process.argv[1] + '\nmodule.exports = { formsDigraph, huntType };')(m);
const { huntType } = m.exports;
const out = [];
for (const c of input.cases) {
    const ctx = { hunt: { finished: false, busy: false, rack: c.rack, built: [], pending: '' },
                  huntAdd(i) { this.hunt.built.push(i); } };
    for (const ch of c.typed) huntType.call(ctx, ch);
    out.push(ctx.hunt.built.map(i => ctx.hunt.rack[i]));
}
console.log(JSON.stringify(out));
"#;

fn digraph_code() -> String {
    block(&app_js(), "const DIGRAPH_TILES = new Set(", "\n}\n")
}

#[test]
fn the_digraph_rule_matches_the_server_for_every_pair_of_tiles() {
    require_node!();
    let mut all: Vec<String> = letters().map(|t| t.as_str().to_string()).collect();
    all.push("Y".to_string()); // önálló Y zseton nincs, de a szabály a GY / LY / NY / TY-ra is szól
    let firsts: Vec<String> = std::iter::once(String::new()).chain(all.iter().cloned()).collect();
    let pairs: Vec<(String, String)> = firsts.iter().flat_map(|a| all.iter().map(move |b| (a.clone(), b.clone()))).collect();
    let got = run_node(DIGRAPH_HARNESS, &[&digraph_code()], &json!({"pairs": pairs}));
    let expected: Vec<bool> = pairs.iter().map(|(a, b)| forms_digraph(a, b)).collect();
    assert_eq!(got, json!(expected));
    assert_eq!(expected.iter().filter(|b| **b).count(), 7);
}

#[test]
fn keyboard_typing_prefers_the_digraph_tile() {
    require_node!();
    let method = block(&app_js(), "    huntType(ch) {", "\n    },\n");
    let function = re(r"^\s*huntType\(ch\) \{").replace(&method, "function (ch) {").to_string();
    let code = format!("{}\nconst huntType = {};", digraph_code(), function.trim_end().trim_end_matches(','));
    let cases = json!([
        // S majd Z: a szabad SZ zseton az S helyére kerül
        {"rack": ["S", "Z", "SZ", "A", "K", "T", "L"], "typed": "SZ"},
        // nincs SZ zseton: külön S és Z kerül a sorba (a beküldéskor utasítja el a szabály)
        {"rack": ["S", "Z", "A", "K", "T", "L", "M"], "typed": "SZ"},
        // csak SZ zseton van: két billentyű is egy zseton
        {"rack": ["SZ", "A", "K", "T", "L", "M", "O"], "typed": "SZ"},
        // nem kétjegyű pár: nem történik csere
        {"rack": ["S", "A", "SZ", "K", "T", "L", "M"], "typed": "SA"},
    ]);
    let got = run_node(HUNT_TYPE_HARNESS, &[&code], &json!({"cases": cases}));
    assert_eq!(got, json!([["SZ"], ["S", "Z"], ["SZ"], ["S", "A"]]));
}

// ===================================================================================================
// A félkész lerakás nem marad a következő játék táblájára
// ===================================================================================================

const PLACEMENT_HARNESS: &str = r#"
const m = { exports: {} };
const body = process.argv[1];
const noop = new Proxy(function () {}, { get: () => noop, apply: () => undefined });
const el = { classList: { contains: () => false, add() {}, remove() {}, toggle() {} }, textContent: '' };
const stubs = {
    socket: { id: 'me' },
    Chat: { clear() {} }, Badges: { newInGame: [] }, Daily: { reset() {}, onGameState() {} },
    AsyncGames: { onGameState() {} }, Report: { sync() {} },
    Preview: { cleared: 0, clear() { this.cleared++; } },
    document: { getElementById: () => el },
    localStorage: { removeItem() {} },
    t: (k) => k, showScreen() {}, setGameRoomName() {},
    SoundManager: noop, ChallengeUI: noop, TurnTimerUI: noop, WaitingRoom: noop, GameOver: noop, BoardZoom: noop,
};
const names = Object.keys(stubs);
new Function('module', ...names, body + '\nmodule.exports = { AppState, BoardState, GameBoard, Preview };')(m, ...names.map(n => stubs[n]));
const { AppState, BoardState, GameBoard, Preview } = m.exports;
const placed = () => [{ row: 7, col: 7, letter: 'A', is_blank: false, handIdx: 0 }];
const noRender = ['renderBoard', 'renderHand', 'renderScoreboard', 'renderGameInfo', 'renderHistory', 'updateButtons',
                  'updateSpectatorUi', 'build', '_animateTurnChange'];
for (const name of noRender) GameBoard[name] = () => {};
const state = (id) => ({ game_id: id, started: true, finished: false, current_player: 'me', players: [] });
const out = {};

// kilépés a játékból (AppState.reset): a megmutatott megoldás / tipp nem marad a táblán
BoardState.placedTiles = placed();
BoardState.selectedTileIdx = 3;
BoardState.exchangeMode = true;
AppState.reset();
out.afterReset = { placed: BoardState.placedTiles.length, selected: BoardState.selectedTileIdx,
                   exchange: BoardState.exchangeMode, previewCleared: Preview.cleared };

// ugyanabban a játékban az újrarajzolás nem törli a lerakást
GameBoard.onGameState(state('g1'));
BoardState.placedTiles = placed();
GameBoard.onGameState(state('g1'));
out.sameGame = BoardState.placedTiles.length;

// új játék (új game_id): a saját körön sem marad ott a régi lerakás
GameBoard.onGameState(state('g2'));
out.newGame = BoardState.placedTiles.length;
console.log(JSON.stringify(out));
"#;

#[test]
fn a_pending_placement_does_not_leak_into_the_next_game() {
    require_node!();
    let js = app_js();
    let on_game_state = block(&js, "    onGameState(state) {\n        const prevPlayer", "\n    },\n");
    let code = [
        block(&js, "const AppState = {", "\n};\n"),
        block(&js, "const BoardState = {", "\n};\n"),
        format!(
            "const GameBoard = {{ _prevCurrentPlayer: null, _gameId: null, _resetAnimState() {{}},\n{}\n}};",
            on_game_state.trim_end().trim_end_matches(',')
        ),
    ]
    .join("\n");
    let result = run_node(PLACEMENT_HARNESS, &[&code], &json!({}));
    // kilépés a játékból: a félkész lerakás törlődik
    assert_eq!(result["afterReset"], json!({"placed": 0, "selected": null, "exchange": false, "previewCleared": 1}));
    // új játék: üres tábla; ugyanabban a játékban az újrarajzolás megtartja a lerakást
    assert_eq!(result["newGame"], 0);
    assert_eq!(result["sameGame"], 1);
}

// ===================================================================================================
// A betűtartó helye (HandLayout)
// ===================================================================================================

const HAND_LAYOUT_HARNESS: &str = r#"
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const run = (storage) => {
    const segments = ['right', 'bottom'].map(position => {
        const classes = new Set();
        return { dataset: { position }, classList: { toggle: (c, on) => (on ? classes.add(c) : classes.delete(c)), has: (c) => classes.has(c) },
                 attrs: {}, setAttribute(k, v) { this.attrs[k] = v; } };
    });
    const doc = { documentElement: { dataset: {} }, querySelectorAll: () => segments };
    const localStorage = storage;
    const m = { exports: {} };
    new Function('module', 'document', 'localStorage', process.argv[1] + '\nmodule.exports = { HandLayout };')(m, doc, localStorage);
    return { HandLayout: m.exports.HandLayout, doc, segments };
};
const memory = (initial) => { const data = { ...initial }; return { data, getItem: (k) => (k in data ? data[k] : null), setItem: (k, v) => { data[k] = String(v); } }; };
const broken = { getItem() { throw new Error('blocked'); }, setItem() { throw new Error('blocked'); } };
const out = {};

let r = run(memory({}));
out.defaultValue = r.HandLayout.get();
out.invalid = run(memory({ 'scrabble-hand-position': 'sideways' })).HandLayout.get();
out.storedBottom = run(memory({ 'scrabble-hand-position': 'bottom' })).HandLayout.get();

const store = memory({});
r = run(store);
r.HandLayout.apply();
out.applied = { data: r.doc.documentElement.dataset.hand, active: r.segments.map(s => s.classList.has('active')), checked: r.segments.map(s => s.attrs['aria-checked']) };
r.HandLayout.set('bottom');
out.afterSet = { data: r.doc.documentElement.dataset.hand, stored: store.data['scrabble-hand-position'], active: r.segments.map(s => s.classList.has('active')) };
r.HandLayout.toggle();
out.afterToggle = { data: r.doc.documentElement.dataset.hand, stored: store.data['scrabble-hand-position'] };
r.HandLayout.set('sideways');
out.afterInvalid = r.doc.documentElement.dataset.hand;

r = run(broken);                      // letiltott tároló (privát mód): marad az alapérték, és a munkamenetben váltható
out.broken = { initial: r.HandLayout.get() };
r.HandLayout.toggle();
out.broken.afterToggle = r.HandLayout.get();
out.broken.data = r.doc.documentElement.dataset.hand;
console.log(JSON.stringify(out));
"#;

#[test]
fn the_hand_layout_setting() {
    require_node!();
    let result = run_node(HAND_LAYOUT_HARNESS, &[&block(&app_js(), "const HandLayout = {", "\n};\n")], &json!({}));
    assert_eq!(result["defaultValue"], "right"); // alapértelmezés: a betűtartó a tábla jobb oldalán
    assert_eq!(result["storedBottom"], "bottom"); // a mentett választást használja,
    assert_eq!(result["invalid"], "right"); // az értelmetlent figyelmen kívül hagyja
    assert_eq!(result["applied"], json!({"data": "right", "active": [true, false], "checked": ["true", "false"]}));
    assert_eq!(result["afterSet"], json!({"data": "bottom", "stored": "bottom", "active": [false, true]}));
    assert_eq!(result["afterToggle"], json!({"data": "right", "stored": "right"}));
    assert_eq!(result["afterInvalid"], "right");
    // letiltott böngésző-tároló nélkül is működik
    assert_eq!(result["broken"], json!({"initial": "right", "afterToggle": "bottom", "data": "bottom"}));
}

// ===================================================================================================
// A gyakorló felület kliensoldali logikája
// ===================================================================================================

const PRACTICE_HARNESS: &str = r#"
const input = JSON.parse(require('fs').readFileSync(0, 'utf8'));
const store = {};
const localStorage = { getItem: (k) => (k in store ? store[k] : null), setItem: (k, v) => { store[k] = String(v); } };
const m = { exports: {} };
new Function('module', 'localStorage', process.argv[1] + '\nmodule.exports = { tokenizeWord, compareWords, localDateKey, PracticeStore };')(m, localStorage);
const { tokenizeWord, compareWords, localDateKey, PracticeStore } = m.exports;
const out = {};

out.tokens = input.words.map(w => tokenizeWord(w));
out.sorted = input.unsorted.slice().sort(compareWords);

const fresh = () => { PracticeStore._data = null; delete store[PracticeStore.KEY]; return PracticeStore; };
const day = (offset) => { const d = new Date(); d.setDate(d.getDate() + offset); return localDateKey(d); };

// sorozat
const streakOf = (offsets) => { const s = fresh(); s.get().days = offsets.map(day); return s.streak(); };
out.streak = {
    three: streakOf([0, -1, -2]), todayEmpty: streakOf([-1, -2]), gap: streakOf([0, -2]),
    old: streakOf([-2]), none: streakOf([]),
};

// mai számláló
let s = fresh();
s.touch(); s.touch();
out.today = { count: s.todayCount(), days: s.get().days.length };
s.get().today.date = '2000-01-01';
out.today.stale = s.todayCount();

// Hibáim pakli
s = fresh();
const deck = () => JSON.parse(JSON.stringify(s.get().missed));
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, false);
out.deck = { afterWrong: deck().ALMA };
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, true);
out.deck.afterOneRight = deck().ALMA;
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, false);              // újra téves: a számláló nullázódik
out.deck.afterRelapse = deck().ALMA;
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, true);
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, true);
out.deck.afterTwoRight = 'ALMA' in deck();
s.recordAnswer('KÖRTE', ['K', 'Ö', 'R', 'T', 'E'], true, true);        // helyes válasz nem kerül a pakliba
out.deck.rightNotAdded = 'KÖRTE' in deck();
out.quiz = { ...s.get().quiz, accuracy: s.accuracy() };
out.words = fresh().missedWords();

s = fresh();
for (let i = 0; i < 205; i++) s.recordAnswer('W' + i, ['A'], false, false);
out.cap = { size: Object.keys(s.get().missed).length, first: Object.keys(s.get().missed)[0], hasOldest: 'W0' in s.get().missed, hasNewest: 'W204' in s.get().missed };

// mentés + újratöltés, sérült tároló
s = fresh();
s.recordAnswer('ALMA', ['A', 'L', 'M', 'A'], true, false);
s.touch();
PracticeStore._data = null;
out.reload = { missed: Object.keys(PracticeStore.get().missed), today: PracticeStore.todayCount() };
PracticeStore._data = null; store[PracticeStore.KEY] = '{nem json';
out.corrupt = { missed: Object.keys(PracticeStore.get().missed).length, questions: PracticeStore.get().quiz.questions };
PracticeStore._data = null; store[PracticeStore.KEY] = JSON.stringify({ quiz: { questions: 4, correct: 3, best_streak: 2 } });
out.partial = { accuracy: PracticeStore.accuracy(), hasMissed: typeof PracticeStore.get().missed, hasHunt: typeof PracticeStore.get().hunt };
console.log(JSON.stringify(out));
"#;

fn practice_result() -> (Value, Vec<String>) {
    scrabble::dictionary::warm_up();
    let stems = scrabble::practice::stem_words();
    // a szótő-lista egyenletesen mintavételezve (determinisztikus), a rövid szavak és a nehéz esetek mind
    let step = (stems.len() / 1500).max(1);
    let mut words: Vec<String> = stems.iter().step_by(step).take(1500).cloned().collect();
    for length in [2, 3] {
        words.extend(scrabble::practice::short_words(length).iter().map(|e| e["word"].as_str().unwrap().to_string()));
    }
    words.extend(["SZÓ", "ASZTAL", "GYÓGYSZER", "NYELV", "LYUK", "TYÚK", "ZSÍR", "CSÓK", "szó", "XYZ", "QWERTY", ""].iter().map(|s| s.to_string()));
    let unsorted = [
        "ZSÍR", "SZÓ", "SÁR", "CSÓK", "CIKK", "ÓRA", "ŐZ", "ÖN", "OLAJ", "ÁBRA", "ABA", "DÉL", "GYŰRŰ", "GÉP", "NYÁR", "NAP", "TYÚK", "TÁL", "LYUK", "LÁNC",
        "ÜVEG", "ÚT", "ŰR", "UTCA",
    ];
    let js = app_js();
    let code = [
        block(&js, "const TILE_VALUES = {", "\n};\n"),
        block(&js, "const HU_ALPHABET = [", "];\n"),
        block(&js, "const HU_ORDER =", ";\n"),
        block(&js, "function tokenizeWord(word) {", "\n}\n"),
        block(&js, "function compareWords(a, b) {", "\n}\n"),
        block(&js, "function localDateKey(", "\n}\n"),
        block(&js, "const PracticeStore = {", "\n};\n"),
    ]
    .join("\n");
    (run_node(PRACTICE_HARNESS, &[&code], &json!({"words": words, "unsorted": unsorted})), words)
}

static PRACTICE: std::sync::OnceLock<(Value, Vec<String>)> = std::sync::OnceLock::new();

fn practice() -> &'static (Value, Vec<String>) {
    PRACTICE.get_or_init(practice_result)
}

fn server_tokens(word: &str) -> Value {
    match tokenize_word(word) {
        None => Value::Null,
        Some(tiles) => json!(tiles.iter().map(|t: &Tile| t.as_str()).collect::<Vec<_>>()),
    }
}

#[test]
fn the_tokenizer_matches_the_server_for_the_dictionary() {
    require_node!();
    let (result, words) = practice();
    let tokens = result["tokens"].as_array().unwrap();
    assert_eq!(tokens.len(), words.len());
    let mismatches: Vec<(&String, &Value, Value)> =
        words.iter().zip(tokens).filter(|(w, t)| **t != server_tokens(w)).map(|(w, t)| (w, t, server_tokens(w))).take(5).collect();
    assert!(mismatches.is_empty(), "{mismatches:?}");
    // a kétjegyű betűk és a kirakhatatlan betűk
    let by_word: BTreeMap<&String, &Value> = words.iter().zip(tokens).collect();
    let word = |w: &str| by_word[&w.to_string()].clone();
    assert_eq!(word("SZÓ"), json!(["SZ", "Ó"]));
    assert_eq!(word("GYÓGYSZER"), json!(["GY", "Ó", "GY", "SZ", "E", "R"]));
    assert_eq!(word("szó"), json!(["SZ", "Ó"]));
    assert_eq!((word("XYZ"), word("QWERTY"), word("")), (Value::Null, Value::Null, json!([])));
}

#[test]
fn the_hungarian_alphabet_order() {
    require_node!();
    assert_eq!(
        practice().0["sorted"],
        json!([
            "ABA", "ÁBRA", "CIKK", "CSÓK", "DÉL", "GÉP", "GYŰRŰ", "LÁNC", "LYUK", "NAP", "NYÁR", "OLAJ", "ÓRA", "ÖN", "ŐZ", "SÁR", "SZÓ", "TÁL", "TYÚK",
            "UTCA", "ÚT", "ÜVEG", "ŰR", "ZSÍR"
        ])
    );
}

#[test]
fn the_streak_and_the_daily_counter() {
    require_node!();
    let result = &practice().0;
    let s = &result["streak"];
    assert_eq!(s["three"], 3);
    assert_eq!(s["todayEmpty"], 2); // a ma még üres nap nem szakítja meg a tegnapig tartó sorozatot
    assert_eq!(s["gap"], 1); // a kihagyott nap megszakítja
    assert_eq!((s["old"].clone(), s["none"].clone()), (json!(0), json!(0)));
    assert_eq!(result["today"], json!({"count": 2, "days": 1, "stale": 0}));
}

#[test]
fn the_mistakes_deck() {
    require_node!();
    let result = &practice().0;
    let deck = &result["deck"];
    assert_eq!(deck["afterWrong"], json!({"tiles": ["A", "L", "M", "A"], "valid": true, "ok": 0}));
    assert_eq!(deck["afterOneRight"]["ok"], 1);
    assert_eq!(deck["afterTwoRight"], false); // két helyes válasz egymás után kiveszi a szót
    assert_eq!(deck["afterRelapse"]["ok"], 0); // egy újabb tévedés nullázza a számlálót
    assert_eq!(deck["rightNotAdded"], false); // helyes válasz nem kerül a pakliba
    assert_eq!((result["quiz"]["questions"].clone(), result["quiz"]["correct"].clone(), result["quiz"]["accuracy"].clone()), (json!(6), json!(4), json!(67)));
    let cap = &result["cap"];
    assert_eq!((cap["size"].clone(), cap["hasOldest"].clone(), cap["hasNewest"].clone()), (json!(200), json!(false), json!(true)));
    assert_eq!(result["words"], json!([]));
}

#[test]
fn the_practice_store_persistence() {
    require_node!();
    let result = &practice().0;
    assert_eq!(result["reload"], json!({"missed": ["ALMA"], "today": 1}));
    assert_eq!(result["corrupt"], json!({"missed": 0, "questions": 0})); // sérült tároló: alapértékek
    assert_eq!(result["partial"], json!({"accuracy": 75, "hasMissed": "object", "hasHunt": "object"})); // részleges adat kiegészül
}
