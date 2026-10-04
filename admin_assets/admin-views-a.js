'use strict';
// ===== ADMIN PANEL — nézetek I.: áttekintés, felhasználók, élő szobák =====

// ----- Közös: letöltés sudo-val (a böngészős letöltés nem tud jelszót kérni, ezért előbb megerősítünk) -----

async function downloadWithSudo(path) {
    if (Session.sudoSeconds() <= 0 && !(await Sudo.prompt())) return;
    location.href = path;
}

// ===== Áttekintés =====

const ALERT_TEXT = {
    dictionary_down: 'admin.al_dictionary_down',
    smtp_missing: 'admin.al_smtp_missing',
    push_missing: 'admin.al_push_missing',
    stuck_games: 'admin.al_stuck_games',
    async_overdue: 'admin.al_async_overdue',
    failed_logins: 'admin.al_failed_logins',
    review_spike: 'admin.al_review_spike',
    backup_old: 'admin.al_backup_old',
};

const ALERT_LINK = {
    dictionary_down: () => Router.href('system'),
    smtp_missing: () => Router.href('system'),
    push_missing: () => Router.href('system'),
    stuck_games: () => Router.href('rooms', '', { stuck: 1 }),
    async_overdue: () => Router.href('async'),
    failed_logins: () => Router.href('security', '', { tab: 'logins', flagged: 1 }),
    review_spike: () => Router.href('dictionary', '', { tab: 'reviewers' }),
    backup_old: () => Router.href('system'),
};

const OverviewView = {
    async render(view) {
        const quick = [
            UI.btn(t('admin.quick_announce'), () => Router.go('comm', '', { tab: 'announcements' }), { kind: 'secondary' }),
            UI.btn(t('admin.quick_maintenance'), () => Router.go('comm', '', { tab: 'maintenance' }), { kind: 'secondary' }),
            UI.btn(t('admin.quick_backup'), () => downloadWithSudo('/api/admin/system/backup'), { kind: 'secondary' }),
            UI.btn(t('admin.refresh'), () => refresh(), { kind: 'secondary' }),
        ];
        view.appendChild(UI.header(t('admin.nav_overview'), t('admin.overview_note'), ...quick));
        const alertsBox = h('div', { class: 'admin-alerts' });
        const liveBox = h('div');
        const todayBox = h('div');
        const serverBox = h('div');
        const chartsBox = h('div');
        view.append(alertsBox, liveBox, todayBox, serverBox, chartsBox);

        let last = null;
        const paint = (data) => {
            last = Object.assign(last || {}, data);
            if (data.alerts) this.alerts(alertsBox, data.alerts);
            if (last.live) this.live(liveBox, last.live);
            if (data.today) this.today(todayBox, data.today);
            if (last.server) this.server(serverBox, last.server, last.services, last.versions);
            if ('maintenance' in data) Nav.showMaintenance(data.maintenance);
        };
        const refresh = async () => {
            const result = await Api.get('/api/admin/overview');
            if (result.ok) paint(result.data);
            else if (!last) view.replaceChildren(UI.errorBox(Api.message(result), () => Router.render()));
        };
        await refresh();
        if (!view.isConnected) return;
        loadInto(chartsBox, () => Api.get('/api/admin/charts', { days: 30 }), (box, data) => this.charts(box, data));

        // Élő: a Socket.IO `admin_overview` ötmásodpercenként; nélküle időzítős lekérdezés
        Live.on('admin_overview', (payload) => paint({ live: payload.live, server: payload.server }));
        Router.interval(() => { if (!Live.connected) refresh(); }, 5000);
        Router.interval(refresh, 60000);   // a lassabban változó adatok (figyelmeztetések, mai számok)
    },

    alerts(box, alerts) {
        box.replaceChildren();
        for (const alert of alerts) {
            const text = t(ALERT_TEXT[alert.code] || 'admin.al_unknown');
            const link = ALERT_LINK[alert.code];
            const card = h(link ? 'a' : 'div', { class: 'admin-alert alert-' + alert.level, href: link ? link() : null },
                makeIcon('flag'),
                h('div', null, h('strong', null, alert.count ? text + ' (' + alert.count + ')' : text),
                    alert.detail ? h('div', { class: 'admin-alert-detail' }, alert.detail) : null));
            box.appendChild(card);
        }
        if (!alerts.length) box.appendChild(h('div', { class: 'admin-alert alert-green' }, makeIcon('shield'),
            h('strong', null, t('admin.al_none'))));
    },

    live(box, live) {
        box.replaceChildren(UI.subtitle(t('admin.live_title')), h('div', { class: 'admin-stat-grid' },
            UI.stat(t('admin.live_users'), fmtNum(live.online_users), { href: Router.href('users', '', { online: 1 }),
                hint: t('admin.live_guests', { n: live.online_guests }) }),
            UI.stat(t('admin.live_sockets'), fmtNum(live.sockets)),
            UI.stat(t('admin.live_rooms'), fmtNum(live.rooms_total), { href: Router.href('rooms'),
                hint: t('admin.live_rooms_split', { waiting: live.rooms_waiting, active: live.rooms_active,
                    async: live.rooms_async, puzzle: live.rooms_puzzle }) }),
            UI.stat(t('admin.live_visibility'), live.rooms_public + ' / ' + live.rooms_private,
                { hint: t('admin.live_visibility_hint') }),
            UI.stat(t('admin.live_games'), fmtNum(live.games_running), { hint: t('admin.live_bots', { n: live.games_with_bots }) }),
            UI.stat(t('admin.live_spectators'), fmtNum(live.spectators)),
            UI.stat(t('admin.live_votes'), fmtNum(live.pending_votes)),
            UI.stat(t('admin.live_timers'), fmtNum(live.turn_timers)),
            UI.stat(t('admin.live_grace'), fmtNum(live.grace_players), { kind: live.grace_players ? 'warn' : '' })));
    },

    today(box, today) {
        box.replaceChildren(UI.subtitle(t('admin.today_title')), h('div', { class: 'admin-stat-grid' },
            UI.stat(t('admin.today_registrations'), fmtNum(today.registrations)),
            UI.stat(t('admin.today_games'), fmtNum(today.finished_games)),
            UI.stat(t('admin.today_daily'), fmtNum(today.daily_attempts)),
            UI.stat(t('admin.today_review'), fmtNum(today.review_decisions),
                { hint: t('admin.today_review_hint', { n: today.review_rejections }) }),
            UI.stat(t('admin.today_rejected'), fmtNum(today.rejected_words), { href: Router.href('dictionary') })));
    },

    server(box, server, services, versions) {
        const yes = (ok) => UI.badge(ok ? t('admin.yes') : t('admin.no'), ok ? 'ok' : 'danger');
        const rows = [
            [t('admin.srv_uptime'), fmtDuration(server.uptime)],
            [t('admin.srv_memory'), fmtBytes(server.rss)],
            [t('admin.srv_cpu'), server.cpu_percent === null ? '–' : fmtPercent(server.cpu_percent)],
            [t('admin.srv_greenlets'), fmtNum(server.greenlets)],
            [t('admin.srv_threads'), fmtNum(server.threads)],
            [t('admin.srv_db'), fmtBytes(server.db_size)],
            [t('admin.srv_python'), server.python],
        ];
        if (services) {
            rows.push([t('admin.srv_dictionary'), yes(services.dictionary)],
                [t('admin.srv_vocabulary'), yes(services.vocabulary)],
                [t('admin.srv_push'), yes(services.push)],
                [t('admin.srv_smtp'), yes(services.smtp)],
                [t('admin.srv_tunnel'), services.tunnel && services.tunnel.url
                    ? h('span', null, UI.badge(t('admin.on'), 'ok'), ' ', services.tunnel.url)
                    : UI.badge((services.tunnel && services.tunnel.state) || '–', 'muted')]);
        }
        if (versions) rows.push([t('admin.srv_assets'), String(versions.asset_version)], [t('admin.srv_git'), versions.git, { wide: true }]);
        box.replaceChildren(UI.subtitle(t('admin.server_title')), UI.card(null, UI.kv(rows)));
    },

    charts(box, data) {
        const series = (key) => data[key].map((p) => p.value);
        box.replaceChildren(UI.subtitle(t('admin.charts_title')), h('div', { class: 'admin-chart-grid' },
            UI.card(null, Chart.render({ title: t('admin.chart_registrations'), days: data.days,
                series: [{ label: t('admin.chart_registrations'), values: series('registrations') }], type: 'bar' })),
            UI.card(null, Chart.render({ title: t('admin.chart_active'), days: data.days,
                series: [{ label: t('admin.chart_active'), values: series('active_users') }], type: 'line' })),
            UI.card(null, Chart.render({ title: t('admin.chart_games'), days: data.days, type: 'stack',
                series: [{ label: t('admin.chart_games_human'), values: series('games_human') },
                    { label: t('admin.chart_games_bots'), values: series('games_bots') }] })),
            UI.card(null, Chart.render({ title: t('admin.chart_daily'), days: data.days,
                series: [{ label: t('admin.chart_daily'), values: series('daily_players') }], type: 'bar' }))));
    },
};

registerSection({ id: 'overview', order: 10, icon: 'home', labelKey: 'admin.nav_overview',
    render: (view) => OverviewView.render(view) });

// ===== Felhasználók =====

const USER_STATUS_OPTIONS = () => [
    { value: '', label: t('admin.any') },
    { value: 'active', label: t('admin.st_active') },
    { value: 'banned', label: t('admin.st_banned') },
    { value: 'muted', label: t('admin.st_muted') },
    { value: 'deleted', label: t('admin.st_deleted') },
    { value: 'review_blocked', label: t('admin.st_review_blocked') },
];

function userStatusBadges(user) {
    const out = [];
    if (user.deleted) out.push(UI.badge(t('admin.st_deleted'), 'muted'));
    else if (user.banned) out.push(UI.badge(user.ban && user.ban.permanent ? t('admin.st_banned_perm') : t('admin.st_banned'), 'danger'));
    if (user.muted && !user.deleted) out.push(UI.badge(t('admin.st_muted'), 'warn'));
    if (user.review_blocked && !user.deleted) out.push(UI.badge(t('admin.st_review_blocked'), 'warn'));
    if (user.is_admin) out.push(UI.badge(t('admin.st_admin'), 'info'));
    if (!out.length) out.push(UI.badge(t('admin.st_active'), 'ok'));
    return out;
}

const UsersView = {
    render(view, ctx) {
        if (ctx.arg) return UserDetailView.render(view, ctx);
        mountList(view, ctx.params, {
            section: 'users', title: t('admin.nav_users'), note: t('admin.users_note'), path: '/api/admin/users',
            defaultSort: 'id', defaultOrder: 'desc',
            filters: [
                { name: 'q', label: t('admin.f_user_q'), placeholder: t('admin.f_user_q_ph') },
                { name: 'status', label: t('admin.f_status'), type: 'select', options: USER_STATUS_OPTIONS() },
                { name: 'online', label: t('admin.f_online'), type: 'checkbox' },
                { name: 'admin', label: t('admin.f_admins'), type: 'checkbox' },
                { name: 'from', label: t('admin.f_registered_from'), type: 'date' },
                { name: 'to', label: t('admin.f_registered_to'), type: 'date' },
                { name: 'min_games', label: t('admin.f_min_games'), type: 'number' },
                { name: 'inactive_days', label: t('admin.f_inactive_days'), type: 'number' },
            ],
            columns: [
                { label: t('admin.col_id'), sortKey: 'id', cell: (u) => '#' + u.id },
                { label: t('admin.col_name'), sortKey: 'name', cell: (u) => h('span', null, UI.dot(u.online), ' ',
                    UI.userLink(u.id, u.display_name)) },
                { label: t('admin.col_email'), sortKey: 'email', cell: (u) => u.email },
                { label: t('admin.col_registered'), sortKey: 'created_at', cell: (u) => formatStamp(u.created_at) },
                { label: t('admin.col_last_login'), sortKey: 'last_login_at', cell: (u) => formatStamp(u.last_login_at) || '–' },
                { label: t('admin.col_games'), sortKey: 'games_played', cell: (u) => u.games_played + ' / ' + u.games_won },
                { label: t('admin.col_rating'), sortKey: 'rating', cell: (u) => u.rating },
                { label: t('admin.col_status'), cell: (u) => userStatusBadges(u) },
                { label: t('admin.col_push'), cell: (u) => u.push_devices || 0 },
            ],
            onRow: (u) => Router.go('users', u.id),
        });
    },
};

registerSection({ id: 'users', order: 20, icon: 'users', labelKey: 'admin.nav_users',
    render: (view, ctx) => UsersView.render(view, ctx) });

// ----- Egy felhasználó oldala -----

const UserDetailView = {
    async render(view, ctx) {
        const id = ctx.arg;
        const tab = ctx.params.get('tab') || 'overview';
        const load = () => Api.get(`/api/admin/users/${encodeURIComponent(id)}`);
        const reload = () => Router.render();
        const holder = h('div');
        view.appendChild(holder);
        await loadInto(holder, load, (box, data) => {
            const user = data.user;
            box.appendChild(h('div', { class: 'admin-breadcrumb' }, UI.link(t('admin.nav_users'), Router.href('users'))));
            box.appendChild(UI.header(h('span', null, UI.dot(data.live.online), ' ', user.display_name),
                '#' + user.id + ' · ' + user.email,
                ...(data.live.rooms || []).map((room) => UI.link(t('admin.jump_room', { name: room.name }),
                    Router.href('rooms', room.room_id), 'admin-link-btn'))));
            box.appendChild(h('div', { class: 'admin-badge-row' }, userStatusBadges(user)));
            const tabs = [
                { id: 'overview', label: t('admin.tab_overview') }, { id: 'actions', label: t('admin.tab_actions') },
                { id: 'games', label: t('admin.tab_games') }, { id: 'stats', label: t('admin.tab_stats') },
                { id: 'security', label: t('admin.tab_security') }, { id: 'notes', label: t('admin.tab_notes') },
            ];
            box.appendChild(UI.tabs(tabs, tab, (next) => Router.go('users', id, { tab: next })));
            const body = h('div', { class: 'admin-tab-body' });
            box.appendChild(body);
            const render = { overview: this.overview, actions: this.actions, games: this.games, stats: this.stats,
                security: this.security, notes: this.notes }[tab] || this.overview;
            render.call(this, body, data, reload);
        });
    },

    overview(box, data) {
        const user = data.user;
        const ban = user.ban;
        const accountCard = UI.card(t('admin.card_account'), UI.kv([
            [t('admin.col_id'), '#' + user.id], [t('admin.col_email'), user.email], [t('admin.col_name'), user.display_name],
            [t('admin.col_registered'), formatStamp(user.created_at)],
            [t('admin.col_last_login'), formatStamp(user.last_login_at)],
            [t('admin.d_last_ip'), user.last_login_ip],
            [t('admin.col_status'), h('span', null, userStatusBadges(user))],
            [t('admin.d_ban_reason'), ban ? ban.reason : null, { wide: true }],
            [t('admin.d_ban_until'), ban ? (ban.permanent ? t('admin.dur_opt_permanent') : formatStamp(ban.until)) : null],
            [t('admin.d_muted_until'), user.muted ? formatStamp(user.muted_until) : null],
            [t('admin.d_deleted_at'), user.deleted ? formatStamp(user.deleted_at) : null],
            [t('admin.d_connections'), data.live.online ? t('admin.d_connections_n', { n: data.live.connections }) : null],
        ]));
        const statsCard = UI.card(t('admin.card_stats'), UI.kv([
            [t('admin.col_games'), `${user.games_played} / ${user.games_won}`],
            [t('admin.d_win_rate'), fmtPercent(user.win_rate)],
            [t('admin.d_avg_score'), fmtNum(user.avg_score, 1)],
            [t('admin.col_rating'), `${user.rating} (${t('admin.d_rated_games', { n: user.rated_games })})`],
        ]));
        const friendsCard = UI.card(t('admin.card_friends'),
            data.friends.length ? h('div', { class: 'admin-chip-row' }, data.friends.map((f) => UI.userLink(f.id, f.display_name)))
                : h('p', { class: 'text-muted' }, t('admin.none')),
            data.pending_in.length || data.pending_out.length ? UI.kv([
                [t('admin.d_pending_in'), data.pending_in.map((f) => f.display_name).join(', ')],
                [t('admin.d_pending_out'), data.pending_out.map((f) => f.display_name).join(', ')],
            ]) : null);
        box.appendChild(UI.cardGrid(accountCard, h('div', { class: 'admin-card-stack' }, statsCard, friendsCard)));
        box.appendChild(UI.card(t('admin.card_history'), data.admin_history.length
            ? UI.table({ compact: true, items: data.admin_history, columns: [
                { label: t('admin.col_time'), cell: (e) => formatStamp(e.created_at) },
                { label: t('admin.col_admin'), cell: (e) => e.admin_name || '?' },
                { label: t('admin.col_action'), cls: 'admin-action', cell: (e) => e.action },
                { label: t('admin.col_reason'), cell: (e) => e.reason || '' },
            ] })
            : h('p', { class: 'text-muted' }, t('admin.none'))));
    },

    actions(box, data, reload) {
        const u = data.user;
        const self = data.is_self;
        const isAdmin = u.is_admin;
        const protectedAccount = u.deleted;
        const reasonOnly = (title, text, request, options = {}) => () => Act.open({
            title, text, run: (v) => request(v), done: reload, ...options,
        });
        const group = (title, buttons) => UI.card(title, h('div', { class: 'admin-action-row' }, buttons));
        const sudoTag = (label) => label + ' 🔒';

        const rename = () => Act.open({
            title: t('admin.act_rename'), fields: [{ name: 'display_name', label: t('admin.col_name'), value: u.display_name,
                required: true, maxlength: 20 }],
            run: (v) => Api.patch(`/api/admin/users/${u.id}`, { display_name: v.display_name, reason: v.reason }),
            done: reload,
        });
        const changeEmail = () => Act.open({
            title: t('admin.act_email'), text: t('admin.act_email_text'),
            fields: [{ name: 'email', type: 'email', label: t('admin.col_email'), value: u.email, required: true }],
            run: (v) => Api.patch(`/api/admin/users/${u.id}`, { email: v.email, reason: v.reason }), done: reload,
        });
        const resetPassword = () => Act.open({
            title: t('admin.act_reset_password'), text: t('admin.act_reset_password_text'), danger: true,
            run: (v) => Api.post(`/api/admin/users/${u.id}/reset-password`, { reason: v.reason }),
            done: (result) => {
                if (result.temp_password) {
                    Modal.show(t('admin.act_reset_password'), h('div', null, h('p', null, t('admin.temp_password_text')),
                        h('pre', { class: 'admin-json' }, result.temp_password), UI.copy(result.temp_password)));
                }
                reload();
            },
        });
        const logoutAll = reasonOnly(t('admin.act_logout_all'), t('admin.act_logout_all_text'),
            (v) => Api.post(`/api/admin/users/${u.id}/logout-all`, { reason: v.reason }), { danger: true });
        const ban = () => Act.open({
            title: t('admin.act_ban'), text: t('admin.act_ban_text'), danger: true, confirmName: u.display_name,
            fields: [{ name: 'until', type: 'select', label: t('admin.f_duration'), options: durationOptions(true), value: '7d' }],
            run: (v) => Api.post(`/api/admin/users/${u.id}/ban`, { until: v.until, reason: v.reason }), done: reload,
        });
        const unban = reasonOnly(t('admin.act_unban'), '', (v) => Api.post(`/api/admin/users/${u.id}/unban`, { reason: v.reason }));
        const mute = () => Act.open({
            title: t('admin.act_mute'), text: t('admin.act_mute_text'),
            fields: [{ name: 'until', type: 'select', label: t('admin.f_duration'), options: durationOptions(true), value: '1d' }],
            run: (v) => Api.post(`/api/admin/users/${u.id}/mute`, { until: v.until, reason: v.reason }), done: reload,
        });
        const unmute = reasonOnly(t('admin.act_unmute'), '', (v) => Api.post(`/api/admin/users/${u.id}/unmute`, { reason: v.reason }));
        const blockReview = (blocked) => reasonOnly(blocked ? t('admin.act_review_block') : t('admin.act_review_unblock'),
            blocked ? t('admin.act_review_block_text') : '',
            (v) => Api.post(`/api/admin/users/${u.id}/reviews/block`, { blocked, reason: v.reason }));
        const revertReviews = () => Act.open({
            title: t('admin.act_review_revert'), text: t('admin.act_review_revert_text'), danger: true,
            fields: [{ name: 'since', type: 'datetime', label: t('admin.f_since_optional'), hint: t('admin.hint_utc') }],
            run: (v) => Api.post(`/api/admin/users/${u.id}/reviews/revert`, { since: v.since || null, reason: v.reason }),
            done: (result) => { showToast(t('admin.reverted_n', { n: result.count })); reload(); },
        });
        const setRating = () => Act.open({
            title: t('admin.act_rating'), text: t('admin.act_rating_text'),
            fields: [{ name: 'rating', type: 'number', label: t('admin.col_rating'), value: u.rating, required: true, min: 100, max: 4000 }],
            run: (v) => Api.post(`/api/admin/users/${u.id}/rating`, { rating: v.rating, reason: v.reason }), done: reload,
        });
        const recompute = reasonOnly(t('admin.act_recompute'), t('admin.act_recompute_text'),
            (v) => Api.post(`/api/admin/users/${u.id}/recompute-stats`, { reason: v.reason }));
        const badge = () => {
            const owned = new Set(data.badges.map((b) => b.badge));
            Act.open({
                title: t('admin.act_badge'),
                fields: [
                    { name: 'badge', type: 'select', label: t('admin.f_badge'),
                        options: data.available_badges.map((b) => ({ value: b, label: b + (owned.has(b) ? ' ✓' : '') })) },
                    { name: 'grant', type: 'select', label: t('admin.f_badge_action'),
                        options: [{ value: true, label: t('admin.badge_grant') }, { value: false, label: t('admin.badge_revoke') }] },
                ],
                run: (v) => Api.post(`/api/admin/users/${u.id}/badges`, { badge: v.badge, grant: v.grant, reason: v.reason }),
                done: reload,
            });
        };
        const pushTest = async () => {
            const result = await Api.post(`/api/admin/users/${u.id}/push-test`);
            if (result.ok) showToast(t('admin.push_test_sent', { n: result.data.sent || 0 }));
            else showToast(Api.message(result), true);
        };
        const del = () => Act.open({
            title: t('admin.act_delete'), text: t('admin.act_delete_text'), danger: true, confirmName: u.display_name,
            fields: [{ name: 'mode', type: 'select', label: t('admin.f_delete_mode'), value: 'anonymize',
                options: [{ value: 'anonymize', label: t('admin.delete_anonymize') }, { value: 'delete', label: t('admin.delete_full') }] }],
            run: (v) => Api.del(`/api/admin/users/${u.id}`, { mode: v.mode, confirm_name: v.confirm_name, reason: v.reason }),
            done: () => Router.go('users'),
        });
        const profile = async () => {
            const result = await Api.get(`/api/admin/users/${u.id}/profile`);
            if (!result.ok) { showToast(Api.message(result), true); return; }
            const p = result.data.profile;
            Modal.show(t('admin.act_profile', { name: p.display_name }), h('div', null,
                h('p', { class: 'form-hint' }, t('admin.profile_note')),
                UI.kv([[t('admin.col_games'), `${p.stats.games_played} / ${p.stats.games_won}`],
                    [t('admin.col_rating'), p.stats.rating], [t('admin.d_avg_score'), fmtNum(p.stats.avg_score, 1)],
                    [t('admin.d_badges'), p.badges.join(', ')]]),
                UI.table({ compact: true, items: p.history, columns: [
                    { label: t('admin.col_time'), cell: (g) => formatStamp(g.created_at) },
                    { label: t('admin.col_game'), cell: (g) => UI.gameLink(g.game_id, g.room_name) },
                    { label: t('admin.col_score'), cell: (g) => g.final_score + (g.is_winner ? ' ★' : '') },
                    { label: t('admin.col_rating_change'), cell: (g) => g.rating_change === null ? '–' : (g.rating_change > 0 ? '+' : '') + g.rating_change },
                ] })), { wide: true });
        };

        if (self) box.appendChild(h('p', { class: 'form-hint' }, t('admin.self_note')));
        const groups = [
            group(t('admin.grp_account'), [
                UI.btn(t('admin.act_rename'), rename, { kind: 'secondary', disabled: protectedAccount }),
                UI.btn(sudoTag(t('admin.act_email')), changeEmail, { kind: 'secondary', disabled: protectedAccount || isAdmin }),
                UI.btn(sudoTag(t('admin.act_reset_password')), resetPassword, { kind: 'secondary', disabled: protectedAccount || isAdmin }),
                UI.btn(t('admin.act_logout_all'), logoutAll, { kind: 'secondary', disabled: protectedAccount }),
                UI.btn(t('admin.act_profile_short'), profile, { kind: 'secondary' }),
                UI.btn(t('admin.act_export'), () => { location.href = `/api/admin/users/${u.id}/export`; }, { kind: 'secondary' }),
            ]),
            group(t('admin.grp_moderation'), [
                u.banned ? UI.btn(t('admin.act_unban'), unban, { kind: 'tinted' })
                    : UI.btn(sudoTag(t('admin.act_ban')), ban, { kind: 'danger', disabled: protectedAccount || self || isAdmin }),
                u.muted ? UI.btn(t('admin.act_unmute'), unmute, { kind: 'tinted' })
                    : UI.btn(t('admin.act_mute'), mute, { kind: 'secondary', disabled: protectedAccount || isAdmin }),
                u.review_blocked ? UI.btn(t('admin.act_review_unblock'), blockReview(false), { kind: 'tinted' })
                    : UI.btn(t('admin.act_review_block'), blockReview(true), { kind: 'secondary', disabled: protectedAccount }),
                UI.btn(sudoTag(t('admin.act_review_revert')), revertReviews, { kind: 'secondary', disabled: protectedAccount }),
            ]),
            group(t('admin.grp_game_data'), [
                UI.btn(sudoTag(t('admin.act_rating')), setRating, { kind: 'secondary', disabled: protectedAccount }),
                UI.btn(t('admin.act_recompute'), recompute, { kind: 'secondary', disabled: protectedAccount }),
                UI.btn(t('admin.act_badge'), badge, { kind: 'secondary', disabled: protectedAccount }),
                UI.btn(t('admin.act_push_test'), pushTest, { kind: 'secondary', disabled: protectedAccount || !data.push.length }),
            ]),
            group(t('admin.grp_danger'), [
                UI.btn(sudoTag(t('admin.act_delete')), del, { kind: 'danger', disabled: protectedAccount || self || isAdmin }),
            ]),
        ];
        box.appendChild(UI.cardGrid(...groups));
    },

    games(box, data) {
        if (!data.games.length) { box.appendChild(UI.empty(t('admin.none'))); return; }
        box.appendChild(UI.table({ items: data.games, onRow: (g) => Router.go('games', g.id), columns: [
            { label: t('admin.col_id'), cell: (g) => '#' + g.id },
            { label: t('admin.col_game'), cell: (g) => g.room_name },
            { label: t('admin.col_status'), cell: (g) => UI.badge(gameStatusLabel(g.status), g.status === 'finished' ? 'ok' : 'info') },
            { label: t('admin.col_type'), cell: (g) => [g.is_async ? UI.badge(t('admin.kind_async'), 'info') : null,
                g.has_bots ? UI.badge(t('admin.kind_bots'), 'muted') : null] },
            { label: t('admin.col_score'), cell: (g) => g.score + (g.is_winner ? ' ★' : '') },
            { label: t('admin.col_updated'), cell: (g) => formatStamp(g.updated_at) },
        ] }));
    },

    stats(box, data) {
        const history = data.rating_history;
        if (history.length > 1) {
            box.appendChild(UI.card(t('admin.card_rating_history'), Chart.render({
                days: history.map((p) => (p.at || '').slice(0, 10)), type: 'line',
                series: [{ label: t('admin.col_rating'), values: history.map((p) => p.after) }] })));
        }
        box.appendChild(UI.card(t('admin.card_badges'), data.badges.length
            ? h('div', { class: 'admin-chip-row' }, data.badges.map((b) => UI.badge(b.badge, 'info')))
            : h('p', { class: 'text-muted' }, t('admin.none'))));
        box.appendChild(UI.card(t('admin.card_daily'), data.daily.length
            ? UI.table({ compact: true, items: data.daily, columns: [
                { label: t('admin.col_date'), cell: (d) => d.puzzle_date },
                { label: t('admin.col_score'), cell: (d) => d.best_score },
                { label: t('admin.col_attempts'), cell: (d) => d.attempts },
                { label: t('admin.col_revealed'), cell: (d) => d.revealed ? t('admin.yes') : '' },
            ] }) : h('p', { class: 'text-muted' }, t('admin.none'))));
        const reviews = data.reviews;
        box.appendChild(UI.card(t('admin.card_reviews'), UI.kv([
            [t('admin.d_review_total'), reviews.total],
            [t('admin.d_review_invalid'), `${reviews.invalid} (${fmtPercent(reviews.invalid_share)})`],
        ]), reviews.recent.length ? UI.table({ compact: true, items: reviews.recent, columns: [
            { label: t('admin.col_time'), cell: (r) => formatStamp(r.at) },
            { label: t('admin.col_word'), cell: (r) => UI.link(r.word, Router.href('dictionary', '', { w: r.word })) },
            { label: t('admin.col_verdict'), cell: (r) => r.valid ? UI.badge(t('admin.review_valid'), 'ok') : UI.badge(t('admin.review_invalid'), 'danger') },
        ] }) : null));
    },

    security(box, data, reload) {
        const u = data.user;
        box.appendChild(UI.card(t('admin.card_sessions'), data.sessions.length ? UI.table({ compact: true, items: data.sessions, columns: [
            { label: t('admin.col_created'), cell: (s) => formatStamp(s.created_at) },
            { label: t('admin.col_last_seen'), cell: (s) => formatStamp(s.last_seen) },
            { label: t('admin.col_expires'), cell: (s) => formatStamp(s.expires_at) },
            { label: t('admin.col_ip'), cell: (s) => s.ip || '' },
            { label: t('admin.col_browser'), cell: (s) => s.user_agent || '' },
            { label: '', cell: (s) => UI.btn(t('admin.revoke'), () => Act.open({
                title: t('admin.act_revoke_session'),
                run: (v) => Api.del(`/api/admin/security/sessions/${s.id}`, { reason: v.reason }), done: reload }), { kind: 'secondary' }) },
        ] }) : h('p', { class: 'text-muted' }, t('admin.none'))));
        box.appendChild(UI.card(t('admin.card_push'), data.push.length ? UI.table({ compact: true, items: data.push, columns: [
            { label: t('admin.col_created'), cell: (p) => formatStamp(p.created_at) },
            { label: t('admin.col_lang'), cell: (p) => p.lang || '' },
            { label: t('admin.col_host'), cell: (p) => p.host || '' },
            { label: '', cell: (p) => UI.btn(t('admin.delete'), () => Act.open({
                title: t('admin.act_delete_device'), danger: true,
                run: (v) => Api.del(`/api/admin/users/${u.id}/push/${p.id}`, { reason: v.reason }), done: reload }), { kind: 'danger' }) },
        ] }) : h('p', { class: 'text-muted' }, t('admin.none'))));
        box.appendChild(UI.card(t('admin.card_logins'), data.logins.length ? UI.table({ compact: true, items: data.logins, columns: [
            { label: t('admin.col_time'), cell: (l) => formatStamp(l.created_at) },
            { label: t('admin.col_result'), cell: (l) => l.success ? UI.badge(t('admin.login_ok'), 'ok') : UI.badge(t('admin.login_fail'), 'danger') },
            { label: t('admin.col_ip'), cell: (l) => l.ip || '' },
            { label: t('admin.col_browser'), cell: (l) => l.user_agent || '' },
        ] }) : h('p', { class: 'text-muted' }, t('admin.none'))));
    },

    notes(box, data, reload) {
        const u = data.user;
        const field = h('textarea', { rows: 3, maxlength: 1000, placeholder: t('admin.note_ph') });
        const add = UI.btn(t('admin.note_add'), async () => {
            const text = field.value.trim();
            if (!text) return;
            const result = await Api.post(`/api/admin/users/${u.id}/notes`, { note: text });
            if (result.ok) { showToast(t('admin.done')); reload(); } else showToast(Api.message(result), true);
        });
        box.appendChild(UI.card(t('admin.card_notes'), h('div', { class: 'admin-note-form' }, field, add)));
        for (const note of data.notes) {
            const remove = UI.btn(t('admin.delete'), async () => {
                const result = await Api.del(`/api/admin/users/${u.id}/notes/${note.id}`);
                if (result.ok) reload(); else showToast(Api.message(result), true);
            }, { kind: 'secondary' });
            box.appendChild(h('div', { class: 'admin-note' },
                h('div', { class: 'admin-note-meta' }, (note.admin_name || '?') + ' · ' + formatStamp(note.created_at), remove),
                h('div', { class: 'admin-note-text' }, note.note)));
        }
        if (!data.notes.length) box.appendChild(UI.empty(t('admin.none')));
    },
};

const GAME_STATUS_LABELS = {
    active: 'admin.gs_active', finished: 'admin.gs_finished', abandoned: 'admin.gs_abandoned', voided: 'admin.gs_voided',
};

function gameStatusLabel(status) {
    return t(GAME_STATUS_LABELS[status] || 'admin.gs_unknown');
}

// ===== Élő szobák =====

const ROOM_KIND_LABELS = {
    game: 'admin.rk_game', async: 'admin.rk_async', puzzle: 'admin.rk_puzzle', restore: 'admin.rk_restore',
};
const ROOM_STATUS_LABELS = {
    waiting: 'admin.rs_waiting', playing: 'admin.rs_playing', voting: 'admin.rs_voting', finished: 'admin.rs_finished',
};
const STUCK_LABELS = {
    bot_idle: 'admin.stuck_bot_idle', vote_overdue: 'admin.stuck_vote_overdue', timer_overdue: 'admin.stuck_timer_overdue',
    no_timer: 'admin.stuck_no_timer', idle: 'admin.stuck_idle',
};

function roomPlayers(room) {
    return h('span', { class: 'admin-player-list' }, room.players.map((p) => h('span', { class: 'admin-player-chip' },
        UI.dot(p.online), p.is_bot ? ' 🤖 ' : ' ', p.name, p.is_bot && p.difficulty ? ' (' + p.difficulty + ')' : '')));
}

function roomBadges(room) {
    const out = [UI.badge(t(ROOM_KIND_LABELS[room.kind] || 'admin.rk_game'), 'info'),
        UI.badge(t(ROOM_STATUS_LABELS[room.status] || 'admin.rs_playing'), room.status === 'voting' ? 'warn' : 'muted')];
    if (room.private) out.push(UI.badge(t('admin.private'), 'muted'));
    if (room.stuck) out.push(UI.badge(t(STUCK_LABELS[room.stuck] || 'admin.stuck_idle'), 'danger'));
    if (room.paused) out.push(UI.badge(t('admin.paused'), 'warn'));
    return out;
}

const RoomsView = {
    render(view, ctx) {
        if (ctx.arg) return RoomDetailView.render(view, ctx);
        const list = mountList(view, ctx.params, {
            section: 'rooms', title: t('admin.nav_rooms'), note: t('admin.rooms_note'), path: '/api/admin/rooms', pageSize: 500,
            filters: [
                { name: 'q', label: t('admin.f_room_q'), placeholder: t('admin.f_room_q_ph') },
                { name: 'kind', label: t('admin.f_kind'), type: 'select', options: [{ value: '', label: t('admin.any') },
                    ...Object.entries(ROOM_KIND_LABELS).map(([value, key]) => ({ value, label: t(key) }))] },
                { name: 'status', label: t('admin.f_status'), type: 'select', options: [{ value: '', label: t('admin.any') },
                    ...Object.entries(ROOM_STATUS_LABELS).map(([value, key]) => ({ value, label: t(key) }))] },
                { name: 'stuck', label: t('admin.f_stuck'), type: 'checkbox' },
                { name: 'bots', label: t('admin.f_bots'), type: 'checkbox' },
            ],
            columns: [
                { label: t('admin.col_code'), cell: (r) => h('code', null, r.code) },
                { label: t('admin.col_room'), cell: (r) => UI.roomLink(r.id, r.name) },
                { label: t('admin.col_type'), cell: (r) => roomBadges(r) },
                { label: t('admin.col_owner'), cell: (r) => r.owner || '' },
                { label: t('admin.col_players'), cell: (r) => roomPlayers(r) },
                { label: t('admin.col_spectators'), cell: (r) => r.spectators },
                { label: t('admin.col_time_limit'), cell: (r) => r.turn_time_limit ? fmtDuration(r.turn_time_limit) : '–' },
                { label: t('admin.col_challenge'), cell: (r) => r.challenge_mode ? t('admin.yes') : '' },
                { label: t('admin.col_created'), cell: (r) => formatEpoch(r.created_at) },
                { label: t('admin.col_idle'), cell: (r) => fmtDuration(r.idle_seconds) },
            ],
            onRow: (r) => Router.go('rooms', r.id),
            rowClass: (r) => r.stuck ? 'row-alert' : '',
            csv: true,
        });
        let timer = null;
        const soon = () => { clearTimeout(timer); timer = setTimeout(() => list.reload(true), 400); };
        Live.on('admin_room_update', soon);
        Router.onLeave(() => clearTimeout(timer));
        Router.interval(() => { if (!Live.connected) list.reload(true); }, 5000);
    },
};

registerSection({ id: 'rooms', order: 30, icon: 'grid', labelKey: 'admin.nav_rooms',
    render: (view, ctx) => RoomsView.render(view, ctx) });

const RoomDetailView = {
    async render(view, ctx) {
        const key = ctx.arg;
        const box = h('div');
        view.appendChild(box);
        let current = null;
        let racks = null;
        let messageText = '';
        let fetchedAt = Date.now();
        let timerBox = null;

        const roomId = () => encodeURIComponent(current ? current.id : key);
        const refresh = async () => {
            const result = await Api.get(`/api/admin/rooms/${roomId()}`);
            if (result.ok) paint(result.data.room);
            else if (result.status === 404) { current = null; box.replaceChildren(UI.errorBox(Api.message(result)), UI.link(t('admin.nav_rooms'), Router.href('rooms'))); }
        };
        const act = (body, options = {}) => Api.post(`/api/admin/rooms/${roomId()}/action`, body).then((result) => {
            if (!result.ok) showToast(Api.message(result), true);
            else { showToast(options.doneText || t('admin.done')); refresh(); }
            return result;
        });
        const askReason = (title, text, body, options = {}) => Act.open({
            title, text, danger: !!options.danger, confirmName: options.confirmName, fields: options.fields,
            run: (v) => Api.post(`/api/admin/rooms/${roomId()}/action`, { ...body(v), reason: v.reason }),
            done: () => { refresh(); if (options.after) options.after(); },
        });

        const remaining = (room) => {
            if (room.timer_paused_left !== null && room.timer_paused_left !== undefined) return room.timer_paused_left;
            if (!room.timer_expires_at) return null;
            return Math.max(0, room.timer_expires_at - room.server_time - (Date.now() - fetchedAt) / 1000);
        };

        const tickTimer = () => {
            if (!timerBox || !current) return;
            const left = remaining(current);
            const paused = current.timer_paused_left !== null && current.timer_paused_left !== undefined;
            timerBox.textContent = left === null ? '–' : formatCountdown(left) + (paused ? ' ⏸' : '');
        };

        const paint = (room) => {
            current = room;
            fetchedAt = Date.now();
            const live = room.status !== 'finished';
            const started = room.status === 'playing' || room.status === 'voting';
            const lastMove = room.history.length ? room.history[room.history.length - 1].tiles : [];
            box.replaceChildren();
            box.appendChild(h('div', { class: 'admin-breadcrumb' }, UI.link(t('admin.nav_rooms'), Router.href('rooms'))));
            box.appendChild(UI.header(room.name, t('admin.room_sub', { code: room.code, owner: room.owner || '–' }),
                UI.copy(room.code), UI.btn(t('admin.refresh'), refresh, { kind: 'secondary' })));
            box.appendChild(h('div', { class: 'admin-badge-row' }, roomBadges(room),
                h('span', { class: 'text-secondary text-sm' }, Live.connected ? t('admin.live_on') : t('admin.live_polling'))));

            timerBox = h('span', { class: 'admin-timer' });
            tickTimer();

            const main = h('div', { class: 'admin-room-layout' });
            const left = h('div', { class: 'admin-room-left' });
            const right = h('div', { class: 'admin-room-right' });
            main.append(left, right);
            box.appendChild(main);

            left.appendChild(renderBoard(room.board, lastMove));
            left.appendChild(UI.card(t('admin.card_bag', { n: room.bag_count }), h('div', { class: 'admin-bag' },
                Object.entries(room.bag).sort(([a], [b]) => a.localeCompare(b, 'hu')).map(([letter, count]) =>
                    h('span', { class: 'admin-bag-item' }, UI.tile(letter === '?' ? '' : letter, letter === '?'), '×' + count)))));

            const players = UI.table({ compact: true, items: room.players, columns: [
                { label: t('admin.col_name'), cell: (p) => h('span', null, UI.dot(p.online), ' ', p.is_bot ? '🤖 ' : '',
                    p.user_id ? UI.userLink(p.user_id, p.name) : p.name, p.name === room.current_player ? ' ◀' : '',
                    p.owner ? ' 👑' : '', p.resigned ? ' 🏳' : '') },
                { label: t('admin.col_score'), cell: (p) => p.score },
                { label: t('admin.col_hand'), cell: (p) => p.hand_count },
                { label: t('admin.col_level'), cell: (p) => p.is_bot ? String(p.difficulty) : '' },
                { label: '', cell: (p) => p.is_bot || !live ? null : h('span', { class: 'admin-action-row' },
                    UI.btn(t('admin.act_kick'), () => askReason(t('admin.act_kick_title', { name: p.name }), t('admin.act_kick_text'),
                        () => ({ action: 'kick', player: p.name }), { danger: true }), { kind: 'danger' }),
                    p.owner ? null : UI.btn(t('admin.act_transfer'), () => askReason(t('admin.act_transfer_title', { name: p.name }), '',
                        () => ({ action: 'transfer', player: p.name })), { kind: 'secondary' })) },
            ] });
            right.appendChild(UI.card(t('admin.card_players'), players,
                h('div', { class: 'admin-kv-inline' }, t('admin.room_turn', { n: room.turn_number, name: room.current_player || '–' }),
                    ' · ', t('admin.room_timer'), ' ', timerBox,
                    room.async ? [' · ', t('admin.room_deadline'), ' ', formatEpoch(room.async.deadline)] : null,
                    ' · ', t('admin.room_spectators', { n: room.spectators }),
                    room.spectator_names.length ? ' (' + room.spectator_names.join(', ') + ')' : ''),
                room.connections.length ? h('div', { class: 'form-hint' }, t('admin.room_grace'), ' ',
                    room.connections.map((c) => `${c.name} (${fmtDuration(c.remaining)})`).join(', ')) : null,
                h('div', { class: 'admin-action-row' },
                    UI.btn(racks ? t('admin.racks_hide') : t('admin.racks_show'), async () => {
                        if (racks) { racks = null; paint(current); return; }
                        const result = await Api.get(`/api/admin/rooms/${roomId()}/racks`);
                        if (result.ok) { racks = result.data.racks; paint(current); } else showToast(Api.message(result), true);
                    }, { kind: 'secondary' })),
                racks ? h('div', { class: 'admin-racks' }, racks.map((r) => h('div', { class: 'admin-rack' },
                    h('strong', null, r.name), h('div', { class: 'admin-rack-tiles' }, r.hand.map((l) => UI.tile(l === '?' ? '' : l, l === '?'))))),
                    h('div', { class: 'form-hint' }, t('admin.racks_logged'))) : null));

            if (room.pending) {
                const votes = Object.entries(room.pending.votes);
                right.appendChild(UI.card(t('admin.card_vote'), UI.kv([
                    [t('admin.d_vote_player'), room.pending.player], [t('admin.d_vote_words'), room.pending.words.join(', ')],
                    [t('admin.col_score'), room.pending.score],
                    [t('admin.d_vote_votes'), votes.length ? votes.map(([n, v]) => `${n}: ${v}`).join(', ') : t('admin.none')],
                    [t('admin.d_vote_voters'), room.pending.voters.join(', ')],
                ]), live ? h('div', { class: 'admin-action-row' },
                    UI.btn(t('admin.act_vote_accept'), () => askReason(t('admin.act_vote_accept'), t('admin.act_vote_text'),
                        () => ({ action: 'resolve_vote', accept: true })), { kind: 'tinted' }),
                    UI.btn(t('admin.act_vote_reject'), () => askReason(t('admin.act_vote_reject'), t('admin.act_vote_text'),
                        () => ({ action: 'resolve_vote', accept: false }), { danger: true }), { kind: 'danger' })) : null));
            }

            if (live) right.appendChild(this.actions(room, started, askReason, act, () => messageText, (v) => { messageText = v; }));

            right.appendChild(UI.card(t('admin.card_moves'), room.history.length ? UI.table({ compact: true,
                items: room.history.slice(-30).reverse(), columns: [
                    { label: '#', cell: (m) => m.n },
                    { label: t('admin.col_player'), cell: (m) => m.player },
                    { label: t('admin.col_action'), cell: (m) => moveTypeLabel(m.type) },
                    { label: t('admin.col_words'), cell: (m) => (m.words || []).join(', ') },
                    { label: t('admin.col_score'), cell: (m) => m.score },
                ] }) : h('p', { class: 'text-muted' }, t('admin.none'))));

            right.appendChild(UI.card(t('admin.card_chat'), room.chat.length
                ? h('div', { class: 'admin-chat-log' }, room.chat.map((m) => h('div', { class: 'admin-chat-line' + (m.system ? ' system' : '') },
                    h('span', { class: 'admin-chat-time' }, m.ts ? new Date(m.ts * 1000).toLocaleTimeString(I18N.locale()) : ''),
                    h('strong', null, m.user_id ? UI.userLink(m.user_id, m.name) : m.name), ' ', m.message)))
                : h('p', { class: 'text-muted' }, t('admin.none'))));
            if (room.saved) right.appendChild(h('p', { class: 'form-hint' }, UI.gameLink(room.saved, t('admin.room_saved_game', { id: room.saved }))));
        };

        await loadInto(box, () => Api.get(`/api/admin/rooms/${roomId()}`), (container, data) => {
            container.replaceChildren();
            paint(data.room);
        });
        if (!current || !view.isConnected) return;
        Router.interval(tickTimer, 1000);
        Live.watchRoom(current.id);
        Live.on('admin_room_state', (state) => { if (state && current && state.id === current.id) paint(state); });
        Router.interval(() => { if (!Live.connected) refresh(); }, 5000);
    },

    actions(room, started, askReason, act, getMessage, setMessage) {
        const card = UI.card(t('admin.card_interventions'));
        const input = h('input', { type: 'text', maxlength: 200, placeholder: t('admin.msg_ph'), value: getMessage() });
        input.addEventListener('input', () => setMessage(input.value));
        const send = UI.btn(t('admin.act_message'), () => {
            const text = input.value.trim();
            if (!text) return;
            act({ action: 'message', message: text }).then((result) => { if (result.ok) { input.value = ''; setMessage(''); } });
        });
        card.appendChild(h('div', { class: 'admin-message-row' }, input, send));
        const buttons = [];
        if (started) {
            buttons.push(UI.btn(t('admin.act_skip'), () => askReason(t('admin.act_skip'), t('admin.act_skip_text'),
                () => ({ action: 'skip' })), { kind: 'secondary' }));
            const current = room.players.find((p) => p.name === room.current_player);
            if (current && current.is_bot) {
                buttons.push(UI.btn(t('admin.act_reschedule'), () => askReason(t('admin.act_reschedule'), t('admin.act_reschedule_text'),
                    () => ({ action: 'reschedule_bot' })), { kind: 'secondary' }));
            }
            if (room.turn_time_limit && !room.async) {
                buttons.push(UI.btn(t('admin.act_extend'), () => askReason(t('admin.act_extend'), '', (v) => ({ action: 'extend', seconds: v.seconds }),
                    { fields: [{ name: 'seconds', type: 'select', label: t('admin.f_extend'), value: 60,
                        options: [30, 60, 120, 300].map((s) => ({ value: s, label: fmtDuration(s) })) }] }), { kind: 'secondary' }));
                const paused = room.timer_paused_left !== null && room.timer_paused_left !== undefined;
                buttons.push(UI.btn(paused ? t('admin.act_resume') : t('admin.act_pause'), () => askReason(
                    paused ? t('admin.act_resume') : t('admin.act_pause'), '', () => ({ action: paused ? 'resume' : 'pause' })), { kind: 'secondary' }));
            }
            buttons.push(UI.btn(t('admin.act_save'), () => askReason(t('admin.act_save'), t('admin.act_save_text'),
                () => ({ action: 'save' })), { kind: 'secondary' }));
            buttons.push(UI.btn(t('admin.act_end') + ' 🔒', () => askReason(t('admin.act_end'), t('admin.act_end_text'),
                () => ({ action: 'end' }), { danger: true, confirmName: room.code }), { kind: 'danger' }));
            buttons.push(UI.btn(t('admin.act_void') + ' 🔒', () => askReason(t('admin.act_void'), t('admin.act_void_text'),
                () => ({ action: 'void' }), { danger: true, confirmName: room.code }), { kind: 'danger' }));
        }
        buttons.push(UI.btn(t('admin.act_disband') + ' 🔒', () => askReason(t('admin.act_disband'), t('admin.act_disband_text'),
            (v) => ({ action: 'disband', save: v.save, message: v.message || undefined }),
            { danger: true, confirmName: room.code, after: () => Router.go('rooms'), fields: [
                { name: 'message', type: 'text', label: t('admin.f_disband_message'), maxlength: 200, placeholder: t('admin.f_disband_message_ph') },
                { name: 'save', type: 'checkbox', label: t('admin.f_disband_save'), value: started },
            ] }), { kind: 'danger' }));
        card.appendChild(h('div', { class: 'admin-action-row' }, buttons));
        return card;
    },
};
