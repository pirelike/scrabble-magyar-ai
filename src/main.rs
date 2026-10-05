//! Magyar Scrabble szerver: `scrabble` (publikus Cloudflare tunnel-lel) vagy `scrabble --no-tunnel` (csak helyi hálózat).

use scrabble::app::App;
use scrabble::db::Db;
use scrabble::{admin, ai, config, daily, dictionary, practice, server};
use std::sync::Arc;

#[tokio::main]
async fn main() {
    let use_tunnel = !std::env::args().any(|a| a == "--no-tunnel");
    let config = Arc::new(config::get().clone());
    let db = match Db::open(&config.db_path).and_then(|db| db.init().map(|_| db)) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("Az adatbázis nem nyitható meg ({}): {e}", config.db_path);
            std::process::exit(1);
        }
    };
    let app = App::new(config.clone(), db.clone());
    admin::system::install_log_capture(&app);

    // Indulási teendők (mint a Python `__main__`)
    db.cleanup_expired();
    server::core::cleanup_finished_saves(&app);
    // a szótár betöltése indításkor (az első lerakásnál ne kelljen várni)
    tokio::task::spawn_blocking(|| {
        dictionary::warm_up();
    })
    .await
    .ok();
    app.apply_runtime_settings(); // beállítások (forgalomkorlátok...) + a szótár-építőn elutasított szavak kizárása
    // a robot szókincse (a ragozott alakokkal) és a gyakorló módok listái is előre épüljenek fel
    tokio::task::spawn_blocking(|| {
        ai::get_vocabulary();
        practice::warm_up();
    })
    .await
    .ok();
    {
        let db = db.clone();
        let result = tokio::task::spawn_blocking(move || daily::ensure_puzzle(&db, None, &ai::get_vocabulary())).await;
        match result {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => println!("[daily] A mai feladvány előállítása nem sikerült: {e}"),
            Err(e) => println!("[daily] A mai feladvány előállítása nem sikerült: {e}"),
        }
    }
    println!("[async] {} levelezős játék visszaállítva", server::core::restore_async_games(&app));
    server::spawn_background_tasks(&app);
    if !config.admin_emails.is_empty() {
        // csak a szerver konzoljára: a webes felület semmit sem árul el
        println!(
            "[admin] Admin panel bekapcsolva ({} admin cím{})",
            config.admin_emails.len(),
            if config.admin_ip_allowlist.is_empty() { "" } else { ", IP-lista aktív" }
        );
        if !app.mailer.is_configured() {
            println!("[admin] Figyelem: SMTP nincs beállítva — az admin cím regisztrációs kódja csak ezen a konzolon jelenik meg.");
        }
    }
    if use_tunnel {
        app.tunnel.start(config.port);
    }
    let result = server::serve(app.clone(), config.port).await;
    app.tunnel.stop();
    if let Err(e) = result {
        eprintln!("A szerver leállt: {e}");
        std::process::exit(1);
    }
}
