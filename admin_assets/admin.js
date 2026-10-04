'use strict';
// ===== ADMIN PANEL (kliens) =====
// Csak az adminnak kiszolgált fájl (`/admin/assets/`), a nyilvános `static/` mappában nincs. A szerver minden
// jogosultságot maga ellenőriz; ez a kód csak a felületet adja. Szabályok: kizárólag DOM API (szövegből nem épül HTML),
// minden szöveg `t()` / `data-i18n` (admin-i18n.js), a közös stíluskészlet tokenjei.

// ----- Fordítások: az admin szövegek a közös fordítóba töltődnek (csak ezen az oldalon) -----
(function mergeAdminTranslations() {
    const extra = window.ADMIN_I18N || {};
    for (const lang of I18N.SUPPORTED) {
        const source = extra[lang] || {};
        const target = I18N.data[lang] || (I18N.data[lang] = {});
        for (const [key, value] of Object.entries(source)) {
            if (key !== 'server') target[key] = value;
        }
    }
    const server = (extra.en || {}).server;
    if (server) {
        const en = I18N.data.en;
        if (!en.server) en.server = { exact: {}, patterns: [] };
        Object.assign(en.server.exact, server.exact);
    }
    I18N.apply();
})();

// ----- Közös segédek -----

const THEME_COLORS = { light: '#f5f5f7', dark: '#000000' };

function applyTheme(theme) {
    document.documentElement.setAttribute('data-theme', theme);
    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute('content', THEME_COLORS[theme]);
}

function toggleTheme() {
    const next = document.documentElement.getAttribute('data-theme') === 'dark' ? 'light' : 'dark';
    applyTheme(next);
    try { localStorage.setItem('scrabble-theme', next); } catch { /* localStorage tiltva */ }
}

function makeEl(tag, className, text) {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined && text !== null) node.textContent = text;
    return node;
}

function makeIcon(name) {
    const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
    svg.setAttribute('class', 'icon');
    const use = document.createElementNS('http://www.w3.org/2000/svg', 'use');
    use.setAttribute('href', '#i-' + name);
    svg.appendChild(use);
    return svg;
}

function showToast(message, isError = false, duration = 3500) {
    const container = document.getElementById('toast-container');
    const toast = makeEl('div', 'toast' + (isError ? ' toast-error' : ''), message);
    container.appendChild(toast);
    const dismiss = () => {
        toast.classList.add('toast-out');
        toast.addEventListener('animationend', () => toast.remove());
    };
    setTimeout(dismiss, duration);
    toast.addEventListener('click', dismiss);
}

// A szerver UTC időbélyegei ("ÉÉÉÉ-HH-NN ÓÓ:PP:MM") nem tartalmaznak időzónát: kézzel jelöljük UTC-nek
function formatStamp(value) {
    if (!value) return '';
    const d = new Date(/(Z|[+-]\d{2}:?\d{2})$/.test(value) ? value : value.replace(' ', 'T') + 'Z');
    return isNaN(d.getTime()) ? value : d.toLocaleString(I18N.locale());
}

function formatCountdown(seconds) {
    const total = Math.max(0, Math.round(seconds));
    const minutes = Math.floor(total / 60);
    return minutes + ':' + String(total % 60).padStart(2, '0');
}

// ----- Párbeszédek -----

const Dialogs = {
    open(id) {
        const dialog = document.getElementById(id);
        dialog.classList.remove('hidden');
        const field = dialog.querySelector('input');
        if (field) { field.value = ''; field.focus(); }
    },
    close(id) {
        document.getElementById(id).classList.add('hidden');
    },
    isOpen(id) {
        return !document.getElementById(id).classList.contains('hidden');
    },
};

function showFormError(id, message) {
    const node = document.getElementById(id);
    node.textContent = message;
    node.classList.remove('hidden');
}

function hideFormError(id) {
    document.getElementById(id).classList.add('hidden');
}

// ----- API -----

const Api = {
    // Nyers kérés: {status, data}. Hálózati hibánál status 0.
    async request(method, path, body) {
        const options = { method, credentials: 'same-origin', headers: { 'X-Admin-Request': '1' } };
        if (body !== undefined) {
            options.headers['Content-Type'] = 'application/json';
            options.body = JSON.stringify(body);
        }
        try {
            const res = await fetch(path, options);
            let data = null;
            try { data = await res.json(); } catch { /* nem JSON válasz */ }
            return { status: res.status, ok: res.ok, data: data || {} };
        } catch {
            return { status: 0, ok: false, data: {} };
        }
    },

    // Kérés a munkamenet kezelésével: tétlenség után jelszót kér, sudo hiányában megerősítést, majd megismétli.
    async call(method, path, body) {
        let result = await this.request(method, path, body);
        if (result.status === 404) {
            // Közben megszűnt a jogosultság vagy a bejelentkezés: nincs itt semmi
            location.replace('/');
            return result;
        }
        if (result.status === 401 && result.data.reauth) {
            await Lock.open();
            result = await this.request(method, path, body);
        } else if (result.status === 401 && result.data.sudo_required) {
            if (await Sudo.prompt()) result = await this.request(method, path, body);
        }
        if (result.status !== 0 && result.status !== 401) Session.bump();
        return result;
    },
};

// ----- Munkamenet: tétlenség és sudo -----

const Session = {
    admin: null,
    idleMinutes: 30,
    sudoMinutes: 10,
    idleDeadline: 0,
    sudoDeadline: 0,

    apply(payload) {
        this.admin = payload.admin;
        const info = payload.session || {};
        this.idleMinutes = info.idle_minutes || this.idleMinutes;
        this.sudoMinutes = info.sudo_minutes || this.sudoMinutes;
        const now = Date.now();
        this.idleDeadline = now + (info.idle_remaining || 0) * 1000;
        this.sudoDeadline = now + (info.sudo_remaining || 0) * 1000;
        document.getElementById('admin-user-name').textContent = this.admin ? this.admin.display_name : '';
        this.renderSudo();
    },

    // Minden sikeres admin kérés újraindítja a tétlenségi időzítőt (a szerveren is)
    bump() {
        this.idleDeadline = Date.now() + this.idleMinutes * 60 * 1000;
    },

    sudoSeconds() {
        return Math.max(0, (this.sudoDeadline - Date.now()) / 1000);
    },

    renderSudo() {
        const chip = document.getElementById('admin-sudo-chip');
        const label = document.getElementById('admin-sudo-label');
        const seconds = this.sudoSeconds();
        chip.classList.toggle('active', seconds > 0);
        label.textContent = seconds > 0
            ? t('admin.sudo_on', { time: formatCountdown(seconds) })
            : t('admin.sudo_off');
    },

    async load() {
        const result = await Api.request('GET', '/api/admin/session');
        if (result.status === 404 || !result.data.admin) {
            showToast(t('admin.err_unavailable'), true);
            if (result.status === 404) location.replace('/');
            return false;
        }
        this.apply(result.data);
        if (result.data.session && result.data.session.idle_expired) await Lock.open();
        return true;
    },

    tick() {
        Session.renderSudo();
        if (Date.now() > Session.idleDeadline && !Lock.isOpen()) Lock.open();
    },

    async toggleSudo() {
        if (this.sudoSeconds() > 0) {
            const result = await Api.call('DELETE', '/api/admin/sudo');
            if (result.ok) {
                this.apply(result.data);
                showToast(t('admin.sudo_ended'));
            }
        } else {
            await Sudo.prompt();
        }
    },
};

// ----- Tétlenség miatti zárolás -----

const Lock = {
    _waiters: [],

    isOpen() { return Dialogs.isOpen('admin-lock'); },

    // Megnyitja a jelszókérőt; akkor teljesül, ha a felhasználó feloldotta
    open() {
        return new Promise((resolve) => {
            this._waiters.push(resolve);
            if (!this.isOpen()) {
                hideFormError('admin-lock-error');
                document.body.classList.add('admin-locked');
                Dialogs.open('admin-lock');
            }
        });
    },

    async submit(event) {
        event.preventDefault();
        const input = document.getElementById('admin-lock-password');
        const button = document.getElementById('admin-lock-submit');
        hideFormError('admin-lock-error');
        button.disabled = true;
        try {
            const result = await Api.request('POST', '/api/admin/reauth', { password: input.value });
            if (result.ok) {
                Session.apply(result.data);
                Dialogs.close('admin-lock');
                document.body.classList.remove('admin-locked');
                const waiters = this._waiters;
                this._waiters = [];
                waiters.forEach((resolve) => resolve());
            } else if (result.status === 404) {
                location.replace('/');
            } else {
                showFormError('admin-lock-error', result.status === 0
                    ? t('admin.err_network') : tServer(result.data.message));
                input.value = '';
                input.focus();
            }
        } finally {
            button.disabled = false;
        }
    },
};

// ----- Sudo: friss jelszó-megerősítés romboló műveletekhez -----

const Sudo = {
    _resolve: null,

    // Igaz, ha sikerült a megerősítés
    prompt() {
        if (this._resolve) return Promise.resolve(false);
        document.getElementById('admin-sudo-text').textContent =
            t('admin.sudo_dialog_text', { minutes: Session.sudoMinutes });
        hideFormError('admin-sudo-error');
        Dialogs.open('admin-sudo-dialog');
        return new Promise((resolve) => { this._resolve = resolve; });
    },

    finish(ok) {
        Dialogs.close('admin-sudo-dialog');
        const resolve = this._resolve;
        this._resolve = null;
        if (resolve) resolve(ok);
    },

    async submit(event) {
        event.preventDefault();
        const input = document.getElementById('admin-sudo-password');
        const button = document.getElementById('admin-sudo-submit');
        hideFormError('admin-sudo-error');
        button.disabled = true;
        try {
            const result = await Api.call('POST', '/api/admin/sudo', { password: input.value });
            if (result.ok) {
                Session.apply(result.data);
                showToast(t('admin.sudo_started'));
                this.finish(true);
            } else {
                showFormError('admin-sudo-error', result.status === 0
                    ? t('admin.err_network') : tServer(result.data.message));
                input.value = '';
                input.focus();
            }
        } finally {
            button.disabled = false;
        }
    },
};

// ----- Naplósor részletei -----

const DetailDialog = {
    show(item) {
        document.getElementById('admin-detail-title').textContent = t('admin.detail_title', { id: item.id });
        const body = document.getElementById('admin-detail-body');
        body.replaceChildren();

        const rows = [
            [t('admin.col_time'), formatStamp(item.created_at)],
            [t('admin.col_admin'), (item.admin_name || '?') + ' (#' + item.admin_user_id + ')'],
            [t('admin.col_action'), item.action],
            [t('admin.col_target'), item.target_type ? item.target_type + (item.target_id ? ' · ' + item.target_id : '') : ''],
            [t('admin.col_ip'), item.ip || ''],
            [t('admin.d_user_agent'), item.user_agent || ''],
            [t('admin.col_reason'), item.reason || ''],
        ];
        const list = makeEl('dl', 'admin-kv');
        for (const [label, value] of rows) {
            if (!value) continue;
            list.appendChild(makeEl('dt', null, label));
            list.appendChild(makeEl('dd', null, value));
        }
        body.appendChild(list);

        const details = item.details;
        if (details && typeof details.before === 'object' && typeof details.after === 'object'
                && details.before && details.after) {
            body.appendChild(this.renderChanges(details.before, details.after));
        }
        if (details) {
            body.appendChild(makeEl('h3', 'subsection-title', t('admin.d_details')));
            body.appendChild(makeEl('pre', 'admin-json', JSON.stringify(details, null, 2)));
        }
        Dialogs.open('admin-detail-dialog');
    },

    // Előtte / utána táblázat: csak a ténylegesen megváltozott mezők
    renderChanges(before, after) {
        const wrap = makeEl('div', 'admin-changes');
        wrap.appendChild(makeEl('h3', 'subsection-title', t('admin.d_changes')));
        const table = makeEl('table', 'admin-table admin-table-compact');
        const head = makeEl('tr');
        for (const key of ['admin.d_field', 'admin.d_before', 'admin.d_after']) head.appendChild(makeEl('th', null, t(key)));
        table.appendChild(makeEl('thead')).appendChild(head);
        const tbody = makeEl('tbody');
        const keys = Array.from(new Set([...Object.keys(before), ...Object.keys(after)]));
        for (const key of keys) {
            const a = JSON.stringify(before[key]);
            const b = JSON.stringify(after[key]);
            if (a === b) continue;
            const row = makeEl('tr');
            row.appendChild(makeEl('td', null, key));
            row.appendChild(makeEl('td', 'admin-diff-old', a === undefined ? '' : a));
            row.appendChild(makeEl('td', 'admin-diff-new', b === undefined ? '' : b));
            tbody.appendChild(row);
        }
        table.appendChild(tbody);
        wrap.appendChild(table);
        return wrap;
    },
};

// ----- Admin napló nézet -----

const AUDIT_PAGE_SIZE = 50;
const AUDIT_FILTERS = [
    { name: 'admin', label: 'admin.f_admin', type: 'text' },
    { name: 'action', label: 'admin.f_action', type: 'text', placeholder: 'admin.f_action_ph' },
    { name: 'target_type', label: 'admin.f_target_type', type: 'text' },
    { name: 'target_id', label: 'admin.f_target_id', type: 'text' },
    { name: 'since', label: 'admin.f_since', type: 'date' },
    { name: 'until', label: 'admin.f_until', type: 'date' },
    { name: 'q', label: 'admin.f_q', type: 'text' },
];

const AuditView = {
    // A szűrők az URL-ben vannak (#audit?action=user.&offset=50): a vissza gomb és a linkmásolás működik
    filterQuery(params) {
        const query = new URLSearchParams();
        for (const filter of AUDIT_FILTERS) {
            const value = params.get(filter.name);
            if (value) query.set(filter.name, value);
        }
        return query;
    },

    render(view, params) {
        view.replaceChildren();
        const header = makeEl('div', 'admin-view-header');
        const titles = makeEl('div');
        titles.appendChild(makeEl('h2', 'section-title', t('admin.audit_title')));
        titles.appendChild(makeEl('p', 'form-hint form-hint-tight', t('admin.audit_note')));
        header.appendChild(titles);
        const refresh = makeEl('button', 'secondary small-btn', t('admin.refresh'));
        refresh.type = 'button';
        refresh.addEventListener('click', () => Router.render());
        header.appendChild(refresh);
        view.appendChild(header);

        view.appendChild(this.renderFilters(params));
        const results = makeEl('div', 'admin-results');
        view.appendChild(results);
        return this.load(results, params);
    },

    renderFilters(params) {
        const form = makeEl('form', 'admin-filters');
        for (const filter of AUDIT_FILTERS) {
            const field = makeEl('label', 'admin-field');
            field.appendChild(makeEl('span', 'admin-field-label', t(filter.label)));
            const input = makeEl('input');
            input.type = filter.type;
            input.name = filter.name;
            input.value = params.get(filter.name) || '';
            input.autocomplete = 'off';
            if (filter.placeholder) input.placeholder = t(filter.placeholder);
            field.appendChild(input);
            form.appendChild(field);
        }
        const actions = makeEl('div', 'admin-filter-actions');
        const apply = makeEl('button', 'small-btn', t('admin.apply'));
        apply.type = 'submit';
        const reset = makeEl('button', 'secondary small-btn', t('admin.reset'));
        reset.type = 'button';
        reset.addEventListener('click', () => Router.go('audit'));
        actions.append(apply, reset);
        form.appendChild(actions);
        form.addEventListener('submit', (event) => {
            event.preventDefault();
            const query = new URLSearchParams();
            for (const filter of AUDIT_FILTERS) {
                const value = form.elements[filter.name].value.trim();
                if (value) query.set(filter.name, value);
            }
            Router.go('audit', query);
        });
        return form;
    },

    async load(container, params) {
        const filters = this.filterQuery(params);
        const offset = Math.max(0, parseInt(params.get('offset') || '0', 10) || 0);
        const query = new URLSearchParams(filters);
        query.set('limit', String(AUDIT_PAGE_SIZE));
        query.set('offset', String(offset));

        container.replaceChildren(makeEl('p', 'text-muted', '…'));
        const result = await Api.call('GET', '/api/admin/audit?' + query.toString());
        if (!result.ok || !result.data.items) {
            container.replaceChildren(makeEl('p', 'auth-error',
                result.status === 0 ? t('admin.err_network') : (tServer(result.data.message) || t('admin.err_load'))));
            return;
        }
        this.renderResults(container, result.data, filters, offset);
    },

    renderResults(container, data, filters, offset) {
        container.replaceChildren();
        container.appendChild(this.renderToolbar(data, filters, offset));
        if (!data.items.length) {
            const empty = makeEl('div', 'empty-state');
            empty.appendChild(makeEl('p', 'empty-msg', t('admin.audit_empty')));
            container.appendChild(empty);
            return;
        }

        const wrap = makeEl('div', 'admin-table-wrap');
        const table = makeEl('table', 'admin-table admin-table-clickable');
        const columns = [
            ['admin.col_time', 'created_at'], ['admin.col_admin', 'admin'], ['admin.col_action', 'action'],
            ['admin.col_target', 'target'], ['admin.col_ip', 'ip'], ['admin.col_reason', 'reason'],
        ];
        const head = makeEl('tr');
        for (const [key] of columns) head.appendChild(makeEl('th', null, t(key)));
        table.appendChild(makeEl('thead')).appendChild(head);

        const tbody = makeEl('tbody');
        for (const item of data.items) {
            const row = makeEl('tr');
            row.tabIndex = 0;
            const target = item.target_type ? item.target_type + (item.target_id ? ' · ' + item.target_id : '') : '';
            const cells = {
                created_at: formatStamp(item.created_at),
                admin: (item.admin_name || '?') + ' (#' + item.admin_user_id + ')',
                action: item.action,
                target,
                ip: item.ip || '',
                reason: item.reason || '',
            };
            for (const [key, field] of columns) {
                const cell = makeEl('td', field === 'action' ? 'admin-action' : null, cells[field]);
                cell.dataset.label = t(key);
                row.appendChild(cell);
            }
            const open = () => DetailDialog.show(item);
            row.addEventListener('click', open);
            row.addEventListener('keydown', (event) => {
                if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); open(); }
            });
            tbody.appendChild(row);
        }
        table.appendChild(tbody);
        wrap.appendChild(table);
        container.appendChild(wrap);
        container.appendChild(this.renderPager(data, filters, offset));
    },

    renderToolbar(data, filters, offset) {
        const bar = makeEl('div', 'admin-toolbar');
        bar.appendChild(makeEl('span', 'text-secondary text-sm',
            data.total ? t('admin.audit_range', {
                from: offset + 1, to: Math.min(offset + data.items.length, data.total), total: data.total,
            }) : ''));
        const exports = makeEl('div', 'admin-toolbar-actions');
        const csv = new URLSearchParams(filters);
        csv.set('format', 'csv');
        const json = new URLSearchParams(filters);
        json.set('download', '1');
        for (const [label, query] of [['admin.export_csv', csv], ['admin.export_json', json]]) {
            const link = makeEl('a', 'admin-link-btn', t(label));
            link.href = '/api/admin/audit?' + query.toString();
            link.setAttribute('download', '');
            exports.appendChild(link);
        }
        bar.appendChild(exports);
        return bar;
    },

    renderPager(data, filters, offset) {
        const pager = makeEl('div', 'admin-pager');
        const go = (newOffset) => {
            const query = new URLSearchParams(filters);
            if (newOffset > 0) query.set('offset', String(newOffset));
            Router.go('audit', query);
        };
        const prev = makeEl('button', 'secondary small-btn', t('admin.prev'));
        prev.type = 'button';
        prev.disabled = offset <= 0;
        prev.addEventListener('click', () => go(Math.max(0, offset - AUDIT_PAGE_SIZE)));
        const next = makeEl('button', 'secondary small-btn', t('admin.next'));
        next.type = 'button';
        next.disabled = offset + data.items.length >= data.total;
        next.addEventListener('click', () => go(offset + AUDIT_PAGE_SIZE));
        pager.append(prev, next);
        return pager;
    },
};

// ----- Navigáció (a további menüpontok ide kerülnek a `SECTIONS` listába) -----

const SECTIONS = [
    { id: 'audit', labelKey: 'admin.nav_audit', icon: 'list', render: (view, params) => AuditView.render(view, params) },
];

const Router = {
    parse() {
        const raw = location.hash.replace(/^#/, '');
        const [path, query = ''] = raw.split('?');
        const section = SECTIONS.find((s) => s.id === path) || SECTIONS[0];
        return { section, params: new URLSearchParams(query) };
    },

    go(sectionId, query) {
        const qs = query ? query.toString() : '';
        const target = sectionId + (qs ? '?' + qs : '');
        // Ugyanarra a hash-re lépve nincs hashchange: ilyenkor kézzel frissítünk
        if (location.hash.replace(/^#/, '') === target) this.render();
        else location.hash = target;
    },

    render() {
        const { section, params } = this.parse();
        document.querySelectorAll('#admin-nav .admin-nav-item').forEach((item) => {
            const active = item.dataset.section === section.id;
            item.classList.toggle('active', active);
            if (active) item.setAttribute('aria-current', 'page'); else item.removeAttribute('aria-current');
        });
        document.body.classList.remove('admin-nav-open');
        const view = document.getElementById('admin-main');
        return section.render(view, params);
    },

    buildNav() {
        const nav = document.getElementById('admin-nav');
        nav.replaceChildren();
        for (const section of SECTIONS) {
            const item = makeEl('button', 'admin-nav-item');
            item.type = 'button';
            item.dataset.section = section.id;
            item.appendChild(makeIcon(section.icon));
            item.appendChild(makeEl('span', null, t(section.labelKey)));
            item.addEventListener('click', () => this.go(section.id));
            nav.appendChild(item);
        }
    },
};

// ----- Indítás -----

function bindEvents() {
    document.addEventListener('click', (event) => {
        if (event.target.closest('.btn-theme-toggle')) toggleTheme();
        else if (event.target.closest('.btn-lang-toggle')) I18N.setLang(I18N.next());
    });
    document.getElementById('admin-menu-btn').addEventListener('click', () => {
        document.body.classList.toggle('admin-nav-open');
    });
    document.getElementById('admin-nav-backdrop').addEventListener('click', () => {
        document.body.classList.remove('admin-nav-open');
    });
    document.getElementById('admin-sudo-chip').addEventListener('click', () => Session.toggleSudo());

    document.getElementById('admin-lock-form').addEventListener('submit', (event) => Lock.submit(event));
    document.getElementById('admin-sudo-form').addEventListener('submit', (event) => Sudo.submit(event));
    document.getElementById('admin-sudo-cancel').addEventListener('click', () => Sudo.finish(false));
    document.getElementById('admin-detail-close').addEventListener('click', () => Dialogs.close('admin-detail-dialog'));

    // Háttérre koppintás és Esc zárja a párbeszédeket (a zárolást nem: az csak jelszóval oldható fel)
    for (const id of ['admin-sudo-dialog', 'admin-detail-dialog']) {
        document.getElementById(id).addEventListener('click', (event) => {
            if (event.target.id !== id) return;
            if (id === 'admin-sudo-dialog') Sudo.finish(false); else Dialogs.close(id);
        });
    }
    document.addEventListener('keydown', (event) => {
        if (event.key !== 'Escape') return;
        if (Dialogs.isOpen('admin-sudo-dialog')) Sudo.finish(false);
        else if (Dialogs.isOpen('admin-detail-dialog')) Dialogs.close('admin-detail-dialog');
        else document.body.classList.remove('admin-nav-open');
    });

    window.addEventListener('hashchange', () => Router.render());
    window.addEventListener('langchange', () => {
        Router.buildNav();
        Session.renderSudo();
        Router.render();
    });
}

async function initAdmin() {
    applyTheme(document.documentElement.getAttribute('data-theme') || 'light');
    bindEvents();
    Router.buildNav();
    if (!(await Session.load())) return;
    await Router.render();
    setInterval(() => Session.tick(), 1000);
}

initAdmin();
