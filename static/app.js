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
        this.updateAiControls();

        // Lobby nav tab switching
        document.querySelectorAll('.lobby-nav-tab').forEach(tab => {
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
        document.getElementById('room-ai-difficulty').disabled = count === 0;
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
        document.querySelectorAll('.lobby-nav-tab').forEach(t => t.classList.remove('active'));
        const activeTab = document.querySelector(`.lobby-nav-tab[data-lobby-tab="${tabId}"]`);
        if (activeTab) activeTab.classList.add('active');

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
        if (createTab) createTab.classList.toggle('hidden', AppState.isGuest);
        if (savedTab) savedTab.classList.toggle('hidden', AppState.isGuest);
        if (friendsTab) friendsTab.classList.toggle('hidden', AppState.isGuest);

        // Hide history section for guests
        const historySection = document.getElementById('home-history-section');
        if (historySection) historySection.classList.toggle('hidden', AppState.isGuest);
        
        if (!AppState.isGuest) {
            Friends.load(); // Kérések badge frissítéséhez
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
        if (!join && !spectate && !action) return;
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
        const aiDifficulty = document.getElementById('room-ai-difficulty').value;
        socket.emit('create_room', {
            name, max_players: maxPlayers,
            challenge_mode: challengeMode, is_private: isPrivate,
            turn_time_limit: turnTimeLimit,
            ai_players: Array(aiCount).fill(aiDifficulty),
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

    async shareLink() {
        if (!AppState.currentRoomCode) return;
        const url = this.inviteUrl();
        if (isTouchDevice && navigator.share) {
            try {
                await navigator.share({
                    title: t('app.title'),
                    text: t('wait.share_text', { room: AppState.roomName || '' }),
                    url,
                });
                return;
            } catch (e) {
                if (e && e.name === 'AbortError') return;  // a felhasználó bezárta a megosztó lapot
            }
        }
        try {
            await navigator.clipboard.writeText(url);
            showMessage(t('wait.link_copied'));
        } catch {
            showMessage(t('common.copy_failed'), true);
        }
    },

    update() {
        const gs = AppState.gameState;
        if (!gs) return;
        const container = document.getElementById('waiting-players');
        const joinedNames = gs.players.map(p => p.name);

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
                <div class="player-item ${i === 0 ? 'owner' : ''} ${p.is_bot ? 'bot' : ''}">
                    <span class="player-avatar">${p.is_bot
                        ? '<svg class="icon"><use href="#i-robot"/></svg>'
                        : escapeHtml(Array.from(p.name)[0] || '?')}</span>
                    <span class="player-name">${escapeHtml(p.name)}</span>
                    ${i === 0 ? `<span class="player-tag">${escapeHtml(t('wait.owner'))}</span>` : ''}
                    ${p.is_bot ? `<span class="player-tag">${escapeHtml(t('ai.' + (p.difficulty || 'medium')))}</span>` : ''}
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
        }
        AppState.gameState = state;
        AppState.myPlayerId = socket.id;
        AppState.isSpectator = !!state.spectator;

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
                GameOver.show();
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
        hintBtn.classList.toggle('hidden', spectator || humans !== 1);
        hintBtn.disabled = !isMyTurn || hasPending;
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
        text.textContent = data.moves.length ? t('hint.intro') : t('hint.none');
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
// Enter: lerak · Esc: visszavon · S: keverés · R: rendezés · Backspace: az utolsó lerakott betű visszavétele

const Shortcuts = {
    init() {
        document.addEventListener('keydown', (e) => {
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
        const sig = (AppState.isSpectator ? 'spectator:' : '') + I18N.lang + ':' +
            (isMyPlacement ? 'placer' : (myVote ? `voted:${myVote}` : 'vote'));
        if (this._buttonsSig !== sig) {
            this._buttonsSig = sig;
            buttonsEl.innerHTML = '';
            if (AppState.isSpectator) {
                this._addWaitText(buttonsEl, t('challenge.spectator'));
            } else if (isMyPlacement) {
                this._addWaitText(buttonsEl, t('challenge.voting'));
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
    init() {
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

    // Végeredmény + játékonkénti érdekességek (legjobb szó, legtöbb pont egy lépésben)
    render() {
        const gs = AppState.gameState;
        if (!gs) return;
        const scoresContainer = document.getElementById('final-scores');
        const sorted = [...gs.players].sort((a, b) => b.score - a.score);
        scoresContainer.innerHTML = sorted.map((p, i) => `
            <div class="score-final ${i === 0 ? 'winner' : ''}">
                <span>${i === 0 ? '&#x1F3C6; ' : ''}${escapeHtml(p.name)}</span>
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
    },

    showDialog() {
        // Megfigyelőként nincs mit megerősíteni: a kilépés azonnali
        if (AppState.isSpectator) { this._doLeave(); return; }
        const gs = AppState.gameState;
        const isActiveGame = gs && gs.started && !gs.finished;
        const showOwner = AppState.isOwner && isActiveGame;

        document.getElementById('exit-dialog-text').textContent =
            showOwner ? t('exit.what_to_do') : t('exit.sure');
        document.getElementById('exit-owner-buttons').classList.toggle('hidden', !showOwner);
        document.getElementById('exit-player-buttons').classList.toggle('hidden', showOwner);
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

// ===== PROFILE =====

const Profile = {
    init() {
        document.getElementById('btn-profile').addEventListener('click', () => this.show());
        document.getElementById('btn-profile-back').addEventListener('click', () => this.back());
        window.addEventListener('langchange', () => {
            if (this._data && !document.getElementById('profile-screen').classList.contains('hidden')) {
                this.renderStats(this._data.stats);
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

    async show() {
        const current = document.querySelector('.screen:not(.hidden)');
        if (current && current.id !== 'profile-screen' && current.id !== 'replay-screen') {
            this._returnTo = current.id;
        }
        try {
            const resp = await fetch('/api/auth/profile');
            const data = await resp.json();
            if (!data.success) {
                showMessage(tServer(data.message) || t('profile.load_error'), true);
                return;
            }

            this._data = data;
            this.renderStats(data.stats);
            this.renderHistory(data.history);
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
            info.appendChild(opponents);
            row.appendChild(info);

            const btn = document.createElement('button');
            btn.className = 'small-btn';
            btn.textContent = t('replay.title');
            btn.addEventListener('click', () => Replay.load(h.game_id));
            row.appendChild(btn);

            container.appendChild(row);
        }
    },
};

// ===== REPLAY =====

const Replay = {
    moves: [],
    currentIdx: -1,
    _returnTo: null,

    init() {
        document.getElementById('btn-replay-back').addEventListener('click', () => {
            showScreen(this._returnTo || 'profile-screen');
            this._returnTo = null;
        });
        document.getElementById('btn-replay-prev').addEventListener('click', () => this.prev());
        document.getElementById('btn-replay-next').addEventListener('click', () => this.next());
        window.addEventListener('langchange', () => {
            if (!document.getElementById('replay-screen').classList.contains('hidden')) this.renderMove();
        });
    },

    async load(gameId) {
        try {
            const resp = await fetch(`/api/game/${gameId}/moves`);
            const data = await resp.json();
            if (!data.success) {
                showMessage(tServer(data.message) || t('replay.load_error'), true);
                return;
            }

            const current = document.querySelector('.screen:not(.hidden)');
            if (current && current.id !== 'replay-screen') this._returnTo = current.id;
            this.moves = data.moves;
            this.currentIdx = -1;
            this.buildBoard();
            this.renderMove();
            showScreen('replay-screen');
        } catch {
            showMessage(t('replay.load_error'), true);
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
        const snapshot = move.board_snapshot_json ? JSON.parse(move.board_snapshot_json) : null;
        // Az éppen lerakott betűk kiemelve
        const highlight = new Set((details.tiles || []).map(tile => `${tile.row},${tile.col}`));
        this._renderSnapshot(snapshot, highlight);
    },

    _renderSnapshot(boardData, highlight = new Set()) {
        const board = document.getElementById('replay-board');
        const cells = board.querySelectorAll('.cell');

        cells.forEach(cell => {
            const r = parseInt(cell.dataset.row);
            const c = parseInt(cell.dataset.col);
            const key = `${r},${c}`;

            cell.classList.remove('has-tile', 'long-letter', 'last-move');

            if (boardData && boardData[r] && boardData[r][c]) {
                const tile = boardData[r][c];
                cell.classList.add('has-tile');
                if (highlight.has(key)) cell.classList.add('last-move');
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
    SHEETS: ['blank-dialog', 'exit-dialog', 'dictionary-dialog', 'tracker-dialog', 'hint-dialog'],

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
    metric: 'wins',
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
Replay.init();
GameOver.init();
Reconnection.init();
Friends.init();

// Auto-login on page load
Auth.checkSession();
