'use strict';
// ===== ADMIN PANEL (kliens) — mag =====
// Csak az adminnak kiszolgált fájl (`/admin/assets/`), a nyilvános `static/` mappában nincs. A szerver minden
// jogosultságot maga ellenőriz; ez a kód csak a felületet adja. Szabályok: kizárólag DOM API (szövegből nem épül HTML),
// minden szöveg `t()` / `data-i18n` (admin-i18n.js; a kulcsok szó szerint szerepelnek a kódban, nincs összefűzés),
// a közös stíluskészlet tokenjei. A hálózati hívás egyetlen helyen van (`Api.request`).
// Fájlok: admin.js (mag) · admin-ui.js (komponensek, űrlap-párbeszéd) · admin-views-*.js (menüpontok) · admin-main.js
// (útvonalválasztó, élő kapcsolat, kereső, indítás).

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

// Rövid elemépítő: h('div', {class: 'x', onclick: fn, dataset: {a: 1}}, 'szöveg', gyerekElem, [lista]).
// A szöveg mindig szövegcsomópont (soha nem HTML); a null / false / undefined gyerek kimarad.
function h(tag, attrs, ...children) {
    const node = document.createElement(tag);
    if (attrs) {
        for (const [key, value] of Object.entries(attrs)) {
            if (value === null || value === undefined || value === false) continue;
            if (key === 'class') node.className = value;
            else if (key === 'text') node.textContent = value;
            else if (key === 'dataset') Object.assign(node.dataset, value);
            else if (key.startsWith('on') && typeof value === 'function') node.addEventListener(key.slice(2), value);
            else if (key === 'value' || key === 'checked' || key === 'disabled' || key === 'selected'
                    || key === 'hidden' || key === 'tabIndex') node[key] = value;
            else node.setAttribute(key, value === true ? '' : String(value));
        }
    }
    appendChildren(node, children);
    return node;
}

function appendChildren(node, children) {
    for (const child of children) {
        if (child === null || child === undefined || child === false) continue;
        if (Array.isArray(child)) appendChildren(node, child);
        else if (child instanceof Node) node.appendChild(child);
        else node.appendChild(document.createTextNode(String(child)));
    }
}

const SVG_NS = 'http://www.w3.org/2000/svg';

function svgEl(tag, attrs, ...children) {
    const node = document.createElementNS(SVG_NS, tag);
    if (attrs) for (const [key, value] of Object.entries(attrs)) node.setAttribute(key, String(value));
    appendChildren(node, children);
    return node;
}

function makeIcon(name) {
    const svg = svgEl('svg', { class: 'icon' });
    svg.appendChild(svgEl('use', { href: '#i-' + name }));
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

function formatDay(value) {
    if (!value) return '';
    const d = new Date(String(value).slice(0, 10) + 'T12:00:00Z');
    return isNaN(d.getTime()) ? value : d.toLocaleDateString(I18N.locale(), { month: 'short', day: 'numeric' });
}

// Unix másodperc (a szerver `time.time()` értékei) → helyi időpont
function formatEpoch(seconds) {
    if (!seconds) return '';
    return new Date(seconds * 1000).toLocaleString(I18N.locale());
}

function formatCountdown(seconds) {
    const total = Math.max(0, Math.round(seconds));
    const minutes = Math.floor(total / 60);
    return minutes + ':' + String(total % 60).padStart(2, '0');
}

function fmtNum(value, digits = 0) {
    if (value === null || value === undefined || value === '') return '–';
    const n = Number(value);
    if (!isFinite(n)) return String(value);
    return n.toLocaleString(I18N.locale(), { maximumFractionDigits: digits });
}

function fmtPercent(value, digits = 1) {
    if (value === null || value === undefined) return '–';
    return fmtNum(value, digits) + '%';
}

function fmtBytes(bytes) {
    if (bytes === null || bytes === undefined) return '–';
    const units = ['B', 'KB', 'MB', 'GB'];
    let value = Number(bytes);
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) { value /= 1024; unit += 1; }
    return fmtNum(value, unit ? 1 : 0) + ' ' + units[unit];
}

// Időtartam (másodperc) röviden: "3 n 4 ó", "5 p", "42 mp"
function fmtDuration(seconds) {
    if (seconds === null || seconds === undefined) return '–';
    const s = Math.max(0, Math.round(seconds));
    if (s >= 86400) return t('admin.dur_dh', { d: Math.floor(s / 86400), h: Math.floor((s % 86400) / 3600) });
    if (s >= 3600) return t('admin.dur_hm', { h: Math.floor(s / 3600), m: Math.floor((s % 3600) / 60) });
    if (s >= 60) return t('admin.dur_m', { m: Math.floor(s / 60) });
    return t('admin.dur_s', { s });
}

// Egy UTC időbélyeg óta eltelt idő
function fmtSince(stamp) {
    if (!stamp) return '';
    const d = new Date(/(Z|[+-]\d{2}:?\d{2})$/.test(stamp) ? stamp : stamp.replace(' ', 'T') + 'Z');
    if (isNaN(d.getTime())) return '';
    return t('admin.ago', { time: fmtDuration((Date.now() - d.getTime()) / 1000) });
}

// ----- Párbeszédek -----

const Dialogs = {
    open(id, focusField = true) {
        const dialog = document.getElementById(id);
        dialog.classList.remove('hidden');
        if (focusField) {
            const field = dialog.querySelector('input, textarea, select');
            if (field) { field.value = ''; field.focus(); }
        }
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
    // Nyers kérés: {status, data}. Hálózati hibánál status 0. A CSRF-fejléc minden kérésen ott van.
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

    // GET lekérdezéssel: {kulcs: érték}; az üres / null értékek kimaradnak
    get(path, query) {
        return this.call('GET', path + queryString(query));
    },

    post(path, body) { return this.call('POST', path, body === undefined ? {} : body); },
    patch(path, body) { return this.call('PATCH', path, body === undefined ? {} : body); },
    del(path, body) { return this.call('DELETE', path, body === undefined ? {} : body); },

    // A felhasználónak szánt hibaszöveg egy sikertelen válaszból
    message(result) {
        if (result.status === 0) return t('admin.err_network');
        return tServer(result.data.message) || t('admin.err_load');
    },
};

function queryString(query) {
    if (!query) return '';
    const params = query instanceof URLSearchParams ? query : new URLSearchParams();
    if (!(query instanceof URLSearchParams)) {
        for (const [key, value] of Object.entries(query)) {
            if (value === null || value === undefined || value === '' || value === false) continue;
            params.set(key, value === true ? '1' : String(value));
        }
    }
    const text = params.toString();
    return text ? '?' + text : '';
}

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
                showFormError('admin-lock-error', Api.message(result));
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
                showFormError('admin-sudo-error', Api.message(result));
                input.value = '';
                input.focus();
            }
        } finally {
            button.disabled = false;
        }
    },
};

// ----- Menüpontok: a nézetfájlok itt regisztrálják magukat (sorrend: `order`) -----

const SECTIONS = [];

function registerSection(section) {
    SECTIONS.push(section);
    SECTIONS.sort((a, b) => a.order - b.order);
}
