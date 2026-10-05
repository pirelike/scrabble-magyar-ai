//! Admin panel: a napló (audit) — csak hozzáfűzhető, a művelettel egy tranzakcióban íródik, szűrhető és exportálható
//! (a Python `test_admin_audit.py` megfelelője).

mod common;
use common::admin::*;
use common::*;
use scrabble::admin::audit::{AuditFilters, audit_csv, query_audit};
use scrabble::admin::{self, AdminContext, AdminError, csv_cell};
use serde_json::{Value, json};

struct Fixture {
    server: TestServer,
    admin_id: i64,
    ctx: AdminContext,
}

async fn fixture() -> Fixture {
    let server = TestServer::start().await;
    let admin_id = server.create_user(ADMIN_EMAIL, "Főnök");
    let ctx = AdminContext { admin_user_id: admin_id, ip: "203.0.113.7".into(), user_agent: "Teszt-Böngésző/1.0".into() };
    Fixture { server, admin_id, ctx }
}

fn rows(server: &TestServer) -> Vec<Value> {
    query_audit(&server.app.db, &AuditFilters::default(), Some("200"), None, admin::AUDIT_MAX_LIMIT).unwrap()["items"].as_array().unwrap().clone()
}

fn log(f: &Fixture, action: &str, target_type: &str, target_id: &str, reason: &str, details: Value) {
    log_as(&f.server, &f.ctx, action, target_type, target_id, reason, details);
}

fn log_as(server: &TestServer, ctx: &AdminContext, action: &str, target_type: &str, target_id: &str, reason: &str, details: Value) {
    admin::action(&server.app.db, ctx, action, Some(target_type), Some(target_id.to_string()), Some(&json!(reason)), details, true, |_| Ok(())).unwrap();
}

fn simple_log(f: &Fixture) {
    log(f, "user.ban", "user", "1", "Teszt ok", json!({}));
}

fn total(server: &TestServer, set: impl FnOnce(&mut AuditFilters)) -> i64 {
    let mut filters = AuditFilters::default();
    set(&mut filters);
    query_audit(&server.app.db, &filters, None, None, admin::AUDIT_MAX_LIMIT).unwrap()["total"].as_i64().unwrap()
}

fn query_error(server: &TestServer, set: impl FnOnce(&mut AuditFilters)) -> AdminError {
    let mut filters = AuditFilters::default();
    set(&mut filters);
    query_audit(&server.app.db, &filters, None, None, admin::AUDIT_MAX_LIMIT).unwrap_err()
}

fn exec(server: &TestServer, sql: &str) -> rusqlite::Result<()> {
    server.app.db.with(|tx| tx.execute_batch(sql))
}

fn display_name(server: &TestServer, id: i64) -> String {
    server.app.db.get_user_by_id(id).unwrap().unwrap().display_name
}

// ===================================================================================================
// Csak hozzáfűzhető
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn rows_cannot_be_updated() {
    let f = fixture().await;
    simple_log(&f);
    assert!(exec(&f.server, "UPDATE admin_audit SET action = 'x'").is_err());
    assert_eq!(rows(&f.server)[0]["action"], "user.ban");
}

#[tokio::test(flavor = "multi_thread")]
async fn rows_cannot_be_deleted() {
    let f = fixture().await;
    simple_log(&f);
    assert!(exec(&f.server, "DELETE FROM admin_audit").is_err());
    assert_eq!(rows(&f.server).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_log_survives_deleting_the_admin_account() {
    let f = fixture().await;
    simple_log(&f);
    exec(&f.server, &format!("DELETE FROM users WHERE id = {}", f.admin_id)).unwrap();
    let list = rows(&f.server);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["admin_user_id"], f.admin_id);
    assert!(list[0]["admin_name"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_protection_is_in_place_after_a_reinitialisation() {
    let f = fixture().await;
    f.server.app.db.init().unwrap();
    simple_log(&f);
    assert!(exec(&f.server, "DELETE FROM admin_audit").is_err());
}

// ===================================================================================================
// Műveletek naplózása
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn one_row_with_everything() {
    let f = fixture().await;
    admin::action(&f.server.app.db, &f.ctx, "user.ban", Some("user"), Some("42".into()), Some(&json!("  Sértő üzenetek  ")), json!({"until": "2030-01-01"}), true, |act| {
        act.details.insert("before".into(), json!({"banned": false}));
        act.details.insert("after".into(), json!({"banned": true}));
        Ok(())
    })
    .unwrap();
    let list = rows(&f.server);
    assert_eq!(list.len(), 1);
    let row = &list[0];
    assert_eq!(row["action"], "user.ban");
    assert_eq!((row["target_type"].clone(), row["target_id"].clone()), (json!("user"), json!("42")));
    assert_eq!((row["admin_user_id"].clone(), row["admin_name"].clone()), (json!(f.admin_id), json!("Főnök")));
    assert_eq!((row["ip"].clone(), row["user_agent"].clone()), (json!("203.0.113.7"), json!("Teszt-Böngésző/1.0")));
    assert_eq!(row["reason"], "Sértő üzenetek");
    assert_eq!(row["details"], json!({"reason": "Sértő üzenetek", "until": "2030-01-01", "before": {"banned": false}, "after": {"banned": true}}));
    assert!(!row["created_at"].as_str().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reason_cannot_be_overridden_by_details() {
    let f = fixture().await;
    log(&f, "user.ban", "user", "1", "Valódi ok", json!({"reason": "Hamis ok"}));
    assert_eq!(rows(&f.server)[0]["reason"], "Valódi ok");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_operation_and_the_log_share_one_transaction() {
    let f = fixture().await;
    admin::action(&f.server.app.db, &f.ctx, "user.rename", Some("user"), Some(f.admin_id.to_string()), Some(&json!("Névjavítás")), json!({}), true, |act| {
        act.tx.execute("UPDATE users SET display_name = ? WHERE id = ?", rusqlite::params!["Új név", f.admin_id])?;
        Ok(())
    })
    .unwrap();
    assert_eq!(display_name(&f.server, f.admin_id), "Új név");
    assert_eq!(rows(&f.server).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_log_write_rolls_the_operation_back() {
    let f = fixture().await;
    exec(&f.server, "ALTER TABLE admin_audit RENAME TO admin_audit_elmozgatva").unwrap();
    let result = admin::action(&f.server.app.db, &f.ctx, "user.rename", Some("user"), Some(f.admin_id.to_string()), Some(&json!("Névjavítás")), json!({}), true, |act| {
        act.tx.execute("UPDATE users SET display_name = ? WHERE id = ?", rusqlite::params!["Új név", f.admin_id])?;
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(display_name(&f.server, f.admin_id), "Főnök");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_operation_leaves_no_log_row() {
    let f = fixture().await;
    let result: Result<(), AdminError> = admin::action(&f.server.app.db, &f.ctx, "user.rename", Some("user"), Some(f.admin_id.to_string()), Some(&json!("Névjavítás")), json!({}), true, |act| {
        act.tx.execute("UPDATE users SET display_name = ? WHERE id = ?", rusqlite::params!["Új név", f.admin_id])?;
        Err(AdminError::new("menet közben hiba", 500))
    });
    assert!(result.is_err());
    assert_eq!(display_name(&f.server, f.admin_id), "Főnök");
    assert!(rows(&f.server).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reason_is_mandatory_and_the_operation_never_starts() {
    let f = fixture().await;
    let too_long = "x".repeat(501);
    let reasons = [None, Some(json!("")), Some(json!("   ")), Some(json!("ab")), Some(json!(5)), Some(json!(["x"])), Some(json!(too_long))];
    for reason in reasons {
        let mut entered = false;
        let result = admin::action(&f.server.app.db, &f.ctx, "user.ban", Some("user"), Some("1".into()), reason.as_ref(), json!({}), true, |_| {
            entered = true;
            Ok(())
        });
        let error = result.expect_err(&format!("{reason:?}"));
        assert_eq!(error.status, 400, "{reason:?}");
        assert!(!entered, "{reason:?}");
    }
    assert!(rows(&f.server).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_optional_reason_is_still_checked_when_present() {
    let f = fixture().await;
    let ok = admin::action(&f.server.app.db, &f.ctx, "announcement.create", None, None, None, json!({}), false, |_| Ok(()));
    assert!(ok.is_ok());
    let bad = admin::action(&f.server.app.db, &f.ctx, "announcement.create", None, None, Some(&json!("ab")), json!({}), false, |_| Ok(()));
    assert_eq!(bad.unwrap_err().status, 400);
    assert_eq!(rows(&f.server).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn view_events_need_no_reason() {
    let f = fixture().await;
    f.server.app.db.with(|tx| admin::record(tx, &f.ctx, "view.user", Some("user"), Some("7"), &Value::Null).map(|_| ())).unwrap();
    let row = rows(&f.server).remove(0);
    assert_eq!(row["action"], "view.user");
    assert!(row["reason"].is_null() && row["details"].is_null());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_long_user_agent_is_truncated() {
    let f = fixture().await;
    let long = AdminContext { admin_user_id: f.admin_id, ip: "1.2.3.4".into(), user_agent: "A".repeat(1000) };
    log_as(&f.server, &long, "user.ban", "user", "1", "Teszt ok", json!({}));
    assert_eq!(rows(&f.server)[0]["user_agent"].as_str().unwrap().len(), admin::MAX_USER_AGENT_LEN);
}

#[tokio::test(flavor = "multi_thread")]
async fn structured_values_in_details_are_kept() {
    let f = fixture().await;
    log(&f, "user.ban", "user", "1", "Teszt ok", json!({"when": "2030-01-02 03:04:05", "list": [1, 2], "nested": {"k": "é"}}));
    let details = rows(&f.server)[0]["details"].clone();
    assert_eq!(details["when"], "2030-01-02 03:04:05");
    assert_eq!(details["list"], json!([1, 2]));
    assert_eq!(details["nested"]["k"], "é");
}

// ===================================================================================================
// Lekérdezés
// ===================================================================================================

struct Sample {
    f: Fixture,
    other_id: i64,
}

async fn sample() -> Sample {
    let f = fixture().await;
    let other_id = f.server.create_user("masodik@example.com", "Moderátor");
    let other = AdminContext { admin_user_id: other_id, ip: "198.51.100.5".into(), user_agent: "Másik".into() };
    log(&f, "user.ban", "user", "5", "Spam", json!({}));
    log(&f, "user.unban", "user", "5", "Tévedés volt", json!({}));
    log(&f, "room.disband", "room", "ABC123", "Elakadt szoba", json!({}));
    log_as(&f.server, &other, "dict.reject_add", "word", "xyzzy", "100% biztos és_más", json!({}));
    log_as(&f.server, &other, "users.export", "user", "6", "Adatkérés", json!({}));
    Sample { f, other_id }
}

#[tokio::test(flavor = "multi_thread")]
async fn newest_first_and_total() {
    let s = sample().await;
    let result = query_audit(&s.f.server.app.db, &AuditFilters::default(), None, None, 200).unwrap();
    assert_eq!(result["total"], 5);
    let actions: Vec<&str> = result["items"].as_array().unwrap().iter().map(|r| r["action"].as_str().unwrap()).collect();
    assert_eq!(actions, vec!["users.export", "dict.reject_add", "room.disband", "user.unban", "user.ban"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn filter_by_admin_id_name_and_email() {
    let s = sample().await;
    let server = &s.f.server;
    let by_id = {
        let mut filters = AuditFilters::default();
        filters.admin = Some(s.other_id.to_string());
        query_audit(&server.app.db, &filters, None, None, 200).unwrap()
    };
    let mut actions: Vec<&str> = by_id["items"].as_array().unwrap().iter().map(|r| r["action"].as_str().unwrap()).collect();
    actions.sort();
    assert_eq!(actions, vec!["dict.reject_add", "users.export"]);
    assert_eq!(total(server, |f| f.admin = Some("moder".into())), 2);
    assert_eq!(total(server, |f| f.admin = Some("ADMIN@".into())), 3);
    assert_eq!(total(server, |f| f.admin = Some("nincs ilyen".into())), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn filter_by_action_prefix() {
    let s = sample().await;
    let server = &s.f.server;
    assert_eq!(total(server, |f| f.action = Some("user.".into())), 2); // a `users.export` nem illeszkedik
    assert_eq!(total(server, |f| f.action = Some("user".into())), 3);
    assert_eq!(total(server, |f| f.action = Some("room.disband".into())), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn like_wildcards_in_filters_are_literal() {
    let s = sample().await;
    let server = &s.f.server;
    assert_eq!(total(server, |f| f.action = Some("%".into())), 0);
    assert_eq!(total(server, |f| f.action = Some("user_ban".into())), 0);
    assert_eq!(total(server, |f| f.q = Some("%".into())), 1); // csak a „100%” indoklás
    assert_eq!(total(server, |f| f.q = Some("és_más".into())), 1);
    assert_eq!(total(server, |f| f.q = Some("_".into())), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn filter_by_target() {
    let s = sample().await;
    let server = &s.f.server;
    assert_eq!(total(server, |f| f.target_type = Some("user".into())), 3);
    assert_eq!(total(server, |f| {
        f.target_type = Some("user".into());
        f.target_id = Some("5".into());
    }), 2);
    assert_eq!(total(server, |f| f.target_id = Some("ABC123".into())), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn free_text_search() {
    let s = sample().await;
    let server = &s.f.server;
    assert_eq!(total(server, |f| f.q = Some("spam".into())), 1); // az indoklásban
    assert_eq!(total(server, |f| f.q = Some("198.51.100".into())), 2); // az IP-ben
    assert_eq!(total(server, |f| f.q = Some("xyzzy".into())), 1); // a célpontban
}

#[tokio::test(flavor = "multi_thread")]
async fn date_filters() {
    let s = sample().await;
    let server = &s.f.server;
    let today = scrabble::util::now_ts()[..10].to_string();
    assert_eq!(total(server, |f| f.since = Some(today.clone())), 5);
    assert_eq!(total(server, |f| f.until = Some(today.clone())), 5); // a végnap egésze benne van
    assert_eq!(total(server, |f| f.since = Some("2999-01-01".into())), 0);
    assert_eq!(total(server, |f| f.until = Some("2000-01-01".into())), 0);
    assert_eq!(total(server, |f| {
        f.since = Some(format!("{today} 00:00"));
        f.until = Some(format!("{today} 23:59:59"));
    }), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn combined_filters() {
    let s = sample().await;
    assert_eq!(total(&s.f.server, |f| {
        f.admin = Some("admin".into());
        f.action = Some("user.".into());
        f.q = Some("spam".into());
    }), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn paging() {
    let s = sample().await;
    let db = &s.f.server.app.db;
    let page = |offset: &str| query_audit(db, &AuditFilters::default(), Some("2"), Some(offset), 200).unwrap();
    let (first, second, third) = (page("0"), page("2"), page("4"));
    let lens: Vec<usize> = [&first, &second, &third].iter().map(|p| p["items"].as_array().unwrap().len()).collect();
    assert_eq!(lens, vec![2, 2, 1]);
    assert!([&first, &second, &third].iter().all(|p| p["total"] == 5));
    let ids: Vec<i64> = [&first, &second, &third].iter().flat_map(|p| p["items"].as_array().unwrap().iter().map(|r| r["id"].as_i64().unwrap())).collect();
    let mut sorted = ids.clone();
    sorted.sort_by(|a, b| b.cmp(a));
    assert_eq!(ids, sorted);
    assert_eq!(ids.iter().collect::<std::collections::HashSet<_>>().len(), 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_are_clamped() {
    let s = sample().await;
    let db = &s.f.server.app.db;
    let query = |limit: Option<&str>, offset: Option<&str>, max: i64| query_audit(db, &AuditFilters::default(), limit, offset, max).unwrap();
    assert_eq!(query(Some("1000000"), None, admin::AUDIT_MAX_LIMIT)["limit"], admin::AUDIT_MAX_LIMIT);
    assert_eq!(query(Some("0"), None, admin::AUDIT_MAX_LIMIT)["limit"], 1);
    assert_eq!(query(Some("szemét"), None, admin::AUDIT_MAX_LIMIT)["limit"], admin::AUDIT_DEFAULT_LIMIT);
    assert_eq!(query(None, Some("-5"), admin::AUDIT_MAX_LIMIT)["offset"], 0);
    assert_eq!(query(Some("1000000"), None, admin::AUDIT_EXPORT_MAX)["limit"], admin::AUDIT_EXPORT_MAX);
}

#[tokio::test(flavor = "multi_thread")]
async fn invalid_filters_are_a_400() {
    let s = sample().await;
    let server = &s.f.server;
    assert_eq!(query_error(server, |f| f.since = Some("tegnap".into())).status, 400);
    assert_eq!(query_error(server, |f| f.until = Some("2030-13-45".into())).status, 400);
    assert_eq!(query_error(server, |f| f.action = Some("x".repeat(101))).status, 400);
}

// ===================================================================================================
// CSV
// ===================================================================================================

/// RFC 4180 szerinti CSV-olvasó (idézőjelek, mezőn belüli vessző és sortörés).
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cell.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
        } else {
            match c {
                '"' => quoted = true,
                ',' => row.push(std::mem::take(&mut cell)),
                '\r' => {}
                '\n' => {
                    row.push(std::mem::take(&mut cell));
                    rows.push(std::mem::take(&mut row));
                }
                other => cell.push(other),
            }
        }
    }
    if !cell.is_empty() || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

fn csv_record(server: &TestServer) -> std::collections::HashMap<String, String> {
    let table = parse_csv(&audit_csv(&rows(server)));
    table[0].iter().cloned().zip(table[1].iter().cloned()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn csv_roundtrip_with_special_characters() {
    let f = fixture().await;
    log(&f, "user.note", "user", "1", "Vessző, \"idézőjel\"\nés új sor", json!({"extra": {"k": "é"}}));
    let table = parse_csv(&audit_csv(&rows(&f.server)));
    assert_eq!(table[0], admin::AUDIT_CSV_FIELDS.iter().map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(table.len(), 2);
    let record: std::collections::HashMap<_, _> = table[0].iter().cloned().zip(table[1].iter().cloned()).collect();
    assert_eq!(record["reason"], "Vessző, \"idézőjel\"\nés új sor");
    assert_eq!(record["admin_name"], "Főnök");
    let details: Value = serde_json::from_str(&record["details_json"]).unwrap();
    assert_eq!(details["extra"], json!({"k": "é"}));
}

#[tokio::test(flavor = "multi_thread")]
async fn formula_injection_is_neutralised() {
    for value in ["=HYPERLINK(\"http://x\")", "+1+1", "-2", "@SUM(A1)"] {
        let f = fixture().await;
        log(&f, "user.ban", "user", "1", &format!("{value} ok"), json!({}));
        assert!(csv_record(&f.server)["reason"].starts_with('\''), "{value}");
    }
}

#[test]
fn the_cell_helper_covers_tab_and_carriage_return_too() {
    assert_eq!(csv_cell(&json!("\tx")), "'\tx");
    assert_eq!(csv_cell(&json!("\rx")), "'\rx");
    assert_eq!(csv_cell(&Value::Null), "");
    assert_eq!(csv_cell(&json!(5)), "5");
}

#[tokio::test(flavor = "multi_thread")]
async fn the_user_agent_is_neutralised_too() {
    let f = fixture().await;
    let ctx = AdminContext { admin_user_id: f.admin_id, ip: "1.1.1.1".into(), user_agent: "=cmd|calc".into() };
    log_as(&f.server, &ctx, "user.ban", "user", "1", "Teszt ok", json!({}));
    assert_eq!(csv_record(&f.server)["user_agent"], "'=cmd|calc");
}

// ===================================================================================================
// A végpont
// ===================================================================================================

#[tokio::test(flavor = "multi_thread")]
async fn the_list() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let admin_id = server.user_id(ADMIN_EMAIL);
    let ctx = AdminContext { admin_user_id: admin_id, ip: "1.1.1.1".into(), user_agent: "t".into() };
    log_as(&server, &ctx, "user.ban", "user", "5", "Spam", json!({}));
    log_as(&server, &ctx, "room.disband", "room", "ABC123", "Elakadt", json!({}));
    let data = http.get("/api/admin/audit").await.json();
    assert_eq!(data["success"], true);
    assert_eq!(data["total"], 2);
    assert_eq!(data["items"][0]["action"], "room.disband");
    assert_eq!(data["items"][0]["details"]["reason"], "Elakadt");
}

#[tokio::test(flavor = "multi_thread")]
async fn filters_and_paging_come_from_the_query_string() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let ctx = AdminContext { admin_user_id: server.user_id(ADMIN_EMAIL), ip: "1.1.1.1".into(), user_agent: "t".into() };
    for number in 0..7 {
        log_as(&server, &ctx, "user.ban", "user", &number.to_string(), &format!("Ok {number}"), json!({}));
    }
    log_as(&server, &ctx, "room.disband", "room", "ABC123", "Elakadt", json!({}));
    let data = http.get("/api/admin/audit?action=user.&limit=3&offset=3").await.json();
    assert_eq!(data["total"], 7);
    assert_eq!(data["items"].as_array().unwrap().len(), 3);
    assert!(data["items"].as_array().unwrap().iter().all(|i| i["action"] == "user.ban"));
    assert_eq!(http.get("/api/admin/audit?q=Elakadt").await.json()["total"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_list_is_capped_at_the_page_maximum() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    assert_eq!(http.get("/api/admin/audit?limit=100000").await.json()["limit"], admin::AUDIT_MAX_LIMIT);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_bad_filter_is_a_400() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let response = http.get("/api/admin/audit?since=tegnap").await;
    assert_eq!(response.status, 400);
    assert_eq!(response.json()["success"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_unknown_format_is_a_400() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    assert_eq!(http.get("/api/admin/audit?format=xml").await.status, 400);
}

#[tokio::test(flavor = "multi_thread")]
async fn listing_is_not_logged() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let ctx = AdminContext { admin_user_id: server.user_id(ADMIN_EMAIL), ip: "1.1.1.1".into(), user_agent: "t".into() };
    log_as(&server, &ctx, "user.ban", "user", "1", "Teszt ok", json!({}));
    http.get("/api/admin/audit").await;
    http.get("/api/admin/audit?action=user.").await;
    assert_eq!(audit_rows(&server).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_csv_export_is_a_download_and_is_logged() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let ctx = AdminContext { admin_user_id: server.user_id(ADMIN_EMAIL), ip: "1.1.1.1".into(), user_agent: "t".into() };
    log_as(&server, &ctx, "user.ban", "user", "5", "Spam", json!({}));
    let response = http.get("/api/admin/audit?format=csv&action=user.").await;
    assert_eq!(response.status, 200);
    assert!(response.header("Content-Type").unwrap().starts_with("text/csv"));
    assert!(response.header("Content-Disposition").unwrap().contains("attachment"));
    let table = parse_csv(&response.text());
    assert_eq!(table.len(), 2);
    let action_column = table[0].iter().position(|c| c == "action").unwrap();
    assert_eq!(table[1][action_column], "user.ban");
    let exports: Vec<Value> = audit_rows(&server).into_iter().filter(|r| r["action"].as_str().unwrap().starts_with("view.")).collect();
    assert_eq!(exports.len(), 1);
    assert_eq!(exports[0]["details"]["format"], "csv");
    assert_eq!(exports[0]["details"]["rows"], 1);
    assert_eq!(exports[0]["details"]["filters"], json!({"action": "user."}));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_json_export_is_a_download_and_is_logged() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let ctx = AdminContext { admin_user_id: server.user_id(ADMIN_EMAIL), ip: "1.1.1.1".into(), user_agent: "t".into() };
    log_as(&server, &ctx, "user.ban", "user", "1", "Teszt ok", json!({}));
    let response = http.get("/api/admin/audit?download=1").await;
    assert!(response.header("Content-Disposition").unwrap().contains("attachment"));
    assert_eq!(response.json()["total"], 1);
    let formats: Vec<Value> = audit_rows(&server).into_iter().filter(|r| r["action"].as_str().unwrap().starts_with("view.")).map(|r| r["details"]["format"].clone()).collect();
    assert_eq!(formats, vec![json!("json")]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_export_ignores_the_page_size_of_the_listing() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let ctx = AdminContext { admin_user_id: server.user_id(ADMIN_EMAIL), ip: "1.1.1.1".into(), user_agent: "t".into() };
    let count = admin::AUDIT_MAX_LIMIT + 5;
    server
        .app
        .db
        .with(|tx| {
            for number in 0..count {
                admin::record(tx, &ctx, "user.ban", Some("user"), Some(&number.to_string()), &json!({"reason": "Teszt ok"}))?;
            }
            Ok(())
        })
        .unwrap();
    let data = http.get("/api/admin/audit?download=1").await.json();
    assert_eq!(data["items"].as_array().unwrap().len() as i64, count);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_log_cannot_be_changed_through_the_api() {
    let server = TestServer::start().await;
    let http = server.admin_http().await;
    let ctx = AdminContext { admin_user_id: server.user_id(ADMIN_EMAIL), ip: "1.1.1.1".into(), user_agent: "t".into() };
    log_as(&server, &ctx, "user.ban", "user", "1", "Teszt ok", json!({}));
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        assert_eq!(http.admin(method, "/audit", Some(json!({}))).await.status, 405, "{method}");
    }
    for method in ["PUT", "PATCH", "DELETE"] {
        assert_eq!(http.admin(method, "/audit/1", Some(json!({}))).await.status, 404, "{method}");
    }
    assert_eq!(audit_rows(&server).len(), 1);
}
