'use strict';
// ===== ADMIN PANEL — nézetek III.: moderáció, kommunikáció, statisztika, biztonság, rendszer, beállítások, napló =====

// ===== Moderáció =====

const REPORT_STATUS_LABELS = { new: 'admin.rp_new', handled: 'admin.rp_handled', rejected: 'admin.rp_rejected' };
const REPORT_STATUS_KINDS = { new: 'danger', handled: 'ok', rejected: 'muted' };

const ModerationView = {
    render(view, ctx) {
        const tab = ctx.params.get('tab') || 'reports';
        view.appendChild(UI.header(t('admin.nav_moderation'), t('admin.moderation_note')));
        view.appendChild(UI.tabs([{ id: 'reports', label: t('admin.tab_reports') }, { id: 'chat', label: t('admin.tab_chat') },
            { id: 'words', label: t('admin.tab_banned_words') }, { id: 'names', label: t('admin.tab_names') }], tab,
        (next) => Router.go('moderation', '', { tab: next })));
        const body = h('div', { class: 'admin-tab-body' });
        view.appendChild(body);
        ({ reports: this.reports, chat: this.chat, words: this.words, names: this.names }[tab] || this.reports).call(this, body, ctx.params);
    },

    reports(box, params) {
        const holder = h('div');
        box.appendChild(holder);
        mountList(holder, params, {
            section: 'moderation', keep: ['tab'], title: t('admin.tab_reports'), path: '/api/admin/reports', csv: false,
            filters: [{ name: 'status', label: t('admin.f_status'), type: 'select', options: [{ value: '', label: t('admin.any') },
                ...Object.entries(REPORT_STATUS_LABELS).map(([value, key]) => ({ value, label: t(key) }))] }],
            columns: [
                { label: t('admin.col_id'), cell: (r) => '#' + r.id },
                { label: t('admin.col_status'), cell: (r) => UI.badge(t(REPORT_STATUS_LABELS[r.status]), REPORT_STATUS_KINDS[r.status]) },
                { label: t('admin.col_type'), cell: (r) => r.kind === 'chat' ? t('admin.rp_kind_chat') : t('admin.rp_kind_player') },
                { label: t('admin.col_time'), cell: (r) => formatStamp(r.created_at) },
                { label: t('admin.rp_reporter'), cell: (r) => r.reporter_user_id ? UI.userLink(r.reporter_user_id, r.reporter_name) : r.reporter_name },
                { label: t('admin.rp_reported'), cell: (r) => r.reported_user_id ? UI.userLink(r.reported_user_id, r.reported_name) : r.reported_name },
                { label: t('admin.rp_message'), cell: (r) => r.message || '' },
                { label: t('admin.col_reason'), cell: (r) => r.reason || '' },
            ],
            onRow: (r) => this.detail(r.id),
            rowClass: (r) => r.status === 'new' ? 'row-alert' : '',
            after: (data) => Nav.set('moderation', data.new || 0),
        });
    },

    async detail(id) {
        const result = await Api.get(`/api/admin/reports/${id}`);
        if (!result.ok) { showToast(Api.message(result), true); return; }
        const r = result.data.report;
        const snapshot = r.snapshot || {};
        const body = h('div', null, UI.kv([
            [t('admin.col_status'), UI.badge(t(REPORT_STATUS_LABELS[r.status]), REPORT_STATUS_KINDS[r.status])],
            [t('admin.col_time'), formatStamp(r.created_at)],
            [t('admin.rp_reporter'), r.reporter_user_id ? UI.userLink(r.reporter_user_id, r.reporter_name) : r.reporter_name],
            [t('admin.rp_reported'), r.reported_user_id ? UI.userLink(r.reported_user_id, r.reported_name) : r.reported_name],
            [t('admin.rp_message'), r.message], [t('admin.col_reason'), r.reason],
            [t('admin.col_room'), r.room_id ? UI.roomLink(r.room_id, r.room_name) : r.room_name],
            [t('admin.rp_note'), r.handler_note], [t('admin.rp_handled_at'), formatStamp(r.handled_at)],
        ]));
        if (snapshot.players) {
            body.appendChild(UI.subtitle(t('admin.rp_snapshot')));
            body.appendChild(h('p', { class: 'form-hint' }, snapshot.players.map((p) => `${p.name} (${p.score})`).join(' · ')
                + ' — ' + t('admin.room_turn', { n: snapshot.turn, name: '–' })));
            body.appendChild(h('div', { class: 'admin-chat-log' }, (snapshot.chat || []).map((m) => h('div', { class: 'admin-chat-line' },
                h('strong', null, m.name), ' ', m.message))));
        }
        const set = (status) => () => { Modal.close(); Act.open({
            title: t(REPORT_STATUS_LABELS[status]), danger: status === 'rejected',
            run: (v) => Api.patch(`/api/admin/reports/${r.id}`, { status, reason: v.reason }),
            done: () => { Nav.loadReports(); Router.render(); } }); };
        body.appendChild(h('div', { class: 'admin-action-row' },
            r.status !== 'handled' ? UI.btn(t('admin.rp_mark_handled'), set('handled'), { kind: 'tinted' }) : null,
            r.status !== 'rejected' ? UI.btn(t('admin.rp_mark_rejected'), set('rejected'), { kind: 'secondary' }) : null,
            r.status !== 'new' ? UI.btn(t('admin.rp_reopen'), set('new'), { kind: 'secondary' }) : null));
        Modal.show(t('admin.rp_title', { id: r.id }), body, { wide: true });
    },

    // Chat napló: élő (a futó szobák) vagy tartós; a tiltott szavas üzenetek kiemelve
    chat(box, params) {
        const source = params.get('source') === 'log' ? 'log' : 'live';
        const holder = h('div');
        box.appendChild(holder);
        const list = mountList(holder, params, {
            section: 'moderation', keep: ['tab'], title: t('admin.tab_chat'), note: source === 'log' ? t('admin.chat_log_note') : t('admin.chat_live_note'),
            path: '/api/admin/moderation/chat', pageSize: source === 'log' ? 100 : 500, csv: source === 'log',
            filters: [
                { name: 'source', label: t('admin.f_source'), type: 'select', options: [{ value: '', label: t('admin.chat_src_live') }, { value: 'log', label: t('admin.chat_src_log') }] },
                { name: 'q', label: t('admin.f_chat_q') },
                ...(source === 'log' ? [{ name: 'room', label: t('admin.f_chat_room') }, { name: 'user', label: t('admin.f_game_user') },
                    { name: 'since', label: t('admin.f_since'), type: 'date' }, { name: 'until', label: t('admin.f_until'), type: 'date' }] : []),
            ],
            columns: [
                { label: t('admin.col_time'), cell: (m) => m.ts ? new Date(m.ts * 1000).toLocaleString(I18N.locale()) : (formatStamp(m.created_at) || '–') },
                { label: t('admin.col_room'), cell: (m) => m.room_id ? UI.roomLink(m.room_id, m.room_name) : (m.room_name || '') },
                { label: t('admin.col_name'), cell: (m) => m.user_id ? UI.userLink(m.user_id, m.name) : m.name },
                { label: t('admin.rp_message'), cell: (m) => h('span', null, m.system ? UI.badge(t('admin.chat_system'), 'info') : null, m.flagged ? UI.badge(t('admin.chat_flagged'), 'danger') : null, ' ', m.message) },
            ],
            rowClass: (m) => m.flagged ? 'row-alert' : '',
        });
        if (source === 'live') {
            Router.interval(() => list.reload(true), 8000);
            Live.on('admin_alert', () => list.reload(true));
        }
    },

    words(box) {
        const reload = () => Router.render();
        const holder = h('div');
        box.appendChild(holder);
        loadInto(holder, () => Api.get('/api/admin/moderation/words'), (container, data) => {
            container.appendChild(UI.card(t('admin.card_banned_words'), h('p', { class: 'form-hint' }, t('admin.banned_words_note')),
                UI.btn(t('admin.act_word_add'), () => Act.open({
                    title: t('admin.act_word_add'), fields: [{ name: 'word', label: t('admin.f_banned_word'), required: true, maxlength: 40 }],
                    run: (v) => Api.post('/api/admin/moderation/words', { word: v.word, reason: v.reason }), done: reload }), { kind: 'secondary' }),
                data.items.length ? UI.table({ compact: true, items: data.items, columns: [
                    { label: t('admin.col_word'), cell: (w) => h('code', null, w.word) }, { label: t('admin.col_admin'), cell: (w) => w.admin_name || '' },
                    { label: t('admin.col_time'), cell: (w) => formatStamp(w.created_at) },
                    { label: '', cell: (w) => UI.btn(t('admin.delete'), () => Act.open({ title: t('admin.act_word_remove'),
                        run: (v) => Api.del('/api/admin/moderation/words', { word: w.word, reason: v.reason }), done: reload }), { kind: 'secondary' }) }] })
                    : h('p', { class: 'text-muted' }, t('admin.none'))));
        });
    },

    names(box) {
        const holder = h('div');
        box.appendChild(holder);
        loadInto(holder, () => Api.get('/api/admin/moderation/names', { limit: 100 }), (container, data) => {
            container.appendChild(h('p', { class: 'form-hint' }, t('admin.names_note')));
            if (!data.items.length) { container.appendChild(UI.empty(t('admin.none'))); return; }
            container.appendChild(UI.table({ items: data.items, rowClass: (n) => n.flagged ? 'row-alert' : '', columns: [
                { label: t('admin.col_time'), cell: (n) => formatStamp(n.at) },
                { label: t('admin.col_name'), cell: (n) => UI.userLink(n.user_id, n.display_name) },
                { label: t('admin.col_type'), cell: (n) => UI.badge(n.kind === 'renamed' ? t('admin.name_renamed') : t('admin.name_registered'), 'muted') },
                { label: t('admin.col_flags'), cell: (n) => n.flagged ? UI.badge(t('admin.chat_flagged'), 'danger') : '' },
                { label: '', cell: (n) => UI.btn(t('admin.act_rename'), () => Act.open({
                    title: t('admin.act_rename'), fields: [{ name: 'display_name', label: t('admin.col_name'), value: n.display_name, required: true, maxlength: 20 }],
                    run: (v) => Api.patch(`/api/admin/users/${n.user_id}`, { display_name: v.display_name, reason: v.reason }),
                    done: () => Router.render() }), { kind: 'secondary' }) },
            ] }));
        });
    },
};

registerSection({ id: 'moderation', order: 90, icon: 'flag', labelKey: 'admin.nav_moderation',
    render: (view, ctx) => ModerationView.render(view, ctx) });

// ===== Kommunikáció =====

const ANNOUNCEMENT_KIND_LABELS = { info: 'admin.ak_info', warning: 'admin.ak_warning', maintenance: 'admin.ak_maintenance' };
const AUDIENCE_LABELS = { all: 'admin.aud_all', registered: 'admin.aud_registered', guests: 'admin.aud_guests' };
const ANNOUNCEMENT_STATUS_LABELS = { active: 'admin.as_active', scheduled: 'admin.as_scheduled', expired: 'admin.as_expired', revoked: 'admin.as_revoked' };

function utcInput(stamp) {
    // "ÉÉÉÉ-HH-NN ÓÓ:PP:MM" → "ÉÉÉÉ-HH-NNTÓÓ:PP" a datetime-local mezőhöz
    return stamp ? stamp.replace(' ', 'T').slice(0, 16) : '';
}

const CommView = {
    render(view, ctx) {
        const tab = ctx.params.get('tab') || 'announcements';
        view.appendChild(UI.header(t('admin.nav_comm'), t('admin.comm_note')));
        view.appendChild(UI.tabs([{ id: 'announcements', label: t('admin.tab_announcements') }, { id: 'maintenance', label: t('admin.tab_maintenance') },
            { id: 'push', label: t('admin.tab_push') }, { id: 'email', label: t('admin.tab_email') }], tab,
        (next) => Router.go('comm', '', { tab: next })));
        const body = h('div', { class: 'admin-tab-body' });
        view.appendChild(body);
        ({ announcements: this.announcements, maintenance: this.maintenance, push: this.push, email: this.email }[tab]
            || this.announcements).call(this, body, ctx.params);
    },

    announcements(box) {
        const reload = () => Router.render();
        const fields = (a) => [
            { name: 'text_hu', type: 'textarea', label: t('admin.f_text_hu'), value: a ? a.text_hu : '', required: true, maxlength: 500 },
            { name: 'text_en', type: 'textarea', label: t('admin.f_text_en'), value: a ? a.text_en : '', maxlength: 500 },
            { name: 'kind', type: 'select', label: t('admin.col_type'), value: a ? a.kind : 'info',
                options: Object.entries(ANNOUNCEMENT_KIND_LABELS).map(([value, key]) => ({ value, label: t(key) })) },
            { name: 'audience', type: 'select', label: t('admin.f_audience'), value: a ? a.audience : 'all',
                options: Object.entries(AUDIENCE_LABELS).map(([value, key]) => ({ value, label: t(key) })) },
            { name: 'starts_at', type: 'datetime', label: t('admin.f_starts'), value: a ? utcInput(a.starts_at) : '', hint: t('admin.hint_utc') },
            { name: 'ends_at', type: 'datetime', label: t('admin.f_ends'), value: a ? utcInput(a.ends_at) : '' },
        ];
        const payload = (v) => ({ text_hu: v.text_hu, text_en: v.text_en, kind: v.kind, audience: v.audience,
            starts_at: v.starts_at || null, ends_at: v.ends_at || null, reason: v.reason || undefined });
        const create = () => Act.open({ title: t('admin.act_announce'), fields: fields(null), reason: 'optional', wide: true,
            run: (v) => Api.post('/api/admin/announcements', payload(v)), done: reload });
        const edit = (a) => Act.open({ title: t('admin.act_announce_edit'), fields: fields(a), reason: 'optional', wide: true,
            run: (v) => Api.patch(`/api/admin/announcements/${a.id}`, payload(v)), done: reload });
        const holder = h('div');
        box.appendChild(holder);
        loadInto(holder, () => Api.get('/api/admin/announcements'), (container, data) => {
            container.appendChild(h('div', { class: 'admin-toolbar' }, h('span'), UI.btn(t('admin.act_announce'), create)));
            if (!data.items.length) { container.appendChild(UI.empty(t('admin.announce_empty'))); return; }
            container.appendChild(UI.table({ items: data.items, columns: [
                { label: t('admin.col_status'), cell: (a) => UI.badge(t(ANNOUNCEMENT_STATUS_LABELS[a.status] || 'admin.as_active'), a.status === 'active' ? 'ok' : 'muted') },
                { label: t('admin.col_type'), cell: (a) => t(ANNOUNCEMENT_KIND_LABELS[a.kind]) },
                { label: t('admin.f_audience'), cell: (a) => t(AUDIENCE_LABELS[a.audience]) },
                { label: t('admin.f_text_hu'), cell: (a) => a.text_hu }, { label: t('admin.f_text_en'), cell: (a) => a.text_en || '' },
                { label: t('admin.f_starts'), cell: (a) => formatStamp(a.starts_at) || '–' }, { label: t('admin.f_ends'), cell: (a) => formatStamp(a.ends_at) || '–' },
                { label: '', cell: (a) => h('span', { class: 'admin-action-row' },
                    a.status !== 'revoked' ? UI.btn(t('admin.edit'), () => edit(a), { kind: 'secondary' }) : null,
                    a.status !== 'revoked' ? UI.btn(t('admin.revoke'), () => Act.open({ title: t('admin.act_announce_revoke'), reason: 'optional',
                        run: () => Api.del(`/api/admin/announcements/${a.id}`), done: reload }), { kind: 'danger' }) : null) },
            ] }));
        });
    },

    maintenance(box) {
        const holder = h('div');
        box.appendChild(holder);
        loadInto(holder, () => Api.get('/api/admin/maintenance'), (container, m) => {
            Nav.showMaintenance(m.active ? m : null);
            container.appendChild(UI.card(t('admin.card_maintenance'), h('p', { class: 'form-hint' }, t('admin.maintenance_note')),
                UI.kv([[t('admin.col_status'), m.active ? UI.badge(t('admin.on'), 'danger') : UI.badge(t('admin.off'), 'muted')],
                    [t('admin.f_text_hu'), m.message_hu], [t('admin.f_text_en'), m.message_en],
                    [t('admin.f_until'), m.until ? formatStamp(m.until) : null], [t('admin.d_started'), m.started_at ? formatStamp(m.started_at) : null]]),
                h('div', { class: 'admin-action-row' },
                    UI.btn(m.active ? t('admin.act_maintenance_edit') : t('admin.act_maintenance_on'), () => Act.open({
                        title: t('admin.act_maintenance_on'), text: t('admin.maintenance_text'), danger: true, wide: true,
                        fields: [
                            { name: 'message_hu', type: 'textarea', label: t('admin.f_text_hu'), value: m.message_hu || '', required: true, maxlength: 500 },
                            { name: 'message_en', type: 'textarea', label: t('admin.f_text_en'), value: m.message_en || '', maxlength: 500 },
                            { name: 'until', type: 'datetime', label: t('admin.f_until_optional'), value: utcInput(m.until), hint: t('admin.hint_utc') }],
                        run: (v) => Api.post('/api/admin/maintenance', { enabled: true, message_hu: v.message_hu, message_en: v.message_en,
                            until: v.until || null, reason: v.reason }),
                        done: () => Router.render() }), { kind: m.active ? 'secondary' : 'danger' }),
                    m.active ? UI.btn(t('admin.act_maintenance_off'), () => Act.open({
                        title: t('admin.act_maintenance_off'),
                        run: (v) => Api.post('/api/admin/maintenance', { enabled: false, message_hu: '', message_en: '', until: null, reason: v.reason }),
                        done: () => Router.render() }), { kind: 'tinted' }) : null)));
        });
    },

    push(box) {
        const state = { target: 'group', group: 'recent', userId: '' };
        const form = h('form', { class: 'admin-form-inline' });
        const target = h('select', { name: 'target' }, h('option', { value: 'group' }, t('admin.push_target_group')),
            h('option', { value: 'user' }, t('admin.push_target_user')));
        const group = h('select', { name: 'group' }, [['online', 'admin.grp_online'], ['recent', 'admin.grp_recent'], ['all', 'admin.grp_all']]
            .map(([value, key]) => h('option', { value }, t(key))));
        group.value = state.group;
        const userId = h('input', { type: 'number', name: 'user_id', placeholder: t('admin.f_user_id') });
        const title = h('input', { type: 'text', name: 'title', maxlength: 80, placeholder: t('admin.f_push_title') });
        const text = h('textarea', { name: 'body', rows: 3, maxlength: 500, placeholder: t('admin.f_push_body') });
        const result = h('div');
        const syncTarget = () => { group.classList.toggle('hidden', target.value !== 'group'); userId.classList.toggle('hidden', target.value !== 'user'); };
        target.addEventListener('change', syncTarget);
        syncTarget();
        const payload = () => ({ target: target.value, group: target.value === 'group' ? group.value : undefined,
            user_id: target.value === 'user' ? Number(userId.value) || null : undefined, title: title.value.trim(), body: text.value.trim() });
        const preview = UI.btn(t('admin.push_preview'), async () => {
            const response = await Api.post('/api/admin/push/preview', payload());
            if (!response.ok) { result.replaceChildren(UI.errorBox(Api.message(response))); return; }
            const d = response.data;
            result.replaceChildren(UI.card(t('admin.push_preview_title'), UI.kv([
                [t('admin.push_notification'), `${d.title} — ${d.body}`], [t('admin.push_recipients'), d.recipients],
                [t('admin.push_devices'), d.devices], [t('admin.push_sample'), d.sample.join(', ')],
                [t('admin.push_available'), d.available ? t('admin.yes') : UI.badge(t('admin.no'), 'danger')]]),
            d.recipients ? UI.btn(t('admin.push_send'), () => Act.open({
                title: t('admin.push_send'), text: t('admin.push_send_text', { n: d.recipients }), danger: true, reason: false,
                run: () => Api.post('/api/admin/push', { ...payload(), confirm: true }),
                done: (r) => result.replaceChildren(UI.card(t('admin.push_result'), UI.kv([[t('admin.push_recipients'), r.recipients],
                    [t('admin.push_ok'), r.sent], [t('admin.push_failed'), r.failed], [t('admin.push_removed'), r.removed]]))) }), { kind: 'danger' }) : null));
        }, { kind: 'secondary' });
        form.append(h('label', { class: 'admin-field' }, h('span', { class: 'admin-field-label' }, t('admin.f_push_target')), target), group, userId, title, text,
            h('div', { class: 'admin-action-row' }, preview));
        box.appendChild(UI.card(t('admin.card_push'), h('p', { class: 'form-hint' }, t('admin.push_note')), form, result));
    },

    email(box) {
        const single = h('form', { class: 'admin-form-inline' });
        const userId = h('input', { type: 'number', name: 'user_id', placeholder: t('admin.f_user_id') });
        const subject = h('input', { type: 'text', maxlength: 150, placeholder: t('admin.f_email_subject') });
        const text = h('textarea', { rows: 4, maxlength: 5000, placeholder: t('admin.f_email_body') });
        single.append(userId, subject, text, h('div', { class: 'admin-action-row' }, UI.btn(t('admin.email_send'), () => {
            const payload = { user_id: Number(userId.value) || null, subject: subject.value.trim(), body: text.value.trim() };
            Act.open({ title: t('admin.email_send'), text: t('admin.email_send_text', { id: payload.user_id || '?' }), reason: false,
                run: () => Api.post('/api/admin/email', payload),
                done: (r) => showToast(r.console ? t('admin.email_console') : t('admin.email_sent')) });
        })));
        box.appendChild(UI.card(t('admin.card_email_single'), h('p', { class: 'form-hint' }, t('admin.email_note')), single));

        const bulk = h('form', { class: 'admin-form-inline' });
        const group = h('select', null, [['recent', 'admin.grp_recent'], ['all', 'admin.grp_all']].map(([value, key]) => h('option', { value }, t(key))));
        const bulkSubject = h('input', { type: 'text', maxlength: 150, placeholder: t('admin.f_email_subject') });
        const bulkBody = h('textarea', { rows: 4, maxlength: 5000, placeholder: t('admin.f_email_body') });
        const info = h('div', { class: 'form-hint' });
        const refreshInfo = async () => {
            const response = await Api.get('/api/admin/email/preview', { group: group.value });
            info.textContent = response.ok ? t('admin.email_recipients', { n: response.data.recipients, limit: response.data.limit,
                sample: response.data.sample.join(', ') }) : Api.message(response);
        };
        group.addEventListener('change', refreshInfo);
        refreshInfo();
        bulk.append(group, info, bulkSubject, bulkBody, h('div', { class: 'admin-action-row' }, UI.btn(t('admin.email_bulk_send') + ' 🔒', () => Act.open({
            title: t('admin.email_bulk_send'), text: t('admin.email_bulk_text'), danger: true, reason: false,
            run: () => Api.post('/api/admin/email/bulk', { group: group.value, subject: bulkSubject.value.trim(), body: bulkBody.value.trim(), confirm: true }),
            done: (r) => showToast(r.console ? t('admin.email_console') : t('admin.email_bulk_done', { sent: r.sent, failed: r.failed })) }), { kind: 'danger' })));
        box.appendChild(UI.card(t('admin.card_email_bulk'), h('p', { class: 'form-hint' }, t('admin.email_bulk_note')), bulk));
    },
};

registerSection({ id: 'comm', order: 100, icon: 'megaphone', labelKey: 'admin.nav_comm',
    render: (view, ctx) => CommView.render(view, ctx) });

// ===== Statisztika =====

const STAT_METRICS = [
    ['registrations', 'admin.sm_registrations'], ['active', 'admin.sm_active'], ['retention', 'admin.sm_retention'],
    ['games', 'admin.sm_games'], ['bots', 'admin.sm_bots'], ['words', 'admin.sm_words'], ['challenges', 'admin.sm_challenges'],
    ['practice', 'admin.sm_practice'], ['review', 'admin.sm_review'], ['heatmap', 'admin.sm_heatmap'],
];
const PRACTICE_LABELS = {
    quiz: 'admin.pr_quiz', answer: 'admin.pr_answer', short_words: 'admin.pr_short_words', rack: 'admin.pr_rack',
    rack_word: 'admin.pr_rack_word', word_review: 'admin.pr_word_review', word_review_vote: 'admin.pr_word_review_vote',
};
const WEEKDAY_LABELS = ['admin.wd_mon', 'admin.wd_tue', 'admin.wd_wed', 'admin.wd_thu', 'admin.wd_fri', 'admin.wd_sat', 'admin.wd_sun'];

const StatsView = {
    render(view, ctx) {
        const metric = STAT_METRICS.some(([id]) => id === ctx.params.get('metric')) ? ctx.params.get('metric') : 'registrations';
        const range = ['7', '30', '90', '365'].includes(ctx.params.get('range')) ? ctx.params.get('range') : '30';
        const rangeSelect = h('select', null, ['7', '30', '90', '365'].map((value) => h('option', { value }, t('admin.range_days', { n: value }))));
        rangeSelect.value = range;
        rangeSelect.addEventListener('change', () => Router.go('stats', '', { metric, range: rangeSelect.value }));
        view.appendChild(UI.header(t('admin.nav_stats'), t('admin.stats_note'),
            h('label', { class: 'admin-field' }, h('span', { class: 'admin-field-label' }, t('admin.f_range')), rangeSelect),
            UI.download(t('admin.export_csv'), '/api/admin/stats', { metric, range, format: 'csv' })));
        view.appendChild(UI.tabs(STAT_METRICS.map(([id, key]) => ({ id, label: t(key) })), metric,
            (next) => Router.go('stats', '', { metric: next, range })));
        const body = h('div', { class: 'admin-tab-body' });
        view.appendChild(body);
        loadInto(body, () => Api.get('/api/admin/stats', { metric, range }), (container, data) => this[metric](container, data));
    },

    values(series) { return series.map((p) => p.value); },

    registrations(box, d) {
        box.append(h('div', { class: 'admin-stat-grid' }, UI.stat(t('admin.st_total_users'), fmtNum(d.total_users)),
            UI.stat(t('admin.st_in_range'), fmtNum(this.values(d.series).reduce((a, b) => a + b, 0)))),
        UI.card(null, Chart.render({ days: d.days, type: 'bar', series: [{ label: t('admin.sm_registrations'), values: this.values(d.series) }] })));
    },

    active(box, d) {
        box.append(h('div', { class: 'admin-stat-grid' }, UI.stat('DAU', fmtNum(d.dau)), UI.stat('WAU', fmtNum(d.wau)), UI.stat('MAU', fmtNum(d.mau))),
            UI.card(null, Chart.render({ days: d.days, type: 'line', series: [{ label: t('admin.sm_active'), values: this.values(d.series) }] })));
    },

    retention(box, d) {
        box.appendChild(h('p', { class: 'form-hint' }, t('admin.retention_note')));
        box.appendChild(UI.table({ items: d.retention, columns: [
            { label: t('admin.col_day'), cell: (r) => t('admin.retention_day', { n: r.day }) }, { label: t('admin.col_eligible'), cell: (r) => r.eligible },
            { label: t('admin.col_returned'), cell: (r) => r.returned }, { label: t('admin.col_share'), cell: (r) => r.share === null ? '–' : fmtPercent(r.share) }] }));
    },

    games(box, d) {
        box.append(h('div', { class: 'admin-stat-grid' }, UI.stat(t('admin.st_finished'), fmtNum(d.finished)), UI.stat(t('admin.st_abandoned'), fmtNum(d.abandoned)),
            UI.stat(t('admin.st_completion'), fmtPercent(d.completion_rate)), UI.stat(t('admin.st_avg_moves'), fmtNum(d.avg_moves, 1)),
            UI.stat(t('admin.st_avg_minutes'), d.avg_minutes === null ? '–' : fmtNum(d.avg_minutes, 1)),
            UI.stat(t('admin.kind_human'), fmtNum(d.types.human)), UI.stat(t('admin.kind_bots'), fmtNum(d.types.bots)),
            UI.stat(t('admin.kind_async'), fmtNum(d.types.async)), UI.stat(t('admin.kind_daily'), fmtNum(d.types.daily))),
        UI.card(null, Chart.render({ days: d.days, type: 'stack', series: [{ label: t('admin.chart_games_human'), values: this.values(d.human) },
            { label: t('admin.chart_games_bots'), values: this.values(d.bots) }] })));
    },

    bots(box, d) {
        box.appendChild(h('p', { class: 'form-hint' }, t('admin.bots_note')));
        box.appendChild(UI.table({ items: d.levels, columns: [
            { label: t('admin.col_level'), cell: (l) => l.level }, { label: t('admin.col_games'), cell: (l) => l.games },
            { label: t('admin.col_human_wins'), cell: (l) => l.human_wins }, { label: t('admin.col_human_win_rate'), cell: (l) => l.human_win_rate === null ? '–' : fmtPercent(l.human_win_rate) },
            { label: t('admin.col_strength'), cell: (l) => fmtNum(l.strength, 1) }] }));
    },

    words(box, d) {
        box.appendChild(h('div', { class: 'admin-stat-grid' }, UI.stat(t('admin.st_bingos'), fmtNum(d.bingos))));
        box.appendChild(UI.card(t('admin.st_top_words'), d.top_words.length ? Chart.hbars(d.top_words.slice(0, 15).map((w) => ({ label: w.word, value: w.count }))) : h('p', { class: 'text-muted' }, t('admin.none'))));
        box.appendChild(UI.card(t('admin.st_top_moves'), d.top_moves.length ? UI.table({ compact: true, items: d.top_moves, columns: [
            { label: t('admin.col_score'), cell: (m) => m.score }, { label: t('admin.col_player'), cell: (m) => m.player },
            { label: t('admin.col_words'), cell: (m) => m.words.join(', ') }, { label: t('admin.col_game'), cell: (m) => UI.gameLink(m.game_id, '#' + m.game_id) }] })
            : h('p', { class: 'text-muted' }, t('admin.none'))));
    },

    challenges(box, d) {
        box.appendChild(h('div', { class: 'admin-stat-grid' }, UI.stat(t('admin.st_challenge_games'), fmtNum(d.games)), UI.stat(t('admin.st_accepted'), fmtNum(d.accepted)),
            UI.stat(t('admin.st_rejected'), fmtNum(d.rejected)), UI.stat(t('admin.st_rejection_rate'), d.rejection_rate === null ? '–' : fmtPercent(d.rejection_rate))));
    },

    practice(box, d) {
        box.appendChild(h('p', { class: 'form-hint' }, t('admin.practice_note')));
        box.appendChild(h('div', { class: 'admin-stat-grid' }, Object.entries(PRACTICE_LABELS).map(([key, label]) => UI.stat(t(label), fmtNum(d.totals[key])))));
        box.appendChild(UI.card(null, Chart.render({ days: d.days, type: 'stack', series: Object.entries(PRACTICE_LABELS).map(([key, label], i) => ({
            label: t(label), values: this.values(d.series[key]), cls: i % 6 })) })));
    },

    review(box, d) {
        box.appendChild(UI.card(null, Chart.render({ days: d.days, type: 'bar', series: [
            { label: t('admin.rv_decisions'), values: this.values(d.decisions) }, { label: t('admin.rv_rejections'), values: this.values(d.rejections) }] })));
    },

    heatmap(box, d) {
        box.appendChild(h('p', { class: 'form-hint' }, t('admin.heatmap_note', { tz: d.timezone })));
        box.appendChild(UI.card(null, Chart.heatmap(d.grid, d.max, WEEKDAY_LABELS.map((key) => t(key)))));
    },
};

registerSection({ id: 'stats', order: 110, icon: 'chart', labelKey: 'admin.nav_stats',
    render: (view, ctx) => StatsView.render(view, ctx) });

// ===== Biztonság =====

const LOGIN_FLAG_LABELS = {
    ip_failures: 'admin.flag_ip_failures', account_failures: 'admin.flag_account_failures', many_accounts: 'admin.flag_many_accounts',
};

const SecurityView = {
    render(view, ctx) {
        const tab = ctx.params.get('tab') || 'logins';
        view.appendChild(UI.header(t('admin.nav_security'), t('admin.security_note')));
        view.appendChild(UI.tabs([{ id: 'logins', label: t('admin.tab_logins') }, { id: 'limits', label: t('admin.tab_limits') },
            { id: 'ipbans', label: t('admin.tab_ipbans') }, { id: 'codes', label: t('admin.tab_codes') },
            { id: 'sessions', label: t('admin.tab_sessions') }], tab, (next) => Router.go('security', '', { tab: next })));
        const body = h('div', { class: 'admin-tab-body' });
        view.appendChild(body);
        ({ logins: this.logins, limits: this.limits, ipbans: this.ipbans, codes: this.codes, sessions: this.sessions }[tab]
            || this.logins).call(this, body, ctx.params);
    },

    logins(box, params) {
        const holder = h('div');
        box.appendChild(holder);
        mountList(holder, params, {
            section: 'security', keep: ['tab'], title: t('admin.tab_logins'), path: '/api/admin/security/logins',
            filters: [
                { name: 'user', label: t('admin.f_login_user') }, { name: 'ip', label: t('admin.col_ip') },
                { name: 'success', label: t('admin.col_result'), type: 'select', options: [{ value: '', label: t('admin.any') },
                    { value: '1', label: t('admin.login_ok') }, { value: '0', label: t('admin.login_fail') }] },
                { name: 'since', label: t('admin.f_since'), type: 'date' }, { name: 'until', label: t('admin.f_until'), type: 'date' },
                { name: 'q', label: t('admin.f_q') },
            ],
            summary: (data) => {
                const s = data.suspicious;
                if (!s.ips.length && !s.accounts.length && !s.spread_ips.length) return h('div');
                return UI.card(t('admin.card_suspicious'), UI.kv([[t('admin.sus_ips'), s.ips.join(', ')], [t('admin.sus_accounts'), s.accounts.join(', ')],
                    [t('admin.sus_spread'), s.spread_ips.join(', ')]]));
            },
            columns: [
                { label: t('admin.col_time'), cell: (e) => formatStamp(e.created_at) },
                { label: t('admin.col_result'), cell: (e) => e.success ? UI.badge(t('admin.login_ok'), 'ok') : UI.badge(t('admin.login_fail'), 'danger') },
                { label: t('admin.col_email'), cell: (e) => e.user_id ? UI.userLink(e.user_id, e.email || e.display_name) : e.email },
                { label: t('admin.col_ip'), cell: (e) => e.ip || '' }, { label: t('admin.col_browser'), cell: (e) => e.user_agent },
                { label: t('admin.col_reason'), cell: (e) => e.reason || '' },
                { label: t('admin.col_flags'), cell: (e) => e.flags.map((f) => UI.badge(t(LOGIN_FLAG_LABELS[f]), 'warn')) },
            ],
            rowClass: (e) => e.flags.length ? 'row-alert' : '',
        });
    },

    limits(box) {
        const reload = () => Router.render();
        const holder = h('div');
        box.appendChild(holder);
        loadInto(holder, () => Api.get('/api/admin/security/rate-limits'), (container, data) => {
            container.appendChild(UI.card(t('admin.card_limited_ips'), data.ips.length ? UI.table({ compact: true, items: data.ips, columns: [
                { label: t('admin.col_ip'), cell: (r) => r.ip }, { label: t('admin.col_action'), cell: (r) => r.action },
                { label: t('admin.col_count'), cell: (r) => `${r.count} / ${r.max}` }, { label: t('admin.col_window'), cell: (r) => fmtDuration(r.window) },
                { label: t('admin.col_retry'), cell: (r) => fmtDuration(r.retry_in) },
                { label: '', cell: (r) => UI.btn(t('admin.unblock'), () => Act.open({ title: t('admin.unblock'), text: r.ip,
                    run: (v) => Api.post('/api/admin/security/rate-limits/unblock', { ip: r.ip, reason: v.reason }), done: reload }), { kind: 'secondary' }) }] })
                : h('p', { class: 'text-muted' }, t('admin.none'))));
            container.appendChild(UI.card(t('admin.card_limited_sids'), data.sids.length ? UI.table({ compact: true, items: data.sids, columns: [
                { label: 'SID', cell: (r) => r.sid.slice(0, 8) }, { label: t('admin.col_action'), cell: (r) => r.event },
                { label: t('admin.col_count'), cell: (r) => `${r.count} / ${r.max}` }, { label: t('admin.col_retry'), cell: (r) => fmtDuration(r.retry_in) }] })
                : h('p', { class: 'text-muted' }, t('admin.none'))));
            for (const [group, titleKey, setting] of [['http', 'admin.card_limits_http', 'rate_limits_http'], ['socket', 'admin.card_limits_socket', 'rate_limits_socket']]) {
                const limits = data.limits[group];
                const overrides = () => Object.fromEntries(Object.entries(limits).filter(([, v]) => v.overridden).map(([k, v]) => [k, v.current]));
                container.appendChild(UI.card(t(titleKey), UI.table({ compact: true, items: Object.entries(limits).sort(([a], [b]) => a.localeCompare(b)), columns: [
                    { label: t('admin.col_name'), cell: ([name]) => h('code', null, name) },
                    { label: t('admin.col_current'), cell: ([, v]) => `${v.current[0]} / ${fmtDuration(v.current[1])}` },
                    { label: t('admin.col_default'), cell: ([, v]) => `${v.default[0]} / ${fmtDuration(v.default[1])}` },
                    { label: t('admin.col_status'), cell: ([, v]) => v.overridden ? UI.badge(t('admin.overridden'), 'warn') : '' },
                    { label: '', cell: ([name, v]) => h('span', { class: 'admin-action-row' },
                        UI.btn(t('admin.edit'), () => Act.open({ title: name, text: t('admin.limit_edit_text'), danger: true,
                            fields: [{ name: 'count', type: 'number', label: t('admin.f_limit_count'), value: v.current[0], min: 1, max: 100000, required: true },
                                { name: 'window', type: 'number', label: t('admin.f_limit_window'), value: v.current[1], min: 1, max: 86400, required: true }],
                            run: (f) => Api.patch('/api/admin/settings', { changes: { [setting]: { ...overrides(), [name]: [f.count, f.window] } }, reason: f.reason }),
                            done: reload }), { kind: 'secondary' }),
                        v.overridden ? UI.btn(t('admin.reset_default'), () => Act.open({ title: t('admin.reset_default'), text: name,
                            run: (f) => { const next = overrides(); delete next[name];
                                return Api.patch('/api/admin/settings', { changes: { [setting]: Object.keys(next).length ? next : null }, reason: f.reason }); },
                            done: reload }), { kind: 'secondary' }) : null) },
                ] })));
            }
        });
    },

    ipbans(box) {
        const reload = () => Router.render();
        const holder = h('div');
        box.appendChild(holder);
        loadInto(holder, () => Api.get('/api/admin/security/ip-bans'), (container, data) => {
            container.appendChild(h('div', { class: 'admin-toolbar' }, h('span', { class: 'text-secondary text-sm' }, t('admin.own_ip', { ip: data.own_ip })),
                UI.btn(t('admin.act_ipban') + ' 🔒', () => Act.open({
                    title: t('admin.act_ipban'), text: t('admin.act_ipban_text'), danger: true,
                    fields: [{ name: 'ip', label: t('admin.f_ip'), placeholder: '203.0.113.7 / 203.0.113.0/24', required: true, maxlength: 60 },
                        { name: 'until', type: 'select', label: t('admin.f_duration'), options: durationOptions(true), value: '7d' }],
                    run: (v) => Api.post('/api/admin/security/ip-bans', { ip: v.ip, until: v.until, reason: v.reason }), done: reload }), { kind: 'danger' })));
            if (!data.items.length) { container.appendChild(UI.empty(t('admin.none'))); return; }
            container.appendChild(UI.table({ items: data.items, columns: [
                { label: t('admin.col_ip'), cell: (b) => h('code', null, b.ip) },
                { label: t('admin.col_status'), cell: (b) => b.active ? UI.badge(t('admin.active'), 'danger') : UI.badge(t('admin.expired'), 'muted') },
                { label: t('admin.col_reason'), cell: (b) => b.reason || '' }, { label: t('admin.f_ends'), cell: (b) => b.expires_at ? formatStamp(b.expires_at) : t('admin.dur_opt_permanent') },
                { label: t('admin.col_admin'), cell: (b) => b.admin_name || '' }, { label: t('admin.col_created'), cell: (b) => formatStamp(b.created_at) },
                { label: '', cell: (b) => UI.btn(t('admin.delete'), () => Act.open({ title: t('admin.act_ipban_remove'), text: b.ip,
                    run: (v) => Api.del(`/api/admin/security/ip-bans/${b.id}`, { reason: v.reason }), done: reload }), { kind: 'secondary' }) },
            ] }));
        });
    },

    codes(box) {
        const reload = () => Router.render();
        const holder = h('div');
        box.appendChild(holder);
        loadInto(holder, () => Api.get('/api/admin/security/codes'), (container, data) => {
            container.appendChild(h('p', { class: 'form-hint' }, data.smtp ? t('admin.codes_note_smtp') : t('admin.codes_note_nosmtp')));
            if (!data.items.length) { container.appendChild(UI.empty(t('admin.none'))); return; }
            container.appendChild(UI.table({ items: data.items, columns: [
                { label: t('admin.col_email'), cell: (c) => c.email }, { label: t('admin.col_code'), cell: (c) => h('code', null, c.code) },
                { label: t('admin.col_created'), cell: (c) => formatStamp(c.created_at) }, { label: t('admin.col_expires'), cell: (c) => formatStamp(c.expires_at) },
                { label: t('admin.col_attempts'), cell: (c) => c.attempts },
                { label: '', cell: (c) => UI.btn(t('admin.invalidate'), () => Act.open({ title: t('admin.invalidate'), text: c.email,
                    run: (v) => Api.del(`/api/admin/security/codes/${c.id}`, { reason: v.reason }), done: reload }), { kind: 'secondary' }) },
            ] }));
        });
    },

    sessions(box, params) {
        const holder = h('div');
        box.appendChild(holder);
        const reload = () => Router.render();
        mountList(holder, params, {
            section: 'security', keep: ['tab'], title: t('admin.tab_sessions'), path: '/api/admin/security/sessions', pageSize: 500, csv: false,
            filters: [{ name: 'q', label: t('admin.f_session_q') }, { name: 'admin', label: t('admin.f_admins'), type: 'checkbox' }],
            headerActions: [
                UI.btn(t('admin.sessions_close_admins'), () => Act.open({ title: t('admin.sessions_close_admins'), text: t('admin.sessions_close_admins_text'), danger: true,
                    run: (v) => Api.post('/api/admin/security/sessions/close-admins', { reason: v.reason }), done: (r) => { showToast(t('admin.sessions_closed_n', { n: r.removed })); reload(); } }), { kind: 'secondary' }),
                UI.btn(t('admin.sessions_revoke_all') + ' 🔒', () => Act.open({ title: t('admin.sessions_revoke_all'), text: t('admin.sessions_revoke_all_text'), danger: true,
                    run: (v) => Api.post('/api/admin/security/sessions/revoke-all', { reason: v.reason }), done: (r) => { showToast(t('admin.sessions_closed_n', { n: r.removed })); reload(); } }), { kind: 'danger' }),
            ],
            columns: [
                { label: t('admin.col_name'), cell: (s) => h('span', null, UI.userLink(s.user_id, s.display_name), s.is_admin ? [' ', UI.badge(t('admin.st_admin'), 'info')] : null,
                    s.current ? [' ', UI.badge(t('admin.this_session'), 'ok')] : null) },
                { label: t('admin.col_created'), cell: (s) => formatStamp(s.created_at) }, { label: t('admin.col_last_seen'), cell: (s) => formatStamp(s.last_seen) },
                { label: t('admin.col_expires'), cell: (s) => formatStamp(s.expires_at) }, { label: t('admin.col_ip'), cell: (s) => s.ip || '' },
                { label: t('admin.col_browser'), cell: (s) => s.user_agent },
                { label: '', cell: (s) => s.current ? null : UI.btn(t('admin.revoke'), () => Act.open({ title: t('admin.act_revoke_session'),
                    run: (v) => Api.del(`/api/admin/security/sessions/${s.id}`, { reason: v.reason }), done: reload }), { kind: 'secondary' }) },
            ],
        });
    },
};

registerSection({ id: 'security', order: 120, icon: 'shield', labelKey: 'admin.nav_security',
    render: (view, ctx) => SecurityView.render(view, ctx) });

// ===== Rendszer =====

const CLEANUP_LABELS = {
    sessions: 'admin.cl_sessions', codes: 'admin.cl_codes', verified_emails: 'admin.cl_verified_emails',
    abandoned_games: 'admin.cl_abandoned_games', chat_log: 'admin.cl_chat_log', ip_bans: 'admin.cl_ip_bans', login_events: 'admin.cl_login_events',
};

// ===== Levelező szerver (SMTP) =====

const MAIL_SECURITY_LABELS = { starttls: 'admin.mail_sec_starttls', ssl: 'admin.mail_sec_ssl', none: 'admin.mail_sec_none' };
const MAIL_ERROR_LABELS = {
    dns: 'admin.mail_err_dns', timeout: 'admin.mail_err_timeout', refused: 'admin.mail_err_refused', connect: 'admin.mail_err_connect',
    tls: 'admin.mail_err_tls', certificate: 'admin.mail_err_certificate', auth: 'admin.mail_err_auth', unsupported: 'admin.mail_err_unsupported',
    sender: 'admin.mail_err_sender', recipient: 'admin.mail_err_recipient', disconnected: 'admin.mail_err_disconnected',
    protocol: 'admin.mail_err_protocol', error: 'admin.mail_err_error',
};

function mailErrorText(code) {
    return t(MAIL_ERROR_LABELS[code] || MAIL_ERROR_LABELS.error);
}

// ISO 8601 időbélyeg (pl. a git commit ideje) helyi formában
function formatIso(value) {
    const date = value ? new Date(value) : null;
    return date && !Number.isNaN(date.getTime()) ? date.toLocaleString(I18N.locale()) : '–';
}

const MailCard = {
    build(mail, reload) {
        const field = (labelKey, control, hintKey) => h('label', { class: 'admin-field' },
            h('span', { class: 'admin-field-label' }, t(labelKey)), control,
            hintKey ? h('span', { class: 'form-hint form-hint-tight' }, t(hintKey)) : null);
        const host = h('input', { type: 'text', maxlength: 253, value: mail.host, placeholder: 'smtp.gmail.com', autocomplete: 'off' });
        const port = h('input', { type: 'number', min: 1, max: 65535, value: mail.port });
        const security = h('select', null, Object.entries(MAIL_SECURITY_LABELS).map(([value, key]) => h('option', { value }, t(key))));
        security.value = mail.security;
        const username = h('input', { type: 'text', maxlength: 254, value: mail.username, autocomplete: 'off' });
        const password = h('input', { type: 'password', maxlength: 256, autocomplete: 'new-password',
            placeholder: mail.password_set ? t('admin.mail_password_keep') : '' });
        const fromAddress = h('input', { type: 'email', maxlength: 254, value: mail.from_address, autocomplete: 'off' });
        const fromName = h('input', { type: 'text', maxlength: 80, value: mail.from_name, autocomplete: 'off' });
        const verify = h('input', { type: 'checkbox', checked: !!mail.verify_tls });
        const plainWarning = h('p', { class: 'form-hint admin-mail-warning' + (mail.security === 'none' ? '' : ' hidden') }, t('admin.mail_plain_warning'));

        // A titkosítási mód váltásakor a port az új mód szokásos portjára áll, ha addig a régi mód alapportja volt
        let lastSecurity = security.value;
        security.addEventListener('change', () => {
            if (Number(port.value) === mail.defaults[lastSecurity]) port.value = mail.defaults[security.value];
            lastSecurity = security.value;
            plainWarning.classList.toggle('hidden', security.value !== 'none');
        });

        // A jelszó üresen hagyva a mentettet jelenti (a szerver csak ugyanahhoz a kiszolgálóhoz / felhasználóhoz tartja meg)
        const collect = (extra) => ({
            host: host.value.trim(), port: port.value === '' ? null : Number(port.value), security: security.value,
            username: username.value.trim(), password: password.value === '' ? null : password.value,
            from_address: fromAddress.value.trim(), from_name: fromName.value.trim(), verify_tls: verify.checked, ...extra,
        });

        const sendTest = h('input', { type: 'checkbox' });
        const result = h('div', { class: 'admin-mail-result', role: 'status' });
        const setResult = (kind, text) => { result.className = 'admin-mail-result' + (kind ? ' ' + kind : ''); result.textContent = text; };
        const checkBtn = UI.btn(t('admin.mail_check') + ' 🔒', async () => {
            setResult('', t('admin.mail_checking'));
            checkBtn.disabled = true;
            const response = await Api.post('/api/admin/system/mail/check', collect({ send: sendTest.checked }));
            checkBtn.disabled = false;
            if (!response.ok) { if (response.status !== 401) setResult('bad', Api.message(response)); else setResult('', ''); return; }
            const data = response.data;
            if (!data.ok) setResult('bad', t('admin.mail_check_failed', { reason: mailErrorText(data.code) }));
            else setResult('good', data.sent_to ? t('admin.mail_check_sent', { to: data.sent_to }) : t('admin.mail_check_ok'));
        }, { kind: 'secondary' });

        const env = mail.environment;
        const sourceBadge = UI.badge(t(mail.source === 'database' ? 'admin.mail_source_database' : 'admin.mail_source_environment'),
            mail.source === 'database' ? 'warn' : 'muted');
        return UI.card(t('admin.card_mail'), h('p', { class: 'form-hint' }, t('admin.mail_note')),
            UI.kv([
                [t('admin.mail_state'), h('span', null, UI.badge(mail.configured ? t('admin.yes') : t('admin.no'), mail.configured ? 'ok' : 'danger'), ' ', sourceBadge)],
                [t('admin.mail_env'), env.host ? `${env.host}:${env.port} · ${env.from_address || '–'} · ` + (env.configured ? t('admin.yes') : t('admin.no')) : null],
                [t('admin.mail_changed'), mail.updated_at ? t('admin.set_changed', { who: mail.updated_by_name || '?', when: formatStamp(mail.updated_at) }) : null],
            ]),
            h('div', { class: 'admin-mail-form' },
                field('admin.f_mail_host', host), field('admin.f_mail_port', port), field('admin.f_mail_security', security),
                field('admin.f_mail_username', username, 'admin.f_mail_username_hint'), field('admin.f_mail_password', password, 'admin.f_mail_password_hint'),
                field('admin.f_mail_from', fromAddress), field('admin.f_mail_from_name', fromName)),
            plainWarning,
            h('label', { class: 'admin-field admin-field-check' }, verify, h('span', { class: 'admin-field-label' }, t('admin.f_mail_verify'))),
            h('p', { class: 'form-hint form-hint-tight' }, t('admin.f_mail_verify_hint')),
            h('label', { class: 'admin-field admin-field-check' }, sendTest, h('span', { class: 'admin-field-label' }, t('admin.f_mail_send_test'))),
            result,
            h('div', { class: 'admin-action-row' },
                checkBtn,
                UI.btn(t('admin.mail_save') + ' 🔒', () => Act.open({
                    title: t('admin.mail_save_title'), text: t('admin.mail_save_text'), doneText: t('admin.mail_saved'),
                    run: (v) => Api.patch('/api/admin/system/mail', collect({ reason: v.reason })), done: reload })),
                UI.btn(t('admin.smtp_test'), async () => {
                    const response = await Api.post('/api/admin/system/smtp-test');
                    if (!response.ok) showToast(response.data.code ? t('admin.smtp_test_failed', { reason: mailErrorText(response.data.code) }) : Api.message(response), true);
                    else showToast(response.data.console ? t('admin.email_console') : t('admin.smtp_test_sent'));
                }, { kind: 'secondary' }),
                mail.source === 'database' ? UI.btn(t('admin.mail_reset') + ' 🔒', () => Act.open({
                    title: t('admin.mail_reset'), text: t('admin.mail_reset_text'), danger: true, doneText: t('admin.mail_reset_done'),
                    run: (v) => Api.del('/api/admin/system/mail', { reason: v.reason }), done: reload }), { kind: 'secondary' }) : null));
    },
};

// ===== Frissítés GitHubról és újraindítás =====

// Újraindítás után a szerver egy ideig nem válaszol: amint újra elérhető, a lap újratölt
async function waitForRestart() {
    showToast(t('admin.upd_restarting'));
    let down = false;
    for (let i = 0; i < 90; i += 1) {
        await new Promise((resolve) => setTimeout(resolve, 2000));
        const probe = await Api.request('GET', '/api/admin/session');
        if (probe.status === 0) down = true;
        else if (down) { location.reload(); return; }
    }
    showToast(t('admin.upd_restart_timeout'), true);
}

const UpdateCard = {
    build(reload) {
        const holder = h('div');
        loadInto(holder, () => Api.get('/api/admin/system/update'), (box, data) => this.paint(box, data.update, reload));
        return UI.card(t('admin.card_update'), h('p', { class: 'form-hint' }, t('admin.update_note')), holder);
    },

    paint(box, u, reload) {
        if (!u.repo) { box.appendChild(h('p', { class: 'text-muted' }, t('admin.upd_not_repo'))); return; }
        const commit = u.commit || {};
        box.appendChild(UI.kv([
            [t('admin.upd_branch'), u.branch || t('admin.upd_detached')],
            [t('admin.upd_commit'), h('span', null, h('code', null, commit.short || '–'), ' ', commit.subject || ''), { wide: true }],
            [t('admin.col_time'), commit.date ? formatIso(commit.date) : null],
            [t('admin.upd_remote'), u.remote ? h('code', { class: 'admin-code-wrap' }, u.remote) : null, { wide: true }],
            [t('admin.upd_running'), h('span', null, h('code', null, u.running_commit || '–'), ' ',
                u.restart_needed ? UI.badge(t('admin.upd_restart_needed'), 'warn') : UI.badge(t('admin.upd_in_sync'), 'ok'))],
        ]));
        if (u.dirty_count) {
            box.appendChild(h('div', { class: 'admin-mail-warning form-hint' }, h('strong', null, t('admin.upd_dirty', { n: u.dirty_count })), ' ',
                t('admin.upd_dirty_note'), h('div', null, u.dirty.map((path) => h('code', { class: 'admin-code-wrap' }, path + ' ')))));
        }

        const planBox = h('div', { class: 'admin-update-plan' });
        const errorText = (response) => Api.message(response) + (response.data.detail ? ' — ' + response.data.detail : '');
        const runCheck = async (branch, fetch = true) => {
            planBox.replaceChildren(h('p', { class: 'text-muted' }, t('admin.upd_checking')));
            const response = await Api.post('/api/admin/system/update/check', { branch, fetch });
            if (!response.ok) { planBox.replaceChildren(response.status === 401 ? '' : UI.errorBox(errorText(response))); return; }
            this.paintPlan(planBox, response.data.update, runCheck, reload);
        };
        box.appendChild(h('div', { class: 'admin-action-row' },
            UI.btn(t('admin.upd_check'), () => runCheck(undefined), { kind: 'secondary' }),
            UI.btn(t('admin.upd_restart') + ' 🔒', () => Act.open({
                title: t('admin.upd_restart'), text: t('admin.upd_restart_text'), danger: true,
                run: (v) => Api.post('/api/admin/system/restart', { reason: v.reason }), done: () => waitForRestart() }), { kind: 'secondary', disabled: u.restart_scheduled })));
        box.appendChild(planBox);
    },

    paintPlan(box, u, recheck, reload) {
        const plan = u.plan;
        const select = h('select', null, u.branches.map((b) => h('option', { value: b.name },
            `${b.name}${b.current ? ' · ' + t('admin.upd_branch_current') : ''} · ${b.short} · ${b.subject}`)));
        select.value = plan.branch;
        select.addEventListener('change', () => recheck(select.value, false));
        const blocked = u.dirty_count > 0 || plan.ahead > 0;
        const children = [h('label', { class: 'admin-field' }, h('span', { class: 'admin-field-label' }, t('admin.upd_pick_branch')), select)];

        if (plan.up_to_date) {
            children.push(h('p', { class: 'form-hint' }, t('admin.upd_up_to_date')));
        } else {
            children.push(UI.kv([
                [t('admin.upd_switch_label'), plan.switch ? t('admin.upd_switch', { from: u.branch || t('admin.upd_detached'), to: plan.branch }) : null],
                [t('admin.upd_new_commits'), String(plan.behind)], [t('admin.upd_files'), String(plan.files_total)]]));
            if (plan.ahead > 0) children.push(h('p', { class: 'form-hint admin-mail-warning' }, t('admin.upd_ahead', { n: plan.ahead })));
            if (plan.incoming.length) {
                children.push(UI.subtitle(t('admin.upd_incoming')), UI.table({ compact: true, items: plan.incoming, columns: [
                    { label: t('admin.upd_col_commit'), cell: (c) => h('code', null, c.short) }, { label: t('admin.upd_col_message'), cell: (c) => c.subject },
                    { label: t('admin.upd_col_author'), cell: (c) => c.author }, { label: t('admin.col_time'), cell: (c) => formatIso(c.date) }] }));
            }
            if (plan.files.length) {
                children.push(h('details', { class: 'admin-details' }, h('summary', null, t('admin.upd_files_list', { n: plan.files_total })),
                    h('div', null, plan.files.map((f) => h('div', null, h('code', null, f.status + ' ' + f.path))))));
            }
        }
        children.push(h('div', { class: 'admin-action-row' }, UI.btn(t('admin.upd_apply') + ' 🔒', () => this.apply(plan, reload),
            { kind: 'primary', disabled: plan.up_to_date || blocked })));
        box.replaceChildren(...children);
    },

    apply(plan, reload) {
        const fields = [];
        if (plan.requirements_changed) fields.push({ name: 'install', type: 'checkbox', label: t('admin.f_upd_install'), value: true });
        fields.push({ name: 'restart', type: 'checkbox', label: t('admin.f_upd_restart'), value: false, hint: t('admin.f_upd_restart_hint') });
        Act.open({
            title: t('admin.upd_apply_title', { branch: plan.branch }), text: t('admin.upd_apply_text', { branch: plan.branch }), danger: true, fields,
            doneText: t('admin.upd_done'),
            run: (v) => Api.post('/api/admin/system/update/apply', { branch: plan.branch, install: !!v.install, restart: !!v.restart, reason: v.reason }),
            done: (data, v) => {
                if (data.update.installed === false) showToast(t('admin.upd_install_failed'), true);
                if (data.update.restarting) waitForRestart(); else reload();
            },
        });
    },
};

// A konfiguráció értéke szövegként (az összetett érték — pl. a forgalomkorlátok szótára — JSON-ként, nem „[object Object]”)
function configValueText(value) {
    if (value === null || value === undefined) return '';
    return typeof value === 'object' ? JSON.stringify(value) : String(value);
}

const SystemView = {
    async render(view) {
        const holder = h('div');
        view.appendChild(holder);
        await loadInto(holder, () => Api.get('/api/admin/system'), (box, data) => this.paint(box, data));
    },

    paint(box, d) {
        const reload = () => Router.render();
        const yes = (ok) => UI.badge(ok ? t('admin.yes') : t('admin.no'), ok ? 'ok' : 'danger');
        box.appendChild(UI.header(t('admin.nav_system'), t('admin.system_note'), UI.btn(t('admin.refresh'), reload, { kind: 'secondary' })));
        const s = d.server;
        const serverCard = UI.card(t('admin.server_title'), UI.kv([
            [t('admin.srv_uptime'), fmtDuration(s.uptime)], [t('admin.srv_memory'), fmtBytes(s.rss)],
            [t('admin.srv_cpu'), s.cpu_percent === null ? '–' : fmtPercent(s.cpu_percent)], [t('admin.srv_greenlets'), fmtNum(s.greenlets)],
            [t('admin.srv_threads'), fmtNum(s.threads)], [t('admin.srv_python'), s.python]]));

        const tunnel = d.tunnel || {};
        const servicesCard = UI.card(t('admin.card_services'), UI.kv([
            [t('admin.srv_dictionary'), yes(d.services.dictionary)], [t('admin.srv_vocabulary'), yes(d.services.vocabulary)],
            [t('admin.srv_push'), h('span', null, yes(d.push.available), ' ', t('admin.push_subs', { n: d.push.subscriptions }))],
            [t('admin.srv_smtp'), h('span', null, yes(d.mail.configured), d.mail.host ? ' ' + d.mail.host : '')],
            [t('admin.srv_tunnel'), h('span', null, UI.badge(tunnel.state || '–', tunnel.url ? 'ok' : 'muted'), tunnel.url ? [' ', tunnel.url] : null)],
            [t('admin.d_vapid'), d.push.public_key ? h('code', { class: 'admin-code-wrap' }, d.push.public_key) : null, { wide: true }]]),
        h('div', { class: 'admin-action-row' },
            tunnel.url ? UI.copy(tunnel.url) : null,
            UI.btn(t('admin.tunnel_restart') + ' 🔒', () => Act.open({ title: t('admin.tunnel_restart'), text: t('admin.tunnel_restart_text'), danger: true,
                run: (v) => Api.post('/api/admin/system/tunnel/restart', { reason: v.reason }), done: reload }), { kind: 'secondary' }),
            UI.btn(t('admin.push_test_self'), async () => {
                const result = await Api.post('/api/admin/system/push-test');
                showToast(result.ok ? t('admin.push_test_sent', { n: result.data.sent }) : Api.message(result), !result.ok);
            }, { kind: 'secondary' })));
        box.appendChild(UI.cardGrid(serverCard, servicesCard));

        box.appendChild(MailCard.build(d.mail, reload));
        box.appendChild(UpdateCard.build(reload));

        const v = d.versions;
        box.appendChild(UI.card(t('admin.card_versions'), UI.kv([
            [t('admin.srv_git'), v.git, { wide: true }], [t('admin.srv_assets'), String(v.asset_version)], [t('admin.d_admin_assets'), String(v.admin_asset_version)],
            [t('admin.srv_python'), v.python], [t('admin.d_platform'), v.platform], [t('admin.d_tests'), v.tests],
            ...Object.entries(v.packages).map(([name, version]) => [name, version])])));

        box.appendChild(UI.card(t('admin.card_config'), h('p', { class: 'form-hint' }, t('admin.config_note')), UI.table({ compact: true, items: d.config, columns: [
            { label: t('admin.col_name'), cell: (c) => h('code', null, c.key) },
            { label: t('admin.col_value'), cell: (c) => c.secret ? h('span', null, '••••  ', UI.badge(c.set ? t('admin.set') : t('admin.not_set'), c.set ? 'ok' : 'muted'))
                : configValueText(c.value) }] })));

        this.database(box, d.database, reload);

        box.appendChild(UI.card(t('admin.card_jobs'), UI.table({ compact: true, items: d.jobs, columns: [
            { label: t('admin.col_name'), cell: (j) => h('code', null, j.name) },
            { label: t('admin.col_status'), cell: (j) => j.ok === null ? UI.badge(t('admin.never_run'), 'muted') : (j.ok ? UI.badge(t('admin.ok'), 'ok') : UI.badge(t('admin.failed'), 'danger')) },
            { label: t('admin.d_sweeper_runs'), cell: (j) => j.runs }, { label: t('admin.d_sweeper_errors'), cell: (j) => j.errors },
            { label: t('admin.d_sweeper_last'), cell: (j) => j.last_run ? formatEpoch(j.last_run) : '–' }, { label: t('admin.d_sweeper_info'), cell: (j) => j.info || '' }] }),
        d.analysis.length ? h('div', null, UI.subtitle(t('admin.analysis_queue')), UI.table({ compact: true, items: d.analysis, columns: [
            { label: t('admin.col_game'), cell: (a) => UI.gameLink(a.game_id, '#' + a.game_id) }, { label: t('admin.col_progress'), cell: (a) => `${a.done} / ${a.total}` },
            { label: t('admin.col_status'), cell: (a) => a.error ? UI.badge(t('admin.failed'), 'danger') : '' }] })) : null));

        box.appendChild(this.logs());
    },

    database(box, db, reload) {
        const backupBtns = h('div', { class: 'admin-action-row' },
            UI.btn(t('admin.backup_create'), () => Act.open({ title: t('admin.backup_create'), text: t('admin.backup_create_text'), reason: false,
                run: () => Api.post('/api/admin/system/backup'), done: (r) => { showToast(t('admin.backup_created', { name: r.name })); reload(); } }), { kind: 'secondary' }),
            UI.btn(t('admin.backup_download') + ' 🔒', () => downloadWithSudo('/api/admin/system/backup'), { kind: 'secondary' }),
            UI.btn(t('admin.vacuum') + ' 🔒', () => Act.open({ title: t('admin.vacuum'), text: t('admin.vacuum_text'),
                run: (v) => Api.post('/api/admin/system/vacuum', { reason: v.reason }),
                done: (r) => showToast(t('admin.vacuum_done', { before: fmtBytes(r.before), after: fmtBytes(r.after) })) }), { kind: 'secondary' }),
            UI.btn(t('admin.cleanup') + ' 🔒', async () => {
                const days = 30;
                const preview = await Api.get('/api/admin/system/cleanup', { days });
                if (!preview.ok) { showToast(Api.message(preview), true); return; }
                const counts = preview.data.counts;
                Act.open({ title: t('admin.cleanup'), text: t('admin.cleanup_text'), danger: true,
                    fields: [{ name: 'items', type: 'check_list', label: t('admin.f_cleanup_items'),
                        options: Object.keys(CLEANUP_LABELS).map((key) => ({ value: key, label: `${t(CLEANUP_LABELS[key])} (${counts[key] || 0})`, checked: !!counts[key] })) },
                    { name: 'days', type: 'number', label: t('admin.f_days'), value: days, min: 1, max: 3650, required: true }],
                    validate: (v) => (v.items.length ? null : t('admin.err_nothing_selected')),
                    run: (v) => Api.post('/api/admin/system/cleanup', { items: v.items, days: v.days, reason: v.reason }),
                    done: (r) => { showToast(t('admin.cleanup_done', { n: Object.values(r.removed).reduce((a, b) => a + b, 0) })); reload(); } });
            }, { kind: 'secondary' }));
        box.appendChild(UI.card(t('admin.card_database'), UI.kv([
            [t('admin.srv_db'), fmtBytes(db.size)], [t('admin.d_last_backup'), db.last_backup ? formatEpoch(db.last_backup) : t('admin.none')]]), backupBtns,
        db.backups.length ? h('div', null, UI.subtitle(t('admin.backups_title')), UI.table({ compact: true, items: db.backups, columns: [
            { label: t('admin.col_name'), cell: (b) => h('code', null, b.name) }, { label: t('admin.col_size'), cell: (b) => fmtBytes(b.size) },
            { label: t('admin.col_time'), cell: (b) => formatEpoch(b.mtime) }] })) : null,
        h('details', { class: 'admin-details' }, h('summary', null, t('admin.tables_title')), UI.table({ compact: true, items: db.tables, columns: [
            { label: t('admin.col_name'), cell: (tb) => h('code', null, tb.name) }, { label: t('admin.col_rows'), cell: (tb) => fmtNum(tb.rows) }] }))));
    },

    // Szerver napló: gyűrűpuffer, szint és szöveg szerint szűrhető, élő követéssel; a hibák csoportosítva
    logs() {
        const card = UI.card(t('admin.card_logs'), h('p', { class: 'form-hint' }, t('admin.logs_note')));
        const level = h('select', null, [['', 'admin.lv_all'], ['INFO', 'admin.lv_info'], ['WARNING', 'admin.lv_warning'], ['ERROR', 'admin.lv_error']]
            .map(([value, key]) => h('option', { value }, t(key))));
        const search = h('input', { type: 'search', placeholder: t('admin.f_q') });
        const follow = h('input', { type: 'checkbox', checked: true });
        const out = h('div', { class: 'admin-log' });
        const groups = h('div');
        const load = async () => {
            const result = await Api.get('/api/admin/system/logs', { level: level.value, q: search.value.trim(), limit: 200 });
            if (!result.ok) { out.replaceChildren(UI.errorBox(Api.message(result))); return; }
            out.replaceChildren(...result.data.items.map((r) => h('div', { class: 'admin-log-line log-' + r.level.toLowerCase() },
                h('span', { class: 'admin-log-time' }, new Date(r.ts * 1000).toLocaleTimeString(I18N.locale())), h('span', { class: 'admin-log-level' }, r.level),
                h('span', { class: 'admin-log-msg' }, (r.logger ? r.logger + ': ' : '') + r.message))));
            if (!result.data.items.length) out.appendChild(h('p', { class: 'text-muted' }, t('admin.none')));
            out.scrollTop = out.scrollHeight;
            groups.replaceChildren(result.data.groups.length ? h('div', null, UI.subtitle(t('admin.error_groups')), UI.table({ compact: true, items: result.data.groups, columns: [
                { label: t('admin.col_error'), cell: (g) => g.exc_type || g.sample }, { label: t('admin.col_location'), cell: (g) => g.location || '' },
                { label: t('admin.col_count'), cell: (g) => g.count }, { label: t('admin.col_last_seen'), cell: (g) => new Date(g.last_ts * 1000).toLocaleString(I18N.locale()) }] })) : '');
        };
        level.addEventListener('change', load);
        search.addEventListener('input', () => { clearTimeout(search._t); search._t = setTimeout(load, 300); });
        card.append(h('div', { class: 'admin-filters admin-filters-inline' }, h('label', { class: 'admin-field' }, h('span', { class: 'admin-field-label' }, t('admin.f_level')), level),
            h('label', { class: 'admin-field' }, h('span', { class: 'admin-field-label' }, t('admin.f_q')), search),
            h('label', { class: 'admin-field admin-field-check' }, follow, h('span', { class: 'admin-field-label' }, t('admin.logs_follow')))), out, groups);
        load();
        Router.interval(() => { if (follow.checked) load(); }, 5000);
        return card;
    },
};

registerSection({ id: 'system', order: 130, icon: 'server', labelKey: 'admin.nav_system',
    render: (view) => SystemView.render(view) });

// ===== Beállítások (funkciókapcsolók) =====

const SETTING_TEXT = {
    registration_open: ['admin.set_registration_open', 'admin.set_registration_open_hint'],
    guest_allowed: ['admin.set_guest_allowed', 'admin.set_guest_allowed_hint'],
    room_creation: ['admin.set_room_creation', 'admin.set_room_creation_hint'],
    max_spectators: ['admin.set_max_spectators', 'admin.set_max_spectators_hint'],
    default_hint_limit: ['admin.set_default_hint_limit', 'admin.set_default_hint_limit_hint'],
    bots_enabled: ['admin.set_bots_enabled', 'admin.set_bots_enabled_hint'],
    max_bots: ['admin.set_max_bots', 'admin.set_max_bots_hint'],
    default_bot_level: ['admin.set_default_bot_level', 'admin.set_default_bot_level_hint'],
    bot_think_multiplier: ['admin.set_bot_think_multiplier', 'admin.set_bot_think_multiplier_hint'],
    feature_daily: ['admin.set_feature_daily', 'admin.set_feature_daily_hint'],
    feature_async: ['admin.set_feature_async', 'admin.set_feature_async_hint'],
    feature_practice: ['admin.set_feature_practice', 'admin.set_feature_practice_hint'],
    feature_word_review: ['admin.set_feature_word_review', 'admin.set_feature_word_review_hint'],
    word_reject_threshold: ['admin.set_word_reject_threshold', 'admin.set_word_reject_threshold_hint'],
    grace_disconnect: ['admin.set_grace_disconnect', 'admin.set_grace_disconnect_hint'],
    grace_waiting_owner: ['admin.set_grace_waiting_owner', 'admin.set_grace_waiting_owner_hint'],
    chat_max_length: ['admin.set_chat_max_length', 'admin.set_chat_max_length_hint'],
    chat_rate_count: ['admin.set_chat_rate_count', 'admin.set_chat_rate_count_hint'],
    chat_rate_window: ['admin.set_chat_rate_window', 'admin.set_chat_rate_window_hint'],
    banned_word_action: ['admin.set_banned_word_action', 'admin.set_banned_word_action_hint'],
    chat_log_enabled: ['admin.set_chat_log_enabled', 'admin.set_chat_log_enabled_hint'],
    chat_log_days: ['admin.set_chat_log_days', 'admin.set_chat_log_days_hint'],
    backup_daily: ['admin.set_backup_daily', 'admin.set_backup_daily_hint'],
    backup_keep: ['admin.set_backup_keep', 'admin.set_backup_keep_hint'],
    rate_limits_http: ['admin.set_rate_limits_http', 'admin.set_rate_limits_http_hint'],
    rate_limits_socket: ['admin.set_rate_limits_socket', 'admin.set_rate_limits_socket_hint'],
};
const SETTING_GROUP_LABELS = {
    access: 'admin.sg_access', rooms: 'admin.sg_rooms', bots: 'admin.sg_bots', features: 'admin.sg_features', dictionary: 'admin.sg_dictionary',
    timing: 'admin.sg_timing', chat: 'admin.sg_chat', limits: 'admin.sg_limits', logging: 'admin.sg_logging',
};
const SETTING_CHOICE_LABELS = {
    everyone: 'admin.sc_everyone', registered: 'admin.sc_registered', none: 'admin.sc_none', mask: 'admin.sc_mask', drop: 'admin.sc_drop',
};

function settingChoiceLabel(value) {
    return SETTING_CHOICE_LABELS[value] ? t(SETTING_CHOICE_LABELS[value]) : String(value);
}

function settingDisplay(item) {
    if (item.type === 'bool') return item.value ? t('admin.on') : t('admin.off');
    if (item.type === 'choice') return settingChoiceLabel(item.value);
    if (item.type === 'limits') return t('admin.limits_count', { n: Object.keys(item.value || {}).length });
    return String(item.value);
}

const SettingsView = {
    render(view) {
        const holder = h('div');
        view.appendChild(holder);
        return loadInto(holder, () => Api.get('/api/admin/settings'), (box, data) => this.paint(box, data));
    },

    paint(box, data) {
        const reload = () => Router.render();
        const column = h('div', { class: 'admin-narrow' });
        box.appendChild(column);
        column.appendChild(UI.header(t('admin.nav_settings'), t('admin.settings_note'), UI.btn(t('admin.refresh'), reload, { kind: 'secondary' })));
        for (const group of data.groups) {
            const items = data.items.filter((i) => i.group === group);
            if (!items.length) continue;
            column.appendChild(UI.subtitle(t(SETTING_GROUP_LABELS[group])));
            const card = UI.card(null);
            for (const item of items) card.appendChild(this.row(item, reload));
            column.appendChild(card);
        }
    },

    row(item, reload) {
        const [labelKey, hintKey] = SETTING_TEXT[item.key];
        let control = null;
        let edit = null;
        let input = null;
        if (item.type === 'bool') {
            // a játékos oldali beállításokkal azonos iOS-kapcsoló
            input = h('input', { type: 'checkbox', checked: !!item.value, 'aria-label': t(labelKey) });
            control = h('label', { class: 'toggle-switch' }, input, h('span', { class: 'toggle-slider' }));
            edit = () => input.checked;
        } else if (item.type === 'choice') {
            input = h('select', { 'aria-label': t(labelKey) }, item.choices.map((c) => h('option', { value: String(c) }, settingChoiceLabel(c))));
            input.value = String(item.value);
            control = input;
            edit = () => item.choices.find((c) => String(c) === input.value);
        } else if (item.type === 'int' || item.type === 'float') {
            input = h('input', { type: 'number', value: item.value, min: item.min, max: item.max, step: item.type === 'float' ? '0.1' : '1',
                'aria-label': t(labelKey) });
            control = input;
            edit = () => (input.value === '' ? null : Number(input.value));
        }
        // A „Mentés” csak módosítás után él (a változatlan érték mentése értelmetlen)
        const save = control ? UI.btn(t('admin.save'), () => {
            const value = edit();
            if (value === null || value === undefined || value === item.value) return;
            Act.open({ title: t(labelKey), text: t('admin.set_change_text', { from: settingDisplay(item), to: settingDisplay({ ...item, value }) }), danger: true,
                run: (v) => Api.patch('/api/admin/settings', { changes: { [item.key]: value }, reason: v.reason }), done: reload });
        }, { disabled: true }) : null;
        if (input) {
            const sync = () => { const value = edit(); save.disabled = value === null || value === undefined || value === item.value; };
            input.addEventListener('input', sync);
            input.addEventListener('change', sync);
        }
        const reset = item.overridden && control ? UI.btn(t('admin.reset_default'), () => Act.open({ title: t('admin.reset_default'), text: t(labelKey),
            run: (v) => Api.patch('/api/admin/settings', { changes: { [item.key]: null }, reason: v.reason }), done: reload }),
            { kind: 'link-btn', small: false }) : null;
        if (reset) reset.classList.add('admin-setting-reset');
        return h('div', { class: 'admin-setting' },
            h('div', { class: 'admin-setting-text' }, h('strong', null, t(labelKey)), h('div', { class: 'form-hint form-hint-tight' }, t(hintKey)),
                h('div', { class: 'admin-setting-meta' },
                    t('admin.set_default', { value: item.type === 'limits' ? '–' : settingDisplay({ ...item, value: item.default }) }),
                    item.overridden ? [' · ', UI.badge(t('admin.overridden'), 'warn')] : null,
                    item.changed_at ? [' · ', t('admin.set_changed', { who: item.changed_by_name || '?', when: formatStamp(item.changed_at) })] : null,
                    reset)),
            h('div', { class: 'admin-setting-control' },
                control || h('span', { class: 'admin-setting-note' }, settingDisplay(item), ' ', UI.link(t('admin.set_open_limits'), Router.href('security', '', { tab: 'limits' }))),
                save));
    },
};

registerSection({ id: 'settings', order: 140, icon: 'sliders', labelKey: 'admin.nav_settings',
    render: (view) => SettingsView.render(view) });

// ===== Admin napló =====

const DetailDialog = {
    show(item) {
        const body = h('div');
        body.appendChild(UI.kv([
            [t('admin.col_time'), formatStamp(item.created_at)], [t('admin.col_admin'), (item.admin_name || '?') + ' (#' + item.admin_user_id + ')'],
            [t('admin.col_action'), item.action],
            [t('admin.col_target'), item.target_type ? item.target_type + (item.target_id ? ' · ' + item.target_id : '') : ''],
            [t('admin.col_ip'), item.ip || ''], [t('admin.d_user_agent'), item.user_agent || '', { wide: true }], [t('admin.col_reason'), item.reason || ''],
        ]));
        const details = item.details;
        if (details && details.before && details.after && typeof details.before === 'object' && typeof details.after === 'object') {
            body.appendChild(this.renderChanges(details.before, details.after));
        }
        if (details) {
            body.appendChild(UI.subtitle(t('admin.d_details')));
            body.appendChild(h('pre', { class: 'admin-json' }, JSON.stringify(details, null, 2)));
        }
        Modal.show(t('admin.detail_title', { id: item.id }), body);
    },

    // Előtte / utána táblázat: csak a ténylegesen megváltozott mezők
    renderChanges(before, after) {
        const keys = Array.from(new Set([...Object.keys(before), ...Object.keys(after)]));
        const rows = keys.filter((key) => JSON.stringify(before[key]) !== JSON.stringify(after[key]));
        return h('div', { class: 'admin-changes' }, UI.subtitle(t('admin.d_changes')), UI.table({ compact: true, items: rows, columns: [
            { label: t('admin.d_field'), cell: (key) => key },
            { label: t('admin.d_before'), cls: 'admin-diff-old', cell: (key) => before[key] === undefined ? '' : JSON.stringify(before[key]) },
            { label: t('admin.d_after'), cls: 'admin-diff-new', cell: (key) => after[key] === undefined ? '' : JSON.stringify(after[key]) }] }));
    },
};

const AuditView = {
    render(view, ctx) {
        mountList(view, ctx.params, {
            section: 'audit', title: t('admin.audit_title'), note: t('admin.audit_note'), path: '/api/admin/audit',
            filters: [
                { name: 'admin', label: t('admin.f_admin') }, { name: 'action', label: t('admin.f_action'), placeholder: t('admin.f_action_ph') },
                { name: 'target_type', label: t('admin.f_target_type') }, { name: 'target_id', label: t('admin.f_target_id') },
                { name: 'since', label: t('admin.f_since'), type: 'date' }, { name: 'until', label: t('admin.f_until'), type: 'date' },
                { name: 'q', label: t('admin.f_q') },
            ],
            toolbar: (data) => UI.download(t('admin.export_json'), '/api/admin/audit', { download: 1 }),
            columns: [
                { label: t('admin.col_time'), cell: (e) => formatStamp(e.created_at) },
                { label: t('admin.col_admin'), cell: (e) => (e.admin_name || '?') + ' (#' + e.admin_user_id + ')' },
                { label: t('admin.col_action'), cls: 'admin-action', cell: (e) => e.action },
                { label: t('admin.col_target'), cell: (e) => e.target_type ? e.target_type + (e.target_id ? ' · ' + e.target_id : '') : '' },
                { label: t('admin.col_ip'), cell: (e) => e.ip || '' }, { label: t('admin.col_reason'), cell: (e) => e.reason || '' },
            ],
            empty: t('admin.audit_empty'), onRow: (e) => DetailDialog.show(e),
        });
    },
};

registerSection({ id: 'audit', order: 150, icon: 'list', labelKey: 'admin.nav_audit',
    render: (view, ctx) => AuditView.render(view, ctx) });
