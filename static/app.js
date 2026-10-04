// ===== THEME SYSTEM =====
// A kezdeti témát a <head> inline szkriptje már beállította (nincs villanás);
// itt csak a váltást és a böngésző témaszínét kezeljük.

const THEME_COLORS = { light: '#f5f5f7', dark: '#000000' };

function applyTheme(theme) {
    document.documentElement.setAttribute('data-theme', theme);
    const meta = document.querySelector('meta[name="theme-color"]');
    if (meta) meta.setAttribute('content', THEME_COLORS[theme]);
}

function toggleTheme() {
    const current = document.documentElement.getAttribute('data-theme');
    const next = current === 'dark' ? 'light' : 'dark';
    applyTheme(next);
    try { localStorage.setItem('scrabble-theme', next); } catch { /* localStorage tiltva */ }
}

applyTheme(document.documentElement.getAttribute('data-theme') || 'light');

// Közös felső sáv gombok (több képernyőn is megjelennek)
document.addEventListener('click', (e) => {
    if (e.target.closest('.btn-theme-toggle')) { toggleTheme(); return; }

    if (e.target.closest('.btn-lang-toggle')) { I18N.setLang(I18N.next()); return; }

    if (e.target.closest('.btn-dictionary')) {
        if (typeof DictionaryTool !== 'undefined') DictionaryTool.show();
        return;
    }

    if (e.target.closest('.btn-profile-nav')) {
        if (typeof Profile !== 'undefined') Profile.show();
        return;
    }

    if (e.target.closest('.btn-logout-nav')) {
        if (typeof Auth !== 'undefined') Auth.logout();
        return;
    }

    if (e.target.closest('.btn-exit-panel')) {
        if (typeof ExitGame !== 'undefined') ExitGame.showDialog();
        return;
    }
});

// ===== CONSTANTS =====

const TILE_VALUES = {
    '': 0, 'A': 1, 'E': 1, 'K': 1, 'T': 1, 'Á': 1, 'L': 1, 'N': 1, 'R': 1,
    'I': 1, 'M': 1, 'O': 1, 'S': 1, 'B': 2, 'D': 2, 'G': 2, 'Ó': 2,
    'É': 3, 'H': 3, 'SZ': 3, 'V': 3, 'F': 4, 'GY': 4, 'J': 4, 'Ö': 4,
    'P': 4, 'U': 4, 'Ü': 4, 'Z': 4, 'C': 5, 'Í': 5, 'NY': 5,
    'CS': 7, 'Ő': 7, 'Ú': 7, 'Ű': 7, 'LY': 8, 'ZS': 8, 'TY': 10
};

// A zsák kezdeti tartalma (a szerver tiles.py-jával egyezik): betű → darabszám ('' = üres zseton)
const TILE_COUNTS = {
    '': 2, 'A': 6, 'E': 6, 'K': 6, 'T': 5, 'Á': 4, 'L': 4, 'N': 4, 'R': 4,
    'I': 3, 'M': 3, 'O': 3, 'S': 3, 'B': 3, 'D': 3, 'G': 3, 'Ó': 3,
    'É': 3, 'H': 2, 'SZ': 2, 'V': 2, 'F': 2, 'GY': 2, 'J': 2, 'Ö': 2,
    'P': 2, 'U': 2, 'Ü': 2, 'Z': 2, 'C': 1, 'Í': 1, 'NY': 1,
    'CS': 1, 'Ő': 1, 'Ú': 1, 'Ű': 1, 'LY': 1, 'ZS': 1, 'TY': 1
};

const ALL_LETTERS = [
    'A', 'Á', 'B', 'C', 'CS', 'D', 'E', 'É', 'F', 'G', 'GY', 'H', 'I', 'Í',
    'J', 'K', 'L', 'LY', 'M', 'N', 'NY', 'O', 'Ó', 'Ö', 'Ő', 'P', 'R', 'S',
    'SZ', 'T', 'TY', 'U', 'Ú', 'Ü', 'Ű', 'V', 'Z', 'ZS'
];

// A prémium mezők feliratai (rövid: kis tábla; hosszú: nagy tábla) — nyelvfüggők
function premiumHtml(key) {
    return `<span class="d-desktop">${escapeHtml(t(key + '_long'))}</span><span class="d-mobile">${escapeHtml(t(key + '_short'))}</span>`;
}

const PREMIUM_LABELS = {
    get DL() { return premiumHtml('board.dl'); },
    get TL() { return premiumHtml('board.tl'); },
    get DW() { return premiumHtml('board.dw'); },
    get TW() { return premiumHtml('board.tw'); },
    get ST() { return '<span class="d-desktop">★</span><span class="d-mobile">★</span>'; },
};

// Premium mező elrendezés (szimmetrikus)
const PREMIUM_MAP = {};
const PREMIUM_QUARTER = [
    [0, 0, 'TW'], [0, 3, 'DL'], [0, 7, 'TW'],
    [1, 1, 'DW'], [1, 5, 'TL'],
    [2, 2, 'DW'], [2, 6, 'DL'],
    [3, 0, 'DL'], [3, 3, 'DW'], [3, 7, 'DL'],
    [4, 4, 'DW'],
    [5, 1, 'TL'], [5, 5, 'TL'],
    [6, 2, 'DL'], [6, 6, 'DL'],
    [7, 0, 'TW'], [7, 3, 'DL'], [7, 7, 'ST'],
];

for (const [r, c, type] of PREMIUM_QUARTER) {
    for (const [rr, cc] of [[r, c], [r, 14 - c], [14 - r, c], [14 - r, 14 - c]]) {
        PREMIUM_MAP[`${rr},${cc}`] = type;
    }
}

// ===== SOCKET =====

// Ha a Socket.IO kliens nem töltődött be (pl. kapcsolat nélküli első indítás), egy üres helyettes
// gondoskodik róla, hogy a felület hibátlanul betöltsön, és a kapcsolati sáv jelezze a problémát.
function createOfflineSocket() {
    return {
        id: null,
        connected: false,
        on() { return this; },
        off() { return this; },
        emit() { return this; },
        connect() { location.reload(); return this; },
    };
}

const socket = (typeof io !== 'undefined')
    ? io({
        reconnection: true,
        reconnectionAttempts: Infinity,
        reconnectionDelay: 1000,
        reconnectionDelayMax: 5000,
        timeout: 120000,
    })
    : createOfflineSocket();

if (typeof io === 'undefined') {
    document.addEventListener('DOMContentLoaded', () => {
        const banner = document.getElementById('connection-banner');
        if (banner) {
            banner.textContent = t('conn.offline_lib');
            banner.classList.remove('hidden');
        }
    });
}

// ===== UTILITIES =====

function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

function showScreen(screenId) {
    document.querySelectorAll('.screen').forEach(s => s.classList.add('hidden'));
    document.getElementById(screenId).classList.remove('hidden');
    // A globális téma gomb csak a bejelentkező képernyőn kell: a többin a felső sávban van
    for (const id of ['theme-toggle', 'lang-toggle']) {
        const globalToggle = document.getElementById(id);
        if (globalToggle) globalToggle.classList.toggle('hidden', screenId !== 'auth-screen');
    }
    window.scrollTo(0, 0);
}

// A szoba neve a felső sávban és (telefonon) a játék menüsorában is megjelenik
function setGameRoomName(name) {
    document.querySelectorAll('.game-room-name').forEach(el => { el.textContent = name; });
}

function showMessage(msg, isError = false, duration = 3000) {
    const container = document.getElementById('toast-container');
    const toast = document.createElement('div');
    toast.className = 'toast' + (isError ? ' toast-error' : '');
    toast.textContent = msg;
    container.appendChild(toast);

    const dismiss = () => {
        toast.classList.add('toast-out');
        toast.addEventListener('animationend', () => toast.remove());
    };
    setTimeout(dismiss, duration);
    toast.addEventListener('click', dismiss);
}

function showConfirm(title, text, confirmLabel, onConfirm) {
    document.getElementById('confirm-dialog-title').textContent = title;
    document.getElementById('confirm-dialog-text').textContent = text;
    const yesBtn = document.getElementById('btn-confirm-yes');
    yesBtn.textContent = confirmLabel;
    const noBtn = document.getElementById('btn-confirm-no');
    const dialog = document.getElementById('confirm-dialog');

    const cleanup = () => {
        dialog.classList.add('hidden');
        yesBtn.replaceWith(yesBtn.cloneNode(true));
        noBtn.replaceWith(noBtn.cloneNode(true));
    };

    yesBtn.addEventListener('click', () => { cleanup(); onConfirm(); }, { once: true });
    noBtn.addEventListener('click', () => { cleanup(); }, { once: true });
    dialog.classList.remove('hidden');
}

// Üres lista jelzése (a szöveg escape-elve kerül a HTML-be)
function emptyStateHtml(text) {
    return `<div class="empty-state"><p class="empty-msg">${escapeHtml(text)}</p></div>`;
}

function showAuthError(el, msg) {
    el.textContent = msg;
    el.classList.remove('hidden');
}

const isTouchDevice = ('ontouchstart' in window) || (navigator.maxTouchPoints > 0);

// JSON POST, a szerver hibaüzenetét (401/409/429...) is visszaadja a hívónak.
// Csak akkor dob hibát, ha a válasz nem JSON (valódi hálózati/szerver hiba).
async function postJson(url, body) {
    const res = await fetch(url, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
    });
    let data = null;
    try { data = await res.json(); } catch { /* nem JSON válasz */ }
    if (!data) throw new Error('Server error');
    return data;
}

// A szerver UTC időbélyegei ("YYYY-MM-DD HH:MM:SS") nem tartalmaznak időzónát,
// ezért kézzel jelöljük UTC-nek, különben a böngészők eltérően (Safari: hibásan) értelmezik.
function parseServerDate(value) {
    if (!value) return null;
    const iso = /(Z|[+-]\d{2}:?\d{2})$/.test(value) ? value : value.replace(' ', 'T') + 'Z';
    const d = new Date(iso);
    return isNaN(d.getTime()) ? null : d;
}

function formatServerDate(value, options) {
    const d = parseServerDate(value);
    return d ? d.toLocaleString(I18N.locale(), options) : '';
}

// Link megosztása: telefonon a rendszer megosztó lapja, egyébként vágólap
async function shareOrCopy(url, title, text, copiedMessage) {
    if (isTouchDevice && navigator.share) {
        try {
            await navigator.share({ title, text, url });
            return;
        } catch (e) {
            if (e && e.name === 'AbortError') return;  // a felhasználó bezárta a megosztó lapot
        }
    }
    try {
        await navigator.clipboard.writeText(url);
        showMessage(copiedMessage);
    } catch {
        showMessage(t('common.copy_failed'), true);
    }
}

// ===== APP STATE =====
// Consolidated game & session state

const AppState = {
    gameState: null,
    myPlayerId: null,
    isOwner: false,
    isGuest: true,
    currentUser: null,
    displayName: null,        // a szervernek bemutatkozáskor használt név
    currentRoomCode: null,
    currentRoomId: null,
    reconnectToken: null,
    challengeModeEnabled: false,
    chatMessages: [],
    roomName: null,
    gameStarted: false,
    isRestoreLobby: false,
    expectedPlayers: [],
    gameOverShown: false,
    isSpectator: false,
    spectateRoomId: null,
    spectateCode: null,
    roomIsPrivate: false,
    turnTimeLimit: 0,
    hintLimit: 0,

    // Megfigyelőként nincs mentett újracsatlakozás: a játékos saját (esetleg függő) mentését nem érintjük
    resetSpectator() {
        this.isSpectator = false;
        this.spectateRoomId = null;
        this.spectateCode = null;
        this.currentRoomId = null;
        this.roomName = null;
        this.gameStarted = false;
        this.gameOverShown = false;
        this.gameState = null;
        Chat.clear();
        const gameScreen = document.getElementById('game-screen');
        if (gameScreen) gameScreen.classList.remove('spectating');
    },

    reset() {
        // A félkész lerakás (pl. a napi feladvány megmutatott megoldása vagy egy tipp) nem maradhat a
        // következő játék táblájára
        BoardState.clearPlacement();
        Preview.clear();
        if (this.isSpectator) { this.resetSpectator(); return; }
        this.currentRoomCode = null;
        this.currentRoomId = null;
        this.reconnectToken = null;
        Chat.clear();
        this.roomName = null;
        this.gameStarted = false;
        this.isRestoreLobby = false;
        this.expectedPlayers = [];
        this.gameOverShown = false;
        this.isSpectator = false;
        this.spectateRoomId = null;
        this.spectateCode = null;
        this.gameState = null;
        Badges.newInGame = [];
        Daily.reset();
        // Clear saved rejoin info
        localStorage.removeItem('scrabble-rejoin');
        // Hide room tab
        const roomTab = document.getElementById('nav-tab-room');
        if (roomTab) {
            roomTab.classList.add('hidden');
            roomTab.disabled = true;
        }
    },
};

// ===== BOARD STATE =====
// Tile placement & selection state

const BoardState = {
    selectedTileIdx: null,
    exchangeMode: false,
    exchangeIndices: new Set(),
    placedTiles: [],      // [{row, col, letter, is_blank, handIdx}]
    boardDragInitialized: false,
    lastDropTarget: null,  // Aktuálisan kijelölt cella drag közben
    handOrder: [],         // a kéz megjelenítési sorrendje (szerver-indexek): keverés / rendezés
    handLetters: null,     // a legutóbb látott kéz (szerver sorrendben)
    newHandIdx: new Set(), // az utolsó állapotban újonnan húzott zsetonok (animációhoz)

    clearPlacement() {
        this.placedTiles = [];
        this.selectedTileIdx = null;
        this.exchangeMode = false;
        this.exchangeIndices.clear();
    },
};

// ===== TOUCH DRAG =====

const TouchDrag = {
    tileIdx: null,
    ghost: null,
    half: 22,

    createGhost(tile, x, y) {
        const size = tile.getBoundingClientRect().width || 44;
        this.half = size / 2;
        const ghost = document.createElement('div');
        ghost.className = 'hand-tile drag-ghost touch-drag-ghost' + (tile.classList.contains('long-letter') ? ' long-letter' : '');
        ghost.style.setProperty('--tile', size + 'px');
        ghost.style.left = (x - this.half) + 'px';
        ghost.style.top = (y - this.half) + 'px';
        ghost.innerHTML = tile.innerHTML;
        document.body.appendChild(ghost);
        this.ghost = ghost;
        return ghost;
    },

    moveGhost(x, y) {
        if (!this.ghost) return;
        this.ghost.style.left = (x - this.half) + 'px';
        this.ghost.style.top = (y - this.half) + 'px';
    },

    cleanup() {
        if (this.ghost) {
            this.ghost.remove();
            this.ghost = null;
        }
        this.tileIdx = null;
        if (BoardState.lastDropTarget) {
            BoardState.lastDropTarget.classList.remove('drop-target', 'drop-invalid');
            BoardState.lastDropTarget = null;
        }
    },
};

// ===== AUTH SYSTEM =====

const Auth = {
    regEmail: '',

    init() {
        // Tab váltás
        document.querySelectorAll('.auth-tab').forEach(tab => {
            tab.addEventListener('click', () => {
                document.querySelectorAll('.auth-tab').forEach(t => t.classList.remove('active'));
                document.querySelectorAll('.auth-tab-content').forEach(c => c.classList.remove('active'));
                tab.classList.add('active');
                document.getElementById('tab-' + tab.dataset.tab).classList.add('active');
            });
        });

        // Bejelentkezés
        document.getElementById('login-form').addEventListener('submit', (e) => {
            e.preventDefault();
            this.login();
        });

        // Regisztráció lépések (Enter is továbblép)
        const onEnter = (id, fn) => document.getElementById(id).addEventListener('keydown', (e) => {
            if (e.key === 'Enter') { e.preventDefault(); fn(); }
        });
        onEnter('reg-email', () => this.sendCode());
        onEnter('reg-code', () => this.verifyCode());
        onEnter('reg-password2', () => this.register());
        document.getElementById('btn-reg-send-code').addEventListener('click', () => this.sendCode());
        document.getElementById('btn-reg-resend').addEventListener('click', () => this.resendCode());
        document.getElementById('btn-reg-verify-code').addEventListener('click', () => this.verifyCode());
        document.getElementById('btn-reg-finish').addEventListener('click', () => this.register());

        // Vendég belépés
        document.getElementById('btn-guest-enter').addEventListener('click', () => this.guestEnter());
        document.getElementById('guest-name').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') this.guestEnter();
        });

        // Kijelentkezés
        document.getElementById('btn-logout').addEventListener('click', () => this.logout());
    },

    async login() {
        const email = document.getElementById('login-email').value.trim();
        const password = document.getElementById('login-password').value;
        const errorEl = document.getElementById('login-error');
        const btn = document.getElementById('btn-login');
        errorEl.classList.add('hidden');

        if (!email || !password) {
            showAuthError(errorEl, t('auth.err_required'));
            return;
        }

        btn.disabled = true;
        try {
            const data = await postJson('/api/auth/login', { email, password });

            if (data.success) {
                AppState.currentUser = data.user;
                AppState.isGuest = false;
                Lobby.enter(data.user.display_name);
            } else {
                showAuthError(errorEl, tServer(data.message));
            }
        } catch {
            showAuthError(errorEl, t('auth.err_network'));
        } finally {
            btn.disabled = false;
        }
    },

    async sendCode() {
        const email = document.getElementById('reg-email').value.trim();
        const errorEl = document.getElementById('reg-error');
        const btn = document.getElementById('btn-reg-send-code');
        errorEl.classList.add('hidden');

        if (!email) {
            showAuthError(errorEl, t('auth.err_email_required'));
            return;
        }

        if (btn) btn.disabled = true;
        try {
            const data = await postJson('/api/auth/request-code', { email });

            if (data.success) {
                this.regEmail = email;
                document.getElementById('reg-step-1').classList.add('hidden');
                document.getElementById('reg-step-2').classList.remove('hidden');
                document.getElementById('reg-code').focus();
                if (data.dev_code) {
                    document.getElementById('reg-code').value = data.dev_code;
                    const info = document.querySelector('#reg-step-2 .step-info');
                    info.textContent = t('auth.dev_code_filled');
                    info.removeAttribute('data-i18n');
                }
            } else {
                showAuthError(errorEl, tServer(data.message));
            }
        } catch {
            showAuthError(errorEl, t('auth.err_network'));
        } finally {
            if (btn) btn.disabled = false;
        }
    },

    async resendCode() {
        const errorEl = document.getElementById('reg-error');
        errorEl.classList.add('hidden');

        try {
            const data = await postJson('/api/auth/request-code', { email: this.regEmail });
            if (data.success) {
                showAuthError(errorEl, t('auth.new_code_sent'));
                errorEl.classList.remove('hidden');
                errorEl.classList.add('text-success');
                setTimeout(() => { errorEl.classList.remove('text-success'); }, 3000);
                if (data.dev_code) {
                    document.getElementById('reg-code').value = data.dev_code;
                }
            } else {
                showAuthError(errorEl, tServer(data.message));
            }
        } catch {
            showAuthError(errorEl, t('auth.err_network_short'));
        }
    },

    async verifyCode() {
        const code = document.getElementById('reg-code').value.trim();
        const errorEl = document.getElementById('reg-error');
        errorEl.classList.add('hidden');

        if (!code || code.length !== 6) {
            showAuthError(errorEl, t('auth.err_code_len'));
            return;
        }

        try {
            const data = await postJson('/api/auth/verify-code', { email: this.regEmail, code });

            if (data.success) {
                document.getElementById('reg-step-2').classList.add('hidden');
                document.getElementById('reg-step-3').classList.remove('hidden');
                document.getElementById('reg-display-name').focus();
            } else {
                showAuthError(errorEl, tServer(data.message));
            }
        } catch {
            showAuthError(errorEl, t('auth.err_network'));
        }
    },

    async register() {
        const displayName = document.getElementById('reg-display-name').value.trim();
        const password = document.getElementById('reg-password').value;
        const password2 = document.getElementById('reg-password2').value;
        const errorEl = document.getElementById('reg-error');
        const btn = document.getElementById('btn-reg-finish');
        errorEl.classList.add('hidden');

        if (!displayName || !password || !password2) {
            showAuthError(errorEl, t('auth.err_required'));
            return;
        }
        if (password.length < 6) {
            showAuthError(errorEl, t('auth.err_password_short'));
            return;
        }
        if (password !== password2) {
            showAuthError(errorEl, t('auth.err_password_mismatch'));
            return;
        }

        if (btn) btn.disabled = true;
        try {
            const data = await postJson('/api/auth/register', {
                email: this.regEmail, password, display_name: displayName,
            });

            if (data.success) {
                AppState.currentUser = data.user;
                AppState.isGuest = false;
                Lobby.enter(data.user.display_name);
            } else {
                showAuthError(errorEl, tServer(data.message));
            }
        } catch {
            showAuthError(errorEl, t('auth.err_network'));
        } finally {
            if (btn) btn.disabled = false;
        }
    },

    guestEnter() {
        const name = document.getElementById('guest-name').value.trim();
        const errorEl = document.getElementById('guest-error');
        if (errorEl) errorEl.classList.add('hidden');
        if (!name) {
            if (errorEl) showAuthError(errorEl, t('auth.err_name_required'));
            return;
        }
        // Ugyanaz a szabály, mint a szerveren: különben a szerver csendben "Névtelen"-re cserélné
        if (!/^[\p{L}\p{N}_\s.-]{1,20}$/u.test(name)) {
            if (errorEl) showAuthError(errorEl, t('auth.err_name_invalid'));
            return;
        }
        AppState.currentUser = null;
        AppState.isGuest = true;
        Lobby.enter(name);
    },

    async logout() {
        if (AppState.currentRoomId || AppState.isSpectator) {
            showConfirm(t('common.logout'), t('auth.logout_confirm'),
                t('common.logout'), () => this._doLogout());
            return;
        }
        await this._doLogout();
    },

    async _doLogout() {
        // A szerver is felejtse el az azonosságot (online státusz, szoba, auth)
        socket.emit('logout');
        if (!AppState.isGuest) {
            try { await fetch('/api/auth/logout', { method: 'POST' }); } catch { /* ignore */ }
        }
        AppState.currentUser = null;
        AppState.isGuest = true;
        AppState.displayName = null;
        AppState.reset();
        ChallengeUI.stopCountdown(); TurnTimerUI._stop();
        // Reset regisztráció
        document.getElementById('reg-step-1').classList.remove('hidden');
        document.getElementById('reg-step-2').classList.add('hidden');
        document.getElementById('reg-step-3').classList.add('hidden');
        for (const id of ['reg-email', 'reg-code', 'reg-display-name', 'reg-password', 'reg-password2']) {
            document.getElementById(id).value = '';
        }
        showScreen('auth-screen');
    },

    async checkSession() {
        try {
            const res = await fetch('/api/auth/me');
            const data = await res.json();
            if (data.success) {
                AppState.currentUser = data.user;
                AppState.isGuest = false;
                Lobby.enter(data.user.display_name);
            }
        } catch { /* no session */ }
        document.documentElement.classList.remove('booting');
        if (!AppState.currentUser) Replay.openSharedLink();
    },
};

// ===== LOBBY =====

const Lobby = {
    init() {
        document.getElementById('btn-create-room').addEventListener('click', () => this.createRoom());
        document.getElementById('btn-join-by-code').addEventListener('click', () => this.joinByCode());
        document.getElementById('btn-spectate-by-code').addEventListener('click', () => this.spectateByCode());
        document.getElementById('join-code-input').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') this.joinByCode();
        });
        document.getElementById('room-ai-count').addEventListener('change', () => this.updateAiControls());
        document.getElementById('room-ai-difficulty').addEventListener('change', () => this.updateAiControls());
        this.updateAiControls();

        // Lobby nav tab switching (a profil képernyő saját, másolt sorát a Profile kezeli)
        document.querySelectorAll('#lobby-nav .lobby-nav-tab').forEach(tab => {
            tab.addEventListener('click', () => this.switchTab(tab.dataset.lobbyTab));
        });

        socket.on('rooms_list', (rooms) => { this._rooms = rooms; this.renderRoomsList(rooms); });
        socket.on('live_games', (games) => { this._liveGames = games; this.renderLiveGames(games); });
        window.addEventListener('langchange', () => this.onLangChange());
    },

    _rooms: [],
    _liveGames: [],
    _history: null,
    _savedGames: null,

    // Nyelvváltáskor a gyorsítótárazott listák újrarajzolása
    onLangChange() {
        this.renderRoomsList(this._rooms);
        this.renderLiveGames(this._liveGames);
        if (this._history) this.renderHistory(this._history);
        if (this._savedGames) this.renderSavedGames(this._savedGames);
        if (AppState.displayName) {
            document.getElementById('lobby-user-name').textContent =
                AppState.displayName + (AppState.isGuest ? t('lobby.guest_suffix') : '');
        }
    },

    // A robotok is foglalnak helyet: a tulajdonosnak is maradnia kell
    updateAiControls() {
        const count = parseInt(document.getElementById('room-ai-count').value) || 0;
        const difficulty = document.getElementById('room-ai-difficulty');
        difficulty.disabled = count === 0;
        document.getElementById('room-ai-auto-hint').classList.toggle('hidden', count === 0 || difficulty.value !== 'auto');
        const maxSel = document.getElementById('room-max-players');
        if (count > 0 && parseInt(maxSel.value) < count + 1) maxSel.value = String(Math.min(count + 1, 4));
        for (const opt of maxSel.options) opt.disabled = count > 0 && parseInt(opt.value) < count + 1;
    },

    switchTab(tabId) {
        // Room tab: navigate to waiting/game screen instead of a panel
        if (tabId === 'room') {
            if (AppState.gameStarted) {
                showScreen('game-screen');
            } else {
                showScreen('waiting-screen');
            }
            return;
        }

        // Update tab buttons
        document.querySelectorAll('#lobby-nav .lobby-nav-tab').forEach(t => t.classList.remove('active'));
        const activeTab = document.querySelector(`#lobby-nav .lobby-nav-tab[data-lobby-tab="${tabId}"]`);
        if (activeTab) activeTab.classList.add('active');
        LobbyNav.reveal(activeTab);

        // Update panels
        document.querySelectorAll('.lobby-tab-panel').forEach(p => p.classList.remove('active'));
        const panel = document.getElementById('lobby-panel-' + tabId);
        if (panel) panel.classList.add('active');

        // Load data for tabs
        if (tabId === 'home') {
            socket.emit('get_rooms');
            if (!AppState.isGuest) this.loadHistory();
        } else if (tabId === 'saved') {
            this.loadSavedGames();
        } else if (tabId === 'friends') {
            Friends.load();
        } else if (tabId === 'leaderboard') {
            Leaderboard.load();
        } else if (tabId === 'practice') {
            Daily.load();
            Practice.onShow();
        } else if (tabId === 'async') {
            AsyncGames.onShow();
        }
    },

    _identitySentForSid: null,

    // A szerver SID-hez köti a nevet és az azonosságot, ezért minden új kapcsolatnál
    // (újracsatlakozás után is) újra be kell mutatkozni. Regisztrált felhasználónál a
    // szerver által kiadott aláírt tokent is küldjük, a user_id önmagában nem elég.
    async sendIdentity() {
        if (!AppState.displayName) return;
        const payload = {
            name: AppState.displayName,
            is_guest: AppState.isGuest,
            user_id: AppState.currentUser ? AppState.currentUser.id : null,
        };
        if (!AppState.isGuest) {
            try {
                const res = await fetch('/api/auth/socket-token');
                const data = await res.json();
                if (data.success) payload.auth_token = data.token;
            } catch { /* a szerver vendégként kezeli, és hibát jelez */ }
        }
        if (!AppState.displayName) return;  // közben kijelentkezett
        socket.emit('set_name', payload);
        if (socket.connected) this._identitySentForSid = socket.id;
    },

    enter(displayName) {
        AppState.displayName = displayName;
        // A meghívó linkes csatlakozás csak a bemutatkozás (set_name) után mehet el: a szerver
        // addig nem ismeri a nevünket és az azonosságunkat
        const identityReady = this.sendIdentity();
        AppState.myPlayerId = socket.id;

        document.getElementById('lobby-user-name').textContent =
            displayName + (AppState.isGuest ? t('lobby.guest_suffix') : '');

        // Hide tabs/buttons for guests
        document.getElementById('btn-profile').classList.toggle('hidden', AppState.isGuest);
        document.querySelectorAll('.btn-profile-nav').forEach(
            btn => btn.classList.toggle('hidden', AppState.isGuest));
        const createTab = document.getElementById('nav-tab-create');
        const savedTab = document.getElementById('nav-tab-saved');
        const friendsTab = document.getElementById('nav-tab-friends');
        const asyncTab = document.getElementById('nav-tab-async');
        if (asyncTab) asyncTab.classList.toggle('hidden', AppState.isGuest);
        if (createTab) createTab.classList.toggle('hidden', AppState.isGuest);
        if (savedTab) savedTab.classList.toggle('hidden', AppState.isGuest);
        if (friendsTab) friendsTab.classList.toggle('hidden', AppState.isGuest);

        // Hide history section for guests
        const historySection = document.getElementById('home-history-section');
        if (historySection) historySection.classList.toggle('hidden', AppState.isGuest);
        
        if (!AppState.isGuest) {
            Friends.load(); // Kérések badge frissítéséhez
            AsyncGames.load();   // a "te jössz" jelvényhez
        }

        // Reset to home tab
        this.switchTab('home');
        showScreen('lobby-screen');
        identityReady.then(() => this.handleUrlActions());
    },

    // Meghívó linkek és PWA-parancsikonok: /?join=123456, /?spectate=123456, /?action=create
    _urlActionsDone: false,
    handleUrlActions() {
        if (this._urlActionsDone) return;
        this._urlActionsDone = true;
        let params;
        try { params = new URLSearchParams(location.search); } catch { return; }
        const join = params.get('join');
        const spectate = params.get('spectate');
        const action = params.get('action');
        const replay = params.get('replay');
        if (!join && !spectate && !action && !replay) return;
        // A cím megtisztítása, hogy frissítésre ne ismétlődjön a művelet
        try { history.replaceState(null, '', location.pathname); } catch { /* ignore */ }

        if (join && /^\d{6}$/.test(join)) {
            document.getElementById('join-code-input').value = join;
            socket.emit('join_room', { code: join });
        } else if (spectate && /^\d{6}$/.test(spectate)) {
            AppState.spectateCode = spectate;
            socket.emit('spectate_room', { code: spectate });
        } else if (action === 'create' && !AppState.isGuest) {
            this.switchTab('create');
        } else if (replay) {
            Replay.loadShared(replay);
        }
    },

    _tryRejoin() {
        const saved = localStorage.getItem('scrabble-rejoin');
        if (!saved) return;
        try {
            const info = JSON.parse(saved);
            if (!info.token) { localStorage.removeItem('scrabble-rejoin'); return; }
            AppState.reconnectToken = info.token;
            socket.emit('rejoin_room', { token: info.token });
        } catch {
            localStorage.removeItem('scrabble-rejoin');
        }
    },

    _dismissRejoin() {
        localStorage.removeItem('scrabble-rejoin');
        socket.emit('get_rooms');
    },

    createRoom() {
        const name = document.getElementById('room-name').value.trim() || t('create.room_placeholder');
        const maxPlayers = document.getElementById('room-max-players').value;
        const challengeMode = document.getElementById('room-challenge-mode').checked;
        const isPrivate = document.getElementById('room-private').checked;
        const turnTimeLimit = parseInt(document.getElementById('room-turn-limit').value) || 0;
        const aiCount = parseInt(document.getElementById('room-ai-count').value) || 0;
        const difficultyValue = document.getElementById('room-ai-difficulty').value;
        const aiDifficulty = difficultyValue === 'auto' ? 'auto' : (parseInt(difficultyValue) || 6);
        const hintLimit = Number(document.getElementById('room-hint-limit').value);
        socket.emit('create_room', {
            name, max_players: maxPlayers,
            challenge_mode: challengeMode, is_private: isPrivate,
            turn_time_limit: turnTimeLimit,
            ai_players: Array(aiCount).fill(aiDifficulty),
            hint_limit: hintLimit,
        });
    },

    joinByCode() {
        const code = document.getElementById('join-code-input').value.trim();
        if (!code || code.length !== 6) {
            showMessage(t('lobby.err_code'), true);
            return;
        }
        socket.emit('join_room', { code });
    },

    spectateByCode() {
        const code = document.getElementById('join-code-input').value.trim();
        if (!/^\d{6}$/.test(code)) {
            showMessage(t('lobby.err_code'), true);
            return;
        }
        AppState.spectateCode = code;
        socket.emit('spectate_room', { code });
    },

    async loadHistory() {
        try {
            const resp = await fetch('/api/auth/profile');
            const data = await resp.json();
            if (!data.success) return;
            this._history = data.history;
            this.renderHistory(data.history);
        } catch { /* ignore */ }
    },

    renderHistory(history) {
        const container = document.getElementById('home-history-container');
        if (!history || !history.length) {
            container.innerHTML = emptyStateHtml(t('common.no_finished_games'));
            return;
        }
        container.innerHTML = '';
        history.forEach(h => {
            const row = document.createElement('div');
            row.className = 'history-row' + (h.is_winner ? ' winner' : '');

            const info = document.createElement('div');
            info.className = 'history-info';

            const date = document.createElement('span');
            date.className = 'history-date';
            date.textContent = formatServerDate(h.created_at, { year: 'numeric', month: 'numeric', day: 'numeric' });
            info.appendChild(date);

            const room = document.createElement('span');
            room.className = 'history-room';
            room.textContent = h.room_name;
            info.appendChild(room);

            const score = document.createElement('span');
            score.className = 'history-score';
            score.textContent = t('common.points', { n: h.final_score });
            info.appendChild(score);

            const result = document.createElement('span');
            result.className = 'history-result';
            result.textContent = h.is_winner ? t('common.win') : t('common.loss');
            info.appendChild(result);

            if (h.opponents && h.opponents.length) {
                const opp = document.createElement('span');
                opp.className = 'history-opponents';
                opp.textContent = t('history.vs', { names: h.opponents.map(o => o.player_name).join(', ') });
                info.appendChild(opp);
            }

            row.appendChild(info);

            const btn = document.createElement('button');
            btn.className = 'small-btn';
            btn.textContent = t('replay.title');
            btn.addEventListener('click', () => Replay.load(h.game_id));
            row.appendChild(btn);

            container.appendChild(row);
        });
    },

    async loadSavedGames() {
        const container = document.getElementById('saved-games-container');
        if (AppState.isGuest) {
            container.innerHTML = emptyStateHtml(t('saved.guest'));
            return;
        }
        try {
            const resp = await fetch('/api/auth/saved-games');
            const data = await resp.json();
            if (!data.success) {
                container.innerHTML = emptyStateHtml(t('common.load_failed'));
                return;
            }
            this._savedGames = data.games;
            this.renderSavedGames(data.games);
        } catch {
            container.innerHTML = emptyStateHtml(t('common.load_failed'));
        }
    },

    renderSavedGames(games) {
        const container = document.getElementById('saved-games-container');
        if (!games || !games.length) {
            container.innerHTML = emptyStateHtml(t('saved.none'));
            return;
        }
        container.innerHTML = '';
        games.forEach(g => {
            const card = document.createElement('div');
            card.className = 'saved-game-card';

            const info = document.createElement('div');
            info.className = 'saved-game-info';

            const name = document.createElement('div');
            name.className = 'saved-game-name';
            name.textContent = g.room_name;
            info.appendChild(name);

            const details = document.createElement('div');
            details.className = 'saved-game-details';
            details.textContent = formatServerDate(g.updated_at || g.created_at, {
                year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
            });
            info.appendChild(details);

            if (g.score !== undefined) {
                const scoreDiv = document.createElement('div');
                scoreDiv.className = 'saved-game-score';
                scoreDiv.textContent = `${g.player_name || t('saved.you')}: ${t('common.points', { n: g.score })}`;
                info.appendChild(scoreDiv);
            }

            if (g.opponents && g.opponents.length) {
                const opp = document.createElement('div');
                opp.className = 'saved-game-opponents';
                const oppTexts = g.opponents.map(o =>
                    typeof o === 'object' ? `${o.name} (${o.score})` : o
                );
                opp.textContent = t('saved.opponents', { names: oppTexts.join(', ') });
                info.appendChild(opp);
            }

            card.appendChild(info);

            const actions = document.createElement('div');
            actions.className = 'saved-game-actions';

            const resumeBtn = document.createElement('button');
            resumeBtn.textContent = g.is_owner ? t('saved.restore') : t('saved.continue');
            resumeBtn.className = 'restored-btn';
            resumeBtn.addEventListener('click', () => {
                if (g.is_owner) {
                    socket.emit('restore_game', { game_id: g.game_id });
                } else {
                    showMessage(t('saved.wait_owner'), true);
                }
            });
            actions.appendChild(resumeBtn);

            const abandonBtn = document.createElement('button');
            abandonBtn.textContent = t('common.delete');
            abandonBtn.className = 'btn-abandon';
            abandonBtn.addEventListener('click', () => {
                showConfirm(t('common.delete'), t('saved.delete_confirm'), t('common.delete'), async () => {
                    try {
                        const resp = await fetch(`/api/game/${g.game_id}/abandon`, { method: 'POST' });
                        const result = await resp.json();
                        if (result.success) {
                            showMessage(t('saved.deleted'));
                            this.loadSavedGames();
                            socket.emit('get_rooms');
                        } else {
                            showMessage(tServer(result.message) || t('common.error'), true);
                        }
                    } catch {
                        showMessage(t('common.error'), true);
                    }
                });
            });
            actions.appendChild(abandonBtn);

            card.appendChild(actions);
            container.appendChild(card);
        });
    },

    renderRoomsList(rooms) {
        const container = document.getElementById('rooms-container');
        container.innerHTML = '';

        // Check if player has an active game to rejoin
        let rejoinInfo = null;
        const saved = localStorage.getItem('scrabble-rejoin');
        if (saved) {
            try {
                const info = JSON.parse(saved);
                if (info.token && info.roomId) rejoinInfo = info;
                else if (info.token && !info.roomId) rejoinInfo = info;  // legacy: no roomId
                else localStorage.removeItem('scrabble-rejoin');
            } catch { localStorage.removeItem('scrabble-rejoin'); }
        }

        // If rejoin target is not in the public rooms list, show a standalone card
        const rejoinInList = rejoinInfo && rejoinInfo.roomId && rooms.some(r => r.id === rejoinInfo.roomId);
        if (rejoinInfo && !rejoinInList) {
            const card = document.createElement('div');
            card.className = 'room-card room-card-rejoin';

            const info = document.createElement('div');
            info.className = 'room-info';
            const nameDiv = document.createElement('div');
            nameDiv.className = 'room-name';
            nameDiv.textContent = rejoinInfo.roomName || t('lobby.active_game');
            info.appendChild(nameDiv);
            const details = document.createElement('div');
            details.className = 'room-details';
            details.textContent = t('lobby.your_game_running');
            info.appendChild(details);
            card.appendChild(info);

            const btns = document.createElement('div');
            btns.className = 'room-card-actions';
            const rejoinBtn = document.createElement('button');
            rejoinBtn.textContent = t('lobby.rejoin');
            rejoinBtn.className = 'btn-join btn-rejoin';
            rejoinBtn.addEventListener('click', () => this._tryRejoin());
            btns.appendChild(rejoinBtn);
            const dismissBtn = document.createElement('button');
            dismissBtn.textContent = t('lobby.dismiss');
            dismissBtn.className = 'btn-rejoin-dismiss';
            dismissBtn.addEventListener('click', () => this._dismissRejoin());
            btns.appendChild(dismissBtn);
            card.appendChild(btns);
            container.appendChild(card);
        }

        if (!rooms.length && !container.children.length) {
            container.innerHTML = emptyStateHtml(t('lobby.no_rooms'));
            return;
        }

        rooms.forEach(room => {
            const isRejoinTarget = rejoinInfo && rejoinInfo.roomId === room.id;
            const card = document.createElement('div');
            card.className = 'room-card' + (isRejoinTarget ? ' room-card-rejoin' : '');

            const info = document.createElement('div');
            info.className = 'room-info';

            const nameDiv = document.createElement('div');
            nameDiv.className = 'room-name';
            nameDiv.textContent = room.name;

            const details = document.createElement('div');
            details.className = 'room-details';
            details.textContent = t('room.players_owner', { n: room.players, max: room.max_players, owner: room.owner });

            info.appendChild(nameDiv);
            info.appendChild(details);

            // Badges
            const badges = document.createElement('div');
            badges.className = 'room-badges';
            if (room.started && !room.finished) {
                const b = document.createElement('span');
                b.className = 'room-badge room-badge-playing';
                b.textContent = t('room.badge_playing');
                badges.appendChild(b);
            }
            if (room.challenge_mode) {
                const b = document.createElement('span');
                b.className = 'room-badge room-badge-challenge';
                b.textContent = t('room.badge_challenge');
                badges.appendChild(b);
            }
            if (room.bots) {
                const b = document.createElement('span');
                b.className = 'room-badge room-badge-bots';
                b.textContent = t('room.badge_bots', { n: room.bots });
                badges.appendChild(b);
            }
            if (room.turn_time_limit) {
                const b = document.createElement('span');
                b.className = 'room-badge room-badge-timer';
                b.textContent = t('room.limit_badge', { n: room.turn_time_limit });
                badges.appendChild(b);
            }
            if (badges.children.length > 0) {
                info.appendChild(badges);
            }

            card.appendChild(info);

            if (isRejoinTarget) {
                // Show rejoin button on this room card
                const btns = document.createElement('div');
                btns.className = 'room-card-actions';
                const rejoinBtn = document.createElement('button');
                rejoinBtn.textContent = t('lobby.rejoin');
                rejoinBtn.className = 'btn-join btn-rejoin';
                rejoinBtn.addEventListener('click', () => this._tryRejoin());
                btns.appendChild(rejoinBtn);
                const dismissBtn = document.createElement('button');
                dismissBtn.textContent = t('lobby.dismiss');
                dismissBtn.className = 'btn-rejoin-dismiss';
                dismissBtn.addEventListener('click', () => this._dismissRejoin());
                btns.appendChild(dismissBtn);
                card.appendChild(btns);
            } else if (!room.started && !room.finished && room.players < room.max_players) {
                const btn = document.createElement('button');
                btn.textContent = t('lobby.join');
                btn.className = 'btn-join';
                btn.addEventListener('click', () => socket.emit('join_room', { room_id: room.id }));
                card.appendChild(btn);
            }

            container.appendChild(card);
        });
    },

    // Élő (nyilvános, folyamatban lévő) játékok: megfigyelhetők
    renderLiveGames(games) {
        const container = document.getElementById('live-games-container');
        if (!container) return;
        if (!games || !games.length) {
            container.innerHTML = emptyStateHtml(t('lobby.no_live_games'));
            return;
        }
        container.innerHTML = '';
        games.forEach(game => {
            const card = document.createElement('div');
            card.className = 'room-card';

            const info = document.createElement('div');
            info.className = 'room-info';
            const nameDiv = document.createElement('div');
            nameDiv.className = 'room-name';
            nameDiv.textContent = game.name;
            const details = document.createElement('div');
            details.className = 'room-details';
            details.textContent = game.players.map(p => `${p.name} ${p.score}`).join(' \u00b7 ');
            info.appendChild(nameDiv);
            info.appendChild(details);

            const badges = document.createElement('div');
            badges.className = 'room-badges';
            const addBadge = (cls, text) => {
                const b = document.createElement('span');
                b.className = 'room-badge ' + cls;
                b.textContent = text;
                badges.appendChild(b);
            };
            if (game.challenge_mode) addBadge('room-badge-challenge', t('room.badge_challenge'));
            const bots = game.players.filter(p => p.is_bot).length;
            if (bots) addBadge('room-badge-bots', t('room.badge_bots', { n: bots }));
            if (game.turn_time_limit) addBadge('room-badge-timer', t('room.limit_badge', { n: game.turn_time_limit }));
            if (game.spectators) addBadge('room-badge-viewers', t('room.badge_viewers', { n: game.spectators }));
            info.appendChild(badges);
            card.appendChild(info);

            const btn = document.createElement('button');
            btn.className = 'btn-join';
            btn.textContent = t('lobby.spectate');
            btn.addEventListener('click', () => socket.emit('spectate_room', { room_id: game.id }));
            card.appendChild(btn);

            container.appendChild(card);
        });
    },
};

// ===== WAITING ROOM =====

const WaitingRoom = {
    init() {
        document.getElementById('btn-leave-room').addEventListener('click', () => socket.emit('leave_room'));
        document.getElementById('btn-start-game').addEventListener('click', () => socket.emit('start_game'));
        document.getElementById('btn-copy-code').addEventListener('click', () => this.copyCode());
        document.getElementById('btn-copy-link').addEventListener('click', () => this.shareLink());
        window.addEventListener('langchange', () => {
            if (AppState.currentRoomId && !AppState.gameStarted) this.refreshBadges();
            this.update();
        });

        socket.on('room_joined', (data) => this.onJoined(data));
        socket.on('room_code', (data) => this.onCode(data));
        socket.on('room_left', () => this.onLeft());
    },

    onJoined(data) {
        AppState.isOwner = data.is_owner;
        AppState.currentRoomId = data.room_id;

        // Clear and load chat history
        Chat.clear();
        if (data.chat_messages) {
            data.chat_messages.forEach(msg => Chat.onMessage(msg, true));
        }

        if (data.reconnect_token) {
            AppState.reconnectToken = data.reconnect_token;
            // Persist rejoin info for page reload recovery
            localStorage.setItem('scrabble-rejoin', JSON.stringify({
                token: data.reconnect_token,
                roomName: data.room_name,
                roomId: data.room_id,
            }));
        }
        AppState.challengeModeEnabled = data.challenge_mode || false;
        AppState.roomName = data.room_name;
        AppState.gameStarted = false;
        AppState.isRestoreLobby = data.is_restore_lobby || false;
        AppState.expectedPlayers = data.expected_players || [];
        document.getElementById('waiting-room-name').textContent = data.room_name;

        AppState.roomIsPrivate = !!data.is_private;
        AppState.turnTimeLimit = data.turn_time_limit || 0;
        AppState.hintLimit = data.hint_limit || 0;
        this.refreshBadges();

        const codeSection = document.getElementById('room-code-display');
        codeSection.classList.toggle('hidden', !AppState.isOwner);

        // Show room tab in lobby nav
        const roomTab = document.getElementById('nav-tab-room');
        if (roomTab) {
            roomTab.textContent = data.room_name;
            roomTab.classList.remove('hidden');
            roomTab.disabled = false;
        }

        showScreen('waiting-screen');
        this.update();
    },

    refreshBadges() {
        document.getElementById('waiting-challenge-mode').classList.toggle('hidden', !AppState.challengeModeEnabled);
        document.getElementById('waiting-private-mode').classList.toggle('hidden', !AppState.roomIsPrivate);
        const turnLimitBadge = document.getElementById('waiting-turn-limit');
        if (AppState.turnTimeLimit) {
            turnLimitBadge.textContent = t('room.limit_per_turn', { n: AppState.turnTimeLimit });
            turnLimitBadge.classList.remove('hidden');
        } else {
            turnLimitBadge.classList.add('hidden');
        }

        // Tipp-jelvény: csak ott érdekes, ahol robotok is vannak (egyedül, robotok ellen érhető el)
        const hintBadge = document.getElementById('waiting-hint-limit');
        const gs = AppState.gameState;
        const hasBots = !!(gs && gs.players && gs.players.some(p => p.is_bot));
        if (hasBots) {
            hintBadge.textContent = AppState.hintLimit > 0
                ? t('room.hint_badge', { n: AppState.hintLimit })
                : t('room.hint_off');
            hintBadge.classList.remove('hidden');
        } else {
            hintBadge.classList.add('hidden');
        }
    },

    onCode(data) {
        AppState.currentRoomCode = data.code;
        document.getElementById('room-code-value').textContent = data.code;
        document.getElementById('room-code-display').classList.remove('hidden');
        AppState.isOwner = true;
        this.update();
    },

    onLeft() {
        AppState.reset();
        ChallengeUI.stopCountdown(); TurnTimerUI._stop();
        showScreen('lobby-screen');
        socket.emit('get_rooms');
    },

    copyCode() {
        if (!AppState.currentRoomCode) return;
        navigator.clipboard.writeText(AppState.currentRoomCode).then(() => {
            const btn = document.getElementById('btn-copy-code');
            btn.textContent = t('common.copied');
            setTimeout(() => { btn.textContent = t('common.copy'); }, 2000);
        }).catch(() => showMessage(t('common.copy_failed'), true));
    },

    // Meghívó link: /?join=KÓD — telefonon a rendszer megosztó lapja, egyébként vágólap
    inviteUrl() {
        return `${location.origin}/?join=${AppState.currentRoomCode}`;
    },

    shareLink() {
        if (!AppState.currentRoomCode) return;
        return shareOrCopy(this.inviteUrl(), t('app.title'),
            t('wait.share_text', { room: AppState.roomName || '' }), t('wait.link_copied'));
    },

    update() {
        const gs = AppState.gameState;
        if (!gs) return;
        const container = document.getElementById('waiting-players');
        const joinedNames = gs.players.map(p => p.name);
        this.refreshBadges();

        if (AppState.isRestoreLobby && AppState.expectedPlayers.length > 0) {
            // Restore lobby: show expected + joined status
            container.innerHTML = AppState.expectedPlayers.map(name => {
                const joined = joinedNames.includes(name);
                return `<div class="player-item ${joined ? 'joined' : 'missing'}">
                    <span class="player-avatar">${escapeHtml(Array.from(name)[0] || '?')}</span>
                    <span class="player-name">${escapeHtml(name)}</span>
                    <span class="player-tag">${joined ? t('wait.joined') : t('wait.waiting')}</span>
                </div>`;
            }).join('');
        } else {
            container.innerHTML = gs.players.map((p, i) => `
                <div class="player-item ${i === 0 ? 'owner' : ''} ${p.is_bot ? 'bot' : ''} ${p.disconnected ? 'offline' : ''}">
                    <span class="player-avatar">${p.is_bot
                        ? '<svg class="icon"><use href="#i-robot"/></svg>'
                        : escapeHtml(Array.from(p.name)[0] || '?')}</span>
                    <span class="player-name">${escapeHtml(p.name)}</span>
                    ${i === 0 ? `<span class="player-tag">${escapeHtml(t('wait.owner'))}</span>` : ''}
                    ${p.is_bot ? `<span class="player-tag">${escapeHtml(t('ai.level_' + (p.difficulty || 6)))}</span>` : ''}
                    ${p.disconnected ? `<span class="player-tag">${escapeHtml(t('wait.offline'))}</span>` : ''}
                </div>
            `).join('');
        }

        const startBtn = document.getElementById('btn-start-game');
        startBtn.classList.toggle('hidden', !(AppState.isOwner && gs.players.length >= 1));
        
        const inviteBtn = document.getElementById('btn-invite-friends');
        if (inviteBtn) {
            inviteBtn.classList.toggle('hidden', !(AppState.isOwner && !AppState.isGuest && !AppState.isRestoreLobby));
        }
    },
};

// ===== GAME BOARD =====

const GameBoard = {
    _prevCurrentPlayer: null,
    _gameId: null,
    _prevBoardKeys: null,     // az előző rajzoláskor a táblán álló betűk (új betűk animálásához)
    _prevPlacedKeys: new Set(),
    _prevScores: null,        // {játékos-id: pontszám} az előző állapotból (pontszám-felugróhoz)
    _historySig: '',

    init() {
        socket.on('game_started', () => {
            AppState.gameStarted = true;
            SoundManager.play('game_start');
            // Save rejoin info to localStorage for page reload recovery
            if (AppState.reconnectToken) {
                localStorage.setItem('scrabble-rejoin', JSON.stringify({
                    token: AppState.reconnectToken,
                    roomName: AppState.roomName,
                    roomId: AppState.currentRoomId,
                }));
            }
            setGameRoomName(AppState.roomName || t('create.room_placeholder'));
            // Update lobby room tab
            const roomTab = document.getElementById('nav-tab-room');
            if (roomTab) roomTab.textContent = t('lobby.active_game');
            showScreen('game-screen');
            this.build();
            BoardZoom.init();
            // Re-render if game_state arrived before game_started (restore case)
            if (AppState.gameState && AppState.gameState.started) this.renderAll();
        });

        socket.on('game_state', (state) => this.onGameState(state));
        socket.on('action_result', (data) => this.onActionResult(data));
        socket.on('error', (data) => {
            showMessage(tServer(data.message), true);
            Spectate.onError();
        });
        socket.on('move_preview', (data) => Preview.onResult(data));
        socket.on('hint_result', (data) => Hint.onResult(data));

        // Action buttons
        document.getElementById('btn-place').addEventListener('click', () => this.placeTiles());
        document.getElementById('btn-exchange').addEventListener('click', () => this.toggleExchange());
        document.getElementById('btn-pass').addEventListener('click', () => socket.emit('pass_turn'));
        document.getElementById('btn-recall').addEventListener('click', () => this.recall());
        document.getElementById('btn-shuffle').addEventListener('click', () => this.shuffleHand());
        document.getElementById('btn-sort').addEventListener('click', () => this.sortHand());
        document.getElementById('btn-tracker').addEventListener('click', () => Tracker.show());
        document.getElementById('btn-hint').addEventListener('click', () => Hint.request());

        window.addEventListener('langchange', () => {
            if (AppState.gameState && AppState.gameState.started) this.renderAll();
            if (!document.getElementById('tracker-dialog').classList.contains('hidden')) Tracker.render();
        });
    },

    _resetAnimState() {
        this._prevBoardKeys = null;
        this._prevPlacedKeys = new Set();
        this._prevScores = null;
        this._historySig = '';
        this._prevCurrentPlayer = null;
    },

    // A teljes játékfelület újrarajzolása a mostani állapotból
    renderAll() {
        this.renderBoard();
        this.renderHand();
        this.renderScoreboard();
        this.renderGameInfo();
        this.renderHistory();
        ChallengeUI.render();
        TurnTimerUI.update(AppState.gameState);
        this.updateButtons();
        this.updateSpectatorUi();
    },

    onGameState(state) {
        const prevPlayer = this._prevCurrentPlayer;
        if (state.game_id !== this._gameId) {
            this._gameId = state.game_id;
            this._resetAnimState();
            BoardState.handOrder = [];
            BoardState.handLetters = null;
            BoardState.clearPlacement();
            Preview.clear();
        }
        AppState.gameState = state;
        AppState.myPlayerId = socket.id;
        AppState.isSpectator = !!state.spectator;
        Daily.onGameState(state);
        AsyncGames.onGameState(state);

        // Clear placed tiles when turn changes away from us
        if (state.current_player !== socket.id && BoardState.placedTiles.length > 0) {
            BoardState.clearPlacement();
            Preview.clear();
        }

        if (state.started) {
            // Újracsatlakozásnál nincs `game_started` esemény, de a játék már fut
            AppState.gameStarted = true;
            const roomTab = document.getElementById('nav-tab-room');
            if (roomTab && !roomTab.classList.contains('hidden')) roomTab.textContent = t('lobby.active_game');
            if (document.getElementById('game-screen').classList.contains('hidden')) {
                showScreen('game-screen');
                this.build();
                BoardZoom.init();
            }
            this.renderBoard();
            this.renderHand();
            this.renderScoreboard();
            this.renderGameInfo();
            this.renderHistory();
            ChallengeUI.render();
            TurnTimerUI.update(state);
            this.updateButtons();
            this.updateSpectatorUi();

            // Your turn notification
            if (!state.finished && state.current_player === socket.id && prevPlayer !== socket.id) {
                SoundManager.play('your_turn');
            }
            if (prevPlayer !== state.current_player) this._animateTurnChange(state);
            this._prevCurrentPlayer = state.current_player;

            if (AppState.roomName) setGameRoomName(AppState.roomName);

            if (state.finished) {
                ChallengeUI.stopCountdown(); TurnTimerUI._stop();
                localStorage.removeItem('scrabble-rejoin');
                if (!state.puzzle) GameOver.show();   // a napi feladvány saját eredmény-ablakot kap
            }
        } else {
            WaitingRoom.update();
        }
    },

    onActionResult(data) {
        if (!data.success) {
            showMessage(tServer(data.message), true);
            // Elutasított szavazat esetén a letiltott szavazógombok újra használhatók legyenek
            ChallengeUI.resetButtons();
        } else if (data.own_turn) {
            // Csak a saját lépésünk eredménye ürítse a lerakást (mentés / időtúllépés üzenete nem)
            BoardState.clearPlacement();
            Preview.clear();
            this.renderBoard();
            this.renderHand();
            this.updateButtons();
        }
    },

    build() {
        const board = document.getElementById('board');
        board.innerHTML = '';
        this._prevBoardKeys = null;
        this._prevPlacedKeys = new Set();
        for (let r = 0; r < 15; r++) {
            for (let c = 0; c < 15; c++) {
                const cell = document.createElement('div');
                cell.className = 'cell';
                cell.dataset.row = r;
                cell.dataset.col = c;

                const premium = PREMIUM_MAP[`${r},${c}`];
                if (premium) {
                    cell.classList.add(`premium-${premium}`);
                    const label = document.createElement('span');
                    label.className = 'premium-label';
                    label.innerHTML = PREMIUM_LABELS[premium];
                    cell.appendChild(label);
                }

                cell.addEventListener('click', () => this.onCellClick(r, c));
                board.appendChild(cell);
            }
        }

        if (!BoardState.boardDragInitialized) {
            BoardState.boardDragInitialized = true;
            this._initBoardDrag(board);
        }
    },

    // Húzás közbeni visszajelzés: a szabad mező zöld, a foglalt piros (oda nem lehet lerakni)
    setDropTarget(cell) {
        if (cell === BoardState.lastDropTarget) return;
        this.clearDropTarget();
        if (cell) cell.classList.add(this.isCellOccupied(cell) ? 'drop-invalid' : 'drop-target');
        BoardState.lastDropTarget = cell;
    },

    clearDropTarget() {
        if (BoardState.lastDropTarget) {
            BoardState.lastDropTarget.classList.remove('drop-target', 'drop-invalid');
            BoardState.lastDropTarget = null;
        }
    },

    isCellOccupied(cell) {
        return cell.classList.contains('has-tile');
    },

    // A tábla "húzás közben" állapota: a foglalt mezők jelölve
    setDragging(active) {
        document.getElementById('board').classList.toggle('is-dragging', active);
    },

    _initBoardDrag(board) {
        // A dragenter-t is el kell fogadni (preventDefault), különben a böngésző a törzset teszi meg
        // aktuális céllá, és a mezők nem kapnak dragover eseményt. Foglalt mezőn a dropEffect 'none':
        // a kurzor "tiltott", és a böngésző nem is engedi a lerakást.
        const onDragOver = (e) => {
            const cell = e.target.closest('.cell');
            const occupied = !cell || this.isCellOccupied(cell);
            e.preventDefault();
            e.dataTransfer.dropEffect = occupied ? 'none' : 'move';
            this.setDropTarget(cell);
        };
        board.addEventListener('dragenter', onDragOver);
        board.addEventListener('dragover', onDragOver);

        board.addEventListener('dragleave', (e) => {
            if (!board.contains(e.relatedTarget)) this.clearDropTarget();
        });

        board.addEventListener('drop', (e) => {
            e.preventDefault();
            this.clearDropTarget();
            this.setDragging(false);
            const cell = e.target.closest('.cell');
            if (cell) {
                const r = parseInt(cell.dataset.row);
                const c = parseInt(cell.dataset.col);
                const handIdx = parseInt(e.dataTransfer.getData('text/plain'));
                if (!isNaN(handIdx)) {
                    this.placeTileOnBoard(handIdx, r, c);
                }
            }
        });
    },

    renderBoard() {
        const gs = AppState.gameState;
        if (!gs) return;
        const cells = document.querySelectorAll('#board .cell');
        const hasSelected = BoardState.selectedTileIdx !== null && !AppState.isSpectator;
        const pendingTiles = gs.pending_challenge ? gs.pending_challenge.tiles : [];
        const lastMove = new Set((gs.last_move_tiles || []).map(tile => `${tile.row},${tile.col}`));

        const placedMap = new Map();
        for (const tile of BoardState.placedTiles) placedMap.set(`${tile.row},${tile.col}`, tile);
        const pendingMap = new Map();
        for (const tile of pendingTiles) pendingMap.set(`${tile.row},${tile.col}`, tile);

        const prevKeys = this._prevBoardKeys;
        const prevPlaced = this._prevPlacedKeys;
        const boardKeys = new Set();
        const placedKeys = new Set();

        cells.forEach(cell => {
            const r = parseInt(cell.dataset.row);
            const c = parseInt(cell.dataset.col);
            const key = `${r},${c}`;
            const boardCell = gs.board[r][c];
            const placed = placedMap.get(key);
            const pending = pendingMap.get(key);

            cell.classList.remove('has-tile', 'placed-this-turn', 'can-place', 'pending-challenge-tile',
                                  'long-letter', 'last-move');

            if (pending) {
                cell.classList.add('has-tile', 'pending-challenge-tile');
                cell.innerHTML = `${escapeHtml(pending.letter)}<span class="tile-value">${pending.is_blank ? 0 : (TILE_VALUES[pending.letter] || 0)}</span>`;
            } else if (placed) {
                cell.classList.add('has-tile', 'placed-this-turn');
                cell.innerHTML = `${escapeHtml(placed.letter)}<span class="tile-value">${placed.is_blank ? 0 : (TILE_VALUES[placed.letter] || 0)}</span>`;
                placedKeys.add(key);
                if (!prevPlaced.has(key)) this._animate(cell, 'tile-pop');
            } else if (boardCell) {
                cell.classList.add('has-tile');
                cell.innerHTML = `${escapeHtml(boardCell.letter)}<span class="tile-value">${boardCell.is_blank ? 0 : (TILE_VALUES[boardCell.letter] || 0)}</span>`;
                boardKeys.add(key);
                if (lastMove.has(key)) cell.classList.add('last-move');
                // Más játékos (vagy robot) most lerakott betűje: becsúszik a helyére
                if (prevKeys && !prevKeys.has(key)) this._animate(cell, 'tile-drop');
            } else {
                if (hasSelected) cell.classList.add('can-place');
                const premium = PREMIUM_MAP[key];
                cell.innerHTML = premium ? `<span class="premium-label">${PREMIUM_LABELS[premium]}</span>` : '';
            }

            // Többkarakteres betű (SZ, CS, ZS...): kisebb betűméret, hogy ne érjen az értékszámra
            const tile = pending || placed || boardCell;
            if (tile && tile.letter.length > 1) cell.classList.add('long-letter');
        });

        this._prevBoardKeys = boardKeys;
        this._prevPlacedKeys = placedKeys;
    },

    // Egyszeri animáció-osztály: az animáció végén lekerül
    _animate(el, className) {
        el.classList.remove(className);
        void el.offsetWidth;   // az animáció újraindításához
        el.classList.add(className);
        el.addEventListener('animationend', () => el.classList.remove(className), { once: true });
    },

    // --- Betűtartó ---

    // A kéz megjelenítési sorrendje (keverés / rendezés után is megmarad). A szerver indexei a kulcsok:
    // új kéznél a megmaradt zsetonok megtartják a sorrendjüket, az újak a végére kerülnek.
    _syncHandOrder(hand) {
        const prevLetters = BoardState.handLetters;
        const oldOrder = BoardState.handOrder;
        BoardState.newHandIdx = new Set();

        if (!prevLetters || oldOrder.length === 0) {
            BoardState.handOrder = hand.map((_, i) => i);
        } else if (prevLetters.length !== hand.length || prevLetters.some((l, i) => l !== hand[i])) {
            const pool = new Map();
            hand.forEach((letter, i) => {
                if (!pool.has(letter)) pool.set(letter, []);
                pool.get(letter).push(i);
            });
            const order = [];
            for (const oldIdx of oldOrder) {
                const list = pool.get(prevLetters[oldIdx]);
                if (list && list.length) order.push(list.shift());
            }
            for (const list of pool.values()) {
                for (const i of list) { order.push(i); BoardState.newHandIdx.add(i); }
            }
            BoardState.handOrder = order;
        }
        BoardState.handLetters = hand.slice();
    },

    shuffleHand() {
        const order = BoardState.handOrder;
        for (let i = order.length - 1; i > 0; i--) {
            const j = Math.floor(Math.random() * (i + 1));
            [order[i], order[j]] = [order[j], order[i]];
        }
        BoardState.newHandIdx = new Set();
        this.renderHand();
    },

    sortHand() {
        const hand = BoardState.handLetters || [];
        const rank = (letter) => (letter === '' ? 1000 : ALL_LETTERS.indexOf(letter));
        BoardState.handOrder.sort((a, b) => rank(hand[a]) - rank(hand[b]) || a - b);
        BoardState.newHandIdx = new Set();
        this.renderHand();
    },

    renderHand() {
        const gs = AppState.gameState;
        if (!gs) return;
        const handContainer = document.getElementById('hand');
        if (AppState.isSpectator) { handContainer.innerHTML = ''; return; }
        const myPlayer = gs.players.find(p => p.id === AppState.myPlayerId);
        if (!myPlayer || !myPlayer.hand) return;

        this._syncHandOrder(myPlayer.hand);
        const placedHandIndices = new Set(BoardState.placedTiles.map(tile => tile.handIdx));

        handContainer.innerHTML = '';
        BoardState.handOrder.forEach((idx) => {
            if (placedHandIndices.has(idx)) return;
            const tile = myPlayer.hand[idx];

            const el = document.createElement('div');
            el.className = 'hand-tile' + (tile.length > 1 ? ' long-letter' : '');
            if (tile === '') {
                el.classList.add('blank-tile');
                el.innerHTML = `?<span class="tile-value">0</span>`;
            } else {
                el.innerHTML = `${escapeHtml(tile)}<span class="tile-value">${TILE_VALUES[tile] || 0}</span>`;
            }

            if (BoardState.selectedTileIdx === idx) el.classList.add('selected');
            if (BoardState.exchangeMode && BoardState.exchangeIndices.has(idx)) el.classList.add('exchange-selected');
            if (BoardState.newHandIdx && BoardState.newHandIdx.has(idx)) el.classList.add('tile-in');

            el.draggable = !isTouchDevice;
            el.addEventListener('dragstart', (e) => {
                e.dataTransfer.setData('text/plain', idx.toString());
                BoardState.selectedTileIdx = idx;
                // Késleltetve, hogy a böngésző a húzás-képet még az eredeti (nem áttetsző) zsetonról készítse
                setTimeout(() => el.classList.add('dragging'), 0);
                this.setDragging(true);
            });
            el.addEventListener('dragend', () => {
                el.classList.remove('dragging');
                this.setDragging(false);
                this.clearDropTarget();
            });

            if (isTouchDevice) this._addTouchHandlers(el, idx);

            el.addEventListener('click', () => {
                if (BoardState.exchangeMode) {
                    if (BoardState.exchangeIndices.has(idx)) {
                        BoardState.exchangeIndices.delete(idx);
                    } else {
                        BoardState.exchangeIndices.add(idx);
                    }
                    this.renderHand();
                } else {
                    BoardState.selectedTileIdx = (BoardState.selectedTileIdx === idx) ? null : idx;
                    this.renderHand();
                    this.renderBoard();   // a "szabad mező" jelölések a kijelölés szerint
                }
            });

            handContainer.appendChild(el);
        });
        BoardState.newHandIdx = new Set();
    },

    _addTouchHandlers(el, idx) {
        let touchStartTimer = null;
        let touchMoved = false;
        let longPressTriggered = false;

        el.addEventListener('touchstart', (e) => {
            touchMoved = false;
            longPressTriggered = false;
            touchStartTimer = setTimeout(() => {
                longPressTriggered = true;
                TouchDrag.tileIdx = idx;
                BoardState.selectedTileIdx = idx;
                const touch = e.touches[0];
                TouchDrag.createGhost(el, touch.clientX, touch.clientY);
                el.classList.add('selected', 'dragging');
                this.setDragging(true);
            }, 200);
        }, { passive: true });

        el.addEventListener('touchmove', (e) => {
            touchMoved = true;
            if (TouchDrag.tileIdx !== null) {
                e.preventDefault();
                const touch = e.touches[0];
                TouchDrag.moveGhost(touch.clientX, touch.clientY);
                const rawEl = document.elementFromPoint(touch.clientX, touch.clientY);
                this.setDropTarget(rawEl && rawEl.closest('#board .cell'));
            }
        }, { passive: false });

        el.addEventListener('touchend', (e) => {
            clearTimeout(touchStartTimer);
            if (TouchDrag.tileIdx !== null && touchMoved) {
                const touch = e.changedTouches[0];
                const rawEl = document.elementFromPoint(touch.clientX, touch.clientY);
                const targetCell = rawEl && rawEl.closest('#board .cell');
                if (targetCell && !this.isCellOccupied(targetCell)) {
                    const r = parseInt(targetCell.dataset.row);
                    const c = parseInt(targetCell.dataset.col);
                    this.placeTileOnBoard(TouchDrag.tileIdx, r, c);
                }
                TouchDrag.cleanup();
                this.setDragging(false);
                return;
            }
            TouchDrag.cleanup();
            this.setDragging(false);
        });

        el.addEventListener('touchcancel', () => {
            clearTimeout(touchStartTimer);
            TouchDrag.cleanup();
            this.setDragging(false);
        });
    },

    // --- Pontszámok, információk ---

    renderScoreboard() {
        const gs = AppState.gameState;
        if (!gs) return;
        const board = document.getElementById('scoreboard');
        board.innerHTML = gs.players.map(p => `
            <div class="score-item ${p.id === gs.current_player ? 'active' : ''} ${p.is_bot ? 'bot' : ''}" data-player-id="${escapeHtml(p.id)}">
                <span>${p.is_bot ? '<svg class="icon icon-inline"><use href="#i-robot"/></svg>' : ''}${escapeHtml(p.name)}${p.disconnected ? ` <small class="text-muted">${escapeHtml(t('game.offline'))}</small>` : ''}</span>
                <span>${p.score}</span>
            </div>
        `).join('');

        // Pontszám-felugró: a változás a játékos sorában "+N"-ként felúszik
        const prev = this._prevScores;
        if (prev) {
            for (const p of gs.players) {
                const before = prev[p.id];
                if (before === undefined || before === p.score) continue;
                const row = board.querySelector(`.score-item[data-player-id="${CSS.escape(p.id)}"]`);
                if (!row) continue;
                const diff = p.score - before;
                const pop = document.createElement('span');
                pop.className = 'score-pop ' + (diff >= 0 ? 'score-pop-up' : 'score-pop-down');
                pop.textContent = (diff > 0 ? '+' : '') + diff;
                row.appendChild(pop);
                pop.addEventListener('animationend', () => pop.remove(), { once: true });
            }
        }
        this._prevScores = Object.fromEntries(gs.players.map(p => [p.id, p.score]));
    },

    // Az utolsó akció szövege a felhasználó nyelvén (a szerver szerkezetes adata alapján)
    formatLastAction(gs) {
        const info = gs.last_action_info;
        if (!info) return tServer(gs.last_action) || '';
        const words = (info.words || []).join(', ');
        switch (info.type) {
            case 'place': return t('last.place', { player: info.player, words, score: info.score });
            case 'pending': return t('last.pending', { player: info.player, words, score: info.score });
            case 'exchange': return t('last.exchange', { player: info.player, n: info.count });
            case 'pass': return t('last.pass', { player: info.player });
            case 'rejected': return t('last.rejected', { player: info.player, words });
            case 'skip': return t('last.skip', { player: info.player });
            case 'vote': return t(info.vote === 'accept' ? 'last.vote_accept' : 'last.vote_reject', { player: info.player });
            case 'game_over': return t('last.game_over', { player: info.player, score: info.score });
            case 'game_over_draw': return t('last.game_over_draw', { players: (info.players || []).join(', '), score: info.score });
            case 'withdrawn': return t('last.withdrawn', { player: info.player });
            case 'timeout': return t('last.timeout', { player: info.player });
            case 'resigned': return t('last.resigned', { player: info.player });
            case 'puzzle': return t('last.place', { player: info.player, words, score: info.score });
            case 'save_revert': return t('last.save_revert', { player: info.player });
            default: return tServer(gs.last_action) || '';
        }
    },

    renderGameInfo() {
        const gs = AppState.gameState;
        if (!gs) return;
        document.getElementById('tiles-remaining').textContent = t('game.bag', { n: gs.tiles_remaining });

        const isMyTurn = gs.current_player === AppState.myPlayerId && !AppState.isSpectator;
        const turnEl = document.getElementById('current-turn');
        turnEl.textContent = isMyTurn ? t('game.your_turn') : t('game.turn_of', { name: gs.current_player_name || '?' });
        turnEl.classList.toggle('turn-active', isMyTurn);
        turnEl.classList.toggle('turn-inactive', !isMyTurn);
        document.getElementById('board-zoom-container').classList.toggle('my-turn', isMyTurn && !gs.finished);

        const lastAction = this.formatLastAction(gs);
        if (lastAction) document.getElementById('last-action').textContent = lastAction;
    },

    // Kör váltáskor a kiírás felvillan (és a tábla keretének fénye jelzi, hogy te jössz)
    _animateTurnChange(state) {
        const turnEl = document.getElementById('current-turn');
        if (turnEl) this._animate(turnEl, 'turn-pulse');
    },

    // --- Lépéstörténet ---

    renderHistory() {
        const gs = AppState.gameState;
        const list = document.getElementById('move-history');
        if (!gs || !list) return;
        const history = gs.history || [];
        const sig = `${I18N.lang}:${history.length}:${history.length ? history[history.length - 1].n : 0}`;
        if (sig === this._historySig) return;
        this._historySig = sig;

        list.innerHTML = '';
        if (!history.length) {
            const empty = document.createElement('li');
            empty.className = 'move-history-empty';
            empty.textContent = t('game.history_empty');
            list.appendChild(empty);
            return;
        }
        // A legutóbbi lépés van felül
        for (let i = history.length - 1; i >= 0; i--) {
            const h = history[i];
            const li = document.createElement('li');
            li.className = 'move-history-item move-' + h.type;

            const who = document.createElement('span');
            who.className = 'move-who';
            who.textContent = h.player;
            li.appendChild(who);

            const what = document.createElement('span');
            what.className = 'move-what';
            if (h.type === 'place' || h.type === 'challenge_accept') {
                what.textContent = (h.words || []).join(', ');
                const score = document.createElement('span');
                score.className = 'move-score';
                score.textContent = `+${h.score}`;
                li.appendChild(what);
                li.appendChild(score);
            } else {
                what.textContent = t('history.' + (['exchange', 'pass', 'challenge_reject'].includes(h.type) ? h.type : 'other'));
                li.appendChild(what);
            }
            list.appendChild(li);
        }
    },

    // --- Gombok ---

    updateButtons() {
        const gs = AppState.gameState;
        if (!gs) return;
        const spectator = AppState.isSpectator;
        const isMyTurn = gs.current_player === AppState.myPlayerId && !gs.finished && !spectator;
        const hasPending = !!gs.pending_challenge;

        document.getElementById('btn-place').disabled = !isMyTurn || BoardState.placedTiles.length === 0 || hasPending;
        document.getElementById('btn-exchange').disabled = !isMyTurn || hasPending;
        document.getElementById('btn-pass').disabled = !isMyTurn || hasPending;

        document.getElementById('btn-exchange').textContent =
            BoardState.exchangeMode ? t('game.exchange_n', { n: BoardState.exchangeIndices.size }) : t('game.exchange');

        // Tipp: csak egyedül (nincs másik emberi játékos) elérhető
        const humans = gs.players.filter(p => !p.is_bot).length;
        const hintBtn = document.getElementById('btn-hint');
        const hintsOff = (gs.hint_limit || 0) === 0;
        const hintsLeft = gs.hints_left || 0;
        hintBtn.classList.toggle('hidden', spectator || humans !== 1 || hintsOff);
        hintBtn.disabled = !isMyTurn || hasPending || hintsLeft <= 0;
        hintBtn.querySelector('span').textContent = t('game.hint_n', { n: hintsLeft });
        hintBtn.title = hintsLeft <= 0 ? t('game.hint_none_left') : t('game.hint_title');
        document.getElementById('btn-shuffle').disabled = spectator;
        document.getElementById('btn-sort').disabled = spectator;
    },

    // Megfigyelő mód: a lépés- és chatgombok el vannak rejtve, sáv jelzi a módot
    updateSpectatorUi() {
        const gs = AppState.gameState;
        const spectating = !!AppState.isSpectator;
        document.getElementById('game-screen').classList.toggle('spectating', spectating);
        const banner = document.getElementById('spectator-banner');
        banner.classList.toggle('hidden', !spectating);
        if (spectating && gs) {
            document.getElementById('spectator-banner-text').textContent =
                t('spec.banner', { n: gs.spectator_count || 1 });
        }
    },

    onCellClick(row, col) {
        const gs = AppState.gameState;
        if (!gs || gs.finished || AppState.isSpectator) return;

        // Clicking a placed tile always removes it (even if another tile is selected)
        const placedIdx = BoardState.placedTiles.findIndex(tile => tile.row === row && tile.col === col);
        if (placedIdx !== -1) {
            BoardState.placedTiles.splice(placedIdx, 1);
            this.afterPlacementChange();
            return;
        }

        if (BoardState.selectedTileIdx !== null) {
            this.placeTileOnBoard(BoardState.selectedTileIdx, row, col);
            return;
        }
    },

    // A lerakás módosulása után: újrarajzolás + élő előnézet
    afterPlacementChange() {
        this.renderBoard();
        this.renderHand();
        this.updateButtons();
        Preview.schedule();
    },

    placeTileOnBoard(handIdx, row, col) {
        const gs = AppState.gameState;
        if (!gs || AppState.isSpectator) return;
        const myPlayer = gs.players.find(p => p.id === AppState.myPlayerId);
        if (!myPlayer || !myPlayer.hand) return;

        if (gs.board[row][col] !== null) return;
        if (BoardState.placedTiles.find(pt => pt.row === row && pt.col === col)) return;
        if (BoardState.placedTiles.find(pt => pt.handIdx === handIdx)) return;

        const tile = myPlayer.hand[handIdx];

        if (tile === '') {
            BlankDialog.show(handIdx, row, col);
        } else {
            BoardState.placedTiles.push({ row, col, letter: tile, is_blank: false, handIdx });
            BoardState.selectedTileIdx = null;
            SoundManager.play('tile_place');
            this.afterPlacementChange();
        }
    },

    placeTiles() {
        if (!BoardState.placedTiles.length) return;
        const tiles = BoardState.placedTiles.map(tile => ({
            row: tile.row, col: tile.col, letter: tile.letter, is_blank: tile.is_blank,
        }));
        socket.emit('place_tiles', { tiles });
    },

    toggleExchange() {
        if (BoardState.exchangeMode) {
            if (BoardState.exchangeIndices.size > 0) {
                socket.emit('exchange_tiles', { indices: Array.from(BoardState.exchangeIndices) });
            }
            BoardState.exchangeMode = false;
            BoardState.exchangeIndices.clear();
            this.renderBoard();
            this.renderHand();
            this.updateButtons();
        } else {
            BoardState.placedTiles = [];
            Preview.clear();
            BoardState.selectedTileIdx = null;
            BoardState.exchangeMode = true;
            BoardState.exchangeIndices.clear();
            this.renderBoard();
            this.renderHand();
            this.updateButtons();
            showMessage(t('game.exchange_hint'));
        }
    },

    recall() {
        BoardState.clearPlacement();
        Preview.clear();
        this.renderBoard();
        this.renderHand();
        this.updateButtons();
    },

    // Tipp elhelyezése: a javasolt lerakás a táblára kerül (a játékos még módosíthatja, majd lerakja)
    applyHint(tiles) {
        const gs = AppState.gameState;
        const me = gs && gs.players.find(p => p.id === AppState.myPlayerId);
        if (!me || !me.hand) return;
        BoardState.clearPlacement();
        const used = new Set();
        const placed = [];
        for (const tile of tiles) {
            const wanted = tile.is_blank ? '' : tile.letter;
            const idx = me.hand.findIndex((letter, i) => letter === wanted && !used.has(i));
            if (idx === -1) { BoardState.placedTiles = []; return; }
            used.add(idx);
            placed.push({ row: tile.row, col: tile.col, letter: tile.letter, is_blank: tile.is_blank, handIdx: idx });
        }
        BoardState.placedTiles = placed;
        SoundManager.play('tile_place');
        this.afterPlacementChange();
    },
};

// ===== BETŰTARTÓ HELYE =====
// A játékban a betűtartó a tábla jobb oldalán áll (alapértelmezés) vagy a tábla alatt; a választás az
// eszközön marad (localStorage). A beállítás az asztali gépre és a fekvő tabletre hat: álló telefonon /
// tableten a betűtartó mindig alul van, fekvő telefonon mindig oldalt (ott a hely dönt) — ezt a CSS intézi.

const HandLayout = {
    KEY: 'scrabble-hand-position',
    DEFAULT: 'right',
    POSITIONS: ['right', 'bottom'],
    _value: null,

    get() {
        if (this._value) return this._value;
        let saved = null;
        try { saved = localStorage.getItem(this.KEY); } catch { /* nincs tároló: marad az alapérték */ }
        this._value = this.POSITIONS.includes(saved) ? saved : this.DEFAULT;
        return this._value;
    },

    set(position) {
        if (!this.POSITIONS.includes(position)) return;
        this._value = position;
        try { localStorage.setItem(this.KEY, position); } catch { /* privát mód: a munkamenetre marad */ }
        this.apply();
    },

    toggle() {
        this.set(this.get() === 'right' ? 'bottom' : 'right');
    },

    // A választás a <html> elemre kerül (a CSS ebből dolgozik), a profil választója igazodik hozzá
    apply() {
        const position = this.get();
        document.documentElement.dataset.hand = position;
        document.querySelectorAll('#hand-position .segment').forEach(btn => {
            const on = btn.dataset.position === position;
            btn.classList.toggle('active', on);
            btn.setAttribute('aria-checked', on ? 'true' : 'false');
        });
    },

    init() {
        document.querySelectorAll('#hand-position .segment').forEach(btn => {
            btn.addEventListener('click', () => this.set(btn.dataset.position));
        });
        document.getElementById('btn-hand-position').addEventListener('click', () => this.toggle());
        this.apply();
    },
};

// ===== ÉLŐ ELŐNÉZET =====
// A félkész lerakás szavait és pontszámát a szerver véglegesítés nélkül kiszámolja.

const Preview = {
    _timer: null,
    DELAY_MS: 250,

    schedule() {
        clearTimeout(this._timer);
        const el = document.getElementById('move-preview');
        if (!el) return;
        const gs = AppState.gameState;
        const myTurn = gs && gs.current_player === AppState.myPlayerId && !gs.finished && !AppState.isSpectator;
        if (!BoardState.placedTiles.length || !myTurn) { this.clear(); return; }
        el.classList.add('stale');
        this._timer = setTimeout(() => {
            socket.emit('preview_move', {
                tiles: BoardState.placedTiles.map(tile => ({ row: tile.row, col: tile.col, letter: tile.letter, is_blank: tile.is_blank })),
            });
        }, this.DELAY_MS);
    },

    clear() {
        clearTimeout(this._timer);
        const el = document.getElementById('move-preview');
        if (!el) return;
        el.classList.add('hidden');
        el.classList.remove('stale', 'move-preview-ok', 'move-preview-bad');
        el.replaceChildren();
    },

    onResult(data) {
        const el = document.getElementById('move-preview');
        if (!el || !BoardState.placedTiles.length) return;
        el.classList.remove('stale', 'move-preview-ok', 'move-preview-bad');
        el.replaceChildren();
        if (!data.valid) {
            if (!data.message) { el.classList.add('hidden'); return; }
            el.classList.add('move-preview-bad');
            el.textContent = tServer(data.message);
        } else {
            el.classList.add('move-preview-ok');
            const words = document.createElement('span');
            words.className = 'preview-words';
            words.textContent = data.words.map(w => `${w.word} (${w.score})`).join(', ');
            const total = document.createElement('span');
            total.className = 'preview-total';
            total.textContent = `+${data.score}`;
            el.appendChild(words);
            el.appendChild(total);
        }
        el.classList.remove('hidden');
    },
};

// ===== ZSETONSZÁMLÁLÓ =====
// Mely betűk lehetnek még a zsákban vagy az ellenfelek kezében (a táblán és a saját kezedben
// lévők kivételével).

const Tracker = {
    unseen() {
        const gs = AppState.gameState;
        const counts = { ...TILE_COUNTS };
        if (!gs) return { counts, total: 0, bag: 0, hands: 0 };
        const take = (letter, isBlank) => {
            const key = isBlank ? '' : letter;
            if (counts[key] > 0) counts[key]--;
        };
        for (const row of gs.board) for (const cell of row) if (cell) take(cell.letter, cell.is_blank);
        if (gs.pending_challenge) for (const tile of gs.pending_challenge.tiles) take(tile.letter, tile.is_blank);
        const me = gs.players.find(p => p.id === AppState.myPlayerId);
        if (me && me.hand) for (const letter of me.hand) take(letter, letter === '');
        const hands = gs.players.filter(p => p.id !== AppState.myPlayerId).reduce((n, p) => n + p.hand_count, 0);
        const total = Object.values(counts).reduce((a, b) => a + b, 0);
        return { counts, total, bag: gs.tiles_remaining, hands };
    },

    show() {
        this.render();
        document.getElementById('tracker-dialog').classList.remove('hidden');
    },

    render() {
        const { counts, total, bag, hands } = this.unseen();
        const vowels = Object.entries(counts).filter(([l, n]) => l && 'AÁEÉIÍOÓÖŐUÚÜŰ'.includes(l[0])).reduce((a, [, n]) => a + n, 0);
        const blanks = counts[''] || 0;
        const consonants = total - vowels - blanks;
        document.getElementById('tracker-summary').textContent =
            t('tracker.summary', { total, bag, hands, vowels, consonants, blanks });

        const grid = document.getElementById('tracker-grid');
        grid.innerHTML = '';
        const letters = [...ALL_LETTERS, ''];
        for (const letter of letters) {
            const n = counts[letter] || 0;
            const cell = document.createElement('div');
            cell.className = 'tracker-tile' + (n === 0 ? ' gone' : '') + (n === 1 ? ' last' : '');
            const face = document.createElement('span');
            face.className = 'tracker-letter' + (letter.length > 1 ? ' long-letter' : '');
            face.textContent = letter === '' ? '?' : letter;
            const badge = document.createElement('span');
            badge.className = 'tracker-count';
            badge.textContent = n;
            cell.appendChild(face);
            cell.appendChild(badge);
            cell.title = `${letter === '' ? t('tracker.blank') : letter}: ${n} / ${TILE_COUNTS[letter]}`;
            grid.appendChild(cell);
        }
    },

    init() {
        document.getElementById('btn-close-tracker').addEventListener('click', () => {
            document.getElementById('tracker-dialog').classList.add('hidden');
        });
    },
};

// ===== TIPP =====
// Egyedül (robotok ellen) játszva a legjobb lépések kérhetők.

const Hint = {
    request() {
        const btn = document.getElementById('btn-hint');
        btn.disabled = true;
        socket.emit('request_hint');
        setTimeout(() => GameBoard.updateButtons(), 4000);
    },

    onResult(data) {
        GameBoard.updateButtons();
        const dialog = document.getElementById('hint-dialog');
        const list = document.getElementById('hint-list');
        const text = document.getElementById('hint-text');
        list.innerHTML = '';
        if (!data.success) {
            showMessage(tServer(data.message), true);
            return;
        }
        text.textContent = data.moves.length
            ? `${t('hint.intro')} ${t('hint.left', { n: data.hints_left })}`
            : t('hint.none');
        data.moves.forEach((move, i) => {
            const row = document.createElement('div');
            row.className = 'hint-item';
            const info = document.createElement('div');
            info.className = 'hint-info';
            const words = document.createElement('div');
            words.className = 'hint-words';
            words.textContent = move.words.join(', ');
            const score = document.createElement('div');
            score.className = 'hint-score';
            score.textContent = t('common.points', { n: move.score });
            info.appendChild(words);
            info.appendChild(score);
            const btn = document.createElement('button');
            btn.className = 'small-btn';
            btn.textContent = t('hint.place');
            btn.addEventListener('click', () => {
                dialog.classList.add('hidden');
                GameBoard.applyHint(move.tiles);
            });
            row.appendChild(info);
            row.appendChild(btn);
            list.appendChild(row);
        });
        dialog.classList.remove('hidden');
    },

    init() {
        document.getElementById('btn-close-hint').addEventListener('click', () => {
            document.getElementById('hint-dialog').classList.add('hidden');
        });
    },
};

// ===== GYORSBILLENTYŰK =====
// Enter: lerak · Esc: visszavon · S: keverés · R: rendezés · Backspace / Ctrl+Z: az utolsó lerakott betű visszavétele

const Shortcuts = {
    init() {
        document.addEventListener('keydown', (e) => {
            if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && (e.key === 'z' || e.key === 'Z')) {
                const tag = e.target && e.target.tagName;
                if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return;
                if (!document.getElementById('game-screen').classList.contains('hidden')
                        && !AppState.isSpectator && BoardState.placedTiles.length) {
                    e.preventDefault();
                    BoardState.placedTiles.pop();
                    GameBoard.afterPlacementChange();
                }
                return;
            }
            if (e.ctrlKey || e.metaKey || e.altKey) return;
            if (document.getElementById('game-screen').classList.contains('hidden')) return;
            const target = e.target;
            if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.tagName === 'SELECT')) return;
            // Fókuszált gombon az Enter/Space magától kattint: ne duplázzuk a műveletet
            if (target && target.tagName === 'BUTTON' && (e.key === 'Enter' || e.key === ' ')) return;
            // Nyitott párbeszédablak mellett nem kezeljük
            if (document.querySelector('.dialog:not(.hidden)')) return;
            const gs = AppState.gameState;
            if (!gs || AppState.isSpectator) return;

            switch (e.key) {
                case 'Enter':
                    if (!document.getElementById('btn-place').disabled) { e.preventDefault(); GameBoard.placeTiles(); }
                    break;
                case 'Escape':
                    if (BoardState.placedTiles.length || BoardState.selectedTileIdx !== null || BoardState.exchangeMode) {
                        e.preventDefault();
                        GameBoard.recall();
                    }
                    break;
                case 'Backspace':
                    if (BoardState.placedTiles.length) {
                        e.preventDefault();
                        BoardState.placedTiles.pop();
                        GameBoard.afterPlacementChange();
                    }
                    break;
                case 's': case 'S':
                    e.preventDefault();
                    GameBoard.shuffleHand();
                    break;
                case 'r': case 'R':
                    e.preventDefault();
                    GameBoard.sortHand();
                    break;
                default:
            }
        });
    },
};

// ===== BLANK TILE DIALOG =====

const BlankDialog = {
    show(handIdx, row, col) {
        const dialog = document.getElementById('blank-dialog');
        const container = document.getElementById('blank-letters');
        container.innerHTML = '';

        ALL_LETTERS.forEach(l => {
            const btn = document.createElement('button');
            btn.textContent = l;
            btn.addEventListener('click', () => {
                BoardState.placedTiles.push({ row, col, letter: l, is_blank: true, handIdx });
                BoardState.selectedTileIdx = null;
                dialog.classList.add('hidden');
                SoundManager.play('tile_place');
                GameBoard.afterPlacementChange();
            });
            container.appendChild(btn);
        });

        const cancelBtn = document.createElement('button');
        cancelBtn.className = 'secondary blank-cancel-btn';
        cancelBtn.textContent = t('common.cancel');
        cancelBtn.addEventListener('click', () => {
            dialog.classList.add('hidden');
        });
        container.appendChild(cancelBtn);

        dialog.classList.remove('hidden');
    },
};

// ===== TURN TIMER UI =====

const TurnTimerUI = {
    _interval: null,
    _expiresAt: null,
    _warningSoundPlayed: false,

    update(gs) {
        const el = document.getElementById('turn-timer-display');
        if (!el) return;

        if (!gs.turn_time_limit || !gs.turn_timer_expires_at || gs.finished) {
            el.classList.add('hidden');
            this._stop();
            return;
        }

        const newExpiresAt = gs.turn_timer_expires_at * 1000; // s → ms
        // A figyelmeztető hang csak új visszaszámlálásnál szólaljon meg újra,
        // ne minden game_state frissítésnél az utolsó 10 másodpercben.
        if (newExpiresAt !== this._expiresAt) this._warningSoundPlayed = false;
        this._expiresAt = newExpiresAt;
        el.classList.remove('hidden');
        this._stop();
        this._interval = setInterval(() => this._tick(el), 250);
        this._tick(el);
    },

    _tick(el) {
        const remaining = Math.max(0, Math.ceil((this._expiresAt - Date.now()) / 1000));
        el.querySelector('.turn-timer-seconds').textContent = remaining;
        const isWarning = remaining <= 10 && remaining > 0;
        el.classList.toggle('turn-timer-warning', isWarning);
        if (isWarning && !this._warningSoundPlayed) {
            const gs = AppState.gameState;
            if (gs && gs.current_player === AppState.myPlayerId) {
                SoundManager.play('your_turn');
            }
            this._warningSoundPlayed = true;
        }
        if (remaining <= 0) this._stop();
    },

    _stop() {
        if (this._interval) { clearInterval(this._interval); this._interval = null; }
    },
};

// ===== CHALLENGE UI =====

const ChallengeUI = {
    timer: null,
    timeLeft: 0,
    wasVotingPhase: false,
    _buttonsSig: null,   // melyik gombkészlet látszik (ne épüljön újra minden másodpercben)

    init() {
        socket.on('challenge_result', (data) => {
            showMessage(tServer(data.message), false);
            this.stopCountdown();
            SoundManager.play(data.challenge_won ? 'challenge_reject' : 'challenge_accept');
        });
    },

    startCountdown(expiresAt) {
        this.stopCountdown();
        if (expiresAt) {
            this.timeLeft = Math.max(0, Math.ceil((expiresAt * 1000 - Date.now()) / 1000));
        } else {
            this.timeLeft = 30;
        }
        this.timer = setInterval(() => {
            this.timeLeft--;
            if (this.timeLeft <= 0) this.stopCountdown();
            this.render();
        }, 1000);
    },

    resetButtons() {
        this._buttonsSig = null;
        if (AppState.gameState && AppState.gameState.pending_challenge) this.render();
    },

    stopCountdown() {
        if (this.timer) {
            clearInterval(this.timer);
            this.timer = null;
        }
        this.timeLeft = 0;
    },

    render() {
        const gs = AppState.gameState;
        if (!gs) return;
        const section = document.getElementById('challenge-section');
        const infoEl = document.getElementById('challenge-info');
        const timerEl = document.getElementById('challenge-timer');
        const buttonsEl = document.getElementById('challenge-buttons');

        if (!gs.pending_challenge) {
            section.classList.add('hidden');
            this._buttonsSig = null;
            if (this.timer) this.stopCountdown();
            return;
        }

        section.classList.remove('hidden');
        const pc = gs.pending_challenge;
        const isMyPlacement = pc.player_id === AppState.myPlayerId;
        const myVote = (pc.votes || {})[AppState.myPlayerId];

        // Timer management + sound on new challenge appearing
        if (!this.timer) {
            this.startCountdown(pc.expires_at);
            if (!isMyPlacement && !AppState.isSpectator) SoundManager.play('vote');
            // Telefonon a panel görgethető: új szavazásnál a tetejére ugrik, hogy a gombok látsszanak
            const panel = document.querySelector('.side-panel');
            if (panel) panel.scrollTop = 0;
        } else if (pc.expires_at) {
            // Re-sync with server timestamp on each game_state update
            this.timeLeft = Math.max(0, Math.ceil((pc.expires_at * 1000 - Date.now()) / 1000));
        }

        // Info
        this._renderInfo(infoEl, pc, gs);

        // Timer display
        timerEl.textContent = this.timeLeft > 0 ? t('challenge.seconds', { n: this.timeLeft }) : '';

        // Buttons — csak állapotváltozáskor építjük újra: a másodpercenkénti újrarajzolás
        // elnyelhette a kattintást és visszakapcsolta a már letiltott gombokat
        const canWithdraw = isMyPlacement && !Object.keys(pc.votes || {}).length;
        const sig = (AppState.isSpectator ? 'spectator:' : '') + I18N.lang + ':' +
            (isMyPlacement ? (canWithdraw ? 'placer' : 'placer-voted') : (myVote ? `voted:${myVote}` : 'vote'));
        if (this._buttonsSig !== sig) {
            this._buttonsSig = sig;
            buttonsEl.innerHTML = '';
            if (AppState.isSpectator) {
                this._addWaitText(buttonsEl, t('challenge.spectator'));
            } else if (isMyPlacement) {
                this._addWaitText(buttonsEl, t('challenge.voting'));
                if (!Object.keys(pc.votes || {}).length) this._renderWithdrawButton(buttonsEl);
            } else if (myVote) {
                this._addWaitText(buttonsEl, myVote === 'accept' ? t('challenge.you_accepted') : t('challenge.you_rejected'));
            } else {
                this._renderVoteButtons(buttonsEl);
            }
        }
    },

    _renderInfo(infoEl, pc, gs) {
        infoEl.replaceChildren();
        const infoText = document.createElement('div');
        infoText.appendChild(document.createTextNode(`${pc.player_name}: `));
        
        pc.words.forEach((word, index) => {
            const link = document.createElement('a');
            const query = `${word} - Kézikönyvtár A magyar nyelv értelmező szótára`;  // a szótár magyar marad
            link.href = `https://www.google.com/search?q=${encodeURIComponent(query)}`;
            link.target = '_blank';
            link.rel = 'noopener noreferrer';
            link.className = 'dict-link';
            link.textContent = word;
            link.title = t('challenge.search_word', { word });
            
            infoText.appendChild(link);
            
            if (index < pc.words.length - 1) {
                infoText.appendChild(document.createTextNode(', '));
            }
        });
        
        infoText.appendChild(document.createTextNode(` (${t('common.points', { n: pc.score })})`));
        infoEl.appendChild(infoText);

        // A szavazók állása, ha több szavazó van (a robotok nem szavaznak)
        const voters = gs.players.filter(p => !p.is_bot && p.id !== pc.player_id);
        if (voters.length > 1) {
            const voteList = document.createElement('div');
            voteList.className = 'vote-list';
            const votes = pc.votes || {};
            for (const player of voters) {
                const vote = votes[player.id];
                const item = document.createElement('span');
                item.className = 'vote-item';
                if (vote === 'accept') {
                    item.textContent = `${player.name}: ${t('challenge.accept_short')}`;
                    item.classList.add('vote-accept');
                } else if (vote === 'reject') {
                    item.textContent = `${player.name}: ${t('challenge.reject_short')}`;
                    item.classList.add('vote-reject');
                } else {
                    item.textContent = `${player.name}: ...`;
                    item.classList.add('vote-pending');
                }
                voteList.appendChild(item);
            }
            infoEl.appendChild(voteList);
        }
    },

    _addWaitText(container, text) {
        const el = document.createElement('div');
        el.className = 'challenge-wait';
        el.textContent = text;
        container.appendChild(el);
    },

    // A lerakó visszavonhatja a lerakását, amíg senki sem szavazott
    _renderWithdrawButton(buttonsEl) {
        const btn = document.createElement('button');
        btn.className = 'secondary';
        btn.textContent = t('challenge.withdraw');
        btn.addEventListener('click', () => {
            btn.disabled = true;
            socket.emit('withdraw_words');
        });
        buttonsEl.appendChild(btn);
    },

    _renderVoteButtons(buttonsEl) {
        const acceptBtn = document.createElement('button');
        acceptBtn.className = 'btn-accept';
        acceptBtn.textContent = t('challenge.accept');
        const rejectBtn = document.createElement('button');
        rejectBtn.className = 'btn-challenge';
        rejectBtn.textContent = t('challenge.reject');

        acceptBtn.addEventListener('click', () => {
            acceptBtn.disabled = true;
            rejectBtn.disabled = true;
            SoundManager.play('vote');
            socket.emit('accept_words');
        });
        rejectBtn.addEventListener('click', () => {
            acceptBtn.disabled = true;
            rejectBtn.disabled = true;
            SoundManager.play('vote');
            socket.emit('reject_words');
        });

        buttonsEl.appendChild(acceptBtn);
        buttonsEl.appendChild(rejectBtn);
    },
};

// ===== CHAT =====

const Chat = {
    init() {
        document.getElementById('btn-send-chat').addEventListener('click', () => this.send());
        document.getElementById('chat-input').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') this.send();
        });

        socket.on('chat_message', (msg) => this.onMessage(msg));
    },

    clear() {
        AppState.chatMessages = [];
        const container = document.getElementById('chat-messages');
        if (container) container.innerHTML = '';
    },

    send() {
        const input = document.getElementById('chat-input');
        const message = input.value.trim();
        if (!message) return;
        socket.emit('send_chat', { message });
        input.value = '';
    },

    // Látszik-e a chat ablak a képernyőn, és nem takarja-e el valami (pl. a ragadós gombsor)?
    _isVisible() {
        const el = document.getElementById('chat-messages');
        if (!el || el.offsetParent === null) return false;
        const r = el.getBoundingClientRect();
        if (r.width === 0 || r.bottom <= 0 || r.top >= window.innerHeight) return false;
        const x = r.left + r.width / 2;
        const y = Math.min(Math.max(r.top + r.height / 2, 1), window.innerHeight - 1);
        const hit = document.elementFromPoint(x, y);
        return !!hit && el.contains(hit);
    },

    onMessage(msg, skipSound = false) {
        AppState.chatMessages.push(msg);
        const fromOther = !msg.sid || msg.sid !== socket.id;
        if (!skipSound && fromOther) {
            SoundManager.play('chat');
            // Telefonon a chat gyakran a képernyőn kívül van: ilyenkor értesítésként is megjelenik
            if (!this._isVisible()) {
                const text = String(msg.message);
                showMessage(`${msg.name}: ${text.length > 80 ? text.slice(0, 80) + '…' : text}`);
            }
        }
        const container = document.getElementById('chat-messages');
        if (!container) return;

        if (AppState.chatMessages.length > 100) {
            AppState.chatMessages.shift();
            if (container.firstChild) container.removeChild(container.firstChild);
        }

        this._appendMsg(container, msg);
        container.scrollTop = container.scrollHeight;
    },

    _appendMsg(container, msg) {
        const msgEl = document.createElement('div');
        msgEl.className = 'chat-msg';

        const nameSpan = document.createElement('span');
        nameSpan.className = 'chat-name';
        nameSpan.textContent = msg.name + ': ';
        msgEl.appendChild(nameSpan);

        const textSpan = document.createElement('span');
        textSpan.textContent = msg.message;
        msgEl.appendChild(textSpan);

        container.appendChild(msgEl);
    },
};

// ===== BOARD ZOOM (pinch-to-zoom) =====

const BoardZoom = {
    scale: 1,
    translateX: 0,
    translateY: 0,
    initialized: false,

    init() {
        if (this.initialized) return;
        const container = document.getElementById('board-zoom-container');
        const board = document.getElementById('board');
        const resetBtn = document.getElementById('board-zoom-reset');
        if (!container || !board || !isTouchDevice) return;
        this.initialized = true;

        let initialPinchDist = 0;
        let initialScale = 1;
        let isPinching = false;
        let isPanning = false;
        let didPan = false;
        let panLastX = 0, panLastY = 0;
        let panStartX = 0, panStartY = 0;
        let pinchMidX = 0, pinchMidY = 0;

        const getTouchDist = (t) => {
            const dx = t[0].clientX - t[1].clientX;
            const dy = t[0].clientY - t[1].clientY;
            return Math.sqrt(dx * dx + dy * dy);
        };

        const clamp = () => {
            const maxX = container.offsetWidth * (this.scale - 1);
            const maxY = container.offsetHeight * (this.scale - 1);
            this.translateX = Math.min(0, Math.max(-maxX, this.translateX));
            this.translateY = Math.min(0, Math.max(-maxY, this.translateY));
        };

        const applyTransform = () => {
            board.style.transform = `translate(${this.translateX}px, ${this.translateY}px) scale(${this.scale})`;
            resetBtn.classList.toggle('hidden', this.scale <= 1.01);
        };

        const resetZoom = () => {
            this.scale = 1;
            this.translateX = 0;
            this.translateY = 0;
            applyTransform();
        };

        resetBtn.addEventListener('click', (e) => {
            e.preventDefault();
            e.stopPropagation();
            resetZoom();
        });

        container.addEventListener('touchstart', (e) => {
            if (TouchDrag.tileIdx !== null) return;

            if (e.touches.length === 2) {
                isPinching = true;
                isPanning = false;
                initialPinchDist = getTouchDist(e.touches);
                initialScale = this.scale;
                const rect = container.getBoundingClientRect();
                pinchMidX = ((e.touches[0].clientX + e.touches[1].clientX) / 2) - rect.left;
                pinchMidY = ((e.touches[0].clientY + e.touches[1].clientY) / 2) - rect.top;
                e.preventDefault();
            } else if (e.touches.length === 1 && this.scale > 1.01) {
                isPanning = true;
                didPan = false;
                panStartX = e.touches[0].clientX;
                panStartY = e.touches[0].clientY;
                panLastX = panStartX;
                panLastY = panStartY;
            }
        }, { passive: false });

        container.addEventListener('touchmove', (e) => {
            if (TouchDrag.tileIdx !== null) return;

            if (isPinching && e.touches.length === 2) {
                e.preventDefault();
                const dist = getTouchDist(e.touches);
                const newScale = Math.min(Math.max(initialScale * (dist / initialPinchDist), 1), 3.5);

                const ratio = newScale / this.scale;
                this.translateX = pinchMidX - ratio * (pinchMidX - this.translateX);
                this.translateY = pinchMidY - ratio * (pinchMidY - this.translateY);
                this.scale = newScale;

                clamp();
                applyTransform();
            } else if (isPanning && e.touches.length === 1 && this.scale > 1.01) {
                const dx = e.touches[0].clientX - panStartX;
                const dy = e.touches[0].clientY - panStartY;
                if (!didPan && (Math.abs(dx) > 6 || Math.abs(dy) > 6)) didPan = true;
                if (didPan) {
                    e.preventDefault();
                    this.translateX += e.touches[0].clientX - panLastX;
                    this.translateY += e.touches[0].clientY - panLastY;
                    panLastX = e.touches[0].clientX;
                    panLastY = e.touches[0].clientY;
                    clamp();
                    applyTransform();
                }
            }
        }, { passive: false });

        container.addEventListener('touchend', (e) => {
            if (e.touches.length < 2) isPinching = false;
            if (e.touches.length === 0) {
                if (didPan) e.preventDefault();
                isPanning = false;
            }
            if (this.scale < 1.05 && !isPinching) resetZoom();
        }, { passive: false });
    },
};

// ===== GAME OVER =====

const GameOver = {
    _rating: null,
    _savedGameId: null,   // a befejezett játék adatbázis-azonosítója (elemzéshez, visszajátszáshoz)

    setSavedGame(gameId) {
        this._savedGameId = gameId || null;
        document.getElementById('btn-final-analysis').classList.toggle('hidden', !this._savedGameId);
    },

    init() {
        document.getElementById('btn-final-analysis').addEventListener('click', () => {
            if (this._savedGameId) Replay.load(this._savedGameId, { analysis: true });
        });
        document.getElementById('btn-back-lobby').addEventListener('click', () => this.backToLobby());
        window.addEventListener('langchange', () => {
            if (!document.getElementById('game-over-dialog').classList.contains('hidden')) this.render();
        });
    },

    show() {
        if (AppState.gameOverShown) return;
        AppState.gameOverShown = true;
        SoundManager.play('game_over');
        this.render();
        document.getElementById('game-over-dialog').classList.remove('hidden');
    },

    // Az értékelt játék után az új értékszám és a változás
    setRating(data) {
        this._rating = data;
        this.renderRating();
    },

    renderRating() {
        const box = document.getElementById('final-rating');
        const data = this._rating;
        box.classList.toggle('hidden', !data);
        if (!data) return;
        const sign = data.change > 0 ? '+' : '';
        box.textContent = t('game.rating_update', { rating: data.rating, change: sign + data.change });
        box.classList.toggle('up', data.change > 0);
        box.classList.toggle('down', data.change < 0);
    },

    // Végeredmény + játékonkénti érdekességek (legjobb szó, legtöbb pont egy lépésben)
    render() {
        const gs = AppState.gameState;
        if (!gs) return;
        const scoresContainer = document.getElementById('final-scores');
        // A győztesek a szerver szerint (döntetlennél több is; aki feladta, nem lehet az); régi állapotban a legtöbb pont
        const serverWinners = new Set((gs.winners || []).map(w => w.name));
        const topScore = gs.players.length ? Math.max(...gs.players.map(p => p.score)) : 0;
        const isWinner = (p) => (serverWinners.size ? serverWinners.has(p.name) : p.score === topScore);
        const sorted = [...gs.players].sort((a, b) => (isWinner(b) - isWinner(a)) || (b.score - a.score));
        scoresContainer.innerHTML = sorted.map((p) => `
            <div class="score-final ${isWinner(p) ? 'winner' : ''}">
                <span>${isWinner(p) ? '&#x1F3C6; ' : ''}${escapeHtml(p.name)}${p.resigned ? ' <small>(' + escapeHtml(t('async.resigned_tag')) + ')</small>' : ''}</span>
                <span>${escapeHtml(t('common.points', { n: p.score }))}</span>
            </div>
        `).join('');

        const stats = document.getElementById('final-stats');
        stats.replaceChildren();
        const best = this.bestMove(gs.history || []);
        const addLine = (text) => {
            const line = document.createElement('div');
            line.className = 'final-stat';
            line.textContent = text;
            stats.appendChild(line);
        };
        Badges.renderGameOver();
        this.renderRating();
        if (best) addLine(t('game.best_move', { player: best.player, words: best.words.join(', '), score: best.score }));
        const moves = (gs.history || []).filter(h => h.type === 'place' || h.type === 'challenge_accept').length;
        if (moves) addLine(t('game.total_moves', { n: (gs.history || []).length, words: moves }));
    },

    // A legtöbb pontot érő lerakás
    bestMove(history) {
        let best = null;
        for (const h of history) {
            if ((h.type === 'place' || h.type === 'challenge_accept') && (!best || h.score > best.score)) best = h;
        }
        return best;
    },

    backToLobby() {
        document.getElementById('game-over-dialog').classList.add('hidden');
        this._rating = null;
        this.setSavedGame(null);
        const spectating = AppState.isSpectator;
        AppState.reset();
        ChallengeUI.stopCountdown(); TurnTimerUI._stop();
        socket.emit(spectating ? 'leave_spectate' : 'leave_room');
        showScreen('lobby-screen');
        socket.emit('get_rooms');
    },
};

// ===== RECONNECTION =====

const Reconnection = {
    init() {
        socket.on('connect', async () => {
            const banner = document.getElementById('connection-banner');
            if (banner) banner.classList.add('hidden');

            // Új kapcsolat = új SID: a szerver nem ismeri a nevünket, újra be kell mutatkozni
            if (AppState.displayName && Lobby._identitySentForSid !== socket.id) {
                await Lobby.sendIdentity();
            }

            if (!AppState.reconnectToken) {
                const saved = localStorage.getItem('scrabble-rejoin');
                if (saved) {
                    try {
                        const info = JSON.parse(saved);
                        if (info.token) AppState.reconnectToken = info.token;
                    } catch { /* ignore */ }
                }
            }

            if (AppState.reconnectToken) {
                socket.emit('rejoin_room', { token: AppState.reconnectToken });
            } else if (AppState.isSpectator) {
                Spectate.resume();
            }
        });

        socket.on('disconnect', () => {
            const banner = document.getElementById('connection-banner');
            if (banner) {
                banner.textContent = t('conn.lost');
                banner.classList.remove('hidden');
            }
        });

        socket.on('connect_error', () => {
            const banner = document.getElementById('connection-banner');
            if (banner) {
                banner.textContent = t('conn.cannot_connect');
                banner.classList.remove('hidden');
            }
        });

        socket.on('rejoin_failed', (data) => {
            AppState.reset();
            ChallengeUI.stopCountdown(); TurnTimerUI._stop();
            showScreen('lobby-screen');
            socket.emit('get_rooms');
            if (data && data.message) showMessage(tServer(data.message), true);
        });

        socket.on('player_disconnected', (data) => {
            showMessage(t('conn.player_disconnected', { name: data.name }), false);
        });

        socket.on('player_reconnected', (data) => {
            showMessage(t('conn.player_reconnected', { name: data.name }), false);
        });

        socket.on('room_disbanded', (data) => {
            AppState.reset();
            ChallengeUI.stopCountdown(); TurnTimerUI._stop();
            localStorage.removeItem('scrabble-rejoin');
            showScreen('lobby-screen');
            socket.emit('get_rooms');
            showMessage(tServer(data.message) || t('conn.room_gone'), true);
        });

        document.addEventListener('visibilitychange', () => {
            if (document.visibilityState === 'visible' && !socket.connected) {
                socket.connect();
            }
        });
    },
};

// ===== EXIT GAME =====

const ExitGame = {
    init() {
        document.getElementById('btn-exit-game').addEventListener('click', () => this.showDialog());
        // Owner buttons
        document.getElementById('btn-exit-save').addEventListener('click', () => this.saveAndLeave());
        document.getElementById('btn-exit-nosave').addEventListener('click', () => this.leave());
        document.getElementById('btn-exit-cancel-owner').addEventListener('click', () => this.hideDialog());
        // Non-owner buttons
        document.getElementById('btn-exit-confirm').addEventListener('click', () => this.leave());
        document.getElementById('btn-exit-cancel').addEventListener('click', () => this.hideDialog());
        // Levelezős játék
        document.getElementById('btn-exit-async-leave').addEventListener('click', () => this.leave());
        document.getElementById('btn-exit-async-cancel').addEventListener('click', () => this.hideDialog());
        document.getElementById('btn-exit-async-resign').addEventListener('click', () => {
            this.hideDialog();
            showConfirm(t('async.resign'), t('async.resign_confirm'), t('async.resign'),
                () => socket.emit('resign_game'));
        });
    },

    showDialog() {
        // Megfigyelőként nincs mit megerősíteni: a kilépés azonnali
        if (AppState.isSpectator) { this._doLeave(); return; }
        // A napi feladványnál nincs mit menteni vagy megerősíteni
        if (AppState.gameState && AppState.gameState.puzzle) { this._doLeave(); return; }
        const gs = AppState.gameState;
        const isActiveGame = gs && gs.started && !gs.finished;
        const isAsync = !!(isActiveGame && gs.async_mode);   // levelezős: kilépés = a játék megmarad
        const showOwner = AppState.isOwner && isActiveGame && !isAsync;

        document.getElementById('exit-dialog-text').textContent =
            isAsync ? t('async.exit_text') : (showOwner ? t('exit.what_to_do') : t('exit.sure'));
        document.getElementById('exit-owner-buttons').classList.toggle('hidden', !showOwner);
        document.getElementById('exit-async-buttons').classList.toggle('hidden', !isAsync);
        document.getElementById('exit-player-buttons').classList.toggle('hidden', showOwner || isAsync);
        document.getElementById('exit-dialog').classList.remove('hidden');
    },

    hideDialog() {
        document.getElementById('exit-dialog').classList.add('hidden');
    },

    saveAndLeave() {
        this.hideDialog();
        socket.emit('save_game');
        // Wait briefly for save confirmation, then leave (pontosan egyszer)
        let done = false;
        const finish = () => {
            if (done) return;
            done = true;
            clearTimeout(fallbackTimer);
            socket.off('action_result', onResult);
            this._doLeave();
        };
        const onResult = (data) => {
            if (data.success) showMessage(t('exit.saved'));
            else showMessage(tServer(data.message) || t('exit.save_error'), true);
            finish();
        };
        socket.on('action_result', onResult);
        // Fallback: leave after 3s even if no response
        const fallbackTimer = setTimeout(finish, 3000);
    },

    leave() {
        this.hideDialog();
        this._doLeave();
    },

    _doLeave() {
        socket.emit(AppState.isSpectator ? 'leave_spectate' : 'leave_room');
        AppState.reset();
        ChallengeUI.stopCountdown(); TurnTimerUI._stop();
        showScreen('lobby-screen');
        socket.emit('get_rooms');
    },
};

// ===== NAPI FELADVÁNY =====
// Naponta egy közös táblaállás és kéz; a játék a szokásos játékképernyőn zajlik (a szerver egyjátékos,
// nem listázott "feladvány-szobát" nyit), egyetlen lerakás után eredményt kapsz.

const Daily = {
    current: null,        // {date, registered, my} a futó feladványról
    _pendingSolution: null,
    _lobbyData: null,

    init() {
        socket.on('daily_started', (data) => this.onStarted(data));
        socket.on('daily_result', (result) => this.onResult(result));
        socket.on('daily_solution', (solution) => this.onSolution(solution));
        document.getElementById('btn-daily-start').addEventListener('click', () => socket.emit('start_daily'));
        document.getElementById('btn-puzzle-retry').addEventListener('click', () => this.retry());
        document.getElementById('btn-puzzle-solution').addEventListener('click', () => this.reveal());
        document.getElementById('btn-puzzle-reveal').addEventListener('click', () => this.confirmReveal());
        document.getElementById('btn-puzzle-exit').addEventListener('click', () => {
            document.getElementById('puzzle-result-dialog').classList.add('hidden');
            ExitGame._doLeave();
        });
        window.addEventListener('langchange', () => {
            if (this._lobbyData) this.render(this._lobbyData);
            if (this.current && AppState.gameState) this.onGameState(AppState.gameState);
        });
    },

    reset() {
        this.current = null;
        this._pendingSolution = null;
        const dialog = document.getElementById('puzzle-result-dialog');
        if (dialog) dialog.classList.add('hidden');
        const screen = document.getElementById('game-screen');
        if (screen) screen.classList.remove('puzzle-mode');
    },

    // --- Lobby ---

    async load() {
        try {
            const res = await fetch('/api/daily');
            const data = await res.json();
            if (!data.success) return;
            this._lobbyData = data;
            this.render(data);
        } catch { /* a lobby többi része enélkül is működik */ }
    },

    render(data) {
        const status = document.getElementById('daily-status');
        const my = data.my;
        if (!my || !my.attempts) {
            status.textContent = my && my.revealed ? t('daily.revealed') : t('daily.not_played');
        } else {
            status.textContent = t('daily.my_best', { score: my.best_score, n: my.attempts })
                + (data.best_score !== null && data.best_score !== undefined
                    ? ' · ' + t('daily.best_known', { score: data.best_score }) : '');
        }
        document.getElementById('btn-daily-start').textContent = my && my.attempts ? t('daily.again') : t('daily.start');

        const y = data.yesterday;
        const yEl = document.getElementById('daily-yesterday');
        yEl.classList.toggle('hidden', !y);
        if (y) {
            yEl.textContent = t('daily.yesterday', { words: y.best_words.join(', '), score: y.best_score });
        }

        const box = document.getElementById('daily-leaderboard');
        box.replaceChildren();
        if (!data.leaderboard.length) {
            box.innerHTML = emptyStateHtml(t('daily.empty'));
            return;
        }
        for (const entry of data.leaderboard) box.appendChild(this._row(entry));
        if (data.me && !data.leaderboard.some(e => e.user_id === data.me.user_id)) {
            const gap = document.createElement('div');
            gap.className = 'lb-gap';
            gap.textContent = '\u22EF';
            box.appendChild(gap);
            box.appendChild(this._row({ ...data.me, is_me: true }));
        }
    },

    _row(entry) {
        const row = document.createElement('div');
        row.className = 'lb-row' + (entry.is_me ? ' me' : '') + (entry.rank <= 3 ? ` top-${entry.rank}` : '');
        const rank = document.createElement('span');
        rank.className = 'lb-rank';
        rank.textContent = entry.rank;
        const info = document.createElement('div');
        info.className = 'lb-info';
        const name = document.createElement('span');
        name.className = 'lb-name';
        name.textContent = entry.display_name + (entry.is_me ? ` (${t('lb.you')})` : '');
        const sub = document.createElement('span');
        sub.className = 'lb-sub';
        sub.textContent = t('daily.attempts', { n: entry.attempts });
        info.appendChild(name);
        info.appendChild(sub);
        const value = document.createElement('span');
        value.className = 'lb-value';
        value.textContent = t('common.points', { n: entry.best_score });
        row.appendChild(rank);
        row.appendChild(info);
        row.appendChild(value);
        return row;
    },

    // --- Játék közben ---

    // A játékképernyő feladvány-módja: a nem releváns elemek rejtve, a banner látszik
    onGameState(state) {
        const puzzle = !!state.puzzle;
        document.getElementById('game-screen').classList.toggle('puzzle-mode', puzzle);
        const banner = document.getElementById('puzzle-banner');
        banner.classList.toggle('hidden', !puzzle);
        if (puzzle) {
            document.getElementById('puzzle-banner-text').textContent = t('daily.banner', { date: state.puzzle.date });
        }
    },

    onStarted(data) {
        this.current = data;
        AppState.roomName = t('daily.title');
        setGameRoomName(AppState.roomName);
        document.getElementById('puzzle-result-dialog').classList.add('hidden');
        if (this._pendingSolution) {
            const tiles = this._pendingSolution;
            this._pendingSolution = null;
            GameBoard.applyHint(tiles);
        } else {
            // Új próbálkozás ugyanabban a szobában (nincs új game_id): tiszta tábla
            GameBoard.recall();
        }
    },

    onResult(r) {
        const body = document.getElementById('puzzle-result-body');
        body.replaceChildren();
        const add = (text, className) => {
            const line = document.createElement('div');
            if (className) line.className = className;
            line.textContent = text;
            body.appendChild(line);
        };
        add(t('daily.result_score', { score: r.score }), 'puzzle-score');
        if (r.is_best) {
            add(t('daily.found_best'), 'puzzle-best');
        } else {
            add(t('daily.result_best', { score: r.best_score, lost: r.best_score - r.score }));
        }
        if (r.recorded) {
            add(t('daily.result_rank', { rank: r.rank, best: r.my_best, n: r.attempts }));
        } else if (this.current && this.current.registered) {
            add(t('daily.not_recorded'));
        } else {
            add(t('daily.guest_note'));
        }
        // A megoldás gomb nem kell, ha megtaláltad a legjobbat
        document.getElementById('btn-puzzle-solution').classList.toggle('hidden', !!r.is_best);
        document.getElementById('puzzle-result-dialog').classList.remove('hidden');
        SoundManager.play(r.is_best ? 'challenge_accept' : 'tile_place');
    },

    retry() {
        document.getElementById('puzzle-result-dialog').classList.add('hidden');
        socket.emit('retry_daily');
    },

    confirmReveal() {
        showConfirm(t('daily.reveal'), t('daily.reveal_confirm'), t('daily.reveal'), () => this.reveal());
    },

    reveal() {
        document.getElementById('puzzle-result-dialog').classList.add('hidden');
        socket.emit('reveal_daily');
    },

    // A megoldás a táblára kerül (a feladvány ranglistás eredménye ezzel lezárul)
    onSolution(solution) {
        showMessage(t('daily.solution', { words: solution.words.join(', '), score: solution.score }), false, 9000);
        if (AppState.gameState && AppState.gameState.finished) {
            this._pendingSolution = solution.tiles;   // először új próbálkozást nyitunk, utána kerül a táblára
            socket.emit('retry_daily');
        } else {
            GameBoard.applyHint(solution.tiles);
        }
    },
};

// ===== KÖZÖS SEGÉDEK (levelezős és gyakorló nézetek) =====

// DOM elem egy sorban: makeEl('div', 'osztály', 'szöveg')
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

const AVATAR_COLORS = ['#0a84ff', '#30b0c7', '#34c759', '#ff9f0a', '#ff6b4a', '#ff375f', '#bf5af2', '#7d7aff'];

// Kerek monogram a névből (a szín a névből képzett, tehát mindig ugyanaz)
function makeAvatar(name, online) {
    const av = makeEl('span', 'avatar', Array.from(name || '?')[0] || '?');
    let hash = 2166136261;                       // FNV-1a: a hasonló nevek is eltérő színt kapnak
    for (const ch of name || '') hash = Math.imul(hash ^ ch.codePointAt(0), 16777619) >>> 0;
    av.style.setProperty('--av', AVATAR_COLORS[hash % AVATAR_COLORS.length]);
    if (online !== undefined) av.appendChild(makeEl('span', 'status-dot' + (online ? ' online' : '')));
    return av;
}

function shuffled(list) {
    const copy = list.slice();
    for (let i = copy.length - 1; i > 0; i--) {
        const j = Math.floor(Math.random() * (i + 1));
        [copy[i], copy[j]] = [copy[j], copy[i]];
    }
    return copy;
}

function formatClock(totalSeconds) {
    const s = Math.max(0, Math.round(totalSeconds));
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
}

// A magyar ábécé sorrendje (a kétjegyű betűk külön betűk): a szólisták rendezéséhez
const HU_ALPHABET = ['A', 'Á', 'B', 'C', 'CS', 'D', 'E', 'É', 'F', 'G', 'GY', 'H', 'I', 'Í', 'J', 'K', 'L', 'LY',
    'M', 'N', 'NY', 'O', 'Ó', 'Ö', 'Ő', 'P', 'R', 'S', 'SZ', 'T', 'TY', 'U', 'Ú', 'Ü', 'Ű', 'V', 'Z', 'ZS'];
const HU_ORDER = Object.fromEntries(HU_ALPHABET.map((letter, i) => [letter, i]));

// Két szomszédos zseton egy-egy betűje kétjegyű betűt adna-e (S + Z = SZ)? A kétjegyű betű csak a saját
// zsetonjával rakható ki (a szerver `tiles.forms_digraph`-jának mása)
const DIGRAPH_TILES = new Set(['CS', 'GY', 'LY', 'NY', 'SZ', 'TY', 'ZS']);
function formsDigraph(first, second) {
    return !!first && first.length === 1 && second.length === 1 && DIGRAPH_TILES.has(first + second);
}

// Szó zsetonokra bontása (a szerver `tokenize_word`-jének mása: a kétjegyű betű egy zseton; a kevesebb
// zsetont használó felbontás nyer, egyenlőségnél a több pontot érő — pl. KÉSZSÉG = K É S ZS É G)
function tokenizeWord(word) {
    const upper = word.toUpperCase();
    const n = upper.length;
    const best = new Array(n + 1).fill(null);       // best[i]: a upper[:i] legjobb felbontása
    best[0] = { count: 0, negScore: 0, parts: [] };
    for (let i = 0; i < n; i++) {
        if (!best[i]) continue;
        for (const size of [1, 2]) {
            const piece = upper.slice(i, i + size);
            if (piece.length !== size || !(piece in TILE_VALUES)) continue;
            const candidate = {
                count: best[i].count + 1,
                negScore: best[i].negScore - TILE_VALUES[piece],
                parts: best[i].parts.concat(piece),
            };
            const current = best[i + size];
            if (!current || candidate.count < current.count
                || (candidate.count === current.count && candidate.negScore < current.negScore)) {
                best[i + size] = candidate;
            }
        }
    }
    return best[n] ? best[n].parts : null;
}

function compareWords(a, b) {
    const ta = tokenizeWord(a) || [a];
    const tb = tokenizeWord(b) || [b];
    for (let i = 0; i < Math.min(ta.length, tb.length); i++) {
        if (ta[i] !== tb[i]) return (HU_ORDER[ta[i]] ?? 99) - (HU_ORDER[tb[i]] ?? 99);
    }
    return ta.length - tb.length;
}

// Szó megjelenítése játékbeli zsetonokkal: <div class="word-tiles"> .qtile * n
function fillWordTiles(box, tiles, state) {
    box.replaceChildren();
    box.style.setProperty('--n', Math.max(tiles.length, 3));
    box.classList.toggle('is-right', state === 'right');
    box.classList.toggle('is-wrong', state === 'wrong');
    tiles.forEach((tile, i) => {
        const node = makeEl('span', 'qtile', tile);
        node.style.setProperty('--i', i);
        node.appendChild(makeEl('small', null, TILE_VALUES[tile] ?? 0));
        box.appendChild(node);
    });
}

function wordChip(word, score, extraClass) {
    const chip = makeEl('span', 'word-chip' + (extraClass ? ' ' + extraClass : ''), word);
    if (score !== undefined && score !== null) chip.appendChild(makeEl('small', null, score));
    return chip;
}

function localDateKey(date = new Date()) {
    return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}-${String(date.getDate()).padStart(2, '0')}`;
}


// ===== LEVELEZŐS JÁTÉKOK =====
// Órák / napok alatt lépő játék barátokkal: a lobby listája a folyamatban lévő játékokat mutatja
// (akinél a sor, az elöl), a játékban a határidő látszik; a szerver értesít, ha rád kerül a sor.
// Új játékot alsó lapon (telefonon) lehet indítani: név, gondolkodási idő, 1–3 barát.

const AsyncGames = {
    games: [],
    hours: 48,
    selected: new Set(),
    MAX_FRIENDS: 3,

    init() {
        document.getElementById('btn-async-new').addEventListener('click', () => this.openNew());
        document.getElementById('btn-close-async-new').addEventListener('click', () => this.closeNew());
        document.getElementById('btn-async-create').addEventListener('click', () => this.create());
        document.querySelectorAll('#async-hours .segment').forEach(btn => btn.addEventListener('click', () => {
            this.hours = Number(btn.dataset.hours);
            this._syncHours();
        }));
        socket.on('async_your_turn', (data) => {
            showMessage(t('async.your_turn_toast', { room: data.room_name }), false, 6000);
            SoundManager.play('your_turn');
            this.load();
        });
        socket.on('async_invited', (data) => {
            showMessage(t('async.invited_toast', { name: data.from_name, room: data.room_name }), false, 6000);
            this.load();
        });
        window.addEventListener('langchange', () => {
            this.render(this.games);
            this.renderFriends();
            if (AppState.gameState) this.onGameState(AppState.gameState);
        });
        setInterval(() => {
            if (AppState.gameState) this.updateDeadline(AppState.gameState);
            if (document.getElementById('lobby-panel-async').classList.contains('active')) this.render(this.games);
        }, 30000);
    },

    onShow() {
        this.load();
        this.render(this.games);
    },

    async load() {
        if (AppState.isGuest) return;
        try {
            const res = await fetch('/api/async/games');
            const data = await res.json();
            if (!data.success) return;
            this.games = data.games;
            this.render(data.games);
            this.updateBadge(data.my_turn_count);
        } catch { /* a lobby többi része enélkül is működik */ }
    },

    updateBadge(count) {
        const badge = document.getElementById('async-badge');
        badge.textContent = count;
        badge.classList.toggle('hidden', !count);
    },

    // "23 ó 40 p" / "2 n 3 ó" / lejárt
    formatRemaining(deadline) {
        const seconds = Math.floor(deadline - Date.now() / 1000);
        if (seconds <= 0) return t('async.overdue');
        const days = Math.floor(seconds / 86400);
        const hours = Math.floor((seconds % 86400) / 3600);
        const minutes = Math.floor((seconds % 3600) / 60);
        if (days) return t('async.remaining_days', { d: days, h: hours });
        return t('async.remaining_hours', { h: hours, m: minutes });
    },

    // --- Lista ---

    render(games) {
        const box = document.getElementById('async-games');
        box.replaceChildren();
        const my = games.filter(g => g.my_turn).length;
        document.getElementById('async-summary').textContent = !games.length ? '' :
            t('async.summary', { n: games.length }) + (my ? ' · ' + t('async.summary_my', { n: my }) : '');
        if (!games.length) {
            box.appendChild(this._emptyState());
            return;
        }
        for (const game of games) box.appendChild(this._card(game));
    },

    _emptyState() {
        const empty = makeEl('div', 'async-empty');
        const icon = makeEl('div', 'async-empty-icon');
        icon.appendChild(makeIcon('mail'));
        const cta = makeEl('button', null, t('async.start_first'));
        cta.type = 'button';
        cta.addEventListener('click', () => this.openNew());
        empty.append(icon, makeEl('h3', null, t('async.empty_title')), makeEl('p', null, t('async.empty')), cta);
        return empty;
    },

    _card(game) {
        const card = makeEl('button', 'async-card' + (game.my_turn ? ' is-my-turn' : ''));
        card.type = 'button';
        card.appendChild(makeAvatar(game.current_player || '?'));

        const main = makeEl('div', 'async-card-main');
        main.appendChild(makeEl('div', 'async-card-title', game.room_name));
        main.appendChild(makeEl('div', 'async-card-scores', game.players.map(p => `${p.name} ${p.score}`).join(' · ')));

        const status = makeEl('div', 'async-card-status');
        status.appendChild(makeEl('span', 'async-pill',
            game.my_turn ? t('async.your_turn') : t('async.waiting_for', { name: game.current_player || '?' })));
        const total = (game.turn_hours || 0) * 3600;
        const remaining = game.turn_deadline ? game.turn_deadline - Date.now() / 1000 : null;
        const urgent = game.my_turn && remaining !== null && (remaining < 6 * 3600 || (total && remaining < total * 0.15));
        if (remaining !== null) {
            status.appendChild(makeEl('span', 'async-time' + (urgent || remaining <= 0 ? ' is-urgent' : ''),
                remaining > 0 ? t('async.time_left', { time: this.formatRemaining(game.turn_deadline) }) : t('async.overdue')));
        }
        main.appendChild(status);
        if (remaining !== null && total) {
            const bar = makeEl('div', 'async-progress' + (urgent ? ' is-urgent' : ''));
            const fill = makeEl('span');
            fill.style.width = `${Math.max(0, Math.min(1, remaining / total)) * 100}%`;
            bar.appendChild(fill);
            main.appendChild(bar);
        }
        card.appendChild(main);
        card.appendChild(makeIcon('chevron')).classList.add('chevron');
        card.addEventListener('click', () => socket.emit('open_async_game', { game_id: game.game_id }));
        return card;
    },

    // --- Új játék (alsó lap) ---

    openNew() {
        this.selected.clear();
        this._syncHours();
        this.renderFriends();
        document.getElementById('async-new-dialog').classList.remove('hidden');
        Friends.load().then(() => this.renderFriends()).catch(() => this.renderFriends());
    },

    closeNew() {
        document.getElementById('async-new-dialog').classList.add('hidden');
    },

    _syncHours() {
        document.querySelectorAll('#async-hours .segment').forEach(btn => {
            const active = Number(btn.dataset.hours) === this.hours;
            btn.classList.toggle('active', active);
            btn.setAttribute('aria-checked', active ? 'true' : 'false');
        });
    },

    renderFriends() {
        const box = document.getElementById('async-friends');
        box.replaceChildren();
        const friends = Friends.friendsList;
        // Már nem létező barát ne maradjon kijelölve
        for (const id of Array.from(this.selected)) if (!friends.some(f => f.id === id)) this.selected.delete(id);
        if (!friends.length) {
            const empty = makeEl('div', 'pick-empty', t('async.no_friends'));
            const go = makeEl('button', 'link-btn', t('async.open_friends'));
            go.type = 'button';
            go.addEventListener('click', () => { this.closeNew(); Lobby.switchTab('friends'); });
            empty.appendChild(document.createElement('br'));
            empty.appendChild(go);
            box.appendChild(empty);
        }
        const full = this.selected.size >= this.MAX_FRIENDS;
        for (const friend of friends) {
            const picked = this.selected.has(friend.id);
            const row = makeEl('label', 'pick-row' + (picked ? ' is-selected' : '') + (full && !picked ? ' is-locked' : ''));
            const input = makeEl('input', 'sr-only');
            input.type = 'checkbox';
            input.checked = picked;
            input.disabled = full && !picked;
            input.addEventListener('change', () => this._toggle(friend.id, input.checked));
            const check = makeEl('span', 'pick-check');
            check.setAttribute('aria-hidden', 'true');
            check.appendChild(makeIcon('check'));
            row.append(input, makeAvatar(friend.display_name, !!friend.online),
                makeEl('span', 'pick-name', friend.display_name), check);
            box.appendChild(row);
        }
        document.getElementById('async-friends-count').textContent = `${this.selected.size}/${this.MAX_FRIENDS}`;
        document.getElementById('btn-async-create').disabled = this.selected.size === 0;
    },

    _toggle(id, on) {
        if (on) {
            if (this.selected.size >= this.MAX_FRIENDS) { showMessage(t('async.max_friends'), true); return; }
            this.selected.add(id);
        } else {
            this.selected.delete(id);
        }
        this.renderFriends();
    },

    selectedFriendIds() {
        return Array.from(this.selected);
    },

    create() {
        const friendIds = this.selectedFriendIds();
        if (!friendIds.length) {
            showMessage(t('async.pick_friends'), true);
            return;
        }
        const name = document.getElementById('async-name').value.trim() || t('async.name_placeholder');
        socket.emit('create_async_game', { name, friend_ids: friendIds, turn_hours: this.hours });
        this.closeNew();
        document.getElementById('async-name').value = '';
    },

    // --- Játék közben: a határidő kijelzése ---

    onGameState(state) {
        this.updateDeadline(state);
    },

    updateDeadline(state) {
        const el = document.getElementById('async-deadline');
        const show = !!(state && state.async_mode && !state.finished && state.turn_deadline);
        el.classList.toggle('hidden', !show);
        if (show) el.textContent = t('async.deadline', { time: this.formatRemaining(state.turn_deadline) });
    },
};


// ===== GYAKORLÁS: tároló (az eszközön, localStorage) =====
// Statisztika, sorozat és a „Hibáim” pakli. Vendégnek is működik; mindez csak a készüléken él.

const PracticeStore = {
    KEY: 'scrabble-practice',
    MAX_MISSED: 200,
    _data: null,

    _fresh() {
        return {
            v: 1,
            days: [],                                   // a gyakorlással töltött napok (YYYY-MM-DD)
            today: { date: '', n: 0 },                  // a mai befejezett gyakorlatok száma
            quiz: { questions: 0, correct: 0, best_streak: 0 },
            hunt: { sessions: 0, best: {} },            // legjobb pontszám időkorlátonként
            bingo: { attempts: 0, solved: 0, streak: 0, best_streak: 0 },
            missed: {},                                 // SZÓ → {tiles, valid, ok}
        };
    },

    get() {
        if (!this._data) {
            let stored = null;
            try { stored = JSON.parse(localStorage.getItem(this.KEY)); } catch { /* sérült / tiltott tároló */ }
            this._data = Object.assign(this._fresh(), stored && typeof stored === 'object' ? stored : {});
        }
        return this._data;
    },

    save() {
        try { localStorage.setItem(this.KEY, JSON.stringify(this._data)); } catch { /* tiltott tároló */ }
    },

    // Egy gyakorlat (kvíz, vadászat, bingó-kéz) befejeződött: napi számláló + sorozat
    touch() {
        const data = this.get();
        const today = localDateKey();
        if (data.today.date !== today) data.today = { date: today, n: 0 };
        data.today.n++;
        if (!data.days.includes(today)) data.days.push(today);
        data.days = data.days.slice(-120);
        this.save();
    },

    todayCount() {
        const data = this.get();
        return data.today.date === localDateKey() ? data.today.n : 0;
    },

    // Egymást követő napok száma (a ma még üres nap nem szakítja meg a tegnapig tartó sorozatot)
    streak() {
        const days = new Set(this.get().days);
        const cursor = new Date();
        if (!days.has(localDateKey(cursor))) cursor.setDate(cursor.getDate() - 1);
        let n = 0;
        while (days.has(localDateKey(cursor))) { n++; cursor.setDate(cursor.getDate() - 1); }
        return n;
    },

    accuracy() {
        const quiz = this.get().quiz;
        return quiz.questions ? Math.round(100 * quiz.correct / quiz.questions) : null;
    },

    // Kvíz-válasz rögzítése: tévesztés → a pakliba; a pakliból a kétszer egymás után helyes válasz töröl
    recordAnswer(word, tiles, valid, correct) {
        const data = this.get();
        data.quiz.questions++;
        if (correct) data.quiz.correct++;
        const entry = data.missed[word];
        if (!correct) {
            delete data.missed[word];                   // a végére kerül (a legrégebbit vágjuk le)
            data.missed[word] = { tiles, valid, ok: 0 };
            const keys = Object.keys(data.missed);
            if (keys.length > this.MAX_MISSED) delete data.missed[keys[0]];
        } else if (entry && ++entry.ok >= 2) {
            delete data.missed[word];
        }
        this.save();
    },

    missedWords() {
        return Object.entries(this.get().missed).map(([word, e]) => ({ word, tiles: e.tiles || tokenizeWord(word) || [] }));
    },
};


// ===== SZÓTÁR-ÉPÍTŐ =====
// Véletlen szavak átnézése: a „Nem szó” döntés szavazatként a szerverre kerül, és a szót kizárja a játék
// szótárából. A szavakat 20-asával kapjuk; a döntés azonnal továbblép, a küldés a háttérben, sorban megy.
// Csak bejelentkezve érhető el (a szavazat mindenki játékát érinti).

const WordBuilder = {
    BATCH: 20,
    PREFETCH_AT: 6,
    userId: null,         // kinek a szavai vannak a sorban (kijelentkezés / fiókváltás után újrakezdjük)
    queue: [],            // [{word, tiles}] a még nem látott szavak
    current: null,
    last: null,           // a legutóbbi döntés a visszavonáshoz: {word, tiles, valid}
    seen: new Set(),      // a munkamenetben már kapott szavak (a kihagyottak nem jönnek vissza)
    stats: null,          // {total, valid, invalid, today}
    status: null,         // {key, params}: az utolsó eseményüzenet (nyelvváltáskor újraformázzuk)
    loading: false,
    exhausted: false,
    _gen: 0,              // fiókváltáskor nő: a régi fiók késői válaszait eldobjuk
    _chain: Promise.resolve(),

    init() {
        document.getElementById('btn-wb-valid').addEventListener('click', () => this.decide(true));
        document.getElementById('btn-wb-invalid').addEventListener('click', () => this.decide(false));
        document.getElementById('btn-wb-skip').addEventListener('click', () => this.skip());
        document.getElementById('btn-wb-undo').addEventListener('click', () => this.undo());
        window.addEventListener('langchange', () => {
            this.renderStatus();
            this.renderMeta();
            if (!this.current) this.renderWord();
        });
    },

    isLoggedIn() {
        return !!(AppState.currentUser && !AppState.isGuest);
    },

    _syncUser() {
        const id = this.isLoggedIn() ? AppState.currentUser.id : null;
        if (id === this.userId) return;
        this.userId = id;
        this._gen++;
        this.queue = [];
        this.current = null;
        this.last = null;
        this.seen = new Set();
        this.stats = null;
        this.status = null;
        this.loading = false;
        this.exhausted = false;
    },

    // --- Nézetek ---

    onHubShow() {
        this._syncUser();
        this.renderMeta();
        this.loadStats();
    },

    async open() {
        this._syncUser();
        const loggedIn = this.isLoggedIn();
        document.getElementById('wb-login').classList.toggle('hidden', loggedIn);
        document.getElementById('wb-play').classList.toggle('hidden', !loggedIn);
        if (!loggedIn) return;
        this.exhausted = false;
        this.renderStats();
        this.renderStatus();
        this.renderWord();
        this.loadStats();
        if (!this.current) await this.fetchBatch();
    },

    onKey(e) {
        if (e.repeat || !this.isLoggedIn()) return;       // a lenyomva tartott billentyű ne döntsön tucatnyi szóról
        if (e.key === 'ArrowLeft') { e.preventDefault(); this.decide(false); }
        else if (e.key === 'ArrowRight') { e.preventDefault(); this.decide(true); }
        else if (e.key === 'ArrowDown') { e.preventDefault(); this.skip(); }
        else if (e.key === 'Backspace') { e.preventDefault(); this.undo(); }
    },

    // --- Szavak betöltése ---

    async fetchBatch() {
        if (this.loading || this.exhausted || !this.isLoggedIn()) return;
        this.loading = true;
        const gen = this._gen;
        try {
            const res = await fetch(`/api/practice/word-review?n=${this.BATCH}`);
            const data = await res.json();
            if (gen !== this._gen) return;
            if (!data.success) {
                showMessage(tServer(data.message) || t('common.load_failed'), true);
                return;
            }
            this.stats = data.stats;
            if (!data.words.length) this.exhausted = true;
            data.words.forEach((word, i) => {
                if (this.seen.has(word)) return;
                this.seen.add(word);
                this.queue.push({ word, tiles: data.tiles[i] });
            });
        } catch {
            if (gen === this._gen) showMessage(t('common.load_failed'), true);
        } finally {
            if (gen === this._gen) this.loading = false;
        }
        if (gen !== this._gen) return;
        if (!this.current) this.advance();
        else this.renderStats();
    },

    async loadStats() {
        if (!this.isLoggedIn()) return;
        const gen = this._gen;
        try {
            const res = await fetch('/api/practice/word-review/stats');
            const data = await res.json();
            if (gen !== this._gen || !data.success) return;
            this.stats = data.stats;
            this.renderStats();
            this.renderMeta();
        } catch { /* a számlálók nélkül is működik */ }
    },

    advance() {
        this.current = this.queue.shift() || null;
        this.renderWord();
        if (this.queue.length < this.PREFETCH_AT) this.fetchBatch();
    },

    // --- Döntések ---

    decide(valid) {
        const entry = this.current;
        if (!entry || !this.isLoggedIn()) return;
        this.last = { ...entry, valid };
        this.current = null;
        if (this.stats) {                                 // azonnali számlálók, a szerver válasza pontosítja
            this.stats.total++;
            this.stats.today++;
            this.stats[valid ? 'valid' : 'invalid']++;
        }
        this.setStatus(valid ? 'wb.last_valid' : 'wb.last_invalid', { word: entry.word }, !valid);
        this.advance();
        this.renderStats();
        this._send('/api/practice/word-review', { word: entry.word, valid }, entry);
    },

    skip() {
        if (!this.current) return;
        this.current = null;
        this.advance();
    },

    undo() {
        const last = this.last;
        if (!last || !this.isLoggedIn()) return;
        this.last = null;
        if (this.current) this.queue.unshift(this.current);
        this.current = { word: last.word, tiles: last.tiles };
        if (this.stats) {
            this.stats.total = Math.max(0, this.stats.total - 1);
            this.stats.today = Math.max(0, this.stats.today - 1);
            const key = last.valid ? 'valid' : 'invalid';
            this.stats[key] = Math.max(0, this.stats[key] - 1);
        }
        this.setStatus('wb.undone', { word: last.word });
        this.renderWord();
        this.renderStats();
        this._send('/api/practice/word-review/undo', { word: last.word }, null);
    },

    // A kéréseket sorban küldjük (a visszavonás a szavazat után érkezzen meg a szerverre)
    _send(url, body, retry) {
        const gen = this._gen;
        this._chain = this._chain.then(async () => {
            if (gen !== this._gen) return;
            try {
                const data = await postJson(url, body);
                if (gen !== this._gen) return;
                if (data.success) {
                    this.stats = data.stats;
                    this.renderStats();
                    return;
                }
                showMessage(tServer(data.message) || t('common.load_failed'), true);
            } catch {
                if (gen !== this._gen) return;
                showMessage(t('common.load_failed'), true);
                if (retry) {                              // a döntés nem ment át: a szó újra sorra kerül
                    this.queue.unshift(retry);
                    if (!this.current) this.advance();
                }
            }
            if (this.last && this.last.word === body.word) { this.last = null; this.renderWord(); }
            this.loadStats();
        });
    },

    // --- Megjelenítés ---

    setStatus(key, params, warn = false) {
        this.status = { key, params, warn };
        this.renderStatus();
    },

    renderStatus() {
        const el = document.getElementById('wb-status');
        el.textContent = this.status ? t(this.status.key, this.status.params) : '';
        el.classList.toggle('is-warn', !!(this.status && this.status.warn));
    },

    renderStats() {
        const stats = this.stats || { total: 0, valid: 0, invalid: 0, today: 0 };
        document.getElementById('wb-stat-today').textContent = stats.today;
        document.getElementById('wb-stat-total').textContent = stats.total;
        document.getElementById('wb-stat-invalid').textContent = stats.invalid;
        this.renderMeta();
    },

    renderMeta() {
        const total = this.stats ? this.stats.total : 0;
        document.getElementById('pmeta-wordbuilder').textContent = total ? t('wb.meta', { n: total }) : '';
    },

    renderWord() {
        const box = document.getElementById('wb-word');
        const entry = this.current;
        if (entry) {
            fillWordTiles(box, entry.tiles);
        } else {
            box.replaceChildren();
            if (this.exhausted) box.appendChild(makeEl('p', 'wb-empty', t('wb.empty')));
        }
        box.setAttribute('aria-busy', entry ? 'false' : 'true');
        const link = document.getElementById('wb-lookup');
        link.classList.toggle('hidden', !entry);
        if (entry) {
            // a szótár magyar marad: a keresés ugyanaz, mint a megtámadásnál és a szótár-böngészőben
            link.href = `https://www.google.com/search?q=${encodeURIComponent(entry.word.toLowerCase() + ' - Kézikönyvtár A magyar nyelv értelmező szótára')}`;
        }
        for (const id of ['btn-wb-valid', 'btn-wb-invalid', 'btn-wb-skip']) {
            document.getElementById(id).disabled = !entry;
        }
        document.getElementById('btn-wb-undo').disabled = !this.last;
    },
};


// ===== GYAKORLÁS =====
// Főoldal (napi feladvány + módok) és al-nézetek: szókvíz, betűvadász, bingó-edző, szólisták.
// A szerver állapotmentes: a kvíz-választ és a kézhez beírt szavakat a játék szótárával bírálja el.

const Practice = {
    view: 'hub',
    quiz: null,             // futó kvíz
    quizMode: 'mixed',
    quizCount: 10,
    lastFeedback: null,
    hunt: null,             // futó betűvadász / bingó
    huntKind: 'hunt',
    huntSeconds: 0,
    lists: { kind: '2', sort: 'abc', letter: '', query: '', data: {} },

    init() {
        document.querySelectorAll('[data-practice-open]').forEach(btn =>
            btn.addEventListener('click', () => this.open(btn.dataset.practiceOpen)));
        document.querySelectorAll('[data-practice-back]').forEach(btn =>
            btn.addEventListener('click', () => this.back()));
        document.getElementById('btn-daily-board').addEventListener('click', () => this.open('daily'));

        // Kvíz
        this._radioGroup('#quiz-modes .choice', 'mode', (v) => { this.quizMode = v; });
        this._radioGroup('#quiz-counts .segment', 'count', (v) => { this.quizCount = Number(v); });
        document.getElementById('btn-quiz-start').addEventListener('click', () => this.startQuiz(this.quizMode));
        document.getElementById('btn-quiz-valid').addEventListener('click', () => this.answer(true));
        document.getElementById('btn-quiz-invalid').addEventListener('click', () => this.answer(false));
        document.getElementById('btn-quiz-next').addEventListener('click', () => this.next());

        // Betűvadász / bingó
        this._radioGroup('#hunt-timers .segment', 'seconds', (v) => { this.huntSeconds = Number(v); this.renderHuntRecord(); });
        document.getElementById('btn-hunt-start').addEventListener('click', () => this.startHunt());
        document.getElementById('btn-hunt-clear').addEventListener('click', () => this.huntClear());
        document.getElementById('btn-hunt-shuffle').addEventListener('click', () => this.huntShuffle());
        document.getElementById('btn-hunt-submit').addEventListener('click', () => this.huntSubmit());
        document.getElementById('btn-hunt-hint').addEventListener('click', () => this.huntHint());
        document.getElementById('btn-hunt-finish').addEventListener('click', () => this.huntFinish());

        // Szólisták
        this._radioGroup('#list-kinds .segment', 'kind', (v) => { this.lists.kind = v; this.lists.letter = ''; this.openList(); });
        this._radioGroup('#list-sorts .segment', 'sort', (v) => { this.lists.sort = v; this.renderList(); });
        document.getElementById('list-search').addEventListener('input', (e) => {
            this.lists.query = e.target.value.trim().toUpperCase();
            this.renderList();
        });

        document.addEventListener('keydown', (e) => this._onKey(e));
        window.addEventListener('langchange', () => this.onLangChange());
    },

    // Kiválasztható gombcsoport: a data-<attr> értéke adja a választást
    _radioGroup(selector, attr, onChange) {
        const buttons = document.querySelectorAll(selector);
        buttons.forEach(btn => btn.addEventListener('click', () => {
            buttons.forEach(b => {
                const on = b === btn;
                b.classList.toggle('active', on);
                b.setAttribute(b.getAttribute('role') === 'tab' ? 'aria-selected' : 'aria-checked', on ? 'true' : 'false');
            });
            onChange(btn.dataset[attr]);
        }));
    },

    onLangChange() {
        this.renderHub();
        const quiz = this.quiz;
        if (quiz && quiz.finished) {
            this.renderQuizResult();
        } else if (quiz && quiz.index < quiz.questions.length) {
            this.renderQuestion(true);
            if (this.lastFeedback) {
                this.renderFeedback(this.lastFeedback);
                document.getElementById('btn-quiz-next').textContent = this._nextLabel();
            }
        }
        if (this.hunt && !this.hunt.finished) this.renderHunt();
        else if (this.hunt && this.hunt.kind === 'hunt') this.renderHuntResult();
        else if (this.hunt) this.renderBingoResult();
        document.getElementById('hunt-heading').textContent = t(this.huntKind === 'bingo' ? 'bingo.title' : 'hunt.title');
        this.renderHuntSetup();
        if (this.lists.data[this.lists.kind] || this.lists.kind === 'tiles') this.openList();
    },

    onShow() {
        this.renderHub();
        WordBuilder.onHubShow();
    },

    // --- Navigáció ---

    _setView(name) {
        this.view = name;
        document.querySelectorAll('.practice-view').forEach(v => v.classList.toggle('active', v.dataset.view === name));
        window.scrollTo({ top: 0 });
    },

    open(what) {
        if (what === 'quiz') {
            this._stopHunt();
            this.quiz = null;
            this.showQuizBlock('setup');
            this._setView('quiz');
        } else if (what === 'mistakes') {
            if (!PracticeStore.missedWords().length) {
                showMessage(t('mistakes.none'), false);
                return;
            }
            this._setView('quiz');
            this.startQuiz('mistakes');
        } else if (what === 'hunt' || what === 'bingo') {
            this._stopHunt();
            this.huntKind = what;
            this.renderHuntSetup();
            this.showHuntBlock('setup');
            this._setView('hunt');
        } else if (what === 'lists') {
            this._stopHunt();
            this._setView('lists');
            this.openList();
        } else if (what === 'daily') {
            Daily.load();
            this._setView('daily');
        } else if (what === 'wordbuilder') {
            this._stopHunt();
            this._setView('wordbuilder');
            WordBuilder.open();
        } else {
            this._stopHunt();
            this._setView('hub');
            this.renderHub();
        }
    },

    back() {
        this._stopHunt();
        this.open('hub');
    },

    // --- Főoldal ---

    renderHub() {
        const data = PracticeStore.get();
        document.getElementById('pstat-streak').textContent = PracticeStore.streak();
        document.getElementById('pstat-today').textContent = PracticeStore.todayCount();
        const accuracy = PracticeStore.accuracy();
        document.getElementById('pstat-accuracy').textContent = accuracy === null ? '–' : accuracy + '%';

        document.getElementById('pmeta-quiz').textContent = data.quiz.questions
            ? t('quiz.meta', { pct: accuracy, n: data.quiz.questions }) : '';
        const best = Math.max(0, ...Object.values(data.hunt.best));
        document.getElementById('pmeta-hunt').textContent = best ? t('hunt.meta', { n: best }) : '';
        document.getElementById('pmeta-bingo').textContent = data.bingo.attempts
            ? t('bingo.meta', { solved: data.bingo.solved, n: data.bingo.attempts }) : '';
        const missed = PracticeStore.missedWords().length;
        document.getElementById('pmeta-mistakes').textContent = missed ? t('mistakes.meta', { n: missed }) : '';
        document.getElementById('practice-mistakes-item').classList.toggle('is-empty', !missed);
        WordBuilder.renderMeta();
    },

    // --- Szókvíz ---

    showQuizBlock(block) {
        document.getElementById('quiz-setup').classList.toggle('hidden', block !== 'setup');
        document.getElementById('quiz-play').classList.toggle('hidden', block !== 'play');
        document.getElementById('quiz-result').classList.toggle('hidden', block !== 'result');
        document.getElementById('quiz-heading').textContent = this.quiz && this.quiz.mode === 'mistakes'
            ? t('mistakes.title') : t('quiz.title');
    },

    async startQuiz(mode) {
        const btn = document.getElementById('btn-quiz-start');
        btn.disabled = true;
        try {
            let questions;
            if (mode === 'mistakes') {
                questions = shuffled(PracticeStore.missedWords()).slice(0, 10);
            } else {
                const res = await fetch(`/api/practice/quiz?n=${this.quizCount}&mode=${encodeURIComponent(mode)}`);
                const data = await res.json();
                if (!data.success || !data.questions.length) {
                    showMessage(tServer(data.message) || t('common.load_failed'), true);
                    return;
                }
                questions = data.questions.map((word, i) => ({ word, tiles: data.tiles[i] }));
            }
            this.quiz = {
                mode, questions, index: 0, correct: 0, streak: 0, bestStreak: 0,
                answered: false, results: [], startedAt: Date.now(), finished: false,
            };
            this.lastFeedback = null;
            this.showQuizBlock('play');
            this.renderQuestion();
        } catch {
            showMessage(t('common.load_failed'), true);
        } finally {
            btn.disabled = false;
        }
    },

    renderQuestion(keepFeedback = false) {
        const quiz = this.quiz;
        if (!quiz || quiz.index >= quiz.questions.length) return;
        const question = quiz.questions[quiz.index];
        document.getElementById('quiz-progress').textContent =
            t('quiz.progress', { n: quiz.index + 1, total: quiz.questions.length });
        document.getElementById('quiz-bar-fill').style.width = `${(quiz.index / quiz.questions.length) * 100}%`;
        document.getElementById('quiz-streak-count').textContent = quiz.streak;
        document.getElementById('quiz-streak').classList.toggle('is-hot', quiz.streak >= 3);
        if (keepFeedback) return;
        fillWordTiles(document.getElementById('quiz-word'), question.tiles);
        const feedback = document.getElementById('quiz-feedback');
        feedback.classList.add('hidden');
        feedback.replaceChildren();
        document.getElementById('btn-quiz-next').classList.add('hidden');
        this._setAnswerButtons(true);
        this.lastFeedback = null;
    },

    _setAnswerButtons(enabled) {
        for (const id of ['btn-quiz-valid', 'btn-quiz-invalid']) {
            const btn = document.getElementById(id);
            btn.disabled = !enabled;
            if (enabled) btn.classList.remove('is-picked');
        }
    },

    async answer(isValid) {
        const quiz = this.quiz;
        if (!quiz || quiz.answered || quiz.finished) return;
        quiz.answered = true;
        this._setAnswerButtons(false);
        document.getElementById(isValid ? 'btn-quiz-valid' : 'btn-quiz-invalid').classList.add('is-picked');
        const question = quiz.questions[quiz.index];
        let data;
        try {
            data = await postJson('/api/practice/answer', { word: question.word, answer: isValid });
            if (!data.success) throw new Error('answer');
        } catch {
            quiz.answered = false;
            this._setAnswerButtons(true);
            showMessage(t('common.load_failed'), true);
            return;
        }
        if (data.correct) { quiz.correct++; quiz.streak++; quiz.bestStreak = Math.max(quiz.bestStreak, quiz.streak); }
        else { quiz.streak = 0; }
        quiz.results.push({ word: question.word, tiles: data.tiles, valid: data.valid, correct: data.correct,
                            score: data.score, suggestions: data.suggestions || [] });
        PracticeStore.recordAnswer(question.word, data.tiles, data.valid, data.correct);

        fillWordTiles(document.getElementById('quiz-word'), data.tiles, data.correct ? 'right' : 'wrong');
        document.getElementById('quiz-streak-count').textContent = quiz.streak;
        document.getElementById('quiz-streak').classList.toggle('is-hot', quiz.streak >= 3);
        document.getElementById('quiz-bar-fill').style.width = `${((quiz.index + 1) / quiz.questions.length) * 100}%`;
        this.lastFeedback = data;
        this.renderFeedback(data);
        SoundManager.play(data.correct ? 'challenge_accept' : 'challenge_reject');
        if (navigator.vibrate) navigator.vibrate(data.correct ? 12 : [30, 40, 30]);

        const next = document.getElementById('btn-quiz-next');
        next.textContent = this._nextLabel();
        next.classList.remove('hidden');
        next.focus({ preventScroll: true });
    },

    _nextLabel() {
        const quiz = this.quiz;
        return quiz.index + 1 >= quiz.questions.length ? t('quiz.finish') : t('quiz.next');
    },

    renderFeedback(data) {
        const box = document.getElementById('quiz-feedback');
        box.replaceChildren();
        box.className = 'quiz-feedback ' + (data.correct ? 'right' : 'wrong');
        const head = makeEl('div', 'quiz-feedback-head');
        head.appendChild(makeIcon(data.correct ? 'check-circle' : 'x-circle'));
        head.appendChild(makeEl('span', null, data.correct ? t('quiz.right') : t('quiz.wrong')));
        box.appendChild(head);
        box.appendChild(makeEl('div', 'quiz-feedback-text',
            data.valid ? t('quiz.is_valid', { score: data.score }) : t('quiz.is_invalid')));
        if (!data.valid && data.suggestions && data.suggestions.length) {
            box.appendChild(makeEl('div', 'quiz-feedback-text', t('quiz.suggestions')));
            const list = makeEl('div', 'chip-list');
            for (const word of data.suggestions) list.appendChild(wordChip(word));
            box.appendChild(list);
        }
    },

    next() {
        const quiz = this.quiz;
        if (!quiz || !quiz.answered) return;
        quiz.index++;
        quiz.answered = false;
        if (quiz.index >= quiz.questions.length) this.finishQuiz();
        else this.renderQuestion();
    },

    finishQuiz() {
        const quiz = this.quiz;
        quiz.finished = true;
        quiz.seconds = (Date.now() - quiz.startedAt) / 1000;
        const data = PracticeStore.get();
        data.quiz.best_streak = Math.max(data.quiz.best_streak, quiz.bestStreak);
        PracticeStore.touch();
        this.showQuizBlock('result');
        this.renderQuizResult();
        const ratio = quiz.correct / quiz.questions.length;
        SoundManager.play(ratio >= 0.7 ? 'challenge_accept' : 'tile_place');
    },

    renderQuizResult() {
        const quiz = this.quiz;
        const box = document.getElementById('quiz-result');
        box.replaceChildren();
        const total = quiz.questions.length;
        const ratio = quiz.correct / total;

        const ring = makeEl('div', 'score-ring' + (ratio === 1 ? ' is-perfect' : ''));
        const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
        svg.setAttribute('viewBox', '0 0 120 120');
        const radius = 54;
        const circumference = 2 * Math.PI * radius;
        for (const cls of ['ring-track', 'ring-fill']) {
            const c = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
            c.setAttribute('class', cls);
            c.setAttribute('cx', 60); c.setAttribute('cy', 60); c.setAttribute('r', radius);
            if (cls === 'ring-fill') {
                c.setAttribute('stroke-dasharray', circumference);
                c.setAttribute('stroke-dashoffset', circumference);
                requestAnimationFrame(() => requestAnimationFrame(() =>
                    c.setAttribute('stroke-dashoffset', circumference * (1 - ratio))));
            }
            svg.appendChild(c);
        }
        const text = makeEl('div', 'score-ring-text');
        text.append(makeEl('b', null, `${quiz.correct}/${total}`), makeEl('span', null, t('quiz.correct_label')));
        ring.append(svg, text);

        const verdictKey = ratio === 1 ? 'quiz.perfect' : ratio >= 0.7 ? 'quiz.good' : 'quiz.practice_more';
        box.append(ring, makeEl('div', 'result-title', t(verdictKey)));
        if (ratio < 0.7) box.appendChild(makeEl('div', 'result-text', t('quiz.practice_tip')));

        const stats = makeEl('div', 'stat-grid');
        for (const [value, label] of [[Math.round(ratio * 100) + '%', t('quiz.stat_accuracy')],
                                      [quiz.bestStreak, t('quiz.stat_streak')],
                                      [formatClock(quiz.seconds), t('quiz.stat_time')]]) {
            const stat = makeEl('div', 'practice-stat');
            stat.append(makeEl('span', 'practice-stat-value', value), makeEl('span', 'practice-stat-label', label));
            stats.appendChild(stat);
        }
        box.appendChild(stats);

        const wrong = quiz.results.filter(r => !r.correct);
        if (wrong.length) {
            box.appendChild(makeEl('div', 'result-section-title', t('quiz.mistakes')));
            const list = makeEl('div', 'mistake-list');
            for (const r of wrong) {
                const row = makeEl('div', 'mistake-row');
                const word = makeEl('div', 'mistake-word', r.word);
                if (!r.valid && r.suggestions.length) {
                    word.appendChild(makeEl('span', 'mistake-note', t('quiz.suggestions_short', { words: r.suggestions.join(', ') })));
                }
                row.append(word, makeEl('span', 'verdict-pill ' + (r.valid ? 'is-valid' : 'is-invalid'),
                    r.valid ? t('quiz.verdict_valid') : t('quiz.verdict_invalid')));
                list.appendChild(row);
            }
            box.appendChild(list);
        }

        const actions = makeEl('div', 'result-actions');
        const again = makeEl('button', null, t('quiz.again'));
        again.type = 'button';
        again.addEventListener('click', () => this.startQuiz(quiz.mode === 'mistakes' && !PracticeStore.missedWords().length ? this.quizMode : quiz.mode));
        actions.appendChild(again);
        if (quiz.mode !== 'mistakes' && PracticeStore.missedWords().length) {
            const review = makeEl('button', 'tinted', t('quiz.review_mistakes', { n: PracticeStore.missedWords().length }));
            review.type = 'button';
            review.addEventListener('click', () => this.startQuiz('mistakes'));
            actions.appendChild(review);
        }
        const done = makeEl('button', 'secondary', t('practice.done'));
        done.type = 'button';
        done.addEventListener('click', () => this.back());
        actions.appendChild(done);
        box.appendChild(actions);
    },

    // --- Betűvadász és bingó-edző ---

    renderHuntSetup() {
        const bingo = this.huntKind === 'bingo';
        document.getElementById('hunt-heading').textContent = t(bingo ? 'bingo.title' : 'hunt.title');
        document.getElementById('hunt-intro').textContent = t(bingo ? 'bingo.intro' : 'hunt.intro');
        document.getElementById('hunt-timer-group').classList.toggle('hidden', bingo);
        this.renderHuntRecord();
    },

    renderHuntRecord() {
        const data = PracticeStore.get();
        const el = document.getElementById('hunt-record');
        if (this.huntKind === 'bingo') {
            el.textContent = data.bingo.attempts
                ? t('bingo.record', { solved: data.bingo.solved, n: data.bingo.attempts, best: data.bingo.best_streak }) : '';
        } else {
            const best = data.hunt.best[String(this.huntSeconds)];
            el.textContent = best ? t('hunt.record', { n: best }) : '';
        }
    },

    showHuntBlock(block) {
        document.getElementById('hunt-setup').classList.toggle('hidden', block !== 'setup');
        document.getElementById('hunt-play').classList.toggle('hidden', block !== 'play');
        document.getElementById('hunt-result').classList.toggle('hidden', block !== 'result');
    },

    _stopHunt() {
        if (this.hunt && this.hunt.timer) clearInterval(this.hunt.timer);
        this.hunt = null;
    },

    async startHunt() {
        const btn = document.getElementById('btn-hunt-start');
        btn.disabled = true;
        const kind = this.huntKind;
        try {
            const res = await fetch(`/api/practice/rack?kind=${kind}`);
            const data = await res.json();
            if (!data.success) {
                showMessage(tServer(data.message) || t('common.load_failed'), true);
                return;
            }
            this._beginHunt(data);
        } catch {
            showMessage(t('common.load_failed'), true);
        } finally {
            btn.disabled = false;
        }
    },

    _beginHunt(data) {
        this._stopHunt();
        const seconds = data.kind === 'bingo' ? 0 : this.huntSeconds;
        this.hunt = {
            kind: data.kind, seconds, rack: data.rack, order: data.rack.map((_, i) => i),
            words: data.words, total: data.total_score,
            solution: new Map(data.words.map(w => [w.word, w])),
            found: [], foundSet: new Set(), score: 0,
            built: [], pending: '', busy: false, finished: false,
            hintTarget: null, hintLevel: 0, hints: 0, gaveUp: false,
            startedAt: Date.now(), timer: null,
        };
        this.showHuntBlock('play');
        const bingo = data.kind === 'bingo';
        document.getElementById('hunt-clock-box').classList.toggle('hidden', bingo);
        document.getElementById('hunt-streak-box').classList.toggle('hidden', !bingo);
        document.getElementById('btn-hunt-finish').textContent = t(bingo ? 'bingo.give_up' : 'hunt.finish');
        document.getElementById('hunt-hint').classList.add('hidden');
        this.setHuntMsg('');
        this.renderHunt();
        if (!bingo) {
            this.hunt.timer = setInterval(() => this.tickHunt(), 250);
            this.tickHunt();
        }
    },

    // A gyakorlás lapja látszik-e (az időkorlátos kör órája csak akkor jár)
    _huntVisible() {
        return this.view === 'hunt' && !document.hidden
            && document.getElementById('lobby-panel-practice').classList.contains('active')
            && !document.getElementById('lobby-screen').classList.contains('hidden');
    },

    tickHunt() {
        const h = this.hunt;
        if (!h || h.finished) return;
        const now = Date.now();
        const last = h.lastTick || now;
        h.lastTick = now;
        if (!this._huntVisible()) {            // szünet: a háttérben töltött idő nem számít
            h.startedAt += now - last;
            return;
        }
        const elapsed = (now - h.startedAt) / 1000;
        const clock = document.getElementById('hunt-clock');
        if (h.seconds) {
            const left = h.seconds - elapsed;
            clock.textContent = formatClock(Math.ceil(left));
            clock.classList.toggle('is-low', left <= 10);
            if (left <= 0) this.huntFinish(true);
        } else {
            clock.textContent = formatClock(elapsed);
            clock.classList.remove('is-low');
        }
    },

    setHuntMsg(text, tone) {
        const el = document.getElementById('hunt-msg');
        el.textContent = text;
        el.className = 'hunt-msg' + (tone ? ' is-' + tone : '');
    },

    // A kéz, a szósor, a találatok újrarajzolása
    renderHunt() {
        const h = this.hunt;
        if (!h) return;
        const bingo = h.kind === 'bingo';
        document.getElementById('hunt-heading').textContent = t(bingo ? 'bingo.title' : 'hunt.title');
        document.getElementById('hunt-score').textContent = h.score;
        document.getElementById('hunt-found').textContent = bingo ? `${h.found.length}/${h.words.length}` : `${h.found.filter(f => !f.bonus).length}/${h.words.length}`;
        document.getElementById('hunt-streak').textContent = PracticeStore.get().bingo.streak;

        const used = new Set(h.built);
        const rack = document.getElementById('hunt-rack');
        rack.replaceChildren();
        for (const i of h.order) {
            const tile = h.rack[i];
            const btn = makeEl('button', 'htile' + (used.has(i) ? ' is-used' : ''), tile);
            btn.type = 'button';
            btn.appendChild(makeEl('small', null, TILE_VALUES[tile]));
            btn.addEventListener('click', () => this.huntAdd(i));
            rack.appendChild(btn);
        }

        const line = document.getElementById('hunt-line');
        line.replaceChildren();
        line.className = 'hunt-line';
        line.setAttribute('aria-label', t('hunt.word_aria'));
        h.built.forEach((rackIndex, pos) => {
            const tile = h.rack[rackIndex];
            const btn = makeEl('button', 'htile', tile);
            btn.type = 'button';
            btn.style.setProperty('--i', pos);
            btn.appendChild(makeEl('small', null, TILE_VALUES[tile]));
            btn.addEventListener('click', () => this.huntRemove(pos));
            line.appendChild(btn);
        });
        if (!h.built.length) for (let i = 0; i < h.rack.length; i++) line.appendChild(makeEl('span', 'hunt-slot'));

        document.getElementById('btn-hunt-submit').disabled = h.built.length < 2 || h.busy || h.finished;
        document.getElementById('btn-hunt-clear').disabled = !h.built.length;
        document.getElementById('btn-hunt-hint').disabled = h.finished;

        this.renderHuntProgress();
        this.renderHuntWords();
    },

    renderHuntProgress() {
        const h = this.hunt;
        const box = document.getElementById('hunt-progress');
        box.replaceChildren();
        if (h.kind === 'bingo') return;
        // Hány szó van hány zsetonosból, és mennyit találtál meg
        const totals = {}, got = {};
        for (const w of h.words) totals[w.tiles] = (totals[w.tiles] || 0) + 1;
        for (const f of h.found) if (!f.bonus && h.solution.has(f.word)) { const n = h.solution.get(f.word).tiles; got[n] = (got[n] || 0) + 1; }
        for (const len of Object.keys(totals).map(Number).sort((a, b) => a - b)) {
            const done = (got[len] || 0) >= totals[len];
            box.appendChild(makeEl('span', 'hp-chip' + (done ? ' is-done' : ''),
                t('hunt.progress_chip', { len, got: got[len] || 0, total: totals[len] })));
        }
    },

    renderHuntWords() {
        const h = this.hunt;
        const box = document.getElementById('hunt-words');
        box.replaceChildren();
        for (const f of h.found.slice().reverse()) {
            box.appendChild(wordChip(f.word, f.score, f.bingo ? 'is-bingo' : (f.bonus ? 'is-bonus' : '')));
        }
    },

    huntAdd(rackIndex) {
        const h = this.hunt;
        if (!h || h.finished || h.busy || h.built.includes(rackIndex)) return;
        h.built.push(rackIndex);
        this.setHuntMsg('');
        this.renderHunt();
        SoundManager.play('tile_place');
    },

    huntRemove(pos) {
        const h = this.hunt;
        if (!h || h.finished || h.busy) return;
        h.built.splice(pos, 1);
        this.setHuntMsg('');
        this.renderHunt();
    },

    huntClear() {
        const h = this.hunt;
        if (!h || h.finished || h.busy) return;
        h.built = [];
        h.pending = '';
        this.setHuntMsg('');
        this.renderHunt();
    },

    huntShuffle() {
        const h = this.hunt;
        if (!h || h.finished) return;
        h.order = shuffled(h.order);
        this.renderHunt();
    },

    // Billentyűzet: a gépelt betű az első szabad ugyanilyen zsetont veszi; a kétjegyű betű (SZ, CS…) két
    // billentyű, ha nincs külön S / C zseton
    huntType(ch) {
        const h = this.hunt;
        if (!h || h.finished || h.busy) return;
        const free = (letter) => h.rack.findIndex((tile, i) => tile === letter && !h.built.includes(i));
        let index = -1;
        if (h.pending) index = free(h.pending + ch);
        h.pending = '';
        if (index < 0) {
            // S, majd Z: ha van szabad SZ zseton, az az S helyére kerül (külön S + Z nem lehet SZ)
            const last = h.built.length ? h.rack[h.built[h.built.length - 1]] : '';
            const joined = formsDigraph(last, ch) ? free(last + ch) : -1;
            if (joined >= 0) {
                h.built.pop();
                index = joined;
            }
        }
        if (index < 0) index = free(ch);
        if (index < 0 && h.rack.some((tile, i) => tile.length === 2 && tile[0] === ch && !h.built.includes(i))) {
            h.pending = ch;
            return;
        }
        if (index >= 0) this.huntAdd(index);
    },

    _onKey(e) {
        if (e.ctrlKey || e.metaKey || e.altKey) return;
        if (!document.getElementById('lobby-panel-practice').classList.contains('active')) return;
        if (document.querySelector('.dialog:not(.hidden)')) return;
        const target = e.target;
        if (target && (target.tagName === 'INPUT' || target.tagName === 'SELECT' || target.tagName === 'TEXTAREA')) return;

        if (this.view === 'quiz' && this.quiz && !this.quiz.finished) {
            if (e.key === 'ArrowLeft') { e.preventDefault(); this.answer(false); }
            else if (e.key === 'ArrowRight') { e.preventDefault(); this.answer(true); }
            else if ((e.key === 'Enter' || e.key === ' ') && this.quiz.answered) { e.preventDefault(); this.next(); }
        } else if (this.view === 'wordbuilder') {
            WordBuilder.onKey(e);
        } else if (this.view === 'hunt' && this.hunt && !this.hunt.finished) {
            if (e.key === 'Enter') { e.preventDefault(); this.huntSubmit(); }
            else if (e.key === 'Backspace') { e.preventDefault(); this.huntRemove(this.hunt.built.length - 1); }
            else if (e.key === 'Escape') this.huntClear();
            else if (e.key === ' ') { e.preventDefault(); this.huntShuffle(); }
            else if (e.key.length === 1 && /\p{L}/u.test(e.key)) this.huntType(e.key.toUpperCase());
        }
    },

    async huntSubmit() {
        const h = this.hunt;
        if (!h || h.finished || h.busy || h.built.length < 2) return;
        const tiles = h.built.map(i => h.rack[i]);
        const word = tiles.join('');
        const bingo = tiles.length === h.rack.length;
        if (tiles.some((tile, i) => i > 0 && formsDigraph(tiles[i - 1], tile))) {
            this.huntReject(t('hunt.reason_split_digraph'));
            return;
        }
        if (h.kind === 'bingo' && !bingo) {
            this.huntReject(t('bingo.need_all'), true);
            return;
        }
        if (h.foundSet.has(word)) {
            this.huntReject(t('hunt.already'));
            return;
        }
        const score = tiles.reduce((sum, tile) => sum + TILE_VALUES[tile], 0) + (bingo ? 50 : 0);
        let bonus = false;
        if (!h.solution.has(word)) {
            // A listán nem szereplő szót a szerver bírálja el (a szókincs nem teljes)
            h.busy = true;
            this.renderHunt();
            let res;
            try {
                res = await postJson('/api/practice/rack-word', { rack: h.rack, word });
            } catch {
                h.busy = false;
                this.renderHunt();
                showMessage(t('common.load_failed'), true);
                return;
            }
            h.busy = false;
            if (this.hunt !== h || h.finished) return;
            if (!res.success || !res.ok) {
                this.huntReject(t('hunt.reason_' + (res.reason || 'not_a_word')));
                return;
            }
            bonus = true;
        }
        h.found.push({ word, score, bonus, bingo, tiles: tiles.length, tileList: tiles });
        h.foundSet.add(word);
        h.score += score;
        h.built = [];
        h.pending = '';
        if (h.kind === 'bingo') {
            this.huntSolved(word);
            return;
        }
        this.setHuntMsg(bingo ? t('hunt.bingo', { n: score }) : bonus ? t('hunt.bonus', { n: score }) : t('hunt.plus', { n: score }),
            bingo ? 'bingo' : 'ok');
        if (h.hintTarget === word) { h.hintTarget = null; h.hintLevel = 0; document.getElementById('hunt-hint').classList.add('hidden'); }
        SoundManager.play(bingo ? 'challenge_accept' : 'tile_place');
        this.renderHunt();
        document.getElementById('hunt-line').classList.add(bingo ? 'is-bingo' : 'is-right');
        if (h.found.filter(f => !f.bonus).length >= h.words.length) this.huntFinish(false, true);
    },

    huntReject(message, soft) {
        this.setHuntMsg(message, soft ? null : 'bad');
        const line = document.getElementById('hunt-line');
        line.classList.remove('is-wrong');
        void line.offsetWidth;          // az animáció újraindításához
        line.classList.add('is-wrong');
        if (!soft) SoundManager.play('challenge_reject');
        if (navigator.vibrate) navigator.vibrate([30, 40, 30]);
    },

    // --- Tipp ---

    huntHint() {
        const h = this.hunt;
        if (!h || h.finished) return;
        const box = document.getElementById('hunt-hint');
        if (!h.hintTarget || h.foundSet.has(h.hintTarget)) {
            const open = h.words.filter(w => !h.foundSet.has(w.word));
            if (!open.length) return;
            h.hintTarget = open[0].word;            // a legtöbb pontot érő megtalálatlan szó
            h.hintLevel = 0;
        }
        const target = h.solution.get(h.hintTarget);
        h.hintLevel = Math.min(h.hintLevel + 1, Math.max(1, target.word.length - 2));
        h.hints++;
        const letters = target.word.slice(0, h.hintLevel);
        box.textContent = h.kind === 'bingo'
            ? t('bingo.hint', { letters })
            : t('hunt.hint_text', { tiles: target.tiles, letters });
        box.classList.remove('hidden');
    },

    // --- Befejezés ---

    huntFinish(timeUp, allFound) {
        const h = this.hunt;
        if (!h || h.finished) return;
        if (h.kind === 'bingo') { this.huntGiveUp(); return; }
        h.finished = true;
        if (h.timer) clearInterval(h.timer);
        h.elapsed = (Date.now() - h.startedAt) / 1000;
        const data = PracticeStore.get();
        data.hunt.sessions++;
        const key = String(h.seconds);
        const record = h.score > (data.hunt.best[key] || 0);
        if (record) data.hunt.best[key] = h.score;
        PracticeStore.touch();
        h.record = record;
        h.timeUp = !!timeUp;
        h.allFound = !!allFound;
        this.showHuntBlock('result');
        this.renderHuntResult();
        SoundManager.play(record ? 'challenge_accept' : 'tile_place');
    },

    renderHuntResult() {
        const h = this.hunt;
        const box = document.getElementById('hunt-result');
        box.replaceChildren();
        const listed = h.found.filter(f => !f.bonus);
        const pct = h.total ? Math.min(100, Math.round(100 * listed.reduce((s, f) => s + f.score, 0) / h.total)) : 0;
        const rankKey = pct >= 85 ? 'master' : pct >= 60 ? 'advanced' : pct >= 35 ? 'solid' : 'beginner';

        box.appendChild(makeEl('div', 'hunt-result-score', h.score));
        box.appendChild(makeEl('div', 'result-text', t('hunt.score')));
        box.appendChild(makeEl('span', 'hunt-rank' + (pct >= 85 ? ' is-top' : ''), t('hunt.rank_' + rankKey)));
        if (h.record) box.appendChild(makeEl('div', 'result-text', t('hunt.new_record')));

        const stats = makeEl('div', 'stat-grid');
        for (const [value, label] of [[`${listed.length}/${h.words.length}`, t('hunt.stat_words')],
                                      [pct + '%', t('hunt.stat_share')],
                                      [h.hints, t('hunt.stat_hints')]]) {
            const stat = makeEl('div', 'practice-stat');
            stat.append(makeEl('span', 'practice-stat-value', value), makeEl('span', 'practice-stat-label', label));
            stats.appendChild(stat);
        }
        box.appendChild(stats);

        const bonus = h.found.filter(f => f.bonus);
        if (bonus.length) {
            box.appendChild(makeEl('div', 'result-section-title', t('hunt.bonus_words')));
            const list = makeEl('div', 'chip-list');
            for (const f of bonus) list.appendChild(wordChip(f.word, f.score, 'is-bonus'));
            box.appendChild(list);
        }
        const missed = h.words.filter(w => !h.foundSet.has(w.word));
        if (missed.length) {
            box.appendChild(makeEl('div', 'result-section-title', t('hunt.missed_words', { n: missed.length })));
            const list = makeEl('div', 'chip-list');
            for (const w of missed.slice(0, 24)) list.appendChild(wordChip(w.word, w.score, w.tiles === h.rack.length ? 'is-bingo' : 'is-missed'));
            if (missed.length > 24) list.appendChild(makeEl('span', 'word-chip is-missed', `+${missed.length - 24}`));
            box.appendChild(list);
        }

        const actions = makeEl('div', 'result-actions');
        const again = makeEl('button', null, t('hunt.again'));
        again.type = 'button';
        again.addEventListener('click', () => this.startHunt());
        const done = makeEl('button', 'secondary', t('practice.done'));
        done.type = 'button';
        done.addEventListener('click', () => this.back());
        actions.append(again, done);
        box.appendChild(actions);
    },

    // Bingó: megtaláltad (huntSolved) vagy feladtad (huntGiveUp) — mindkettő a következő kézhez vezet
    huntSolved(word) {
        const h = this.hunt;
        h.finished = true;
        const data = PracticeStore.get();
        data.bingo.attempts++;
        data.bingo.solved++;
        data.bingo.streak++;
        data.bingo.best_streak = Math.max(data.bingo.best_streak, data.bingo.streak);
        PracticeStore.touch();
        h.solvedWord = word;
        this.showHuntBlock('result');
        this.renderBingoResult();
        SoundManager.play('challenge_accept');
    },

    huntGiveUp() {
        const h = this.hunt;
        h.finished = true;
        h.gaveUp = true;
        const data = PracticeStore.get();
        data.bingo.attempts++;
        data.bingo.streak = 0;
        PracticeStore.touch();
        this.showHuntBlock('result');
        this.renderBingoResult();
    },

    renderBingoResult() {
        const h = this.hunt;
        const box = document.getElementById('hunt-result');
        box.replaceChildren();
        const data = PracticeStore.get();
        const solved = !h.gaveUp;
        box.appendChild(makeEl('div', 'result-title', solved ? t('bingo.solved_title') : t('bingo.missed_title')));
        if (solved) {
            box.appendChild(makeEl('div', 'result-text', t('bingo.solved_text', { n: h.score, streak: data.bingo.streak })));
        } else {
            box.appendChild(makeEl('div', 'result-text', t('bingo.missed_text')));
        }
        const word = solved ? h.solvedWord : h.words[0].word;
        const shown = makeEl('div', 'word-tiles');
        fillWordTiles(shown, solved ? h.found[h.found.length - 1].tileList : (tokenizeWord(word) || []), solved ? 'right' : '');
        box.appendChild(shown);

        const others = h.words.filter(w => w.word !== word);
        if (others.length) {
            box.appendChild(makeEl('div', 'result-section-title', t('bingo.other_words')));
            const list = makeEl('div', 'chip-list');
            for (const w of others) list.appendChild(wordChip(w.word, w.score, 'is-bingo'));
            box.appendChild(list);
        }
        const stats = makeEl('div', 'stat-grid');
        for (const [value, label] of [[data.bingo.streak, t('bingo.streak')], [data.bingo.best_streak, t('bingo.best_streak')],
                                      [`${data.bingo.solved}/${data.bingo.attempts}`, t('bingo.stat_solved')]]) {
            const stat = makeEl('div', 'practice-stat');
            stat.append(makeEl('span', 'practice-stat-value', value), makeEl('span', 'practice-stat-label', label));
            stats.appendChild(stat);
        }
        box.appendChild(stats);
        const actions = makeEl('div', 'result-actions');
        const next = makeEl('button', null, t('bingo.next'));
        next.type = 'button';
        next.addEventListener('click', () => this.startHunt());
        const done = makeEl('button', 'secondary', t('practice.done'));
        done.type = 'button';
        done.addEventListener('click', () => this.back());
        actions.append(next, done);
        box.appendChild(actions);
    },

    // --- Szólisták ---

    async openList() {
        const kind = this.lists.kind;
        document.getElementById('list-words-panel').classList.toggle('hidden', kind === 'tiles');
        document.getElementById('list-tiles-panel').classList.toggle('hidden', kind !== 'tiles');
        if (kind === 'tiles') { this.renderTilesTable(); return; }
        if (!this.lists.data[kind]) {
            try {
                const res = await fetch(`/api/practice/short-words?length=${kind}`);
                const data = await res.json();
                if (!data.success) { showMessage(tServer(data.message) || t('common.load_failed'), true); return; }
                this.lists.data[kind] = data.words.slice().sort((a, b) => compareWords(a.word, b.word));
            } catch {
                showMessage(t('common.load_failed'), true);
                return;
            }
            if (this.lists.kind !== kind) return;       // közben másik listára váltottak
        }
        this.renderList();
    },

    _initial(word) {
        const tokens = tokenizeWord(word);
        return tokens ? tokens[0] : word[0];
    },

    renderList() {
        const { kind, sort, letter, query } = this.lists;
        const words = this.lists.data[kind];
        if (!words) return;
        document.getElementById('list-intro').textContent = t(kind === '2' ? 'lists.intro_2' : 'lists.intro_3');

        // Kezdőbetű-szűrő
        const initials = [...new Set(words.map(w => this._initial(w.word)))].sort((a, b) => (HU_ORDER[a] ?? 99) - (HU_ORDER[b] ?? 99));
        if (letter && !initials.includes(letter)) this.lists.letter = '';
        const chips = document.getElementById('list-letters');
        chips.replaceChildren();
        for (const value of ['', ...initials]) {
            const chip = makeEl('button', 'chip-btn' + (value === this.lists.letter ? ' active' : ''), value || t('lists.all'));
            chip.type = 'button';
            chip.addEventListener('click', () => { this.lists.letter = value; this.renderList(); });
            chips.appendChild(chip);
        }

        let shown = words;
        if (this.lists.letter) shown = shown.filter(w => this._initial(w.word) === this.lists.letter);
        if (query) shown = shown.filter(w => w.word.includes(query));
        document.getElementById('list-count').textContent = shown.length === words.length
            ? t('lists.count', { n: words.length }) : t('lists.count_filtered', { n: shown.length, total: words.length });

        const box = document.getElementById('list-words');
        box.replaceChildren();
        if (!shown.length) {
            box.innerHTML = emptyStateHtml(t('lists.none'));
            return;
        }
        if (sort === 'score') {
            shown = shown.slice().sort((a, b) => b.score - a.score || compareWords(a.word, b.word));
            const group = makeEl('div', 'chip-list');
            for (const w of shown) group.appendChild(wordChip(w.word, w.score));
            box.appendChild(group);
            return;
        }
        let current = null, group = null;
        for (const w of shown) {
            const initial = this._initial(w.word);
            if (initial !== current) {
                current = initial;
                const section = makeEl('div', 'list-group');
                section.appendChild(makeEl('div', 'list-group-title', initial));
                group = makeEl('div', 'chip-list');
                section.appendChild(group);
                box.appendChild(section);
            }
            group.appendChild(wordChip(w.word, w.score));
        }
    },

    // Zsetonok: pontérték szerint csoportosítva, darabszámmal
    renderTilesTable() {
        document.getElementById('tiles-intro').textContent = t('lists.tiles_intro');
        const box = document.getElementById('tiles-table');
        box.replaceChildren();
        const groups = new Map();
        for (const [letter, value] of Object.entries(TILE_VALUES)) {
            if (!groups.has(value)) groups.set(value, []);
            groups.get(value).push(letter);
        }
        for (const value of Array.from(groups.keys()).sort((a, b) => a - b)) {
            const letters = groups.get(value).sort((a, b) => (HU_ORDER[a] ?? 99) - (HU_ORDER[b] ?? 99));
            const count = letters.reduce((sum, l) => sum + (TILE_COUNTS[l] || 0), 0);
            const row = makeEl('div', 'tiles-group');
            const label = makeEl('div', 'tiles-group-value', t('common.points', { n: value }));
            label.appendChild(makeEl('small', null, t('lists.tile_total', { n: count })));
            const tiles = makeEl('div', 'tiles-group-tiles');
            for (const letter of letters) {
                const cell = makeEl('div', 'tile-with-count');
                const tile = makeEl('span', 'qtile', letter || ' ');
                if (letter) tile.appendChild(makeEl('small', null, value));
                cell.append(tile, makeEl('em', null, `×${TILE_COUNTS[letter]}`));
                if (!letter) cell.title = t('tracker.blank');
                tiles.appendChild(cell);
            }
            row.append(label, tiles);
            box.appendChild(row);
        }
    },
};


// ===== LOBBY NAVIGÁCIÓ: görgethető fülsor =====
// Keskeny képernyőn a fülek elférnek-e: a kijelölt fül középre görgetődik, a széleken elhalványul a sor.

const LobbyNav = {
    // A lobby és a profil képernyő is ugyanilyen sort használ
    _navs() {
        return document.querySelectorAll('.lobby-nav');
    },

    init() {
        for (const nav of this._navs()) {
            nav.addEventListener('scroll', () => this.update(nav), { passive: true });
            if ('ResizeObserver' in window) new ResizeObserver(() => this.update(nav)).observe(nav);
        }
        window.addEventListener('resize', () => this._navs().forEach(nav => this.update(nav)));
        this._navs().forEach(nav => this.update(nav));
    },

    update(nav) {
        const max = nav.scrollWidth - nav.clientWidth;
        nav.classList.toggle('can-scroll-left', max > 2 && nav.scrollLeft > 4);
        nav.classList.toggle('can-scroll-right', max > 2 && nav.scrollLeft < max - 4);
    },

    reveal(tab) {
        const nav = tab && tab.closest('.lobby-nav');
        if (!nav || nav.scrollWidth <= nav.clientWidth) return;
        const navRect = nav.getBoundingClientRect();
        const tabRect = tab.getBoundingClientRect();
        const target = nav.scrollLeft + (tabRect.left - navRect.left) - (navRect.width - tabRect.width) / 2;
        const reduce = window.matchMedia && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
        nav.scrollTo({ left: Math.max(0, target), behavior: reduce ? 'auto' : 'smooth' });
    },
};

// ===== WEB PUSH: értesítés, ha te jössz =====

const Push = {
    supported: ('serviceWorker' in navigator) && ('PushManager' in window) && ('Notification' in window),

    init() {
        const toggle = document.getElementById('push-toggle');
        toggle.addEventListener('change', () => (toggle.checked ? this.enable() : this.disable()));
        // A háttérbe került lapnak a szerver push értesítést küld a saját körről
        document.addEventListener('visibilitychange', () => this.reportVisibility());
        socket.on('connect', () => this.reportVisibility());
        window.addEventListener('langchange', () => this.syncLanguage());
    },

    reportVisibility() {
        if (socket.connected && document.hidden !== undefined) socket.emit('set_visibility', { hidden: document.hidden });
    },

    _key(base64) {
        const padding = '='.repeat((4 - (base64.length % 4)) % 4);
        const raw = atob((base64 + padding).replace(/-/g, '+').replace(/_/g, '/'));
        return Uint8Array.from(raw, ch => ch.charCodeAt(0));
    },

    async _subscription() {
        const registration = await navigator.serviceWorker.ready;
        return registration.pushManager.getSubscription();
    },

    // A profil megnyitásakor: látszik-e a kapcsoló, és be van-e kapcsolva
    async refresh() {
        const box = document.getElementById('push-settings');
        const hint = document.getElementById('push-hint');
        const toggle = document.getElementById('push-toggle');
        box.classList.add('hidden');
        if (!this.supported || AppState.isGuest) return;
        try {
            const res = await fetch('/api/push/public-key');
            const data = await res.json();
            if (!data.success || !data.available) return;
            this._publicKey = data.public_key;
            const sub = await this._subscription();
            toggle.checked = !!sub && data.subscribed && Notification.permission === 'granted';
            if (Notification.permission === 'denied') hint.textContent = t('push.denied');
            else if (PWA.isIosSafari() && !PWA.isStandalone()) hint.textContent = t('push.ios_hint');
            else hint.textContent = '';
            box.classList.remove('hidden');
        } catch { /* az értesítés nem kritikus */ }
    },

    async enable() {
        const toggle = document.getElementById('push-toggle');
        try {
            const permission = await Notification.requestPermission();
            if (permission !== 'granted') throw new Error('denied');
            const registration = await navigator.serviceWorker.ready;
            const sub = await registration.pushManager.subscribe({
                userVisibleOnly: true, applicationServerKey: this._key(this._publicKey),
            });
            const data = await postJson('/api/push/subscribe', { ...sub.toJSON(), lang: I18N.lang });
            if (!data.success) throw new Error('server');
            showMessage(t('push.enabled'));
        } catch {
            toggle.checked = false;
            showMessage(Notification.permission === 'denied' ? t('push.denied') : t('push.error'), true);
        }
    },

    async disable() {
        try {
            const sub = await this._subscription();
            if (sub) {
                await postJson('/api/push/unsubscribe', { endpoint: sub.endpoint });
                await sub.unsubscribe();
            }
            showMessage(t('push.disabled'));
        } catch {
            showMessage(t('push.error'), true);
        }
    },

    // Nyelvváltáskor a szerver a feliratkozás nyelvén küldi az értesítést
    async syncLanguage() {
        if (!this.supported || AppState.isGuest || Notification.permission !== 'granted') return;
        try {
            const sub = await this._subscription();
            if (sub) await postJson('/api/push/subscribe', { ...sub.toJSON(), lang: I18N.lang });
        } catch { /* nem kritikus */ }
    },
};

// ===== KITÜNTETÉSEK =====

const BADGE_ICONS = {
    first_game: '🎲', first_win: '🥇', bingo: '🎯', score_100: '💯', long_word: '📏',
    joker_play: '🃏', game_300: '🏔️', bot_slayer: '🤖', wins_10: '🏆', games_25: '🎖️',
    daily_best: '🧩',
};

const Badges = {
    newInGame: [],   // az éppen befejezett játékban megszerzett kitüntetések

    init() {
        socket.on('achievements_earned', (data) => this.onEarned((data && data.badges) || []));
        socket.on('rating_update', (data) => GameOver.setRating(data));
        socket.on('game_saved', (data) => GameOver.setSavedGame(data && data.game_id));
        window.addEventListener('langchange', () => {
            if (!document.getElementById('game-over-dialog').classList.contains('hidden')) this.renderGameOver();
        });
    },

    onEarned(keys) {
        for (const key of keys) {
            if (!BADGE_ICONS[key]) continue;
            this.newInGame.push(key);
            showMessage(t('badge.earned', { name: t('badge.' + key) }), false, 5000);
        }
        // A játék vége ablak már nyitva lehet: frissítjük
        if (!document.getElementById('game-over-dialog').classList.contains('hidden')) this.renderGameOver();
    },

    _card(key, owned) {
        const card = document.createElement('div');
        card.className = 'badge-card' + (owned ? '' : ' locked');
        card.title = t('badge.' + key + '_desc');
        const icon = document.createElement('span');
        icon.className = 'badge-icon';
        icon.textContent = BADGE_ICONS[key];
        const text = document.createElement('div');
        text.className = 'badge-text';
        const name = document.createElement('strong');
        name.textContent = t('badge.' + key);
        const desc = document.createElement('small');
        desc.textContent = t('badge.' + key + '_desc');
        text.appendChild(name);
        text.appendChild(desc);
        card.appendChild(icon);
        card.appendChild(text);
        return card;
    },

    // A profilon minden kitüntetés látszik, a meg nem szerzettek halványan
    renderProfile(owned) {
        const grid = document.getElementById('profile-badges');
        grid.replaceChildren();
        const have = new Set(owned.map(b => b.badge));
        for (const key of Object.keys(BADGE_ICONS)) grid.appendChild(this._card(key, have.has(key)));
    },

    renderGameOver() {
        const box = document.getElementById('final-badges');
        box.replaceChildren();
        box.classList.toggle('hidden', !this.newInGame.length);
        if (!this.newInGame.length) return;
        const title = document.createElement('div');
        title.className = 'final-badges-title';
        title.textContent = t('badge.new_title');
        box.appendChild(title);
        for (const key of new Set(this.newInGame)) box.appendChild(this._card(key, true));
    },
};

// ===== PROFILE =====

const Profile = {
    init() {
        document.getElementById('btn-profile').addEventListener('click', () => this.show());
        document.getElementById('btn-profile-back').addEventListener('click', () => this.back());
        document.querySelectorAll('#profile-nav .lobby-nav-tab').forEach(tab => {
            tab.addEventListener('click', () => this.openLobbyTab(tab.dataset.lobbyTab));
        });
        window.addEventListener('langchange', () => {
            if (this._data && !document.getElementById('profile-screen').classList.contains('hidden')) {
                this.renderStats(this._data.stats);
                Badges.renderProfile(this._data.badges || []);
                this.renderHistory(this._data.history);
            }
        });
    },

    // Vissza arra a képernyőre, ahonnan a profilt megnyitottuk (lobby, várakozó szoba vagy játék)
    back() {
        let target = this._returnTo || 'lobby-screen';
        this._returnTo = null;
        const inRoom = target === 'waiting-screen' || target === 'game-screen';
        if (inRoom && !AppState.currentRoomId) target = 'lobby-screen';
        showScreen(target);
        if (target === 'lobby-screen') socket.emit('get_rooms');
    },

    // A navigációs sor fülére kattintva a lobbyba lépünk, és az adott fület nyitjuk meg
    openLobbyTab(tabId) {
        this._returnTo = null;
        if (tabId !== 'room') showScreen('lobby-screen');
        Lobby.switchTab(tabId);
    },

    // A profil sora a lobbyé másolata: a jelvények és a nyitott szoba füle a lobby mostani állapotát mutatja
    syncNav() {
        const mirror = (fromId, toId) => {
            const from = document.getElementById(fromId);
            const to = document.getElementById(toId);
            if (!from || !to) return;
            to.textContent = from.textContent;
            to.classList.toggle('hidden', from.classList.contains('hidden'));
        };
        mirror('friend-badge', 'profile-friend-badge');
        mirror('async-badge', 'profile-async-badge');
        mirror('nav-tab-room', 'profile-nav-tab-room');
    },

    async show() {
        const current = document.querySelector('.screen:not(.hidden)');
        if (current && current.id !== 'profile-screen' && current.id !== 'replay-screen') {
            this._returnTo = current.id;
        }
        this.syncNav();
        try {
            const resp = await fetch('/api/auth/profile');
            const data = await resp.json();
            if (!data.success) {
                showMessage(tServer(data.message) || t('profile.load_error'), true);
                return;
            }

            this._data = data;
            this.renderStats(data.stats);
            Badges.renderProfile(data.badges || []);
            this.renderHistory(data.history);
            Push.refresh();
            const nameEl = document.getElementById('profile-user-name');
            if (nameEl) nameEl.textContent = AppState.currentUser?.display_name || '';
            showScreen('profile-screen');
        } catch {
            showMessage(t('profile.load_error'), true);
        }
    },

    _data: null,

    renderStats(stats) {
        const container = document.getElementById('profile-stats');
        container.innerHTML = '';
        const cards = [
            { label: t('profile.played'), value: stats.games_played },
            { label: t('common.win'), value: stats.games_won },
            { label: t('profile.win_rate'), value: stats.win_rate + '%' },
            { label: t('profile.avg_score'), value: stats.avg_score },
            { label: t('profile.rating'), value: stats.rating },
        ];
        for (const card of cards) {
            const el = document.createElement('div');
            el.className = 'stat-card';

            const val = document.createElement('div');
            val.className = 'stat-value';
            val.textContent = card.value;

            const lbl = document.createElement('div');
            lbl.className = 'stat-label';
            lbl.textContent = card.label;

            el.appendChild(val);
            el.appendChild(lbl);
            container.appendChild(el);
        }
    },

    renderHistory(history) {
        const container = document.getElementById('profile-history');
        if (!history.length) {
            container.innerHTML = emptyStateHtml(t('common.no_finished_games'));
            return;
        }
        container.innerHTML = '';
        for (const h of history) {
            const row = document.createElement('div');
            row.className = 'history-row' + (h.is_winner ? ' winner' : '');

            const info = document.createElement('div');
            info.className = 'history-info';

            const date = document.createElement('span');
            date.className = 'history-date';
            date.textContent = formatServerDate(h.created_at, {
                year: 'numeric', month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit',
            });

            const name = document.createElement('span');
            name.className = 'history-room';
            name.textContent = h.room_name || t('create.room_placeholder');

            const score = document.createElement('span');
            score.className = 'history-score';
            score.textContent = t('common.points', { n: h.final_score });

            const result = document.createElement('span');
            result.className = 'history-result';
            result.textContent = h.is_winner ? t('common.win') : t('common.loss');

            const opponents = document.createElement('span');
            opponents.className = 'history-opponents';
            opponents.textContent = h.opponents.map(o => o.player_name).join(', ');

            info.appendChild(date);
            info.appendChild(name);
            info.appendChild(score);
            info.appendChild(result);
            if (h.rating_change !== null && h.rating_change !== undefined) {
                const delta = document.createElement('span');
                delta.className = 'history-rating ' + (h.rating_change >= 0 ? 'up' : 'down');
                delta.textContent = (h.rating_change > 0 ? '+' : '') + h.rating_change;
                delta.title = t('profile.rating');
                info.appendChild(delta);
            }
            info.appendChild(opponents);
            row.appendChild(info);

            const btn = document.createElement('button');
            btn.className = 'small-btn';
            btn.textContent = t('replay.title');
            btn.addEventListener('click', () => Replay.load(h.game_id));
            row.appendChild(btn);

            const shareBtn = document.createElement('button');
            shareBtn.className = 'small-btn secondary';
            shareBtn.textContent = t('replay.share');
            shareBtn.addEventListener('click', () => Replay.share(h.game_id));
            row.appendChild(shareBtn);

            container.appendChild(row);
        }
    },
};

// ===== REPLAY =====

const Replay = {
    moves: [],
    players: [],
    gameId: null,
    currentIdx: -1,
    _returnTo: null,
    analysis: null,        // a kész elemzés ({moves, players}) vagy null
    _analysisByMove: null, // lépésszám → elemzési bejegyzés
    _analysisTimer: null,
    showBest: false,       // a legjobb lépés mutatása a játszott helyett

    init() {
        document.getElementById('btn-replay-back').addEventListener('click', () => {
            showScreen(this._returnTo || (AppState.displayName ? 'profile-screen' : 'auth-screen'));
            this._returnTo = null;
        });
        document.getElementById('btn-replay-share').addEventListener('click', () => {
            if (this.gameId) this.share(this.gameId);
        });
        document.getElementById('btn-replay-analysis').addEventListener('click', () => this.runAnalysis());
        document.getElementById('btn-replay-best').addEventListener('click', () => {
            this.showBest = !this.showBest;
            this.renderMove();
        });
        document.getElementById('btn-replay-prev').addEventListener('click', () => this.prev());
        document.getElementById('btn-replay-next').addEventListener('click', () => this.next());
        window.addEventListener('langchange', () => {
            if (!document.getElementById('replay-screen').classList.contains('hidden')) {
                this.renderMove();
                this.renderPlayers();
                this.renderAnalysisSummary();
            }
        });
    },

    async load(gameId, options = {}) {
        try {
            const resp = await fetch(`/api/game/${gameId}/moves`);
            const data = await resp.json();
            if (!data.success) {
                showMessage(tServer(data.message) || t('replay.load_error'), true);
                return;
            }
            this._open(data, gameId);
            if (options.analysis && data.finished) this.runAnalysis();
        } catch {
            showMessage(t('replay.load_error'), true);
        }
    },

    // Megosztott visszajátszás megnyitása a linkből (bejelentkezés nélkül is)
    async loadShared(token) {
        try {
            const resp = await fetch(`/api/replay/${encodeURIComponent(token)}`);
            const data = await resp.json();
            if (!data.success) {
                showMessage(tServer(data.message) || t('replay.load_error'), true);
                return;
            }
            this._open(data, null);
        } catch {
            showMessage(t('replay.load_error'), true);
        }
    },

    // A /?replay=TOKEN link kezelése, ha a látogató nincs bejelentkezve
    openSharedLink() {
        let token = null;
        try { token = new URLSearchParams(location.search).get('replay'); } catch { return; }
        if (!token) return;
        try { history.replaceState(null, '', location.pathname); } catch { /* ignore */ }
        this.loadShared(token);
    },

    _open(data, gameId) {
        const current = document.querySelector('.screen:not(.hidden)');
        if (current && current.id !== 'replay-screen') this._returnTo = current.id;
        this._resetAnalysis();
        this.gameId = gameId;
        this.moves = data.moves;
        this.players = data.players || [];
        this.currentIdx = -1;
        this.buildBoard();
        this.renderMove();
        this.renderPlayers();
        document.getElementById('btn-replay-share').classList.toggle('hidden', !gameId);
        // Elemezni csak a saját, befejezett játékot lehet
        document.getElementById('replay-analysis').classList.toggle('hidden', !(gameId && data.finished));
        showScreen('replay-screen');
    },

    // --- Elemzés ---

    _resetAnalysis() {
        clearTimeout(this._analysisTimer);
        this._analysisTimer = null;
        this.analysis = null;
        this._analysisByMove = null;
        this.showBest = false;
        document.getElementById('btn-replay-analysis').classList.remove('hidden');
        document.getElementById('analysis-status').classList.add('hidden');
        document.getElementById('analysis-body').classList.add('hidden');
        document.getElementById('analysis-move').textContent = '';
        document.getElementById('analysis-summary').replaceChildren();
    },

    _setStatus(text) {
        const el = document.getElementById('analysis-status');
        el.textContent = text;
        el.classList.toggle('hidden', !text);
    },

    async runAnalysis() {
        const gameId = this.gameId;
        if (!gameId) return;
        clearTimeout(this._analysisTimer);
        document.getElementById('btn-replay-analysis').classList.add('hidden');
        try {
            const resp = await fetch(`/api/game/${gameId}/analysis`);
            const data = await resp.json();
            if (gameId !== this.gameId) return;   // közben másik játékot nyitottak meg
            if (!data.success) {
                this._setStatus(tServer(data.message) || t('analysis.load_error'));
                document.getElementById('btn-replay-analysis').classList.remove('hidden');
                return;
            }
            if (data.status === 'running') {
                this._setStatus(t('analysis.running', { done: data.done || 0, total: data.total || 0 }));
                this._analysisTimer = setTimeout(() => this.runAnalysis(), 2000);
            } else if (data.status === 'ready') {
                this._setStatus('');
                this._setAnalysis(data);
            } else if (data.status === 'unavailable') {
                this._setStatus(t('analysis.unavailable'));
            } else {
                this._setStatus(t('analysis.load_error'));
                document.getElementById('btn-replay-analysis').classList.remove('hidden');
            }
        } catch {
            this._setStatus(t('analysis.load_error'));
            document.getElementById('btn-replay-analysis').classList.remove('hidden');
        }
    },

    _setAnalysis(data) {
        this.analysis = data;
        this._analysisByMove = new Map((data.moves || []).map(e => [e.n, e]));
        document.getElementById('analysis-body').classList.remove('hidden');
        this.renderAnalysisSummary();
        this.renderMove();
    },

    // Játékosonkénti összesítés: hatékonyság, kint maradt pontok, legnagyobb kihagyott lehetőség
    renderAnalysisSummary() {
        const box = document.getElementById('analysis-summary');
        box.replaceChildren();
        if (!this.analysis) return;
        for (const p of this.analysis.players || []) {
            const card = document.createElement('div');
            card.className = 'analysis-player';
            const head = document.createElement('strong');
            head.textContent = `${p.player} — ${p.efficiency}%`;
            const line = document.createElement('div');
            line.textContent = t('analysis.summary_line', {
                missed: p.missed, optimal: p.optimal, turns: p.turns,
            });
            card.appendChild(head);
            card.appendChild(line);
            if (p.biggest_miss) {
                const miss = document.createElement('small');
                miss.textContent = t('analysis.biggest_miss', {
                    n: p.biggest_miss.n, words: p.biggest_miss.best_words.join(', '),
                    score: p.biggest_miss.best_score, lost: p.biggest_miss.lost,
                });
                card.appendChild(miss);
            }
            box.appendChild(card);
        }
    },

    // A kijelölt lépés elemzése: a legjobb lehetséges lépés és a kint maradt pont
    _currentAnalysis() {
        if (!this._analysisByMove || this.currentIdx < 0) return null;
        return this._analysisByMove.get(this.moves[this.currentIdx].move_number) || null;
    },

    renderAnalysisMove() {
        const el = document.getElementById('analysis-move');
        const bestBtn = document.getElementById('btn-replay-best');
        const entry = this._currentAnalysis();
        if (!entry) {
            el.textContent = '';
            bestBtn.disabled = true;
            this.showBest = false;
        } else if (entry.lost === 0) {
            el.textContent = t('analysis.optimal');
            el.classList.remove('missed');
            bestBtn.disabled = true;
            this.showBest = false;
        } else {
            el.classList.add('missed');
            el.textContent = entry.best_words.length
                ? t('analysis.best', { words: entry.best_words.join(', '), score: entry.best_score, lost: entry.lost })
                : t('analysis.no_move');
            bestBtn.disabled = !entry.best_tiles.length;
            if (!entry.best_tiles.length) this.showBest = false;
        }
        bestBtn.textContent = this.showBest ? t('analysis.hide_best') : t('analysis.show_best');
    },

    // A végeredmény a visszajátszás fölött (holtversenynél több győztes is lehet)
    renderPlayers() {
        const box = document.getElementById('replay-players');
        box.replaceChildren();
        for (const p of this.players) {
            const item = document.createElement('span');
            item.className = 'replay-player' + (p.is_winner ? ' winner' : '');
            item.textContent = `${p.is_winner ? '🏆 ' : ''}${p.player_name} · ${t('common.points', { n: p.final_score })}`;
            box.appendChild(item);
        }
    },

    // Megosztható link készítése a saját befejezett játékhoz
    async share(gameId) {
        try {
            const resp = await fetch(`/api/game/${gameId}/share`, { method: 'POST' });
            const data = await resp.json();
            if (!data.success) {
                showMessage(tServer(data.message) || t('replay.share_error'), true);
                return;
            }
            const url = `${location.origin}/?replay=${data.token}`;
            await shareOrCopy(url, t('app.title'), t('replay.share_text'), t('replay.link_copied'));
        } catch {
            showMessage(t('replay.share_error'), true);
        }
    },

    buildBoard() {
        const board = document.getElementById('replay-board');
        board.innerHTML = '';
        for (let r = 0; r < 15; r++) {
            for (let c = 0; c < 15; c++) {
                const cell = document.createElement('div');
                cell.className = 'cell';
                cell.dataset.row = r;
                cell.dataset.col = c;

                const premium = PREMIUM_MAP[`${r},${c}`];
                if (premium) {
                    cell.classList.add(`premium-${premium}`);
                    const label = document.createElement('span');
                    label.className = 'premium-label';
                    label.innerHTML = PREMIUM_LABELS[premium];
                    cell.appendChild(label);
                }

                board.appendChild(cell);
            }
        }
    },

    renderMove() {
        const counter = document.getElementById('replay-counter');
        const info = document.getElementById('replay-move-info');

        counter.textContent = `${this.currentIdx + 1} / ${this.moves.length}`;
        document.getElementById('btn-replay-prev').disabled = this.currentIdx < 0;
        document.getElementById('btn-replay-next').disabled = this.currentIdx >= this.moves.length - 1;

        if (this.currentIdx < 0) {
            info.textContent = t('replay.start');
            this._renderSnapshot(null);
            this.renderAnalysisMove();
            return;
        }

        const move = this.moves[this.currentIdx];
        let text = `${move.player_name}: `;
        const details = move.details_json ? JSON.parse(move.details_json) : {};

        switch (move.action_type) {
            case 'place':
            case 'challenge_accept':
                text += (details.words || []).join(', ');
                if (details.score) text += ` (${t('common.points', { n: details.score })})`;
                break;
            case 'exchange':
                text += t('history.exchange');
                break;
            case 'pass':
                text += t('history.pass');
                break;
            case 'challenge_reject':
                text += t('history.challenge_reject');
                break;
            default:
                text += move.action_type;
        }

        info.textContent = text;
        this.renderAnalysisMove();
        const entry = this._currentAnalysis();
        if (this.showBest && entry && entry.best_tiles.length) {
            // A lépés előtti tábla + a legjobb lépés halvány zsetonokkal
            const before = this.currentIdx > 0 ? this.moves[this.currentIdx - 1].board_snapshot_json : null;
            this._renderSnapshot(before ? JSON.parse(before) : null, new Set(), entry.best_tiles);
            return;
        }
        const snapshot = move.board_snapshot_json ? JSON.parse(move.board_snapshot_json) : null;
        // Az éppen lerakott betűk kiemelve
        const highlight = new Set((details.tiles || []).map(tile => `${tile.row},${tile.col}`));
        this._renderSnapshot(snapshot, highlight);
    },

    _renderSnapshot(boardData, highlight = new Set(), suggest = []) {
        const board = document.getElementById('replay-board');
        const cells = board.querySelectorAll('.cell');
        const suggested = new Map(suggest.map(tile => [`${tile.row},${tile.col}`, tile]));

        cells.forEach(cell => {
            const r = parseInt(cell.dataset.row);
            const c = parseInt(cell.dataset.col);
            const key = `${r},${c}`;

            cell.classList.remove('has-tile', 'long-letter', 'last-move', 'analysis-suggest');

            if (boardData && boardData[r] && boardData[r][c]) {
                const tile = boardData[r][c];
                cell.classList.add('has-tile');
                if (highlight.has(key)) cell.classList.add('last-move');
                if (tile.letter.length > 1) cell.classList.add('long-letter');
                const value = tile.is_blank ? 0 : (TILE_VALUES[tile.letter] || 0);
                cell.innerHTML = `${escapeHtml(tile.letter)}<span class="tile-value">${value}</span>`;
            } else if (suggested.has(key)) {
                const tile = suggested.get(key);
                cell.classList.add('has-tile', 'analysis-suggest');
                if (tile.letter.length > 1) cell.classList.add('long-letter');
                const value = tile.is_blank ? 0 : (TILE_VALUES[tile.letter] || 0);
                cell.innerHTML = `${escapeHtml(tile.letter)}<span class="tile-value">${value}</span>`;
            } else {
                const premium = PREMIUM_MAP[key];
                cell.innerHTML = premium
                    ? `<span class="premium-label">${PREMIUM_LABELS[premium]}</span>`
                    : '';
            }
        });
    },

    prev() {
        if (this.currentIdx >= 0) {
            this.currentIdx--;
            this.renderMove();
        }
    },

    next() {
        if (this.currentIdx < this.moves.length - 1) {
            this.currentIdx++;
            this.renderMove();
        }
    },
};

// ===== SOUND MANAGER =====

const SoundManager = {
    _ctx: null,
    _settings: null,
    _defaultSettings: {
        volume: 0.65,
        enabled: {
            tile_place: true,
            vote: true,
            challenge_result: true,
            your_turn: true,
            chat: true,
            game_events: true,
        },
    },

    init() {
        const saved = localStorage.getItem('scrabble-sound');
        try { this._settings = saved ? JSON.parse(saved) : null; } catch { this._settings = null; }
        if (!this._settings) this._settings = JSON.parse(JSON.stringify(this._defaultSettings));
        if (!this._settings.enabled) this._settings.enabled = {};
        for (const key of Object.keys(this._defaultSettings.enabled)) {
            if (this._settings.enabled[key] === undefined) this._settings.enabled[key] = true;
        }
    },

    _ctx_get() {
        if (!this._ctx) this._ctx = new (window.AudioContext || window.webkitAudioContext)();
        if (this._ctx.state === 'suspended') this._ctx.resume();
        return this._ctx;
    },

    play(name) {
        const cats = {
            tile_place: 'tile_place',
            vote: 'vote',
            challenge_accept: 'challenge_result',
            challenge_reject: 'challenge_result',
            your_turn: 'your_turn',
            chat: 'chat',
            game_start: 'game_events',
            game_over: 'game_events',
        };
        const cat = cats[name];
        if (!cat || this._settings.enabled[cat] === false) return;
        try {
            const ctx = this._ctx_get();
            const vol = this._settings.volume ?? 0.65;
            this['_snd_' + name](ctx, vol);
        } catch (e) { /* AudioContext not supported or blocked */ }
    },

    // Shared helper: plays a single tone with attack + exponential decay
    _tone(ctx, freq, t, dur, vol, type = 'sine') {
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        osc.connect(gain);
        gain.connect(ctx.destination);
        osc.type = type;
        osc.frequency.setValueAtTime(freq, t);
        gain.gain.setValueAtTime(0, t);
        gain.gain.linearRampToValueAtTime(vol, t + 0.008);
        gain.gain.exponentialRampToValueAtTime(0.001, t + dur);
        osc.start(t);
        osc.stop(t + dur + 0.01);
    },

    // Wooden "tock" when tile lands on board
    _snd_tile_place(ctx, vol) {
        const t = ctx.currentTime;
        const osc = ctx.createOscillator();
        const gain = ctx.createGain();
        const filter = ctx.createBiquadFilter();
        osc.connect(filter); filter.connect(gain); gain.connect(ctx.destination);
        osc.type = 'triangle';
        filter.type = 'lowpass';
        filter.frequency.setValueAtTime(900, t);
        osc.frequency.setValueAtTime(280, t);
        osc.frequency.exponentialRampToValueAtTime(90, t + 0.07);
        gain.gain.setValueAtTime(vol * 0.55, t);
        gain.gain.exponentialRampToValueAtTime(0.001, t + 0.07);
        osc.start(t); osc.stop(t + 0.08);
    },

    // Soft ping when clicking accept/reject vote
    _snd_vote(ctx, vol) {
        this._tone(ctx, 660, ctx.currentTime, 0.22, vol * 0.28);
    },

    // Two ascending notes — words accepted
    _snd_challenge_accept(ctx, vol) {
        const t = ctx.currentTime;
        this._tone(ctx, 523, t, 0.22, vol * 0.28);          // C5
        this._tone(ctx, 659, t + 0.14, 0.30, vol * 0.28);   // E5
    },

    // Two descending notes — words rejected
    _snd_challenge_reject(ctx, vol) {
        const t = ctx.currentTime;
        this._tone(ctx, 392, t, 0.22, vol * 0.28);          // G4
        this._tone(ctx, 262, t + 0.14, 0.30, vol * 0.28);   // C4
    },

    // Gentle two-tone chime — your turn
    _snd_your_turn(ctx, vol) {
        const t = ctx.currentTime;
        this._tone(ctx, 880, t, 0.28, vol * 0.22);
        this._tone(ctx, 1108, t + 0.18, 0.38, vol * 0.22);  // C#6
    },

    // Soft pop — incoming chat
    _snd_chat(ctx, vol) {
        this._tone(ctx, 820, ctx.currentTime, 0.10, vol * 0.18);
    },

    // Ascending 4-note fanfare — game starts
    _snd_game_start(ctx, vol) {
        const t = ctx.currentTime;
        [523, 659, 784, 1047].forEach((f, i) =>
            this._tone(ctx, f, t + i * 0.13, 0.28, vol * 0.28));
    },

    // Resolution flourish — game ends
    _snd_game_over(ctx, vol) {
        const t = ctx.currentTime;
        [784, 659, 523, 392, 523].forEach((f, i) =>
            this._tone(ctx, f, t + i * 0.14, 0.32, vol * 0.22));
    },

    setVolume(v) { this._settings.volume = v; this._save(); },
    setEnabled(cat, val) { this._settings.enabled[cat] = val; this._save(); },
    getSettings() { return this._settings; },
    _save() { localStorage.setItem('scrabble-sound', JSON.stringify(this._settings)); },
};

// ===== SOUND SETTINGS UI =====

const SoundSettings = {
    CATEGORIES: [
        { key: 'tile_place' },
        { key: 'vote' },
        { key: 'challenge_result' },
        { key: 'your_turn' },
        { key: 'chat' },
        { key: 'game_events' },
    ],

    _previousVolume: 0.7,

    init() {
        const slider = document.getElementById('sound-volume');
        const masterMute = document.getElementById('sound-master-mute');
        
        document.getElementById('btn-close-sound-settings').addEventListener('click', () => this.hide());
        document.getElementById('sound-settings-overlay').addEventListener('click', (e) => {
            if (e.target === e.currentTarget) this.hide();
        });
        
        slider.addEventListener('input', (e) => {
            const v = parseFloat(e.target.value);
            SoundManager.setVolume(v);
            this._updateSliderTrack(e.target, v);
            if (v > 0 && masterMute.checked) {
                masterMute.checked = false;
                this._updateMasterMuteState(false);
            }
        });
        
        document.getElementById('btn-volume-mute').addEventListener('click', () => {
            if (!masterMute.checked) {
                masterMute.checked = true;
                this._updateMasterMuteState(true);
            }
        });
        
        document.getElementById('btn-volume-max').addEventListener('click', () => {
            masterMute.checked = false;
            this._updateMasterMuteState(false);
            SoundManager.setVolume(1);
            slider.value = 1;
            this._updateSliderTrack(slider, 1);
        });
        
        masterMute.addEventListener('change', (e) => {
            this._updateMasterMuteState(e.target.checked);
        });

        document.addEventListener('click', (e) => {
            if (e.target.closest('.btn-sound-settings')) this.show();
        });
    },

    _updateMasterMuteState(isMuted) {
        const slider = document.getElementById('sound-volume');
        const togglesContainer = document.getElementById('sound-toggles');
        
        if (isMuted) {
            if (parseFloat(slider.value) > 0) {
                this._previousVolume = parseFloat(slider.value);
            }
            SoundManager.setVolume(0);
            slider.value = 0;
            slider.disabled = true;
            togglesContainer.style.opacity = '0.5';
            togglesContainer.style.pointerEvents = 'none';
        } else {
            const newVol = this._previousVolume || 0.7;
            SoundManager.setVolume(newVol);
            slider.value = newVol;
            slider.disabled = false;
            togglesContainer.style.opacity = '1';
            togglesContainer.style.pointerEvents = 'auto';
        }
        this._updateSliderTrack(slider, slider.value);
    },

    _updateSliderTrack(slider, value) {
        const pct = (value * 100).toFixed(1);
        slider.style.setProperty('--val', `${pct}%`);
        const pctDisplay = document.getElementById('sound-volume-pct');
        if (pctDisplay) {
            pctDisplay.textContent = Math.round(value * 100) + '%';
        }
    },

    show() {
        this._renderToggles();
        const vol = SoundManager.getSettings().volume;
        const slider = document.getElementById('sound-volume');
        const masterMute = document.getElementById('sound-master-mute');
        
        if (vol === 0) {
            masterMute.checked = true;
            this._updateMasterMuteState(true);
        } else {
            masterMute.checked = false;
            slider.value = vol;
            slider.disabled = false;
            const togglesContainer = document.getElementById('sound-toggles');
            togglesContainer.style.opacity = '1';
            togglesContainer.style.pointerEvents = 'auto';
            this._updateSliderTrack(slider, vol);
        }
        
        document.getElementById('sound-settings-overlay').classList.remove('hidden');
    },

    hide() {
        document.getElementById('sound-settings-overlay').classList.add('hidden');
    },

    _renderToggles() {
        const container = document.getElementById('sound-toggles');
        container.innerHTML = '';
        const settings = SoundManager.getSettings();

        for (const cat of this.CATEGORIES) {
            const row = document.createElement('label');
            row.className = 'sound-toggle-row';

            const info = document.createElement('div');
            info.className = 'toggle-info';
            const name = document.createElement('span');
            name.className = 'toggle-name';
            name.textContent = t('sound.cat_' + cat.key);
            const desc = document.createElement('span');
            desc.className = 'toggle-desc';
            desc.textContent = t('sound.cat_' + cat.key + '_desc');
            info.appendChild(name);
            info.appendChild(desc);

            const sw = document.createElement('div');
            sw.className = 'toggle-switch';
            const input = document.createElement('input');
            input.type = 'checkbox';
            input.checked = settings.enabled[cat.key] !== false;
            input.addEventListener('change', () => SoundManager.setEnabled(cat.key, input.checked));
            const slider = document.createElement('span');
            slider.className = 'toggle-slider';
            sw.appendChild(input);
            sw.appendChild(slider);

            row.appendChild(info);
            row.appendChild(sw);
            container.appendChild(row);
        }
    },
};

// ===== FRIENDS SYSTEM =====

const Friends = {
    friendsList: [],
    pendingRequests: [],
    sentRequests: [],

    init() {
        document.getElementById('btn-search-friends').addEventListener('click', () => this.search());
        document.getElementById('friend-search-input').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') this.search();
        });

        // Socket események
        socket.on('friend_request_result', (data) => {
            showMessage(tServer(data.message), !data.success);
            if (data.success) this.load();
        });

        socket.on('friend_request_received', (data) => {
            showMessage(t('friends.request_received', { name: data.from_name }));
            this.load();
        });

        socket.on('friend_request_accepted', (data) => {
            showMessage(t('friends.request_accepted', { name: data.display_name }));
            this.load();
        });

        socket.on('friend_presence_changed', (data) => {
            if (!data || typeof data.friend_id !== 'number') return;
            const friend = this.friendsList.find(f => f.id === data.friend_id);
            if (!friend) return;
            friend.online = !!data.online;
            this.renderList();
        });

        socket.on('invite_sent', (data) => {
            showMessage(tServer(data.message), !data.success);
        });

        socket.on('game_invite', (data) => this.showInvitePopup(data));

        socket.on('invite_accepted', (data) => {
            socket.emit('join_room', { code: data.join_code });
        });

        // Várakozó szobában barátok meghívása
        document.getElementById('btn-invite-friends')?.addEventListener('click', () => this.toggleInviteList());

        window.addEventListener('langchange', () => { if (!AppState.isGuest) this.render(); });
    },

    async load() {
        if (AppState.isGuest) return;
        try {
            const resp = await fetch('/api/auth/friends');
            const data = await resp.json();
            if (data.success) {
                this.friendsList = data.friends;
                this.pendingRequests = data.pending_requests;
                this.sentRequests = data.sent_requests;
                this.render();
                this.updateBadge();
            }
        } catch { /* ignore */ }
    },

    updateBadge() {
        const badge = document.getElementById('friend-badge');
        if (!badge) return;
        if (this.pendingRequests.length > 0) {
            badge.textContent = this.pendingRequests.length;
            badge.classList.remove('hidden');
        } else {
            badge.classList.add('hidden');
        }
    },

    render() {
        this.renderList();
        this.renderPending();
        this.renderSent();

        const pendingSection = document.getElementById('friends-pending-section');
        if (this.pendingRequests.length > 0) {
            pendingSection.classList.remove('hidden');
        } else {
            pendingSection.classList.add('hidden');
        }

        const sentSection = document.getElementById('friends-sent-section');
        if (this.sentRequests.length > 0) {
            sentSection.classList.remove('hidden');
        } else {
            sentSection.classList.add('hidden');
        }
    },

    renderList() {
        const container = document.getElementById('friends-list-container');
        if (!this.friendsList.length) {
            container.innerHTML = emptyStateHtml(t('friends.none'));
            return;
        }

        container.innerHTML = this.friendsList.map(f => `
            <div class="friend-item">
                <div class="friend-item-name">
                    <span class="status-dot ${f.online ? 'online' : 'offline'}"></span>
                    ${escapeHtml(f.display_name)}
                </div>
                <div class="friend-item-actions">
                    <button class="small-btn danger" onclick="Friends.removeFriend(${f.id})">${escapeHtml(t('common.delete'))}</button>
                </div>
            </div>
        `).join('');
    },

    renderPending() {
        const container = document.getElementById('friends-pending-container');
        if (!this.pendingRequests.length) {
            container.innerHTML = '';
            return;
        }

        container.innerHTML = this.pendingRequests.map(r => `
            <div class="friend-item">
                <div class="friend-item-name">${escapeHtml(r.display_name)}</div>
                <div class="friend-item-actions">
                    <button class="small-btn" onclick="Friends.acceptRequest(${r.id})">${escapeHtml(t('friends.accept'))}</button>
                    <button class="small-btn danger" onclick="Friends.declineRequest(${r.id})">${escapeHtml(t('friends.decline'))}</button>
                </div>
            </div>
        `).join('');
    },

    renderSent() {
        const container = document.getElementById('friends-sent-container');
        if (!this.sentRequests.length) {
            container.innerHTML = '';
            return;
        }

        container.innerHTML = this.sentRequests.map(r => `
            <div class="friend-item">
                <div class="friend-item-name">${escapeHtml(r.display_name)}</div>
                <div class="text-muted text-sm">${escapeHtml(t('friends.pending'))}</div>
            </div>
        `).join('');
    },
    async search() {
        const query = document.getElementById('friend-search-input').value.trim();
        const resultsEl = document.getElementById('friend-search-results');
        
        if (query.length < 2) {
            resultsEl.classList.add('hidden');
            return;
        }

        try {
            const resp = await fetch(`/api/auth/search-users?q=${encodeURIComponent(query)}`);
            const data = await resp.json();
            
            if (data.success && data.users.length > 0) {
                resultsEl.innerHTML = data.users.map(u => {
                    const isFriend = this.friendsList.some(f => f.id === u.id);
                    const isPending = this.pendingRequests.some(r => r.id === u.id);
                    const isSent = this.sentRequests.some(r => r.id === u.id);
                    
                    let actionHtml = '';
                    if (isFriend) actionHtml = `<span class="text-muted text-sm">${escapeHtml(t('friends.is_friend'))}</span>`;
                    else if (isPending) actionHtml = `<span class="text-muted text-sm">${escapeHtml(t('friends.request_in'))}</span>`;
                    else if (isSent) actionHtml = `<span class="text-muted text-sm">${escapeHtml(t('friends.request_out'))}</span>`;
                    else actionHtml = `<button class="small-btn" onclick="Friends.sendRequest(${u.id})">${escapeHtml(t('friends.send_request'))}</button>`;

                    return `
                        <div class="friend-item">
                            <div class="friend-item-name">${escapeHtml(u.display_name)}</div>
                            <div class="friend-item-actions">${actionHtml}</div>
                        </div>
                    `;
                }).join('');
                resultsEl.classList.remove('hidden');
            } else {
                resultsEl.innerHTML = emptyStateHtml(t('friends.no_results'));
                resultsEl.classList.remove('hidden');
            }
        } catch {
            showMessage(t('friends.search_error'), true);
        }
    },

    sendRequest(friendId) {
        socket.emit('send_friend_request', { friend_id: friendId });
        document.getElementById('friend-search-results').classList.add('hidden');
        document.getElementById('friend-search-input').value = '';
    },

    acceptRequest(requesterId) {
        socket.emit('accept_friend_request', { requester_id: requesterId });
    },

    declineRequest(requesterId) {
        socket.emit('decline_friend_request', { requester_id: requesterId });
    },

    removeFriend(friendId) {
        showConfirm(t('friends.remove_title'), t('friends.remove_confirm'), t('common.delete'), () => {
            socket.emit('remove_friend', { friend_id: friendId });
        });
    },

    toggleInviteList() {
        const container = document.getElementById('invite-friends-list');
        if (!container.classList.contains('hidden')) {
            container.classList.add('hidden');
            return;
        }

        // Frissítsük a listát online státusz miatt
        this.load().then(() => {
            const onlineFriends = this.friendsList.filter(f => f.online);
            if (onlineFriends.length === 0) {
                container.innerHTML = emptyStateHtml(t('friends.none_online'));
            } else {
                container.innerHTML = onlineFriends.map(f => `
                    <div class="friend-item">
                        <div class="friend-item-name">
                            <span class="status-dot online"></span>
                            ${escapeHtml(f.display_name)}
                        </div>
                        <button class="small-btn tinted" onclick="Friends.inviteToRoom(${f.id})">${escapeHtml(t('friends.invite'))}</button>
                    </div>
                `).join('');
            }
            container.classList.remove('hidden');
        });
    },

    inviteToRoom(friendId) {
        socket.emit('invite_to_room', { friend_id: friendId });
        document.getElementById('invite-friends-list').classList.add('hidden');
    },

    showInvitePopup(data) {
        const container = document.getElementById('toast-container');
        if (!container) return;
        
        const toast = document.createElement('div');
        toast.className = 'toast toast-invite';

        const text = document.createElement('div');
        text.textContent = t('friends.invited', { name: data.from_name, room: data.room_name });

        const btnContainer = document.createElement('div');
        btnContainer.className = 'toast-invite-actions';

        const declineBtn = document.createElement('button');
        declineBtn.className = 'secondary small-btn';
        declineBtn.textContent = t('friends.invite_decline');

        const acceptBtn = document.createElement('button');
        acceptBtn.className = 'small-btn';
        acceptBtn.textContent = t('lobby.join');

        btnContainer.appendChild(declineBtn);
        btnContainer.appendChild(acceptBtn);

        toast.appendChild(text);
        toast.appendChild(btnContainer);
        
        container.appendChild(toast);
        
        const dismiss = () => {
            if (toast.classList.contains('toast-out')) return;
            toast.classList.add('toast-out');
            toast.addEventListener('animationend', () => toast.remove());
        };
        
        acceptBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            socket.emit('respond_invite', { invite_id: data.invite_id, accept: true });
            dismiss();
        });
        
        declineBtn.addEventListener('click', (e) => {
            e.stopPropagation();
            socket.emit('respond_invite', { invite_id: data.invite_id, accept: false });
            dismiss();
        });

        // Auto-dismiss after 15 seconds
        setTimeout(() => {
            if (toast.parentElement && !toast.classList.contains('toast-out')) {
                socket.emit('respond_invite', { invite_id: data.invite_id, accept: false });
                dismiss();
            }
        }, 15000);

        SoundManager.play('chat');
    }
};

// ===== DIALOGS (Esc billentyű, háttérre koppintás) =====

const Dialogs = {
    // A háttérre koppintással / Esc-szel bezárható lapok
    SHEETS: ['blank-dialog', 'exit-dialog', 'dictionary-dialog', 'tracker-dialog', 'hint-dialog', 'async-new-dialog'],

    init() {
        for (const id of this.SHEETS) {
            document.getElementById(id).addEventListener('click', (e) => {
                if (e.target === e.currentTarget) e.currentTarget.classList.add('hidden');
            });
        }

        document.addEventListener('keydown', (e) => {
            if (e.key !== 'Escape') return;
            const open = ['confirm-dialog', 'sound-settings-overlay', ...this.SHEETS]
                .map(id => document.getElementById(id))
                .find(el => el && !el.classList.contains('hidden'));
            if (!open) return;
            if (open.id === 'confirm-dialog') document.getElementById('btn-confirm-no').click();
            else open.classList.add('hidden');
        });
    },
};

// ===== RANGLISTA =====

const Leaderboard = {
    metric: 'rating',
    _data: null,

    init() {
        document.querySelectorAll('.lb-metric').forEach(btn => {
            btn.addEventListener('click', () => this.setMetric(btn.dataset.metric));
        });
        window.addEventListener('langchange', () => { if (this._data) this.render(this._data); });
    },

    setMetric(metric) {
        this.metric = metric;
        document.querySelectorAll('.lb-metric').forEach(b => b.classList.toggle('active', b.dataset.metric === metric));
        this.load();
    },

    async load() {
        const container = document.getElementById('leaderboard-container');
        try {
            const res = await fetch(`/api/leaderboard?metric=${encodeURIComponent(this.metric)}&limit=50`);
            const data = await res.json();
            if (!data.success) {
                container.innerHTML = emptyStateHtml(tServer(data.message) || t('common.load_failed'));
                return;
            }
            this._data = data;
            this.render(data);
        } catch {
            container.innerHTML = emptyStateHtml(t('common.load_failed'));
        }
    },

    _value(entry, metric) {
        switch (metric) {
            case 'rating': return `${entry.rating}`;
            case 'win_rate': return `${entry.win_rate}%`;
            case 'avg_score': return `${entry.avg_score}`;
            case 'best_game': return `${entry.best_score}`;
            default: return t('lb.wins_count', { n: entry.games_won });
        }
    },

    _row(entry, metric) {
        const row = document.createElement('div');
        row.className = 'lb-row' + (entry.is_me ? ' me' : '') + (entry.rank <= 3 ? ` top-${entry.rank}` : '');

        const rank = document.createElement('span');
        rank.className = 'lb-rank';
        rank.textContent = entry.rank;

        const info = document.createElement('div');
        info.className = 'lb-info';
        const name = document.createElement('span');
        name.className = 'lb-name';
        name.textContent = entry.display_name + (entry.is_me ? ` (${t('lb.you')})` : '');
        const sub = document.createElement('span');
        sub.className = 'lb-sub';
        sub.textContent = t('lb.sub', { played: entry.games_played, won: entry.games_won, rate: entry.win_rate });
        info.appendChild(name);
        info.appendChild(sub);

        const value = document.createElement('span');
        value.className = 'lb-value';
        value.textContent = this._value(entry, metric);

        row.appendChild(rank);
        row.appendChild(info);
        row.appendChild(value);
        return row;
    },

    render(data) {
        const container = document.getElementById('leaderboard-container');
        const note = document.getElementById('lb-note');
        note.textContent = data.min_games > 1 ? t('lb.min_games', { n: data.min_games }) : '';
        container.innerHTML = '';
        if (!data.entries.length) {
            container.innerHTML = emptyStateHtml(t('lb.empty'));
            return;
        }
        for (const entry of data.entries) container.appendChild(this._row(entry, data.metric));
        // Ha a saját helyezésed nincs a listában, külön sorban jelenik meg
        if (data.me && !data.entries.some(e => e.user_id === data.me.user_id)) {
            const gap = document.createElement('div');
            gap.className = 'lb-gap';
            gap.textContent = '\u22EF';
            container.appendChild(gap);
            container.appendChild(this._row({ ...data.me, is_me: true }, data.metric));
        }
    },
};

// ===== SZÓTÁR-BÖNGÉSZŐ (kereső / ellenőrző) =====

const DictionaryTool = {
    _data: null,

    init() {
        document.getElementById('dictionary-form').addEventListener('submit', (e) => {
            e.preventDefault();
            this.check();
        });
        document.getElementById('btn-close-dictionary').addEventListener('click', () => this.hide());
        window.addEventListener('langchange', () => { if (this._data) this.render(this._data); });
    },

    show() {
        document.getElementById('dictionary-dialog').classList.remove('hidden');
        // Érintőkijelzőn nem ugratjuk fel a billentyűzetet magától
        if (!isTouchDevice) document.getElementById('dictionary-input').focus();
    },

    hide() {
        document.getElementById('dictionary-dialog').classList.add('hidden');
    },

    async check(query) {
        const input = document.getElementById('dictionary-input');
        if (query !== undefined) input.value = query;
        const q = input.value.trim();
        if (!q) return;
        const button = document.getElementById('btn-dictionary-check');
        button.disabled = true;
        try {
            const res = await fetch(`/api/dictionary/check?q=${encodeURIComponent(q)}`);
            const data = await res.json();
            if (!data.success) {
                showMessage(tServer(data.message) || t('common.error'), true);
                return;
            }
            this._data = data;
            this.render(data);
        } catch {
            showMessage(t('common.error'), true);
        } finally {
            button.disabled = false;
        }
    },

    render(data) {
        const container = document.getElementById('dictionary-results');
        container.replaceChildren();
        for (const result of data.results) {
            const card = document.createElement('div');
            card.className = 'dict-result ' + (result.valid ? 'dict-valid' : 'dict-invalid');

            const head = document.createElement('div');
            head.className = 'dict-result-head';
            const word = document.createElement('span');
            word.className = 'dict-word';
            word.textContent = result.word;
            const badge = document.createElement('span');
            badge.className = 'dict-badge';
            badge.textContent = result.valid ? t('dict.valid') : t('dict.invalid');
            head.appendChild(word);
            head.appendChild(badge);
            card.appendChild(head);

            if (result.tiles && result.tiles.length) {
                const tiles = document.createElement('div');
                tiles.className = 'dict-tiles';
                for (const letter of result.tiles) {
                    const chip = document.createElement('span');
                    chip.className = 'dict-tile' + (letter.length > 1 ? ' long-letter' : '');
                    chip.textContent = letter;
                    const val = document.createElement('small');
                    val.textContent = TILE_VALUES[letter] || 0;
                    chip.appendChild(val);
                    tiles.appendChild(chip);
                }
                card.appendChild(tiles);
            }

            const meta = document.createElement('div');
            meta.className = 'dict-meta';
            const parts = [];
            if (result.score !== null && result.score !== undefined) parts.push(t('dict.base_score', { n: result.score }));
            if (!result.valid && result.reason) parts.push(t('dict.reason_' + result.reason));
            meta.textContent = parts.join(' \u00b7 ');
            if (parts.length) card.appendChild(meta);

            if (result.suggestions && result.suggestions.length) {
                const sug = document.createElement('div');
                sug.className = 'dict-suggestions';
                const label = document.createElement('span');
                label.className = 'text-muted text-sm';
                label.textContent = t('dict.suggestions');
                sug.appendChild(label);
                for (const s of result.suggestions) {
                    const btn = document.createElement('button');
                    btn.type = 'button';
                    btn.className = 'small-btn tinted dict-suggestion';
                    btn.textContent = s;
                    btn.addEventListener('click', () => this.check(s));
                    sug.appendChild(btn);
                }
                card.appendChild(sug);
            }

            if (result.valid) {
                const link = document.createElement('a');
                link.className = 'dict-link dict-lookup';
                link.href = `https://www.google.com/search?q=${encodeURIComponent(result.word.toLowerCase() + ' - Kézikönyvtár A magyar nyelv értelmező szótára')}`;
                link.target = '_blank';
                link.rel = 'noopener noreferrer';
                link.textContent = t('dict.lookup');
                card.appendChild(link);
            }
            container.appendChild(card);
        }
    },
};

// ===== MEGFIGYELŐ MÓD =====

const Spectate = {
    _resuming: false,

    init() {
        socket.on('spectate_joined', (data) => this.onJoined(data));
        socket.on('spectate_left', () => this.onLeft());
    },

    onJoined(data) {
        AppState.isSpectator = true;
        AppState.spectateRoomId = data.room_id;
        AppState.currentRoomId = data.room_id;
        AppState.roomName = data.room_name;
        AppState.gameStarted = true;
        AppState.gameOverShown = false;
        AppState.challengeModeEnabled = !!data.challenge_mode;
        this._resuming = false;

        Chat.clear();
        (data.chat_messages || []).forEach(msg => Chat.onMessage(msg, true));

        // A játékállapot közvetlenül ezután érkezik (game_state), az tölti fel a képernyőt
        setGameRoomName(data.room_name);
        showScreen('game-screen');
        GameBoard.build();
        BoardZoom.init();
        GameBoard.updateSpectatorUi();
    },

    onLeft() {
        document.getElementById('game-over-dialog').classList.add('hidden');
        AppState.reset();
        ChallengeUI.stopCountdown(); TurnTimerUI._stop();
        showScreen('lobby-screen');
        socket.emit('get_rooms');
    },

    // Újracsatlakozás után a megfigyelés újraindul (a szerver a lecsatlakozáskor törölte)
    resume() {
        this._resuming = true;
        socket.emit('spectate_room', AppState.spectateCode
            ? { code: AppState.spectateCode }
            : { room_id: AppState.spectateRoomId });
        setTimeout(() => { this._resuming = false; }, 4000);
    },

    // Ha az újraindítás sikertelen (a játék közben véget ért, a szoba megszűnt): vissza a lobbyba
    onError() {
        if (this._resuming && AppState.isSpectator) {
            this._resuming = false;
            this.onLeft();
        }
    },
};

// ===== PWA: service worker, telepítés =====

const PWA = {
    deferredPrompt: null,

    isStandalone() {
        return window.matchMedia('(display-mode: standalone)').matches || window.navigator.standalone === true;
    },

    isIosSafari() {
        const ua = navigator.userAgent;
        return /iphone|ipad|ipod/i.test(ua) && /safari/i.test(ua) && !/crios|fxios|edgios/i.test(ua);
    },

    init() {
        const secure = location.protocol === 'https:' || ['localhost', '127.0.0.1'].includes(location.hostname);
        if ('serviceWorker' in navigator && secure) {
            window.addEventListener('load', () => {
                navigator.serviceWorker.register('/sw.js').catch(() => { /* nem kritikus */ });
            });
        }

        const btn = document.getElementById('btn-install-app');
        if (!btn) return;
        if (this.isStandalone()) return;

        window.addEventListener('beforeinstallprompt', (e) => {
            e.preventDefault();
            this.deferredPrompt = e;
            btn.classList.remove('hidden');
        });
        // iOS Safari nem küld telepítési eseményt: a gomb a kézi lépéseket mutatja
        if (this.isIosSafari()) btn.classList.remove('hidden');

        btn.addEventListener('click', async () => {
            if (this.deferredPrompt) {
                this.deferredPrompt.prompt();
                try { await this.deferredPrompt.userChoice; } catch { /* ignore */ }
                this.deferredPrompt = null;
                btn.classList.add('hidden');
            } else {
                showMessage(t('pwa.ios_hint'), false, 9000);
            }
        });
        window.addEventListener('appinstalled', () => {
            this.deferredPrompt = null;
            btn.classList.add('hidden');
            showMessage(t('pwa.installed'));
        });
    },
};

// ===== INITIALIZATION =====

SoundManager.init();
SoundSettings.init();
Dialogs.init();
Tracker.init();
Hint.init();
Shortcuts.init();
DictionaryTool.init();
Leaderboard.init();
Spectate.init();
PWA.init();
Auth.init();
Lobby.init();
WaitingRoom.init();
GameBoard.init();
ChallengeUI.init();
Chat.init();
ExitGame.init();
Profile.init();
Badges.init();
Daily.init();
WordBuilder.init();
Practice.init();
AsyncGames.init();
LobbyNav.init();
HandLayout.init();
Push.init();
Replay.init();
GameOver.init();
Reconnection.init();
Friends.init();

// Auto-login on page load
Auth.checkSession();
