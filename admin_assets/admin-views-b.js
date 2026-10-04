'use strict';
// ===== ADMIN PANEL — nézetek II.: játékarchívum, levelezős játékok, ranglista, napi feladvány, szótár =====

// ===== Játékok (archívum) =====

const TRI_OPTIONS = () => [
    { value: '', label: t('admin.any') }, { value: '1', label: t('admin.yes') }, { value: '0', label: t('admin.no') },
];

const GamesView = {
    render(view, ctx) {
        if (ctx.arg) return GameDetailView.render(view, ctx);
        const cleanup = UI.btn(t('admin.games_cleanup'), () => this.cleanup(), { kind: 'secondary' });
        mountList(view, ctx.params, {
            section: 'games', title: t('admin.nav_games'), note: t('admin.games_note'), path: '/api/admin/games',
            defaultSort: 'id', defaultOrder: 'desc', headerActions: [cleanup],
            filters: [
                { name: 'q', label: t('admin.f_game_q'), placeholder: t('admin.f_game_q_ph') },
                { name: 'status', label: t('admin.f_status'), type: 'select', options: [{ value: '', label: t('admin.any') },
                    ...Object.entries(GAME_STATUS_LABELS).map(([value, key]) => ({ value, label: t(key) }))] },
                { name: 'user', label: t('admin.f_game_user'), placeholder: t('admin.f_game_user_ph') },
                { name: 'since', label: t('admin.f_since'), type: 'date' },
                { name: 'until', label: t('admin.f_until'), type: 'date' },
                { name: 'bots', label: t('admin.f_with_bots'), type: 'select', options: TRI_OPTIONS() },
                { name: 'async', label: t('admin.f_async'), type: 'select', options: TRI_OPTIONS() },
                { name: 'suspicious', label: t('admin.f_suspicious'), type: 'checkbox' },
            ],
            columns: [
                { label: t('admin.col_id'), sortKey: 'id', cell: (g) => '#' + g.id },
                { label: t('admin.col_game'), cell: (g) => g.name },
                { label: t('admin.col_status'), cell: (g) => UI.badge(gameStatusLabel(g.status), g.status === 'finished' ? 'ok' : (g.status === 'voided' ? 'danger' : 'info')) },
                { label: t('admin.col_type'), cell: (g) => [g.async ? UI.badge(t('admin.kind_async'), 'info') : null,
                    g.bots ? UI.badge(t('admin.kind_bots'), 'muted') : null, g.challenge ? UI.badge(t('admin.kind_challenge'), 'muted') : null] },
                { label: t('admin.col_players'), cell: (g) => g.players.map((p) => `${p.name} ${p.score}${p.winner ? ' ★' : ''}`).join(' · ') },
                { label: t('admin.col_moves'), cell: (g) => g.moves },
                { label: t('admin.col_created'), sortKey: 'created_at', cell: (g) => formatStamp(g.created_at) },
                { label: t('admin.col_updated'), sortKey: 'updated_at', cell: (g) => formatStamp(g.updated_at) },
                { label: t('admin.col_flags'), cell: (g) => [g.shared ? UI.badge(t('admin.flag_shared'), 'info') : null,
                    g.analysed ? UI.badge(t('admin.flag_analysed'), 'muted') : null] },
            ],
            onRow: (g) => Router.go('games', g.id),
        });
    },

    // Régi, félbehagyott játékok takarítása: előbb az előnézet (hány és melyik), aztán a megerősítés
    async cleanup() {
        const days = 30;
        const preview = await Api.get('/api/admin/games/cleanup', { days });
        if (!preview.ok) { showToast(Api.message(preview), true); return; }
        Act.open({
            title: t('admin.games_cleanup'), text: t('admin.games_cleanup_text', { n: preview.data.total, days }), danger: true,
            fields: [{ name: 'days', type: 'number', label: t('admin.f_days'), value: days, min: 1, max: 3650, required: true }],
            run: (v) => Api.post('/api/admin/games/cleanup', { days: v.days, reason: v.reason }),
            done: (result) => { showToast(t('admin.deleted_n', { n: result.deleted })); Router.render(); },
        });
    },
};

registerSection({ id: 'games', order: 40, icon: 'archive', labelKey: 'admin.nav_games',
    render: (view, ctx) => GamesView.render(view, ctx) });

const GameDetailView = {
    async render(view, ctx) {
        const id = ctx.arg;
        const tab = ctx.params.get('tab') || 'overview';
        const holder = h('div');
        view.appendChild(holder);
        await loadInto(holder, () => Api.get(`/api/admin/games/${encodeURIComponent(id)}`), (box, data) => {
            const game = data.game;
            const reload = () => Router.render();
            box.appendChild(h('div', { class: 'admin-breadcrumb' }, UI.link(t('admin.nav_games'), Router.href('games'))));
            box.appendChild(UI.header(game.name, '#' + game.id + ' · ' + t('admin.game_room_id', { id: game.room_id || '–' })));
            box.appendChild(h('div', { class: 'admin-badge-row' },
                UI.badge(gameStatusLabel(game.status), game.status === 'finished' ? 'ok' : (game.status === 'voided' ? 'danger' : 'info')),
                game.async ? UI.badge(t('admin.kind_async'), 'info') : null, game.bots ? UI.badge(t('admin.kind_bots'), 'muted') : null,
                game.challenge ? UI.badge(t('admin.kind_challenge'), 'muted') : null));
            box.appendChild(UI.tabs([{ id: 'overview', label: t('admin.tab_overview') }, { id: 'moves', label: t('admin.tab_moves') },
                { id: 'replay', label: t('admin.tab_replay') }, { id: 'actions', label: t('admin.tab_actions') }], tab,
            (next) => Router.go('games', id, { tab: next })));
            const body = h('div', { class: 'admin-tab-body' });
            box.appendChild(body);
            ({ overview: this.overview, moves: this.moves, replay: this.replay, actions: this.actions }[tab] || this.overview)
                .call(this, body, game, reload);
        });
    },

    overview(box, game) {
        box.appendChild(UI.card(t('admin.card_game'), UI.kv([
            [t('admin.col_id'), '#' + game.id], [t('admin.col_status'), gameStatusLabel(game.status)],
            [t('admin.col_owner'), game.owner], [t('admin.col_created'), formatStamp(game.created_at)],
            [t('admin.col_updated'), formatStamp(game.updated_at)],
            [t('admin.d_share_token'), game.share_token ? h('code', null, game.share_token) : null],
        ])));
        box.appendChild(UI.card(t('admin.card_players'), UI.table({ compact: true, items: game.players, columns: [
            { label: t('admin.col_name'), cell: (p) => p.user_id ? UI.userLink(p.user_id, p.name) : p.name },
            { label: t('admin.col_score'), cell: (p) => p.score + (p.winner ? ' ★' : '') },
            { label: t('admin.col_rating_before'), cell: (p) => p.rating_before === null ? '–' : p.rating_before },
            { label: t('admin.col_rating_after'), cell: (p) => p.rating_after === null ? '–' : p.rating_after },
            { label: t('admin.col_rating_change'), cell: (p) => p.rating_change === null ? '–' : (p.rating_change > 0 ? '+' : '') + p.rating_change },
        ] })));
        if (game.analysis) {
            box.appendChild(UI.card(t('admin.card_analysis'), UI.table({ compact: true, items: game.analysis.players, columns: [
                { label: t('admin.col_player'), cell: (p) => p.player },
                { label: t('admin.col_turns'), cell: (p) => p.turns },
                { label: t('admin.col_scored'), cell: (p) => p.scored },
                { label: t('admin.col_possible'), cell: (p) => p.possible },
                { label: t('admin.col_efficiency'), cell: (p) => p.possible ? fmtPercent(100 * p.scored / p.possible, 0) : '–' },
            ] })));
        }
        box.appendChild(UI.card(t('admin.card_history'), game.admin_history.length ? UI.table({ compact: true, items: game.admin_history, columns: [
            { label: t('admin.col_time'), cell: (e) => formatStamp(e.created_at) },
            { label: t('admin.col_admin'), cell: (e) => e.admin_name || '?' },
            { label: t('admin.col_action'), cls: 'admin-action', cell: (e) => e.action },
            { label: t('admin.col_reason'), cell: (e) => e.reason || '' },
        ] }) : h('p', { class: 'text-muted' }, t('admin.none'))));
    },

    moves(box, game) {
        if (!game.moves.length) { box.appendChild(UI.empty(t('admin.none'))); return; }
        box.appendChild(h('p', { class: 'form-hint' }, t('admin.moves_racks_logged')));
        box.appendChild(UI.table({ compact: true, items: game.moves, columns: [
            { label: '#', cell: (m) => m.n },
            { label: t('admin.col_time'), cell: (m) => formatStamp(m.at) },
            { label: t('admin.col_player'), cell: (m) => m.player },
            { label: t('admin.col_action'), cell: (m) => h('span', null, m.type, m.admin ? [' ', UI.badge(t('admin.by_admin'), 'warn')] : null) },
            { label: t('admin.col_words'), cell: (m) => (m.words || []).join(', ') },
            { label: t('admin.col_score'), cell: (m) => m.score === null ? '' : m.score },
            { label: t('admin.col_hand'), cell: (m) => m.rack ? h('span', { class: 'admin-rack-tiles' }, m.rack.map((l) => UI.tile(l === '?' ? '' : l, l === '?'))) : '' },
        ] }));
    },

    async replay(box, game) {
        const holder = h('div');
        box.appendChild(holder);
        await loadInto(holder, () => Api.get(`/api/admin/games/${game.id}/moves`), (container, data) => {
            const moves = data.moves;
            if (!moves.length) { container.appendChild(UI.empty(t('admin.none'))); return; }
            let index = moves.length;
            const stage = h('div', { class: 'admin-replay-stage' });
            const info = h('div', { class: 'admin-replay-info' });
            const slider = h('input', { type: 'range', min: 0, max: moves.length, value: index, class: 'admin-replay-slider' });
            const paint = () => {
                const move = index > 0 ? moves[index - 1] : null;
                const board = move && Array.isArray(move.board) ? move.board : null;
                stage.replaceChildren(renderBoard(board, move ? move.tiles : []));
                info.textContent = move
                    ? t('admin.replay_move', { n: move.n, total: moves.length, player: move.player, type: move.type,
                        words: (move.words || []).join(', ') || '–', score: move.score === null ? '–' : move.score })
                    : t('admin.replay_start');
                slider.value = index;
            };
            const step = (delta) => { index = Math.max(0, Math.min(moves.length, index + delta)); paint(); };
            slider.addEventListener('input', () => { index = Number(slider.value); paint(); });
            container.append(h('div', { class: 'admin-replay-controls' },
                UI.btn('⏮', () => step(-moves.length), { kind: 'secondary', title: t('admin.replay_first') }),
                UI.btn('◀', () => step(-1), { kind: 'secondary', title: t('admin.replay_prev') }),
                slider,
                UI.btn('▶', () => step(1), { kind: 'secondary', title: t('admin.replay_next') }),
                UI.btn('⏭', () => step(moves.length), { kind: 'secondary', title: t('admin.replay_last') })), info, stage);
            paint();
        });
    },

    actions(box, game, reload) {
        const row = (title, text, buttons) => UI.card(title, text ? h('p', { class: 'form-hint' }, text) : null,
            h('div', { class: 'admin-action-row' }, buttons));
        const id = game.id;
        const finished = game.status === 'finished';
        const voided = game.status === 'voided';

        box.appendChild(row(t('admin.grp_validity'), t('admin.validity_text'), [
            !voided ? UI.btn(t('admin.act_void_game'), () => Act.open({
                title: t('admin.act_void_game'), text: t('admin.act_void_game_text'), danger: true,
                fields: [{ name: 'revoke_badges', type: 'checkbox', label: t('admin.f_revoke_badges') }],
                run: (v) => Api.post(`/api/admin/games/${id}/void`, { revoke_badges: v.revoke_badges, reason: v.reason }),
                done: (r) => { showToast(t('admin.rating_changes_n', { n: r.rating_changes })); reload(); } }),
            { kind: 'danger', disabled: !finished }) : null,
            voided ? UI.btn(t('admin.act_unvoid_game'), () => Act.open({
                title: t('admin.act_unvoid_game'), text: t('admin.act_unvoid_game_text'),
                run: (v) => Api.post(`/api/admin/games/${id}/unvoid`, { reason: v.reason }),
                done: (r) => { showToast(t('admin.rating_changes_n', { n: r.rating_changes })); reload(); } }), { kind: 'tinted' }) : null,
            UI.btn(t('admin.act_status'), () => Act.open({
                title: t('admin.act_status'), text: t('admin.act_status_text'),
                fields: [{ name: 'status', type: 'select', label: t('admin.col_status'), value: game.status,
                    options: ['active', 'finished', 'abandoned'].map((s) => ({ value: s, label: gameStatusLabel(s) })) }],
                run: (v) => Api.post(`/api/admin/games/${id}/status`, { status: v.status, reason: v.reason }), done: reload }),
            { kind: 'secondary', disabled: voided }),
        ]));
        box.appendChild(row(t('admin.grp_sharing'), null, [
            UI.btn(t('admin.act_unshare'), () => Act.open({
                title: t('admin.act_unshare'), text: t('admin.act_unshare_text'),
                run: (v) => Api.post(`/api/admin/games/${id}/unshare`, { reason: v.reason }), done: reload }),
            { kind: 'secondary', disabled: !game.share_token }),
            UI.btn(t('admin.act_reanalyze'), () => Act.open({
                title: t('admin.act_reanalyze'), text: t('admin.act_reanalyze_text'),
                run: (v) => Api.post(`/api/admin/games/${id}/reanalyze`, { reason: v.reason }), done: reload }),
            { kind: 'secondary', disabled: !finished }),
            UI.btn(t('admin.act_export'), () => { location.href = `/api/admin/games/${id}/export`; }, { kind: 'secondary' }),
        ]));
        box.appendChild(row(t('admin.grp_danger'), null, [
            UI.btn(t('admin.act_delete_game') + ' 🔒', () => Act.open({
                title: t('admin.act_delete_game'), text: t('admin.act_delete_game_text'), danger: true, confirmName: String(id),
                fields: finished ? [{ name: 'confirm_finished', type: 'checkbox', label: t('admin.f_confirm_finished') }] : [],
                validate: (v) => (finished && !v.confirm_finished ? t('admin.err_confirm_finished') : null),
                run: (v) => Api.del(`/api/admin/games/${id}`, { confirm_finished: !!v.confirm_finished, reason: v.reason }),
                done: () => Router.go('games') }), { kind: 'danger' }),
        ]));
    },
};

// ===== Levelezős játékok =====

const AsyncView = {
    async render(view) {
        const box = h('div');
        view.appendChild(box);
        const load = async () => {
            await loadInto(box, () => Api.get('/api/admin/async'), (container, data) => this.paint(container, data, load));
        };
        await load();
    },

    paint(box, data, reload) {
        const sweeper = data.sweeper;
        box.appendChild(UI.header(t('admin.nav_async'), t('admin.async_note'),
            UI.btn(t('admin.async_sweep'), () => Act.open({
                title: t('admin.async_sweep'), text: t('admin.async_sweep_text'), reason: false,
                run: () => Api.post('/api/admin/async/sweep'),
                done: (r) => { showToast(t('admin.async_swept', { n: r.expired })); reload(); } }), { kind: 'secondary' }),
            UI.btn(t('admin.refresh'), reload, { kind: 'secondary' })));
        box.appendChild(UI.card(t('admin.card_sweeper'), UI.kv([
            [t('admin.d_sweeper_last'), sweeper && sweeper.last_run ? formatEpoch(sweeper.last_run) : t('admin.none')],
            [t('admin.d_sweeper_runs'), sweeper ? sweeper.runs : 0],
            [t('admin.d_sweeper_errors'), sweeper ? sweeper.errors : 0],
            [t('admin.d_sweeper_info'), sweeper ? sweeper.info : null],
            [t('admin.d_async_loaded'), data.loaded_count],
        ])));
        if (!data.items.length) { box.appendChild(UI.empty(t('admin.async_empty'))); return; }
        box.appendChild(UI.table({ items: data.items, onRow: (g) => this.detail(g, reload), rowClass: (g) => g.remaining !== null && g.remaining < 0 ? 'row-alert' : '',
            columns: [
                { label: t('admin.col_id'), cell: (g) => '#' + g.id },
                { label: t('admin.col_game'), cell: (g) => UI.gameLink(g.id, g.name) },
                { label: t('admin.col_players'), cell: (g) => g.players.map((p) => `${p.name} ${p.score}${p.resigned ? ' 🏳' : ''}`).join(' · ') },
                { label: t('admin.col_current'), cell: (g) => g.current || '' },
                { label: t('admin.col_deadline'), cell: (g) => g.remaining === null ? '–' : UI.badge(
                    g.remaining < 0 ? t('admin.overdue', { time: fmtDuration(-g.remaining) }) : fmtDuration(g.remaining),
                    g.remaining < 0 ? 'danger' : (g.remaining < 6 * 3600 ? 'warn' : 'ok')) },
                { label: t('admin.col_turn_hours'), cell: (g) => g.turn_hours },
                { label: t('admin.col_moves'), cell: (g) => g.moves },
                { label: t('admin.col_last_move'), cell: (g) => formatStamp(g.last_move_at) || '–' },
                { label: t('admin.col_timeouts'), cell: (g) => g.max_timeouts || '' },
                { label: t('admin.col_loaded'), cell: (g) => g.loaded ? UI.badge(t('admin.yes'), 'ok') : UI.badge(t('admin.no'), 'muted') },
            ] }));
    },

    detail(game, reload) {
        const body = h('div');
        const done = () => { Modal.close(); reload(); };
        const open = (title, text, request, options = {}) => () => { Modal.close(); Act.open({ title, text, danger: !!options.danger,
            fields: options.fields, reason: options.reason !== false, run: request, done }); };
        body.appendChild(UI.kv([
            [t('admin.col_players'), game.players.map((p) => `${p.name} (${p.score}${p.timeouts ? ', ' + t('admin.timeouts_n', { n: p.timeouts }) : ''})`).join(' · ')],
            [t('admin.col_current'), game.current], [t('admin.col_turn_hours'), game.turn_hours],
            [t('admin.col_deadline'), game.remaining === null ? null : (game.remaining < 0 ? t('admin.overdue', { time: fmtDuration(-game.remaining) }) : fmtDuration(game.remaining))],
        ]));
        const hours = [6, 12, 24, 48, 72, 168];
        body.appendChild(h('div', { class: 'admin-action-row' },
            UI.btn(t('admin.act_extend'), open(t('admin.act_extend'), t('admin.async_extend_text'),
                (v) => Api.post(`/api/admin/async/${game.id}/extend`, { hours: v.hours, reason: v.reason }),
                { fields: [{ name: 'hours', type: 'select', label: t('admin.f_hours'), value: 24,
                    options: hours.map((n) => ({ value: n, label: t('admin.hours_n', { n }) })) }] }), { kind: 'secondary' }),
            UI.btn(t('admin.act_expire'), open(t('admin.act_expire'), t('admin.async_expire_text'),
                (v) => Api.post(`/api/admin/async/${game.id}/expire`, { reason: v.reason })), { kind: 'secondary' }),
            UI.btn(t('admin.act_remind'), async () => {
                const result = await Api.post(`/api/admin/async/${game.id}/remind`);
                if (result.ok) showToast(t('admin.push_test_sent', { n: result.data.sent })); else showToast(Api.message(result), true);
            }, { kind: 'secondary' }),
            UI.btn(t('admin.act_resign'), open(t('admin.act_resign'), t('admin.async_resign_text'),
                (v) => Api.post(`/api/admin/async/${game.id}/resign`, { player: v.player, reason: v.reason }),
                { danger: true, fields: [{ name: 'player', type: 'select', label: t('admin.col_player'),
                    options: game.players.filter((p) => !p.resigned).map((p) => ({ value: p.name, label: p.name })) }] }), { kind: 'danger' }),
            UI.btn(t('admin.act_void_game'), open(t('admin.act_void_game'), t('admin.async_void_text'),
                (v) => Api.post(`/api/admin/async/${game.id}/void`, { reason: v.reason }), { danger: true }), { kind: 'danger' }),
            game.loaded ? UI.btn(t('admin.act_unload'), open(t('admin.act_unload'), t('admin.async_unload_text'),
                (v) => Api.post(`/api/admin/async/${game.id}/unload`, { reason: v.reason })), { kind: 'secondary' })
                : UI.btn(t('admin.act_load'), open(t('admin.act_load'), t('admin.async_load_text'),
                    () => Api.post(`/api/admin/async/${game.id}/load`), { reason: false }), { kind: 'secondary' })));
        Modal.show(game.name, body, { wide: true });
    },
};

registerSection({ id: 'async', order: 50, icon: 'mail', labelKey: 'admin.nav_async',
    render: (view) => AsyncView.render(view) });

// ===== Ranglista és értékszám =====

const METRIC_LABELS = {
    rating: 'admin.metric_rating', wins: 'admin.metric_wins', win_rate: 'admin.metric_win_rate',
    avg_score: 'admin.metric_avg_score', best_game: 'admin.metric_best_game',
};

const RatingsView = {
    render(view, ctx) {
        const tab = ctx.params.get('tab') || 'leaderboard';
        view.appendChild(UI.header(t('admin.nav_ratings'), t('admin.ratings_note')));
        view.appendChild(UI.tabs([{ id: 'leaderboard', label: t('admin.tab_leaderboard') },
            { id: 'suspicious', label: t('admin.tab_suspicious') }, { id: 'recompute', label: t('admin.tab_recompute') }], tab,
        (next) => Router.go('ratings', '', { tab: next })));
        const body = h('div', { class: 'admin-tab-body' });
        view.appendChild(body);
        ({ leaderboard: this.leaderboard, suspicious: this.suspicious, recompute: this.recompute }[tab] || this.leaderboard)
            .call(this, body, ctx.params);
    },

    leaderboard(box, params) {
        const metric = params.get('metric') || 'rating';
        const select = h('select', null, Object.entries(METRIC_LABELS).map(([value, key]) => h('option', { value }, t(key))));
        select.value = metric;
        select.addEventListener('change', () => Router.go('ratings', '', { tab: 'leaderboard', metric: select.value }));
        box.appendChild(h('div', { class: 'admin-toolbar' }, h('label', { class: 'admin-field' },
            h('span', { class: 'admin-field-label' }, t('admin.f_metric')), select),
        UI.download(t('admin.export_csv'), '/api/admin/ratings', { metric, format: 'csv' })));
        const results = h('div');
        box.appendChild(results);
        loadInto(results, () => Api.get('/api/admin/ratings', { metric, limit: 100 }), (container, data) => {
            if (!data.entries.length) { container.appendChild(UI.empty(t('admin.none'))); return; }
            container.appendChild(UI.table({ items: data.entries, onRow: (e) => Router.go('users', e.user_id), columns: [
                { label: '#', cell: (e) => e.rank },
                { label: t('admin.col_name'), cell: (e) => UI.userLink(e.user_id, e.display_name) },
                { label: t('admin.col_rating'), cell: (e) => e.rating },
                { label: t('admin.col_rated_games'), cell: (e) => e.rated_games },
                { label: t('admin.col_games'), cell: (e) => e.games_played + ' / ' + e.games_won },
                { label: t('admin.d_win_rate'), cell: (e) => fmtPercent(e.win_rate) },
                { label: t('admin.d_avg_score'), cell: (e) => fmtNum(e.avg_score, 1) },
                { label: t('admin.col_best_score'), cell: (e) => e.best_score },
            ] }));
        });
    },

    suspicious(box) {
        loadInto(box, () => Api.get('/api/admin/ratings/suspicious'), (container, data) => {
            const th = data.thresholds;
            container.appendChild(h('p', { class: 'form-hint' }, t('admin.suspicious_note', {
                games: th.pair_games, share: Math.round(th.pair_share * 100), eff: th.efficiency, effGames: th.efficiency_games })));
            const gamesCell = (ids) => h('span', { class: 'admin-chip-row' }, ids.map((id) => UI.gameLink(id, '#' + id)));
            const usersCell = (users) => users.map((u) => u.name + (u.wins !== undefined ? ` (${u.wins})` : '')).join(' ↔ ');
            const block = (title, items, columns) => container.appendChild(UI.card(title, items.length
                ? UI.table({ compact: true, items, columns }) : h('p', { class: 'text-muted' }, t('admin.none'))));
            block(t('admin.sus_pairs'), data.one_sided_pairs, [
                { label: t('admin.col_players'), cell: (e) => usersCell(e.users) }, { label: t('admin.col_games'), cell: (e) => e.games },
                { label: t('admin.col_game'), cell: (e) => gamesCell(e.game_ids) }]);
            block(t('admin.sus_same_ip'), data.same_ip, [
                { label: t('admin.col_players'), cell: (e) => usersCell(e.users) }, { label: t('admin.col_games'), cell: (e) => e.games },
                { label: t('admin.col_ip'), cell: (e) => e.ips.join(', ') }, { label: t('admin.col_game'), cell: (e) => gamesCell(e.game_ids) }]);
            block(t('admin.sus_fast_pass'), data.fast_pass_games, [
                { label: t('admin.col_game'), cell: (e) => UI.gameLink(e.game_id, '#' + e.game_id) },
                { label: t('admin.col_players'), cell: (e) => e.players.join(' ↔ ') },
                { label: t('admin.col_moves'), cell: (e) => e.moves }, { label: t('admin.col_passes'), cell: (e) => e.passes }]);
            block(t('admin.sus_efficiency'), data.high_efficiency, [
                { label: t('admin.col_name'), cell: (e) => UI.userLink(e.user_id, e.name) },
                { label: t('admin.col_games'), cell: (e) => e.games }, { label: t('admin.col_efficiency'), cell: (e) => fmtPercent(e.average) }]);
        });
    },

    recompute(box) {
        box.appendChild(h('p', { class: 'form-hint' }, t('admin.recompute_note')));
        const results = h('div');
        const preview = UI.btn(t('admin.recompute_preview'), async () => {
            results.replaceChildren(UI.loading());
            const result = await Api.post('/api/admin/ratings/recompute', { dry_run: true });
            if (!result.ok) { results.replaceChildren(UI.errorBox(Api.message(result))); return; }
            const data = result.data;
            results.replaceChildren(h('p', null, t('admin.recompute_summary', { changed: data.changed, unchanged: data.unchanged })));
            if (data.items.length) {
                results.appendChild(UI.table({ compact: true, items: data.items, columns: [
                    { label: t('admin.col_name'), cell: (e) => UI.userLink(e.user_id, e.display_name) },
                    { label: t('admin.col_current'), cell: (e) => e.current }, { label: t('admin.col_new'), cell: (e) => e.new },
                    { label: t('admin.col_delta'), cell: (e) => (e.delta > 0 ? '+' : '') + e.delta }] }));
                results.appendChild(UI.btn(t('admin.recompute_apply') + ' 🔒', () => Act.open({
                    title: t('admin.recompute_apply'), text: t('admin.recompute_apply_text', { n: data.changed }), danger: true,
                    run: (v) => Api.post('/api/admin/ratings/recompute', { dry_run: false, reason: v.reason }),
                    done: () => Router.render() }), { kind: 'danger' }));
            }
        }, { kind: 'secondary' });
        box.append(preview, results);
    },
};

registerSection({ id: 'ratings', order: 60, icon: 'award', labelKey: 'admin.nav_ratings',
    render: (view, ctx) => RatingsView.render(view, ctx) });

// ===== Napi feladvány =====

const DailyView = {
    render(view, ctx) {
        const tab = ctx.params.get('tab') || 'day';
        view.appendChild(UI.header(t('admin.nav_daily'), t('admin.daily_note')));
        view.appendChild(UI.tabs([{ id: 'day', label: t('admin.tab_daily_day') }, { id: 'archive', label: t('admin.tab_daily_archive') },
            { id: 'preview', label: t('admin.tab_daily_preview') }], tab, (next) => Router.go('daily', '', { tab: next })));
        const body = h('div', { class: 'admin-tab-body' });
        view.appendChild(body);
        ({ day: this.day, archive: this.archive, preview: this.preview }[tab] || this.day).call(this, body, ctx.params);
    },

    dateField(params, tab) {
        const input = h('input', { type: 'date', value: params.get('date') || '' });
        const form = h('form', { class: 'admin-filters' }, h('label', { class: 'admin-field' },
            h('span', { class: 'admin-field-label' }, t('admin.f_date')), input),
        h('div', { class: 'admin-filter-actions' }, h('button', { type: 'submit', class: 'small-btn' }, t('admin.apply'))));
        form.addEventListener('submit', (event) => { event.preventDefault(); Router.go('daily', '', { tab, date: input.value }); });
        return form;
    },

    day(box, params) {
        box.appendChild(this.dateField(params, 'day'));
        const results = h('div');
        box.appendChild(results);
        const date = params.get('date');
        loadInto(results, () => Api.get('/api/admin/daily', { date }), (container, data) => {
            const reload = () => Router.render();
            container.appendChild(UI.subtitle(data.date));
            if (!data.puzzle) container.appendChild(h('p', { class: 'text-muted' }, t('admin.daily_no_puzzle')));
            else {
                container.appendChild(h('div', { class: 'admin-room-layout' },
                    h('div', { class: 'admin-room-left' }, renderBoard(data.puzzle.board, [])),
                    h('div', { class: 'admin-room-right' }, UI.card(t('admin.card_daily_puzzle'), UI.kv([
                        [t('admin.d_daily_best'), data.puzzle.best_score], [t('admin.d_daily_participants'), data.participants],
                        [t('admin.d_daily_revealed'), data.revealed]]),
                    h('div', { class: 'admin-rack-tiles' }, data.puzzle.rack.map((l) => UI.tile(l === '?' ? '' : l, l === '?'))),
                    h('p', { class: 'form-hint' }, t('admin.d_daily_best_words', { words: ((data.puzzle.best || {}).words || []).join(', ') || '–' })),
                    h('div', { class: 'admin-action-row' }, UI.btn(t('admin.act_regenerate') + ' 🔒', () => this.regenerate(data, reload), { kind: 'danger' })))
                    , UI.card(t('admin.card_daily_dist'), h('div', { class: 'admin-chart-grid' },
                        Chart.hbars(Object.entries(data.attempts).map(([label, value]) => ({ label: t('admin.daily_attempts_n', { n: label }), value }))),
                        Chart.hbars(Object.entries(data.scores).map(([label, value]) => ({ label: t(DAILY_SCORE_LABELS[label]), value }))))))));
            }
            container.appendChild(UI.card(t('admin.card_daily_board'), data.leaderboard.length ? UI.table({ compact: true, items: data.leaderboard, columns: [
                { label: '#', cell: (e) => e.rank },
                { label: t('admin.col_name'), cell: (e) => e.user_id ? UI.userLink(e.user_id, e.display_name) : '' },
                { label: t('admin.col_score'), cell: (e) => e.best_score }, { label: t('admin.col_attempts'), cell: (e) => e.attempts },
                { label: t('admin.col_revealed'), cell: (e) => e.revealed ? t('admin.yes') : '' },
                { label: t('admin.col_time'), cell: (e) => formatStamp(e.at) },
                { label: '', cell: (e) => e.user_id ? h('span', { class: 'admin-action-row' },
                    e.revealed ? UI.btn(t('admin.act_reset_revealed'), () => Act.open({ title: t('admin.act_reset_revealed'), text: t('admin.act_reset_revealed_text'),
                        run: (v) => Api.post(`/api/admin/daily/${data.date}/scores/${e.user_id}/reset-revealed`, { reason: v.reason }), done: reload }), { kind: 'secondary' }) : null,
                    UI.btn(t('admin.delete'), () => Act.open({ title: t('admin.act_delete_score'), text: t('admin.act_delete_score_text'), danger: true,
                        run: (v) => Api.del(`/api/admin/daily/${data.date}/scores/${e.user_id}`, { reason: v.reason }), done: reload }), { kind: 'danger' })) : null },
            ] }) : h('p', { class: 'text-muted' }, t('admin.none'))));
        });
    },

    regenerate(data, reload) {
        Act.open({
            title: t('admin.act_regenerate'), text: t('admin.act_regenerate_text', { n: data.participants }), danger: true,
            fields: [{ name: 'reset_scores', type: 'checkbox', label: t('admin.f_reset_scores') }],
            run: (v) => Api.post(`/api/admin/daily/${data.date}/regenerate`, { reset_scores: v.reset_scores, reason: v.reason }),
            done: reload,
        });
    },

    archive(box) {
        loadInto(box, () => Api.get('/api/admin/daily/archive', { days: 60 }), (container, data) => {
            if (!data.items.length) { container.appendChild(UI.empty(t('admin.none'))); return; }
            container.appendChild(UI.table({ items: data.items, onRow: (d) => Router.go('daily', '', { tab: 'day', date: d.date }), columns: [
                { label: t('admin.col_date'), cell: (d) => d.date }, { label: t('admin.d_daily_best'), cell: (d) => d.best_score },
                { label: t('admin.d_daily_participants'), cell: (d) => d.participants }, { label: t('admin.col_attempts'), cell: (d) => d.attempts },
                { label: t('admin.d_avg_score'), cell: (d) => d.avg_score === null ? '–' : d.avg_score }] }));
        });
    },

    preview(box, params) {
        box.appendChild(h('p', { class: 'form-hint' }, t('admin.daily_preview_note')));
        box.appendChild(this.dateField(params, 'preview'));
        // A következő hét nap egy kattintásra
        const upcoming = [];
        for (let i = 1; i <= 7; i += 1) {
            const day = new Date(Date.now() + i * 86400000).toISOString().slice(0, 10);
            upcoming.push(UI.link(day, Router.href('daily', '', { tab: 'preview', date: day }), 'admin-inline-link admin-chip'));
        }
        box.appendChild(h('div', { class: 'admin-chip-row' }, upcoming));
        const date = params.get('date');
        if (!date) return;
        const results = h('div');
        box.appendChild(results);
        loadInto(results, () => Api.get('/api/admin/daily/preview', { date }), (container, data) => {
            container.appendChild(h('div', { class: 'admin-room-layout' },
                h('div', { class: 'admin-room-left' }, renderBoard(data.board, data.best.tiles.map((p) => ({ row: p.row, col: p.col })))),
                h('div', { class: 'admin-room-right' }, UI.card(data.date, UI.kv([[t('admin.d_daily_best'), data.best_score],
                    [t('admin.d_daily_best_words_label'), (data.best.words || []).join(', ')]]),
                h('div', { class: 'admin-rack-tiles' }, data.rack.map((l) => UI.tile(l === '?' ? '' : l, l === '?')))))));
        });
    },
};

const DAILY_SCORE_LABELS = {
    perfect: 'admin.daily_score_perfect', high: 'admin.daily_score_high', good: 'admin.daily_score_good',
    half: 'admin.daily_score_half', low: 'admin.daily_score_low',
};

registerSection({ id: 'daily', order: 80, icon: 'calendar', labelKey: 'admin.nav_daily',
    render: (view, ctx) => DailyView.render(view, ctx) });

// ===== Szótár =====

const DictionaryView = {
    render(view, ctx) {
        const tab = ctx.params.get('tab') || 'word';
        view.appendChild(UI.header(t('admin.nav_dictionary'), t('admin.dictionary_note')));
        view.appendChild(UI.tabs([
            { id: 'word', label: t('admin.tab_word') }, { id: 'rejected', label: t('admin.tab_rejected') },
            { id: 'custom', label: t('admin.tab_custom') }, { id: 'reviewers', label: t('admin.tab_reviewers') },
            { id: 'caches', label: t('admin.tab_caches') }], tab, (next) => Router.go('dictionary', '', { tab: next })));
        const body = h('div', { class: 'admin-tab-body' });
        view.appendChild(body);
        ({ word: this.word, rejected: this.rejected, custom: this.custom, reviewers: this.reviewers, caches: this.caches }[tab]
            || this.word).call(this, body, ctx.params);
    },

    // --- Szó-vizsgáló: a teljes döntési lánc ---
    word(box, params) {
        const input = h('input', { type: 'text', name: 'w', value: params.get('w') || '', maxlength: 20, autocomplete: 'off',
            placeholder: t('admin.f_word_ph') });
        const form = h('form', { class: 'admin-filters' }, h('label', { class: 'admin-field' },
            h('span', { class: 'admin-field-label' }, t('admin.f_word')), input),
        h('div', { class: 'admin-filter-actions' }, h('button', { type: 'submit', class: 'small-btn' }, t('admin.inspect'))));
        form.addEventListener('submit', (event) => {
            event.preventDefault();
            Router.go('dictionary', '', { tab: 'word', w: input.value.trim() });
        });
        box.appendChild(form);
        const word = (params.get('w') || '').trim();
        if (!word) { box.appendChild(UI.empty(t('admin.word_hint'))); return; }
        const results = h('div');
        box.appendChild(results);
        loadInto(results, () => Api.get('/api/admin/dictionary/word', { w: word }), (container, data) => this.inspect(container, data));
    },

    inspect(box, data) {
        const reload = () => Router.render();
        const yesNo = (value) => UI.badge(value ? t('admin.yes') : t('admin.no'), value ? 'ok' : 'muted');
        box.appendChild(h('div', { class: 'admin-word-head' },
            h('span', { class: 'admin-rack-tiles' }, (data.tiles || []).map((l) => UI.tile(l))),
            UI.badge(data.valid ? t('admin.word_valid') : t('admin.word_invalid'), data.valid ? 'ok' : 'danger'),
            data.score !== undefined ? h('span', { class: 'text-secondary' }, t('admin.word_score', { n: data.score })) : null));
        const balance = data.balance || {};
        box.appendChild(UI.card(t('admin.card_decision'), UI.kv([
            [t('admin.w_dictionary'), data.dictionary_available ? yesNo(data.in_dictionary) : UI.badge(t('admin.unavailable'), 'danger')],
            [t('admin.w_vowel'), yesNo(data.has_vowel)], [t('admin.w_vocabulary'), yesNo(data.in_vocabulary)],
            [t('admin.w_listed'), yesNo(data.rejected_listed)], [t('admin.w_voted'), yesNo(data.rejected_voted)],
            [t('admin.w_addition'), yesNo(data.addition)],
            [t('admin.w_override'), data.override ? UI.badge(data.override.verdict === 'allow' ? t('admin.override_allow') : t('admin.override_reject'), 'warn') : yesNo(false)],
            [t('admin.w_attested'), data.needs_attestation ? yesNo(data.attested) : null],
            [t('admin.w_balance'), t('admin.w_balance_value', { good: balance.good || 0, bad: balance.bad || 0, threshold: balance.threshold })],
        ])));
        if (data.derivations && data.derivations.length) {
            box.appendChild(UI.card(t('admin.card_derivations'), UI.table({ compact: true, items: data.derivations, columns: [
                { label: t('admin.col_stem'), cell: (d) => d.stem }, { label: t('admin.col_prefix'), cell: (d) => d.prefix || '' },
                { label: t('admin.col_suffixes'), cell: (d) => (d.suffixes || []).join(' + ') },
                { label: t('admin.col_risk'), cell: (d) => d.risky ? UI.badge((d.risk || []).join(', ') || t('admin.risky'), 'warn') : ((d.risk || []).join(', ')) }] })));
        }
        if (data.suggestions && data.suggestions.length) {
            box.appendChild(UI.card(t('admin.card_suggestions'), h('div', { class: 'admin-chip-row' },
                data.suggestions.map((w) => UI.link(w, Router.href('dictionary', '', { tab: 'word', w }))))));
        }
        box.appendChild(UI.card(t('admin.card_votes'), data.votes.length ? UI.table({ compact: true, items: data.votes, columns: [
            { label: t('admin.col_name'), cell: (v) => v.user_id ? UI.userLink(v.user_id, v.display_name) : '' },
            { label: t('admin.col_verdict'), cell: (v) => v.valid ? UI.badge(t('admin.review_valid'), 'ok') : UI.badge(t('admin.review_invalid'), 'danger') },
            { label: t('admin.col_blocked'), cell: (v) => v.blocked ? t('admin.yes') : '' },
            { label: t('admin.col_time'), cell: (v) => formatStamp(v.at) }] }) : h('p', { class: 'text-muted' }, t('admin.none'))));

        const word = data.word;
        const call = (title, text, request, options = {}) => () => Act.open({
            title, text, danger: !!options.danger, fields: options.fields, run: request, done: reload });
        const actions = [];
        actions.push(UI.btn(data.rejected_listed ? t('admin.act_unreject') : t('admin.act_reject'),
            data.rejected_listed
                ? call(t('admin.act_unreject'), t('admin.act_unreject_text'), (v) => Api.del('/api/admin/dictionary/rejected', { words: word, reason: v.reason }), { danger: true })
                : call(t('admin.act_reject'), t('admin.act_reject_text'), (v) => Api.post('/api/admin/dictionary/rejected', { words: word, reason: v.reason }), { danger: true }),
            { kind: data.rejected_listed ? 'tinted' : 'danger', disabled: !data.rejected_listed && !data.valid }));
        actions.push(UI.btn(t('admin.act_override_allow'), call(t('admin.act_override_allow'), t('admin.act_override_allow_text'),
            (v) => Api.post('/api/admin/dictionary/overrides', { word, verdict: 'allow', reason: v.reason })), { kind: 'secondary' }));
        actions.push(UI.btn(t('admin.act_override_reject'), call(t('admin.act_override_reject'), t('admin.act_override_reject_text'),
            (v) => Api.post('/api/admin/dictionary/overrides', { word, verdict: 'reject', reason: v.reason }), { danger: true }), { kind: 'secondary' }));
        if (data.override) {
            actions.push(UI.btn(t('admin.act_override_remove'), call(t('admin.act_override_remove'), '',
                (v) => Api.del('/api/admin/dictionary/overrides', { word, reason: v.reason })), { kind: 'secondary' }));
        }
        actions.push(data.addition
            ? UI.btn(t('admin.act_addition_remove'), call(t('admin.act_addition_remove'), '',
                (v) => Api.del('/api/admin/dictionary/additions', { word, reason: v.reason }), { danger: true }), { kind: 'secondary' })
            : UI.btn(t('admin.act_addition_add'), call(t('admin.act_addition_add'), t('admin.act_addition_add_text'),
                (v) => Api.post('/api/admin/dictionary/additions', { word, reason: v.reason })), { kind: 'secondary', disabled: data.in_dictionary }));
        if (data.votes.length) {
            actions.push(UI.btn(t('admin.act_votes_delete') + ' 🔒', call(t('admin.act_votes_delete'), t('admin.act_votes_delete_text'),
                (v) => Api.del('/api/admin/dictionary/votes', { word, reason: v.reason }), { danger: true }), { kind: 'danger' }));
        }
        box.appendChild(UI.card(t('admin.card_word_actions'), h('div', { class: 'admin-action-row' }, actions)));
    },

    // --- Kizárt szavak ---
    rejected(box, params) {
        const view = h('div');
        const importCard = UI.card(t('admin.card_bulk'), h('p', { class: 'form-hint' }, t('admin.bulk_note')));
        const area = h('textarea', { rows: 4, placeholder: t('admin.bulk_ph') });
        const result = h('div');
        const previewBtn = UI.btn(t('admin.bulk_preview'), async () => {
            const text = area.value.trim();
            if (!text) return;
            const response = await Api.post('/api/admin/dictionary/rejected/preview', { words: text });
            if (!response.ok) { result.replaceChildren(UI.errorBox(Api.message(response))); return; }
            const d = response.data;
            result.replaceChildren(h('p', null, t('admin.bulk_summary', { fresh: d.new.length, already: d.already.length,
                invalid: d.invalid.length, bad: d.bad ? d.bad.length : 0 })),
            d.new.length ? h('div', { class: 'admin-chip-row' }, d.new.slice(0, 80).map((w) => UI.badge(w, 'info'))) : null,
            d.new.length ? UI.btn(t('admin.bulk_add'), () => Act.open({
                title: t('admin.bulk_add'), text: t('admin.bulk_add_text', { n: d.new.length }), danger: true,
                run: (v) => Api.post('/api/admin/dictionary/rejected', { words: text, reason: v.reason }),
                done: () => Router.render() })) : null);
        }, { kind: 'secondary' });
        importCard.append(area, h('div', { class: 'admin-action-row' }, previewBtn,
            UI.download(t('admin.list_download'), '/api/admin/dictionary/rejected/download'),
            UI.download(t('admin.list_diff'), '/api/admin/dictionary/rejected/download', { diff: 1 }),
            UI.btn(t('admin.export_votes') + ' 🔒', () => Act.open({
                title: t('admin.export_votes'), text: t('admin.export_votes_text'), danger: true,
                run: (v) => Api.post('/api/admin/dictionary/rejected/export-votes', { reason: v.reason }),
                done: (r) => { showToast(t('admin.export_votes_done', { n: (r.new || []).length })); Router.render(); } }), { kind: 'secondary' })),
        result);
        box.appendChild(importCard);
        box.appendChild(view);
        mountList(view, params, {
            section: 'dictionary', keep: ['tab'], title: t('admin.card_rejected_list'), path: '/api/admin/dictionary/rejected', pageSize: 100,
            filters: [
                { name: 'q', label: t('admin.f_word') },
                { name: 'source', label: t('admin.f_source'), type: 'select', options: [{ value: '', label: t('admin.any') },
                    { value: 'listed', label: t('admin.src_listed') }, { value: 'voted', label: t('admin.src_voted') }] },
            ],
            columns: [
                { label: t('admin.col_word'), cell: (w) => UI.link(w.word, Router.href('dictionary', '', { tab: 'word', w: w.word })) },
                { label: t('admin.col_source'), cell: (w) => [w.listed ? UI.badge(t('admin.src_listed'), 'info') : null,
                    w.voted ? UI.badge(t('admin.src_voted'), 'warn') : null, w.allowed ? UI.badge(t('admin.override_allow'), 'ok') : null] },
                { label: t('admin.col_votes'), cell: (w) => `${w.good} / ${w.bad}` },
                { label: '', cell: (w) => w.listed ? UI.btn(t('admin.act_unreject'), () => Act.open({
                    title: t('admin.act_unreject'), text: t('admin.act_unreject_text'), danger: true,
                    run: (v) => Api.del('/api/admin/dictionary/rejected', { words: w.word, reason: v.reason }),
                    done: () => Router.render() }), { kind: 'secondary' }) : null },
            ],
            summary: (data) => h('p', { class: 'form-hint' }, t('admin.rejected_counts', { listed: data.counts.listed, voted: data.counts.voted })),
        });
    },

    // --- Felülbírálatok és saját szavak ---
    custom(box) {
        const reload = () => Router.render();
        const overrides = h('div');
        const additions = h('div');
        box.append(overrides, additions);
        loadInto(overrides, () => Api.get('/api/admin/dictionary/overrides'), (container, data) => {
            container.appendChild(UI.card(t('admin.card_overrides'), h('p', { class: 'form-hint' }, t('admin.overrides_note')),
                UI.btn(t('admin.act_override_add'), () => Act.open({
                    title: t('admin.act_override_add'),
                    fields: [{ name: 'word', label: t('admin.f_word'), required: true, maxlength: 20 },
                        { name: 'verdict', type: 'select', label: t('admin.f_verdict'), options: [
                            { value: 'allow', label: t('admin.override_allow') }, { value: 'reject', label: t('admin.override_reject') }] }],
                    run: (v) => Api.post('/api/admin/dictionary/overrides', { word: v.word, verdict: v.verdict, reason: v.reason }), done: reload }), { kind: 'secondary' }),
                data.items.length ? UI.table({ compact: true, items: data.items, columns: [
                    { label: t('admin.col_word'), cell: (o) => UI.link(o.word, Router.href('dictionary', '', { tab: 'word', w: o.word })) },
                    { label: t('admin.f_verdict'), cell: (o) => UI.badge(o.verdict === 'allow' ? t('admin.override_allow') : t('admin.override_reject'), o.verdict === 'allow' ? 'ok' : 'danger') },
                    { label: t('admin.col_reason'), cell: (o) => o.reason || '' }, { label: t('admin.col_admin'), cell: (o) => o.admin || '' },
                    { label: t('admin.col_time'), cell: (o) => formatStamp(o.at) },
                    { label: '', cell: (o) => UI.btn(t('admin.delete'), () => Act.open({ title: t('admin.act_override_remove'),
                        run: (v) => Api.del('/api/admin/dictionary/overrides', { word: o.word, reason: v.reason }), done: reload }), { kind: 'secondary' }) }] })
                    : h('p', { class: 'text-muted' }, t('admin.none'))));
        });
        loadInto(additions, () => Api.get('/api/admin/dictionary/additions'), (container, data) => {
            container.appendChild(UI.card(t('admin.card_additions'), h('p', { class: 'form-hint' }, t('admin.additions_note')),
                UI.btn(t('admin.act_addition_add'), () => Act.open({
                    title: t('admin.act_addition_add'), text: t('admin.act_addition_add_text'),
                    fields: [{ name: 'word', label: t('admin.f_word'), required: true, maxlength: 20 }],
                    run: (v) => Api.post('/api/admin/dictionary/additions', { word: v.word, reason: v.reason }), done: reload }), { kind: 'secondary' }),
                data.items.length ? UI.table({ compact: true, items: data.items, columns: [
                    { label: t('admin.col_word'), cell: (o) => UI.link(o.word, Router.href('dictionary', '', { tab: 'word', w: o.word })) },
                    { label: t('admin.col_reason'), cell: (o) => o.reason || '' }, { label: t('admin.col_admin'), cell: (o) => o.admin || '' },
                    { label: t('admin.col_time'), cell: (o) => formatStamp(o.at) },
                    { label: '', cell: (o) => UI.btn(t('admin.delete'), () => Act.open({ title: t('admin.act_addition_remove'), danger: true,
                        run: (v) => Api.del('/api/admin/dictionary/additions', { word: o.word, reason: v.reason }), done: reload }), { kind: 'danger' }) }] })
                    : h('p', { class: 'text-muted' }, t('admin.none'))));
        });
    },

    // --- Szótár-építő felügyelet ---
    reviewers(box) {
        const reload = () => Router.render();
        loadInto(box, async () => {
            const [summary, reviewers, threshold, second] = await Promise.all([
                Api.get('/api/admin/dictionary/review-summary', { days: 30 }), Api.get('/api/admin/dictionary/reviewers'),
                Api.get('/api/admin/dictionary/threshold'), Api.get('/api/admin/dictionary/second-opinion', { n: 20 })]);
            for (const part of [summary, reviewers, threshold, second]) if (!part.ok) return part;
            return { ok: true, data: { summary: summary.data, reviewers: reviewers.data.items, threshold: threshold.data, second: second.data.items } };
        }, (container, data) => {
            const s = data.summary;
            container.appendChild(h('div', { class: 'admin-stat-grid' },
                UI.stat(t('admin.rv_decisions'), fmtNum(s.decisions)), UI.stat(t('admin.rv_words'), fmtNum(s.words_reviewed)),
                UI.stat(t('admin.rv_reviewers'), fmtNum(s.reviewers), { hint: t('admin.rv_active', { n: s.active_reviewers }) }),
                UI.stat(t('admin.rv_rejected_voted'), fmtNum(s.rejected)), UI.stat(t('admin.rv_rejected_listed'), fmtNum(s.listed))));
            if (s.daily.length > 1) {
                container.appendChild(UI.card(null, Chart.render({ title: t('admin.rv_chart'), days: s.daily.map((d) => d.day), type: 'bar',
                    series: [{ label: t('admin.rv_decisions'), values: s.daily.map((d) => d.decisions) },
                        { label: t('admin.rv_rejections'), values: s.daily.map((d) => d.rejections) }] })));
            }
            const th = data.threshold;
            container.appendChild(UI.card(t('admin.card_threshold'), h('p', { class: 'form-hint' }, t('admin.threshold_note')),
                UI.kv([[t('admin.threshold_value'), th.value + (th.overridden ? ' (' + t('admin.overridden') + ')' : '')], [t('admin.threshold_default'), th.default]]),
                UI.btn(t('admin.act_threshold'), () => Act.open({
                    title: t('admin.act_threshold'), text: t('admin.act_threshold_text'), danger: true,
                    fields: [{ name: 'value', type: 'number', label: t('admin.threshold_value'), value: th.value, min: 1, max: 50, required: true }],
                    run: (v) => Api.patch('/api/admin/settings', { changes: { word_reject_threshold: v.value }, reason: v.reason }), done: reload }), { kind: 'secondary' })));
            container.appendChild(UI.card(t('admin.card_reviewers'), data.reviewers.length ? UI.table({ compact: true, items: data.reviewers,
                rowClass: (r) => r.suspicious ? 'row-alert' : '', onRow: (r) => Router.go('users', r.user_id), columns: [
                    { label: t('admin.col_name'), cell: (r) => UI.userLink(r.user_id, r.display_name) },
                    { label: t('admin.col_decisions'), cell: (r) => r.total }, { label: t('admin.col_invalid'), cell: (r) => `${r.invalid} (${fmtPercent(r.invalid_share)})` },
                    { label: t('admin.col_agreement'), cell: (r) => r.agreement === null ? '–' : fmtPercent(r.agreement) },
                    { label: t('admin.col_last_seen'), cell: (r) => formatStamp(r.last_at) },
                    { label: t('admin.col_status'), cell: (r) => [r.blocked ? UI.badge(t('admin.st_review_blocked'), 'warn') : null,
                        r.suspicious ? UI.badge(t('admin.suspicious'), 'danger') : null] }] }) : h('p', { class: 'text-muted' }, t('admin.none'))));
            container.appendChild(UI.card(t('admin.card_second_opinion'), h('p', { class: 'form-hint' }, t('admin.second_note')),
                data.second.length ? h('div', { class: 'admin-chip-row' }, data.second.map((w) => UI.link(w.word, Router.href('dictionary', '', { tab: 'word', w: w.word }), 'admin-inline-link admin-chip'))) : h('p', { class: 'text-muted' }, t('admin.none'))));
        });
    },

    // --- Gyorsítótárak ---
    caches(box) {
        const names = [['valid', 'admin.cache_valid'], ['short_words', 'admin.cache_short'], ['vocabulary', 'admin.cache_vocabulary'],
            ['analysis', 'admin.cache_analysis']];
        box.appendChild(UI.card(t('admin.card_caches'), h('p', { class: 'form-hint' }, t('admin.caches_note')),
            h('div', { class: 'admin-action-row' }, names.map(([which, key]) => UI.btn(t(key), () => Act.open({
                title: t(key), text: t('admin.cache_clear_text'), danger: true,
                run: (v) => Api.post('/api/admin/dictionary/caches/clear', { which, reason: v.reason }),
                done: (r) => showToast(t('admin.cache_cleared', { seconds: r.seconds })) }), { kind: 'secondary' })))));
    },
};

registerSection({ id: 'dictionary', order: 70, icon: 'book', labelKey: 'admin.nav_dictionary',
    render: (view, ctx) => DictionaryView.render(view, ctx) });
