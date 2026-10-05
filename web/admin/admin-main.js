'use strict';
// ===== ADMIN PANEL — útvonalválasztó, élő kapcsolat, globális kereső, indítás =====

// ----- Navigáció: `#menüpont/azonosító?szűrők` (a vissza gomb és a linkmásolás működik) -----

const Router = {
    _leave: [],

    parse() {
        const raw = location.hash.replace(/^#/, '');
        const queryAt = raw.indexOf('?');
        const path = queryAt < 0 ? raw : raw.slice(0, queryAt);
        const query = queryAt < 0 ? '' : raw.slice(queryAt + 1);
        const slash = path.indexOf('/');
        const id = slash < 0 ? path : path.slice(0, slash);
        let arg = slash < 0 ? '' : path.slice(slash + 1);
        try { arg = decodeURIComponent(arg); } catch { /* hibás kódolás: nyersen marad */ }
        const section = SECTIONS.find((s) => s.id === id) || SECTIONS[0];
        return { section, arg, params: new URLSearchParams(query) };
    },

    href(sectionId, arg, query) {
        const qs = query ? (query instanceof URLSearchParams ? query.toString() : new URLSearchParams(query).toString()) : '';
        const hasArg = arg !== undefined && arg !== null && arg !== '';
        return '#' + sectionId + (hasArg ? '/' + encodeURIComponent(arg) : '') + (qs ? '?' + qs : '');
    },

    go(sectionId, arg, query) {
        const target = this.href(sectionId, arg, query);
        // Ugyanarra a hash-re lépve nincs hashchange: ilyenkor kézzel frissítünk
        if (location.hash === target) this.render();
        else location.hash = target;
    },

    // A nézetből való kilépéskor lefutó takarítás (időzítők, élő feliratkozások)
    onLeave(fn) { this._leave.push(fn); },

    // Időzítő, amely a nézet elhagyásakor leáll
    interval(fn, ms) {
        const id = setInterval(fn, ms);
        this.onLeave(() => clearInterval(id));
        return id;
    },

    async render() {
        for (const fn of this._leave.splice(0)) {
            try { fn(); } catch { /* a takarítás hibája nem akadályozhatja a navigációt */ }
        }
        const { section, arg, params } = this.parse();
        document.querySelectorAll('#admin-nav .admin-nav-item').forEach((item) => {
            const active = item.dataset.section === section.id;
            item.classList.toggle('active', active);
            if (active) item.setAttribute('aria-current', 'page'); else item.removeAttribute('aria-current');
        });
        document.body.classList.remove('admin-nav-open');
        Nav.revealActive();
        document.title = t(section.labelKey) + ' — ' + t('admin.title');
        const view = h('div', { class: 'admin-view' });
        const main = document.getElementById('admin-main');
        main.replaceChildren(view);
        window.scrollTo(0, 0);
        try {
            await section.render(view, { arg, params });
        } catch (error) {
            console.error(error);
            view.replaceChildren(UI.errorBox(t('admin.err_view')));
        }
    },

    buildNav() {
        const nav = document.getElementById('admin-nav');
        nav.replaceChildren();
        for (const section of SECTIONS) {
            const item = h('button', { type: 'button', class: 'admin-nav-item', dataset: { section: section.id } },
                makeIcon(section.icon), h('span', null, t(section.labelKey)),
                h('span', { class: 'admin-nav-badge hidden', dataset: { badge: section.id } }));
            item.addEventListener('click', () => this.go(section.id));
            nav.appendChild(item);
        }
        Nav.refreshBadges();
    },
};

// Menü-jelvények (pl. új bejelentések száma) és a karbantartási mód sávja
const Nav = {
    badges: {},

    set(sectionId, count) {
        this.badges[sectionId] = count;
        this.refreshBadges();
    },

    // Alacsony ablakban / telefonon az oldalmenü görgethető: a kijelölt menüpont ne maradjon a képen kívül
    revealActive() {
        const nav = document.getElementById('admin-nav');
        revealInScroller(nav, nav && nav.querySelector('.admin-nav-item.active'), { axis: 'y' });
    },

    refreshBadges() {
        document.querySelectorAll('#admin-nav [data-badge]').forEach((node) => {
            const count = this.badges[node.dataset.badge] || 0;
            node.textContent = count > 99 ? '99+' : String(count);
            node.classList.toggle('hidden', !count);
        });
    },

    // A bejelentések (moderáció) számának lekérése
    async loadReports() {
        const result = await Api.get('/api/admin/reports', { status: 'new', limit: 1 });
        if (result.ok) this.set('moderation', result.data.new || 0);
    },

    async loadMaintenance() {
        const result = await Api.get('/api/admin/maintenance');
        if (result.ok) this.showMaintenance(result.data.active ? result.data : null);
    },

    showMaintenance(info) {
        const banner = document.getElementById('admin-banner');
        if (!info) { banner.classList.add('hidden'); banner.replaceChildren(); return; }
        banner.replaceChildren(makeIcon('lock'), h('span', null, t('admin.banner_maintenance')),
            UI.link(t('admin.banner_open'), Router.href('comm', '', { tab: 'maintenance' })));
        banner.classList.remove('hidden');
    },
};

// ----- Élő kapcsolat: Socket.IO `admin` szoba; ha nem elérhető, a nézetek időzítővel frissítenek -----

const Live = {
    socket: null,
    connected: false,
    _handlers: {},
    _watched: null,

    // Feliratkozás egy eseményre; visszatér: a leiratkozó függvény (a nézet elhagyásakor automatikusan lefut)
    on(event, fn) {
        (this._handlers[event] = this._handlers[event] || new Set()).add(fn);
        const off = () => this._handlers[event] && this._handlers[event].delete(fn);
        Router.onLeave(off);
        return off;
    },

    _emit(event, payload) {
        for (const fn of Array.from(this._handlers[event] || [])) {
            try { fn(payload); } catch (error) { console.error(error); }
        }
    },

    async start() {
        if (typeof io === 'undefined') return;   // a CDN nem érhető el: időzítős frissítés
        const tokenResult = await Api.request('GET', '/api/auth/socket-token');
        if (!tokenResult.ok || !tokenResult.data.token) return;
        const socket = io({ transports: ['websocket', 'polling'], reconnection: true });
        this.socket = socket;
        socket.on('connect', async () => {
            // Az aláírt token rövid életű: újracsatlakozáskor friss kell
            const fresh = await Api.request('GET', '/api/auth/socket-token');
            socket.emit('admin_subscribe', { auth_token: fresh.ok ? fresh.data.token : tokenResult.data.token });
        });
        socket.on('admin_subscribed', () => {
            this.connected = true;
            if (this._watched) socket.emit('admin_watch_room', { room_id: this._watched });
            this._emit('connection', true);
        });
        socket.on('disconnect', () => { this.connected = false; this._emit('connection', false); });
        for (const event of ['admin_overview', 'admin_room_update', 'admin_room_state']) {
            socket.on(event, (payload) => this._emit(event, payload));
        }
        socket.on('admin_alert', (payload) => {
            showToast(t('admin.alert_banned_word', { name: payload.name || '?', room: payload.room || '?' }), true, 6000);
            this._emit('admin_alert', payload);
        });
        socket.on('admin_report', (payload) => {
            showToast(t('admin.alert_report', { name: payload.reported || '?', room: payload.room || '?' }), true, 8000);
            Nav.set('moderation', (Nav.badges.moderation || 0) + 1);
            this._emit('admin_report', payload);
        });
    },

    // Egy szoba figyelése (egyszerre egy); a nézet elhagyásakor a figyelés megszűnik
    watchRoom(roomId) {
        this._watched = roomId;
        if (this.socket && this.connected) this.socket.emit('admin_watch_room', { room_id: roomId });
        Router.onLeave(() => { this._watched = null; });
    },
};

// ----- Globális kereső (Ctrl+K): felhasználó, szoba, játék, szó -----

const Search = {
    _timer: null,
    _seq: 0,
    _items: [],
    _active: -1,

    open() {
        const input = document.getElementById('admin-search-input');
        document.getElementById('admin-search-results').replaceChildren();
        this._items = [];
        this._active = -1;
        Dialogs.open('admin-search-dialog', false);
        input.value = '';
        input.focus();
    },

    close() { Dialogs.close('admin-search-dialog'); },

    onInput() {
        clearTimeout(this._timer);
        this._timer = setTimeout(() => this.run(), 220);
    },

    async run() {
        const query = document.getElementById('admin-search-input').value.trim();
        const box = document.getElementById('admin-search-results');
        if (!query) { box.replaceChildren(); this._items = []; return; }
        const seq = ++this._seq;
        const result = await Api.get('/api/admin/search', { q: query });
        if (seq !== this._seq) return;
        if (!result.ok) { box.replaceChildren(UI.errorBox(Api.message(result))); return; }
        const data = result.data;
        const groups = [
            ['admin.search_users', data.users.map((u) => ({ label: u.display_name + (u.deleted ? ' †' : ''),
                sub: u.email + ' · #' + u.id, hash: Router.href('users', u.id) }))],
            ['admin.search_rooms', data.rooms.map((r) => ({ label: r.name, sub: r.code, hash: Router.href('rooms', r.id) }))],
            ['admin.search_games', data.games.map((g) => ({ label: g.name, sub: '#' + g.id + ' · ' + g.status,
                hash: Router.href('games', g.id) }))],
            ['admin.search_words', data.words.map((w) => ({ label: w.word, sub: '', hash: Router.href('dictionary', '', { w: w.word }) }))],
        ];
        box.replaceChildren();
        this._items = [];
        for (const [key, entries] of groups) {
            if (!entries.length) continue;
            box.appendChild(h('div', { class: 'admin-search-group' }, t(key)));
            for (const entry of entries) {
                const row = h('a', { class: 'admin-search-row', href: entry.hash },
                    h('span', { class: 'admin-search-main' }, entry.label), entry.sub ? h('span', { class: 'admin-search-sub' }, entry.sub) : null);
                row.addEventListener('click', () => this.close());
                box.appendChild(row);
                this._items.push(row);
            }
        }
        if (!this._items.length) box.appendChild(UI.empty(t('admin.search_none')));
        this._active = -1;
    },

    // Nyilak és Enter a találatok között
    onKey(event) {
        if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
            event.preventDefault();
            if (!this._items.length) return;
            this._active = (this._active + (event.key === 'ArrowDown' ? 1 : -1) + this._items.length) % this._items.length;
            this._items.forEach((row, i) => row.classList.toggle('active', i === this._active));
            this._items[this._active].scrollIntoView({ block: 'nearest' });
        } else if (event.key === 'Enter') {
            event.preventDefault();
            const row = this._items[Math.max(0, this._active)];
            if (row) { location.hash = row.getAttribute('href'); this.close(); }
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
        Nav.revealActive();
    });
    document.getElementById('admin-nav-backdrop').addEventListener('click', () => {
        document.body.classList.remove('admin-nav-open');
    });
    document.getElementById('admin-sudo-chip').addEventListener('click', () => Session.toggleSudo());
    document.getElementById('admin-search-btn').addEventListener('click', () => Search.open());
    document.getElementById('admin-kbd').textContent = /Mac|iPhone|iPad/.test(navigator.platform || '') ? '⌘ K' : 'Ctrl K';

    document.getElementById('admin-lock-form').addEventListener('submit', (event) => Lock.submit(event));
    document.getElementById('admin-sudo-form').addEventListener('submit', (event) => Sudo.submit(event));
    document.getElementById('admin-sudo-cancel').addEventListener('click', () => Sudo.finish(false));
    document.getElementById('admin-form').addEventListener('submit', (event) => Act.submit(event));
    document.getElementById('admin-form-cancel').addEventListener('click', () => Act.close());
    document.getElementById('admin-detail-close').addEventListener('click', () => Modal.close());
    document.getElementById('admin-search-close').addEventListener('click', () => Search.close());
    const searchInput = document.getElementById('admin-search-input');
    searchInput.addEventListener('input', () => Search.onInput());
    searchInput.addEventListener('keydown', (event) => Search.onKey(event));

    // Háttérre koppintás és Esc zárja a párbeszédeket (a zárolást nem: az csak jelszóval oldható fel)
    const closers = {
        'admin-sudo-dialog': () => Sudo.finish(false),
        'admin-form-dialog': () => Act.close(),
        'admin-detail-dialog': () => Modal.close(),
        'admin-search-dialog': () => Search.close(),
    };
    for (const [id, close] of Object.entries(closers)) {
        document.getElementById(id).addEventListener('click', (event) => {
            if (event.target.id === id) close();
        });
    }
    document.addEventListener('keydown', (event) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'k') {
            event.preventDefault();
            if (!Lock.isOpen()) Search.open();
            return;
        }
        if (event.key !== 'Escape') return;
        for (const id of ['admin-sudo-dialog', 'admin-search-dialog', 'admin-form-dialog', 'admin-detail-dialog']) {
            if (Dialogs.isOpen(id)) { closers[id](); return; }
        }
        document.body.classList.remove('admin-nav-open');
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
    Nav.loadReports();
    Nav.loadMaintenance();
    Live.start();
}

initAdmin();
