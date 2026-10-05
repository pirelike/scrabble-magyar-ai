//! Admin panel: a konzolra írt üzenetek gyűjtése a naplónézethez (a Python `test_print_output_is_captured`
//! megfelelője). A gyűjtés a folyamat stdout / stderr fájlleíróit egy csőre irányítja át, ezért külön binárisban fut.

mod common;
use common::*;
use std::io::Write;

#[tokio::test(flavor = "multi_thread")]
async fn print_output_is_captured() {
    let server = TestServer::start().await;
    let api = server.admin_http().await;
    scrabble::admin::system::install_log_capture(&server.app);
    // közvetlenül a fájlleíróra írunk: a `println!` a tesztkeretrendszer saját pufferébe menne
    let mut out = std::io::stdout();
    writeln!(out, "[bot] Hiba a lépés keresésében: teszt-kimenet").unwrap();
    writeln!(out, "FIGYELEM: teszt-figyelmeztetés").unwrap();
    writeln!(out, "szokásos tájékoztató teszt-sor").unwrap();
    out.flush().unwrap();
    let mut rows = std::collections::HashMap::new();
    for _ in 0..100 {
        let data = api.admin_get("/system/logs?q=teszt-").await.json();
        rows = data["items"].as_array().unwrap().iter().map(|i| (i["message"].as_str().unwrap().to_string(), i["level"].as_str().unwrap().to_string())).collect();
        if rows.len() >= 2 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(rows["[bot] Hiba a lépés keresésében: teszt-kimenet"], "ERROR");
    assert_eq!(rows["FIGYELEM: teszt-figyelmeztetés"], "WARNING");
    scrabble::admin::system::restore_output();
}
