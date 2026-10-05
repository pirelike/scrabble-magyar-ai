        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            email_lower TEXT NOT NULL UNIQUE,
            display_name TEXT NOT NULL,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            games_played INTEGER NOT NULL DEFAULT 0,
            games_won INTEGER NOT NULL DEFAULT 0,
            total_score INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS verification_codes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            email TEXT NOT NULL,
            code TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            expires_at TEXT NOT NULL,
            attempts INTEGER NOT NULL DEFAULT 0,
            used INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            token TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            expires_at TEXT NOT NULL,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS verified_emails (
            email TEXT PRIMARY KEY,
            expires_at TEXT NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_sessions_token ON sessions(token);
        CREATE INDEX IF NOT EXISTS idx_users_email_lower ON users(email_lower);
        CREATE INDEX IF NOT EXISTS idx_verification_codes_email ON verification_codes(email);

        CREATE TABLE IF NOT EXISTS saved_games (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            room_id TEXT NOT NULL,
            room_name TEXT NOT NULL DEFAULT '',
            state_json TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'active',
            challenge_mode INTEGER NOT NULL DEFAULT 0,
            owner_name TEXT NOT NULL DEFAULT '',
            owner_token TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS game_players (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            game_id INTEGER NOT NULL,
            user_id INTEGER,
            player_name TEXT NOT NULL,
            final_score INTEGER NOT NULL DEFAULT 0,
            is_winner INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (game_id) REFERENCES saved_games(id) ON DELETE CASCADE,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE SET NULL
        );

        CREATE TABLE IF NOT EXISTS game_moves (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            game_id INTEGER NOT NULL,
            move_number INTEGER NOT NULL,
            player_name TEXT NOT NULL,
            action_type TEXT NOT NULL,
            details_json TEXT,
            board_snapshot_json TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (game_id) REFERENCES saved_games(id) ON DELETE CASCADE
        );

        CREATE INDEX IF NOT EXISTS idx_saved_games_room_id ON saved_games(room_id);
        CREATE INDEX IF NOT EXISTS idx_saved_games_status ON saved_games(status);
        CREATE INDEX IF NOT EXISTS idx_game_players_game_id ON game_players(game_id);
        CREATE INDEX IF NOT EXISTS idx_game_players_user_id ON game_players(user_id);
        CREATE INDEX IF NOT EXISTS idx_game_moves_game_id ON game_moves(game_id);

        CREATE TABLE IF NOT EXISTS app_settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS push_subscriptions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            endpoint TEXT NOT NULL UNIQUE,
            p256dh TEXT NOT NULL,
            auth TEXT NOT NULL,
            lang TEXT NOT NULL DEFAULT 'hu',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_push_subscriptions_user ON push_subscriptions(user_id);

        CREATE TABLE IF NOT EXISTS daily_puzzles (
            puzzle_date TEXT PRIMARY KEY,
            board_json TEXT NOT NULL,
            rack_json TEXT NOT NULL,
            best_score INTEGER NOT NULL,
            best_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS daily_scores (
            puzzle_date TEXT NOT NULL,
            user_id INTEGER NOT NULL,
            best_score INTEGER NOT NULL DEFAULT 0,
            attempts INTEGER NOT NULL DEFAULT 0,
            first_best_at TEXT NOT NULL DEFAULT (datetime('now')),
            revealed INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (puzzle_date, user_id),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_daily_scores_ranking
            ON daily_scores(puzzle_date, best_score DESC);

        CREATE TABLE IF NOT EXISTS game_analysis (
            game_id INTEGER PRIMARY KEY,
            version INTEGER NOT NULL,
            result_json TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (game_id) REFERENCES saved_games(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS achievements (
            user_id INTEGER NOT NULL,
            badge TEXT NOT NULL,
            game_id INTEGER,
            earned_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (user_id, badge),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );

        CREATE TABLE IF NOT EXISTS friendships (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            friend_id INTEGER NOT NULL,
            status TEXT NOT NULL DEFAULT 'pending',
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
            FOREIGN KEY (friend_id) REFERENCES users(id) ON DELETE CASCADE,
            UNIQUE(user_id, friend_id)
        );
        CREATE INDEX IF NOT EXISTS idx_friendships_user_id ON friendships(user_id);
        CREATE INDEX IF NOT EXISTS idx_friendships_friend_id ON friendships(friend_id);

        CREATE TABLE IF NOT EXISTS word_reviews (
            word TEXT NOT NULL,
            user_id INTEGER NOT NULL,
            verdict INTEGER NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            PRIMARY KEY (word, user_id),
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_word_reviews_user ON word_reviews(user_id, created_at);

        -- Admin napló: csak hozzáfűzhető (a triggerek a módosítást és a törlést is megtiltják)
        CREATE TABLE IF NOT EXISTS admin_audit (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            admin_user_id INTEGER NOT NULL,
            action TEXT NOT NULL,
            target_type TEXT,
            target_id TEXT,
            details_json TEXT,
            ip TEXT,
            user_agent TEXT,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_admin_audit_created ON admin_audit(created_at);
        CREATE INDEX IF NOT EXISTS idx_admin_audit_target ON admin_audit(target_type, target_id);
        CREATE TRIGGER IF NOT EXISTS admin_audit_no_update BEFORE UPDATE ON admin_audit
        BEGIN
            SELECT RAISE(ABORT, 'admin_audit is append-only');
        END;
        CREATE TRIGGER IF NOT EXISTS admin_audit_no_delete BEFORE DELETE ON admin_audit
        BEGIN
            SELECT RAISE(ABORT, 'admin_audit is append-only');
        END;

        -- Admin panel: belső jegyzetek, kézi értékszám-módosítások, szótári felülbírálatok
        CREATE TABLE IF NOT EXISTS user_admin_notes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            admin_user_id INTEGER NOT NULL,
            note TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_user_admin_notes_user ON user_admin_notes(user_id);

        CREATE TABLE IF NOT EXISTS rating_adjustments (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER NOT NULL,
            admin_user_id INTEGER NOT NULL,
            rating_before INTEGER NOT NULL,
            rating_after INTEGER NOT NULL,
            reason TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_rating_adjustments_user ON rating_adjustments(user_id);

        CREATE TABLE IF NOT EXISTS word_overrides (
            word TEXT PRIMARY KEY,
            verdict TEXT NOT NULL,
            admin_user_id INTEGER,
            reason TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS word_additions (
            word TEXT PRIMARY KEY,
            admin_user_id INTEGER,
            reason TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        -- Közlemények (banner), bejelentések, belépési napló, IP tiltások, chat napló, tiltott szavak
        CREATE TABLE IF NOT EXISTS announcements (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            text_hu TEXT NOT NULL,
            text_en TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL DEFAULT 'info',
            starts_at TEXT,
            ends_at TEXT,
            audience TEXT NOT NULL DEFAULT 'all',
            active INTEGER NOT NULL DEFAULT 1,
            created_by INTEGER,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            updated_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS reports (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            reporter_user_id INTEGER,
            reporter_name TEXT NOT NULL DEFAULT '',
            reported_user_id INTEGER,
            reported_name TEXT NOT NULL DEFAULT '',
            kind TEXT NOT NULL DEFAULT 'player',
            message TEXT NOT NULL DEFAULT '',
            reason TEXT NOT NULL DEFAULT '',
            room_id TEXT,
            room_name TEXT NOT NULL DEFAULT '',
            snapshot_json TEXT,
            status TEXT NOT NULL DEFAULT 'new',
            handled_by INTEGER,
            handled_at TEXT,
            handler_note TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_reports_status ON reports(status, created_at);

        CREATE TABLE IF NOT EXISTS login_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            user_id INTEGER,
            email TEXT NOT NULL DEFAULT '',
            ip TEXT,
            user_agent TEXT,
            success INTEGER NOT NULL DEFAULT 0,
            reason TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_login_events_created ON login_events(created_at);
        CREATE INDEX IF NOT EXISTS idx_login_events_ip ON login_events(ip, created_at);
        CREATE INDEX IF NOT EXISTS idx_login_events_user ON login_events(user_id, created_at);

        CREATE TABLE IF NOT EXISTS ip_bans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            ip TEXT NOT NULL,
            reason TEXT NOT NULL DEFAULT '',
            expires_at TEXT,
            created_by INTEGER,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS chat_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            room_id TEXT NOT NULL,
            room_name TEXT NOT NULL DEFAULT '',
            user_id INTEGER,
            name TEXT NOT NULL DEFAULT '',
            message TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_chat_log_created ON chat_log(created_at);
        CREATE INDEX IF NOT EXISTS idx_chat_log_room ON chat_log(room_id);

        CREATE TABLE IF NOT EXISTS usage_counters (
            day TEXT NOT NULL,
            key TEXT NOT NULL,
            count INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (day, key)
        );

        CREATE TABLE IF NOT EXISTS banned_words (
            word TEXT PRIMARY KEY,
            created_by INTEGER,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
    
