// ===== THEME SYSTEM =====
(function initTheme() {
    const saved = localStorage.getItem('scrabble-theme');
    const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
    const theme = saved || (prefersDark ? 'dark' : 'light');
    document.body.setAttribute('data-theme', theme);
})();

document.getElementById('theme-toggle').addEventListener('click', () => {
    const current = document.body.getAttribute('data-theme');
    const next = current === 'dark' ? 'light' : 'dark';
    document.body.setAttribute('data-theme', next);
    localStorage.setItem('scrabble-theme', next);
});

// Shared topbar buttons (appear on multiple screens)
function toggleTheme() {
    const current = document.body.getAttribute('data-theme');
    const next = current === 'dark' ? 'light' : 'dark';
    document.body.setAttribute('data-theme', next);
    localStorage.setItem('scrabble-theme', next);
}
document.addEventListener('click', (e) => {
    const themeBtn = e.target.closest('.btn-theme-toggle');
    if (themeBtn) { toggleTheme(); return; }

    const profileBtn = e.target.closest('.btn-profile-nav');
    if (profileBtn) {
        if (typeof Profile !== 'undefined') Profile.show();
        return;
    }

    const logoutBtn = e.target.closest('.btn-logout-nav');
    if (logoutBtn) {
        if (typeof Auth !== 'undefined') Auth.logout();
        return;
    }

    const exitPanelBtn = e.target.closest('.btn-exit-panel');
    if (exitPanelBtn) {
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

const ALL_LETTERS = [
    'A', 'Á', 'B', 'C', 'CS', 'D', 'E', 'É', 'F', 'G', 'GY', 'H', 'I', 'Í',
    'J', 'K', 'L', 'LY', 'M', 'N', 'NY', 'O', 'Ó', 'Ö', 'Ő', 'P', 'R', 'S',
    'SZ', 'T', 'TY', 'U', 'Ú', 'Ü', 'Ű', 'V', 'Z', 'ZS'
];

const PREMIUM_LABELS = {
    'DL': '<span class="d-desktop">DUPLA\nBETŰ</span><span class="d-mobile">2×\nBETŰ</span>',
    'TL': '<span class="d-desktop">TRIPLA\nBETŰ</span><span class="d-mobile">3×\nBETŰ</span>',
    'DW': '<span class="d-desktop">DUPLA\nSZÓ</span><span class="d-mobile">2×\nSZÓ</span>',
    'TW': '<span class="d-desktop">TRIPLA\nSZÓ</span><span class="d-mobile">3×\nSZÓ</span>',
    'ST': '<span class="d-desktop">★</span><span class="d-mobile">★</span>',
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

const socket = io({
    reconnection: true,
    reconnectionAttempts: Infinity,
    reconnectionDelay: 1000,
    reconnectionDelayMax: 5000,
    timeout: 120000,
});

// ===== UTILITIES =====

function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

function showScreen(screenId) {
    document.querySelectorAll('.screen').forEach(s => s.classList.add('hidden'));
    document.getElementById(screenId).classList.remove('hidden');
    // Hide global theme toggle on screens that have their own in the topbar
    const globalToggle = document.getElementById('theme-toggle');
    const hasOwnToggle = screenId === 'lobby-screen' || screenId === 'game-screen' || screenId === 'profile-screen' || screenId === 'replay-screen' || screenId === 'waiting-screen';
    if (globalToggle) globalToggle.classList.toggle('hidden', hasOwnToggle);
}

function showMessage(msg, isError = false) {
    const container = document.getElementById('toast-container');
    const toast = document.createElement('div');
    toast.className = 'toast' + (isError ? ' toast-error' : '');
    toast.textContent = msg;
    container.appendChild(toast);

    const dismiss = () => {
        toast.classList.add('toast-out');
        toast.addEventListener('animationend', () => toast.remove());
    };
    setTimeout(dismiss, 3000);
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

function showAuthError(el, msg) {
    el.textContent = msg;
    el.classList.remove('hidden');
}

const isTouchDevice = ('ontouchstart' in window) || (navigator.maxTouchPoints > 0);

// ===== APP STATE =====
// Consolidated game & session state

const AppState = {
    gameState: null,
    myPlayerId: null,
    isOwner: false,
    isGuest: true,
    currentUser: null,
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

    reset() {
        this.currentRoomCode = null;
        this.currentRoomId = null;
        this.reconnectToken = null;
        Chat.clear();
        this.roomName = null;
        this.gameStarted = false;
        this.isRestoreLobby = false;
        this.expectedPlayers = [];
        this.gameOverShown = false;
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

    createGhost(tile, x, y) {
        const ghost = document.createElement('div');
        ghost.className = 'hand-tile drag-ghost touch-drag-ghost';
        ghost.style.left = (x - 22) + 'px';
        ghost.style.top = (y - 22) + 'px';
        ghost.innerHTML = tile.innerHTML;
        document.body.appendChild(ghost);
        this.ghost = ghost;
        return ghost;
    },

    cleanup() {
        if (this.ghost) {
            this.ghost.remove();
            this.ghost = null;
        }
        this.tileIdx = null;
        if (BoardState.lastDropTarget) {
            BoardState.lastDropTarget.classList.remove('drop-target');
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

        // Regisztráció lépések
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
            showAuthError(errorEl, 'Minden mező kitöltése kötelező.');
            return;
        }

        btn.disabled = true;
        try {
            const res = await fetch('/api/auth/login', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ email, password }),
            });
            if (!res.ok) throw new Error('Server error');
            const data = await res.json();

            if (data.success) {
                AppState.currentUser = data.user;
                AppState.isGuest = false;
                Lobby.enter(data.user.display_name);
            } else {
                showAuthError(errorEl, data.message);
            }
        } catch {
            showAuthError(errorEl, 'Hálózati hiba. Próbáld újra.');
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
            showAuthError(errorEl, 'Email cím megadása kötelező.');
            return;
        }

        if (btn) btn.disabled = true;
        try {
            const res = await fetch('/api/auth/request-code', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ email }),
            });
            if (!res.ok) throw new Error('Server error');
            const data = await res.json();

            if (data.success) {
                this.regEmail = email;
                document.getElementById('reg-step-1').classList.add('hidden');
                document.getElementById('reg-step-2').classList.remove('hidden');
                if (data.dev_code) {
                    document.getElementById('reg-code').value = data.dev_code;
                    document.querySelector('#reg-step-2 .step-info').textContent =
                        'Fejlesztői mód: a kód automatikusan kitöltve.';
                }
            } else {
                showAuthError(errorEl, data.message);
            }
        } catch {
            showAuthError(errorEl, 'Hálózati hiba. Próbáld újra.');
        } finally {
            if (btn) btn.disabled = false;
        }
    },

    async resendCode() {
        const errorEl = document.getElementById('reg-error');
        errorEl.classList.add('hidden');

        try {
            const res = await fetch('/api/auth/request-code', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ email: this.regEmail }),
            });
            if (!res.ok) throw new Error('Server error');
            const data = await res.json();
            if (data.success) {
                showAuthError(errorEl, 'Új kód elküldve!');
                errorEl.classList.remove('hidden');
                errorEl.classList.add('text-success');
                setTimeout(() => { errorEl.classList.remove('text-success'); }, 3000);
                if (data.dev_code) {
                    document.getElementById('reg-code').value = data.dev_code;
                }
            } else {
                showAuthError(errorEl, data.message);
            }
        } catch {
            showAuthError(errorEl, 'Hálózati hiba.');
        }
    },

    async verifyCode() {
        const code = document.getElementById('reg-code').value.trim();
        const errorEl = document.getElementById('reg-error');
        errorEl.classList.add('hidden');

        if (!code || code.length !== 6) {
            showAuthError(errorEl, 'A kód 6 számjegyből áll.');
            return;
        }

        try {
            const res = await fetch('/api/auth/verify-code', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ email: this.regEmail, code }),
            });
            if (!res.ok) throw new Error('Server error');
            const data = await res.json();

            if (data.success) {
                document.getElementById('reg-step-2').classList.add('hidden');
                document.getElementById('reg-step-3').classList.remove('hidden');
            } else {
                showAuthError(errorEl, data.message);
            }
        } catch {
            showAuthError(errorEl, 'Hálózati hiba. Próbáld újra.');
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
            showAuthError(errorEl, 'Minden mező kitöltése kötelező.');
            return;
        }
        if (password.length < 6) {
            showAuthError(errorEl, 'A jelszó legalább 6 karakter legyen.');
            return;
        }
        if (password !== password2) {
            showAuthError(errorEl, 'A két jelszó nem egyezik.');
            return;
        }

        if (btn) btn.disabled = true;
        try {
            const res = await fetch('/api/auth/register', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ email: this.regEmail, password, display_name: displayName }),
            });
            if (!res.ok) throw new Error('Server error');
            const data = await res.json();

            if (data.success) {
                AppState.currentUser = data.user;
                AppState.isGuest = false;
                Lobby.enter(data.user.display_name);
            } else {
                showAuthError(errorEl, data.message);
            }
        } catch {
            showAuthError(errorEl, 'Hálózati hiba. Próbáld újra.');
        } finally {
            if (btn) btn.disabled = false;
        }
    },

    guestEnter() {
        const name = document.getElementById('guest-name').value.trim();
        const errorEl = document.getElementById('guest-error');
        if (errorEl) errorEl.classList.add('hidden');
        if (!name) {
            if (errorEl) showAuthError(errorEl, 'Add meg a neved a belépéshez.');
            return;
        }
        AppState.currentUser = null;
        AppState.isGuest = true;
        Lobby.enter(name);
    },

    async logout() {
        if (!AppState.isGuest) {
            try { await fetch('/api/auth/logout', { method: 'POST' }); } catch { /* ignore */ }
        }
        AppState.currentUser = null;
        AppState.isGuest = true;
        AppState.currentRoomCode = null;
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
    },
};

// ===== LOBBY =====

const Lobby = {
    init() {
        document.getElementById('btn-create-room').addEventListener('click', () => this.createRoom());
        document.getElementById('btn-join-by-code').addEventListener('click', () => this.joinByCode());
        document.getElementById('join-code-input').addEventListener('keypress', (e) => {
            if (e.key === 'Enter') this.joinByCode();
        });

        // Lobby nav tab switching
        document.querySelectorAll('.lobby-nav-tab').forEach(tab => {
            tab.addEventListener('click', () => this.switchTab(tab.dataset.lobbyTab));
        });

        // Theme toggle in lobby topbar
        const lobbyThemeBtn = document.getElementById('theme-toggle-lobby');
        if (lobbyThemeBtn) {
            lobbyThemeBtn.addEventListener('click', () => {
                const body = document.body;
                const current = body.getAttribute('data-theme');
                const next = current === 'dark' ? 'light' : 'dark';
                body.setAttribute('data-theme', next);
                localStorage.setItem('scrabble-theme', next);
            });
        }

        socket.on('rooms_list', (rooms) => this.renderRoomsList(rooms));
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
        }
    },

    enter(displayName) {
        socket.emit('set_name', {
            name: displayName,
            is_guest: AppState.isGuest,
            user_id: AppState.currentUser ? AppState.currentUser.id : null,
        });
        AppState.myPlayerId = socket.id;

        document.getElementById('lobby-user-name').textContent =
            displayName + (AppState.isGuest ? ' (vendég)' : '');

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
        const name = document.getElementById('room-name').value.trim() || 'Szoba';
        const maxPlayers = document.getElementById('room-max-players').value;
        const challengeMode = document.getElementById('room-challenge-mode').checked;
        const isPrivate = document.getElementById('room-private').checked;
        const turnTimeLimit = parseInt(document.getElementById('room-turn-limit').value) || 0;
        socket.emit('create_room', {
            name, max_players: maxPlayers,
            challenge_mode: challengeMode, is_private: isPrivate,
            turn_time_limit: turnTimeLimit,
        });
    },

    joinByCode() {
        const code = document.getElementById('join-code-input').value.trim();
        if (!code || code.length !== 6) {
            showMessage('6 számjegyű kódot adj meg.', true);
            return;
        }
        socket.emit('join_room', { code });
    },

    async loadHistory() {
        try {
            const resp = await fetch('/api/auth/profile');
            const data = await resp.json();
            if (!data.success) return;
            this.renderHistory(data.history);
        } catch { /* ignore */ }
    },

    renderHistory(history) {
        const container = document.getElementById('home-history-container');
        if (!history || !history.length) {
            container.innerHTML = '<div class="empty-state"><p class="empty-msg">Nincs még befejezett játék.</p></div>';
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
            date.textContent = new Date(h.created_at).toLocaleDateString('hu-HU');
            info.appendChild(date);

            const room = document.createElement('span');
            room.className = 'history-room';
            room.textContent = h.room_name;
            info.appendChild(room);

            const score = document.createElement('span');
            score.className = 'history-score';
            score.textContent = h.final_score + ' pont';
            info.appendChild(score);

            const result = document.createElement('span');
            result.className = 'history-result';
            result.textContent = h.is_winner ? 'Győzelem' : 'Vereség';
            info.appendChild(result);

            if (h.opponents && h.opponents.length) {
                const opp = document.createElement('span');
                opp.className = 'history-opponents';
                opp.textContent = 'vs ' + h.opponents.map(o => o.player_name).join(', ');
                info.appendChild(opp);
            }

            row.appendChild(info);

            const btn = document.createElement('button');
            btn.className = 'small-btn';
            btn.textContent = 'Visszajátszás';
            btn.addEventListener('click', () => Replay.load(h.game_id));
            row.appendChild(btn);

            container.appendChild(row);
        });
    },

    async loadSavedGames() {
        const container = document.getElementById('saved-games-container');
        if (AppState.isGuest) {
            container.innerHTML = '<div class="empty-state"><p class="empty-msg">Vendégeknek nincs mentett játék.</p></div>';
            return;
        }
        try {
            const resp = await fetch('/api/auth/saved-games');
            const data = await resp.json();
            if (!data.success) {
                container.innerHTML = '<div class="empty-state"><p class="empty-msg">Nem sikerült betölteni.</p></div>';
                return;
            }
            this.renderSavedGames(data.games);
        } catch {
            container.innerHTML = '<div class="empty-state"><p class="empty-msg">Nem sikerült betölteni.</p></div>';
        }
    },

    renderSavedGames(games) {
        const container = document.getElementById('saved-games-container');
        if (!games || !games.length) {
            container.innerHTML = '<div class="empty-state"><p class="empty-msg">Nincs mentett játék.</p></div>';
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
            details.textContent = new Date(g.updated_at || g.created_at).toLocaleDateString('hu-HU', {
                year: 'numeric', month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
            });
            info.appendChild(details);

            if (g.score !== undefined) {
                const scoreDiv = document.createElement('div');
                scoreDiv.className = 'saved-game-score';
                scoreDiv.textContent = `${g.player_name || 'Te'}: ${g.score} pont`;
                info.appendChild(scoreDiv);
            }

            if (g.opponents && g.opponents.length) {
                const opp = document.createElement('div');
                opp.className = 'saved-game-opponents';
                const oppTexts = g.opponents.map(o =>
                    typeof o === 'object' ? `${o.name} (${o.score})` : o
                );
                opp.textContent = 'Ellenfelek: ' + oppTexts.join(', ');
                info.appendChild(opp);
            }

            card.appendChild(info);

            const actions = document.createElement('div');
            actions.className = 'saved-game-actions';

            const resumeBtn = document.createElement('button');
            resumeBtn.textContent = g.is_owner ? 'Visszaállítás' : 'Folytatás';
            resumeBtn.className = 'restored-btn';
            resumeBtn.addEventListener('click', () => {
                if (g.is_owner) {
                    socket.emit('restore_game', { game_id: g.game_id });
                } else {
                    showMessage('Várd meg, amíg a szoba tulajdonosa visszaállítja a játékot.', true);
                }
            });
            actions.appendChild(resumeBtn);

            const abandonBtn = document.createElement('button');
            abandonBtn.textContent = 'Törlés';
            abandonBtn.className = 'btn-abandon';
            abandonBtn.addEventListener('click', () => {
                showConfirm('Törlés', 'Biztosan törölni akarod ezt a mentett játékot?', 'Törlés', async () => {
                    try {
                        const resp = await fetch(`/api/game/${g.game_id}/abandon`, { method: 'POST' });
                        const result = await resp.json();
                        if (result.success) {
                            showMessage('Játék törölve.');
                            this.loadSavedGames();
                            socket.emit('get_rooms');
                        } else {
                            showMessage(result.message || 'Hiba történt.', true);
                        }
                    } catch {
                        showMessage('Hiba történt.', true);
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
            nameDiv.textContent = rejoinInfo.roomName || 'Aktív játék';
            info.appendChild(nameDiv);
            const details = document.createElement('div');
            details.className = 'room-details';
            details.textContent = 'Folyamatban lévő játékod van';
            info.appendChild(details);
            card.appendChild(info);

            const btns = document.createElement('div');
            btns.className = 'room-card-actions';
            const rejoinBtn = document.createElement('button');
            rejoinBtn.textContent = 'Visszacsatlakozás';
            rejoinBtn.className = 'btn-join btn-rejoin';
            rejoinBtn.addEventListener('click', () => this._tryRejoin());
            btns.appendChild(rejoinBtn);
            const dismissBtn = document.createElement('button');
            dismissBtn.textContent = 'Elvetés';
            dismissBtn.className = 'btn-rejoin-dismiss';
            dismissBtn.addEventListener('click', () => this._dismissRejoin());
            btns.appendChild(dismissBtn);
            card.appendChild(btns);
            container.appendChild(card);
        }

        if (!rooms.length && !container.children.length) {
            container.innerHTML = '<div class="empty-state"><p class="empty-msg">Nincs elérhető szoba.</p></div>';
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
            details.textContent = `${room.players}/${room.max_players} játékos \u00b7 ${room.owner}`;

            info.appendChild(nameDiv);
            info.appendChild(details);

            // Badges
            const badges = document.createElement('div');
            badges.className = 'room-badges';
            if (room.started && !room.finished) {
                const b = document.createElement('span');
                b.className = 'room-badge room-badge-playing';
                b.textContent = 'Folyamatban';
                badges.appendChild(b);
            }
            if (room.challenge_mode) {
                const b = document.createElement('span');
                b.className = 'room-badge room-badge-challenge';
                b.textContent = 'Kihívás';
                badges.appendChild(b);
            }
            if (room.turn_time_limit) {
                const b = document.createElement('span');
                b.className = 'room-badge room-badge-timer';
                b.textContent = `\u23F1 ${room.turn_time_limit}mp`;
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
                rejoinBtn.textContent = 'Visszacsatlakozás';
                rejoinBtn.className = 'btn-join btn-rejoin';
                rejoinBtn.addEventListener('click', () => this._tryRejoin());
                btns.appendChild(rejoinBtn);
                const dismissBtn = document.createElement('button');
                dismissBtn.textContent = 'Elvetés';
                dismissBtn.className = 'btn-rejoin-dismiss';
                dismissBtn.addEventListener('click', () => this._dismissRejoin());
                btns.appendChild(dismissBtn);
                card.appendChild(btns);
            } else if (!room.started && !room.finished && room.players < room.max_players) {
                const btn = document.createElement('button');
                btn.textContent = 'Csatlakozás';
                btn.className = 'btn-join';
                btn.addEventListener('click', () => socket.emit('join_room', { room_id: room.id }));
                card.appendChild(btn);
            }

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

        document.getElementById('waiting-challenge-mode').classList.toggle('hidden', !AppState.challengeModeEnabled);
        document.getElementById('waiting-private-mode').classList.toggle('hidden', !data.is_private);
        const turnLimitBadge = document.getElementById('waiting-turn-limit');
        if (data.turn_time_limit) {
            turnLimitBadge.textContent = `\u23F1 ${data.turn_time_limit} mp / kör`;
            turnLimitBadge.classList.remove('hidden');
        } else {
            turnLimitBadge.classList.add('hidden');
        }

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

    onCode(data) {
        AppState.currentRoomCode = data.code;
        document.getElementById('room-code-value').textContent = data.code;
        document.getElementById('room-code-display').classList.remove('hidden');
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
            btn.textContent = 'Másolva!';
            setTimeout(() => { btn.textContent = 'Másolás'; }, 2000);
        }).catch(() => showMessage('Másolás sikertelen.', true));
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
                return `<div class="player-item ${joined ? 'joined' : 'missing'}">${escapeHtml(name)} ${joined ? '(csatlakozott)' : '(v\u00e1rakoz\u00e1s...)'}</div>`;
            }).join('');
        } else {
            container.innerHTML = gs.players.map((p, i) => `
                <div class="player-item ${i === 0 ? 'owner' : ''}">${escapeHtml(p.name)}</div>
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
            // Update game topbar + panel room name
            const gameRoomName = document.getElementById('game-room-name');
            if (gameRoomName) gameRoomName.textContent = AppState.roomName || 'Szoba';
            const panelRoomName = document.getElementById('panel-room-name');
            if (panelRoomName) panelRoomName.textContent = AppState.roomName || 'Szoba';
            // Update lobby room tab
            const roomTab = document.getElementById('nav-tab-room');
            if (roomTab) roomTab.textContent = 'Aktív játék';
            showScreen('game-screen');
            this.build();
            BoardZoom.init();
            // Re-render if game_state arrived before game_started (restore case)
            if (AppState.gameState && AppState.gameState.started) {
                this.renderBoard();
                this.renderHand();
                this.renderScoreboard();
                this.renderGameInfo();
                ChallengeUI.render();
                TurnTimerUI.update(AppState.gameState);
                this.updateButtons();
            }
        });

        socket.on('game_state', (state) => this.onGameState(state));
        socket.on('action_result', (data) => this.onActionResult(data));
        socket.on('error', (data) => showMessage(data.message, true));

        // Action buttons
        document.getElementById('btn-place').addEventListener('click', () => this.placeTiles());
        document.getElementById('btn-exchange').addEventListener('click', () => this.toggleExchange());
        document.getElementById('btn-pass').addEventListener('click', () => socket.emit('pass_turn'));
        document.getElementById('btn-recall').addEventListener('click', () => this.recall());

    },

    onGameState(state) {
        const prevPlayer = this._prevCurrentPlayer;
        AppState.gameState = state;
        AppState.myPlayerId = socket.id;

        // Clear placed tiles when turn changes away from us
        if (state.current_player !== socket.id && BoardState.placedTiles.length > 0) {
            BoardState.clearPlacement();
        }

        if (state.started) {
            if (document.getElementById('game-screen').classList.contains('hidden')) {
                showScreen('game-screen');
                this.build();
                BoardZoom.init();
            }
            this.renderBoard();
            this.renderHand();
            this.renderScoreboard();
            this.renderGameInfo();
            ChallengeUI.render();
            TurnTimerUI.update(state);
            this.updateButtons();

            // Your turn notification
            if (!state.finished && state.current_player === socket.id && prevPlayer !== socket.id) {
                SoundManager.play('your_turn');
            }
            this._prevCurrentPlayer = state.current_player;

            // Update game topbar + panel room name if not set
            const gameRoomName = document.getElementById('game-room-name');
            if (gameRoomName && AppState.roomName) gameRoomName.textContent = AppState.roomName;
            const panelRoomName = document.getElementById('panel-room-name');
            if (panelRoomName && AppState.roomName) panelRoomName.textContent = AppState.roomName;

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
            showMessage(data.message, true);
        } else {
            BoardState.clearPlacement();
            this.renderBoard();
            this.renderHand();
            this.updateButtons();
        }
    },

    build() {
        const board = document.getElementById('board');
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

                cell.addEventListener('click', () => this.onCellClick(r, c));
                board.appendChild(cell);
            }
        }

        if (!BoardState.boardDragInitialized) {
            BoardState.boardDragInitialized = true;
            this._initBoardDrag(board);
        }
    },

    _initBoardDrag(board) {
        board.addEventListener('dragover', (e) => {
            e.preventDefault();
            e.dataTransfer.dropEffect = 'move';
            const cell = e.target.closest('.cell');
            if (cell !== BoardState.lastDropTarget) {
                if (BoardState.lastDropTarget) BoardState.lastDropTarget.classList.remove('drop-target');
                if (cell) cell.classList.add('drop-target');
                BoardState.lastDropTarget = cell;
            }
        });

        board.addEventListener('dragleave', (e) => {
            if (!board.contains(e.relatedTarget)) {
                if (BoardState.lastDropTarget) {
                    BoardState.lastDropTarget.classList.remove('drop-target');
                    BoardState.lastDropTarget = null;
                }
            }
        });

        board.addEventListener('drop', (e) => {
            e.preventDefault();
            if (BoardState.lastDropTarget) {
                BoardState.lastDropTarget.classList.remove('drop-target');
                BoardState.lastDropTarget = null;
            }
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
        const cells = document.querySelectorAll('.cell');
        const hasSelected = BoardState.selectedTileIdx !== null;
        const pendingTiles = gs.pending_challenge ? gs.pending_challenge.tiles : [];

        const placedMap = new Map();
        for (const t of BoardState.placedTiles) placedMap.set(`${t.row},${t.col}`, t);
        const pendingMap = new Map();
        for (const t of pendingTiles) pendingMap.set(`${t.row},${t.col}`, t);

        cells.forEach(cell => {
            const r = parseInt(cell.dataset.row);
            const c = parseInt(cell.dataset.col);
            const key = `${r},${c}`;
            const boardCell = gs.board[r][c];
            const placed = placedMap.get(key);
            const pending = pendingMap.get(key);

            cell.classList.remove('has-tile', 'placed-this-turn', 'can-place', 'pending-challenge-tile');

            if (pending) {
                cell.classList.add('has-tile', 'pending-challenge-tile');
                cell.innerHTML = `${escapeHtml(pending.letter)}<span class="tile-value">${pending.is_blank ? 0 : (TILE_VALUES[pending.letter] || 0)}</span>`;
            } else if (placed) {
                cell.classList.add('has-tile', 'placed-this-turn');
                cell.innerHTML = `${escapeHtml(placed.letter)}<span class="tile-value">${placed.is_blank ? 0 : (TILE_VALUES[placed.letter] || 0)}</span>`;
            } else if (boardCell) {
                cell.classList.add('has-tile');
                cell.innerHTML = `${escapeHtml(boardCell.letter)}<span class="tile-value">${boardCell.is_blank ? 0 : (TILE_VALUES[boardCell.letter] || 0)}</span>`;
            } else {
                if (hasSelected) cell.classList.add('can-place');
                const premium = PREMIUM_MAP[key];
                cell.innerHTML = premium ? `<span class="premium-label">${PREMIUM_LABELS[premium]}</span>` : '';
            }
        });
    },

    renderHand() {
        const gs = AppState.gameState;
        if (!gs) return;
        const handContainer = document.getElementById('hand');
        const myPlayer = gs.players.find(p => p.id === AppState.myPlayerId);
        if (!myPlayer || !myPlayer.hand) return;

        const placedHandIndices = new Set(BoardState.placedTiles.map(t => t.handIdx));

        handContainer.innerHTML = '';
        myPlayer.hand.forEach((tile, idx) => {
            if (placedHandIndices.has(idx)) return;

            const el = document.createElement('div');
            el.className = 'hand-tile';
            if (tile === '') {
                el.classList.add('blank-tile');
                el.innerHTML = `?<span class="tile-value">0</span>`;
            } else {
                el.innerHTML = `${escapeHtml(tile)}<span class="tile-value">${TILE_VALUES[tile] || 0}</span>`;
            }

            if (BoardState.selectedTileIdx === idx) el.classList.add('selected');
            if (BoardState.exchangeMode && BoardState.exchangeIndices.has(idx)) el.classList.add('exchange-selected');

            el.draggable = !isTouchDevice;
            el.addEventListener('dragstart', (e) => {
                e.dataTransfer.setData('text/plain', idx.toString());
                BoardState.selectedTileIdx = idx;
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
                }
            });

            handContainer.appendChild(el);
        });
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
                el.classList.add('selected');
            }, 200);
        }, { passive: true });

        el.addEventListener('touchmove', (e) => {
            touchMoved = true;
            if (TouchDrag.tileIdx !== null) {
                e.preventDefault();
                const touch = e.touches[0];
                if (TouchDrag.ghost) {
                    TouchDrag.ghost.style.left = (touch.clientX - 22) + 'px';
                    TouchDrag.ghost.style.top = (touch.clientY - 22) + 'px';
                }
                const rawEl = document.elementFromPoint(touch.clientX, touch.clientY);
                const targetCell = rawEl && rawEl.closest('.cell');
                if (targetCell !== BoardState.lastDropTarget) {
                    if (BoardState.lastDropTarget) BoardState.lastDropTarget.classList.remove('drop-target');
                    if (targetCell) targetCell.classList.add('drop-target');
                    BoardState.lastDropTarget = targetCell;
                }
            }
        }, { passive: false });

        el.addEventListener('touchend', (e) => {
            clearTimeout(touchStartTimer);
            if (TouchDrag.tileIdx !== null && touchMoved) {
                const touch = e.changedTouches[0];
                const rawEl = document.elementFromPoint(touch.clientX, touch.clientY);
                const targetCell = rawEl && rawEl.closest('.cell');
                if (targetCell) {
                    const r = parseInt(targetCell.dataset.row);
                    const c = parseInt(targetCell.dataset.col);
                    this.placeTileOnBoard(TouchDrag.tileIdx, r, c);
                }
                TouchDrag.cleanup();
                return;
            }
            TouchDrag.cleanup();
        });

        el.addEventListener('touchcancel', () => {
            clearTimeout(touchStartTimer);
            TouchDrag.cleanup();
        });
    },

    renderScoreboard() {
        const gs = AppState.gameState;
        if (!gs) return;
        document.getElementById('scoreboard').innerHTML = gs.players.map(p => `
            <div class="score-item ${p.id === gs.current_player ? 'active' : ''}">
                <span>${escapeHtml(p.name)}</span>
                <span>${p.score}</span>
            </div>
        `).join('');
    },

    renderGameInfo() {
        const gs = AppState.gameState;
        if (!gs) return;
        document.getElementById('tiles-remaining').textContent = `Zsák: ${gs.tiles_remaining} zseton`;

        const isMyTurn = gs.current_player === AppState.myPlayerId;
        const turnEl = document.getElementById('current-turn');
        turnEl.textContent = isMyTurn ? 'Te következel!' : `${gs.current_player_name || '?'} következik`;
        turnEl.classList.toggle('turn-active', isMyTurn);
        turnEl.classList.toggle('turn-inactive', !isMyTurn);

        if (gs.last_action) {
            document.getElementById('last-action').textContent = gs.last_action;
        }
    },

    updateButtons() {
        const gs = AppState.gameState;
        if (!gs) return;
        const isMyTurn = gs.current_player === AppState.myPlayerId && !gs.finished;
        const hasPending = !!gs.pending_challenge;

        document.getElementById('btn-place').disabled = !isMyTurn || BoardState.placedTiles.length === 0 || hasPending;
        document.getElementById('btn-exchange').disabled = !isMyTurn || hasPending;
        document.getElementById('btn-pass').disabled = !isMyTurn || hasPending;

        document.getElementById('btn-exchange').textContent =
            BoardState.exchangeMode ? `Csere (${BoardState.exchangeIndices.size})` : 'Csere';
    },

    onCellClick(row, col) {
        const gs = AppState.gameState;
        if (!gs || gs.finished) return;

        // Clicking a placed tile always removes it (even if another tile is selected)
        const placedIdx = BoardState.placedTiles.findIndex(t => t.row === row && t.col === col);
        if (placedIdx !== -1) {
            BoardState.placedTiles.splice(placedIdx, 1);
            this.renderBoard();
            this.renderHand();
            this.updateButtons();
            return;
        }

        if (BoardState.selectedTileIdx !== null) {
            this.placeTileOnBoard(BoardState.selectedTileIdx, row, col);
            return;
        }
    },

    placeTileOnBoard(handIdx, row, col) {
        const gs = AppState.gameState;
        if (!gs) return;
        const myPlayer = gs.players.find(p => p.id === AppState.myPlayerId);
        if (!myPlayer || !myPlayer.hand) return;

        if (gs.board[row][col] !== null) return;
        if (BoardState.placedTiles.find(t => t.row === row && t.col === col)) return;
        if (BoardState.placedTiles.find(t => t.handIdx === handIdx)) return;

        const tile = myPlayer.hand[handIdx];

        if (tile === '') {
            BlankDialog.show(handIdx, row, col);
        } else {
            BoardState.placedTiles.push({ row, col, letter: tile, is_blank: false, handIdx });
            BoardState.selectedTileIdx = null;
            SoundManager.play('tile_place');
            this.renderBoard();
            this.renderHand();
            this.updateButtons();
        }
    },

    placeTiles() {
        if (!BoardState.placedTiles.length) return;
        const tiles = BoardState.placedTiles.map(t => ({
            row: t.row, col: t.col, letter: t.letter, is_blank: t.is_blank,
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
            BoardState.selectedTileIdx = null;
            BoardState.exchangeMode = true;
            BoardState.exchangeIndices.clear();
            this.renderBoard();
            this.renderHand();
            this.updateButtons();
            showMessage('Jelöld ki a cserélendő zsetonokat, majd nyomd meg újra a Csere gombot.');
        }
    },

    recall() {
        BoardState.clearPlacement();
        this.renderBoard();
        this.renderHand();
        this.updateButtons();
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
                GameBoard.renderBoard();
                GameBoard.renderHand();
                GameBoard.updateButtons();
            });
            container.appendChild(btn);
        });

        const cancelBtn = document.createElement('button');
        cancelBtn.className = 'btn btn-secondary blank-cancel-btn';
        cancelBtn.textContent = 'Mégsem';
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

        this._expiresAt = gs.turn_timer_expires_at * 1000; // s → ms
        this._warningSoundPlayed = false;
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

    init() {
        socket.on('challenge_result', (data) => {
            showMessage(data.message, false);
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
            if (!isMyPlacement) SoundManager.play('vote');
        } else if (pc.expires_at) {
            // Re-sync with server timestamp on each game_state update
            this.timeLeft = Math.max(0, Math.ceil((pc.expires_at * 1000 - Date.now()) / 1000));
        }

        // Info
        this._renderInfo(infoEl, pc, gs);

        // Timer display
        timerEl.textContent = this.timeLeft > 0 ? `${this.timeLeft} mp` : '';

        // Buttons
        buttonsEl.innerHTML = '';
        if (isMyPlacement) {
            this._addWaitText(buttonsEl, 'Szavazás folyamatban...');
        } else if (myVote) {
            this._addWaitText(buttonsEl, myVote === 'accept' ? 'Elfogadtad — várakozás...' : 'Elutasítottad — várakozás...');
        } else {
            this._renderVoteButtons(buttonsEl);
        }
    },

    _renderInfo(infoEl, pc, gs) {
        infoEl.replaceChildren();
        const infoText = document.createElement('div');
        infoText.appendChild(document.createTextNode(`${escapeHtml(pc.player_name)}: `));
        
        pc.words.forEach((word, index) => {
            const link = document.createElement('a');
            const query = `${word} - Kézikönyvtár A magyar nyelv értelmező szótára`;
            link.href = `https://www.google.com/search?q=${encodeURIComponent(query)}`;
            link.target = '_blank';
            link.rel = 'noopener noreferrer';
            link.className = 'dict-link';
            link.textContent = word;
            link.title = `Keresés: ${word}`;
            
            infoText.appendChild(link);
            
            if (index < pc.words.length - 1) {
                infoText.appendChild(document.createTextNode(', '));
            }
        });
        
        infoText.appendChild(document.createTextNode(` (${pc.score} pont)`));
        infoEl.appendChild(infoText);

        // Show vote status for all players (3+ players)
        if ((pc.player_count || 0) > 2) {
            const voteList = document.createElement('div');
            voteList.className = 'vote-list';
            const votes = pc.votes || {};
            for (const player of gs.players) {
                if (player.id === pc.player_id) continue;
                const vote = votes[player.id];
                const item = document.createElement('span');
                item.className = 'vote-item';
                if (vote === 'accept') {
                    item.textContent = `${escapeHtml(player.name)}: Elfogad`;
                    item.classList.add('vote-accept');
                } else if (vote === 'reject') {
                    item.textContent = `${escapeHtml(player.name)}: Elutasít`;
                    item.classList.add('vote-reject');
                } else {
                    item.textContent = `${escapeHtml(player.name)}: ...`;
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
        acceptBtn.textContent = 'Elfogadom';
        const rejectBtn = document.createElement('button');
        rejectBtn.className = 'btn-challenge';
        rejectBtn.textContent = 'Elutasítom';

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

    onMessage(msg, skipSound = false) {
        AppState.chatMessages.push(msg);
        if (!skipSound && (!msg.sid || msg.sid !== socket.id)) {
            SoundManager.play('chat');
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
    },

    show() {
        if (AppState.gameOverShown) return;
        AppState.gameOverShown = true;
        const gs = AppState.gameState;
        const dialog = document.getElementById('game-over-dialog');
        const scoresContainer = document.getElementById('final-scores');
        SoundManager.play('game_over');

        const sorted = [...gs.players].sort((a, b) => b.score - a.score);
        scoresContainer.innerHTML = sorted.map((p, i) => `
            <div class="score-final ${i === 0 ? 'winner' : ''}">
                <span>${i === 0 ? '&#x1F3C6; ' : ''}${escapeHtml(p.name)}</span>
                <span>${p.score} pont</span>
            </div>
        `).join('');

        dialog.classList.remove('hidden');
    },

    backToLobby() {
        document.getElementById('game-over-dialog').classList.add('hidden');
        AppState.reset();
        ChallengeUI.stopCountdown(); TurnTimerUI._stop();
        socket.emit('leave_room');
        showScreen('lobby-screen');
        socket.emit('get_rooms');
    },
};

// ===== RECONNECTION =====

const Reconnection = {
    init() {
        socket.on('connect', () => {
            const banner = document.getElementById('connection-banner');
            if (banner) banner.classList.add('hidden');
            
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
            }
        });

        socket.on('disconnect', () => {
            const banner = document.getElementById('connection-banner');
            if (banner) {
                banner.textContent = 'Kapcsolat megszakadt — újracsatlakozás...';
                banner.classList.remove('hidden');
            }
        });

        socket.on('connect_error', () => {
            const banner = document.getElementById('connection-banner');
            if (banner) {
                banner.textContent = 'Nem sikerül csatlakozni a szerverhez...';
                banner.classList.remove('hidden');
            }
        });

        socket.on('rejoin_failed', (data) => {
            AppState.reset();
            ChallengeUI.stopCountdown(); TurnTimerUI._stop();
            showScreen('lobby-screen');
            socket.emit('get_rooms');
            if (data && data.message) showMessage(data.message, true);
        });

        socket.on('player_disconnected', (data) => {
            showMessage(`${data.name} lecsatlakozott, várakozás újracsatlakozásra...`, false);
        });

        socket.on('player_reconnected', (data) => {
            showMessage(`${data.name} újracsatlakozott!`, false);
        });

        socket.on('room_disbanded', (data) => {
            AppState.reset();
            ChallengeUI.stopCountdown(); TurnTimerUI._stop();
            localStorage.removeItem('scrabble-rejoin');
            showScreen('lobby-screen');
            socket.emit('get_rooms');
            showMessage(data.message || 'A szoba megszűnt.', true);
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
        const gs = AppState.gameState;
        const isActiveGame = gs && gs.started && !gs.finished;
        const showOwner = AppState.isOwner && isActiveGame;

        document.getElementById('exit-dialog-text').textContent =
            showOwner ? 'Mit szeretnél tenni?' : 'Biztosan ki akarsz lépni?';
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
        // Wait briefly for save confirmation, then leave
        const onResult = (data) => {
            socket.off('action_result', onResult);
            if (data.success) showMessage('Játék mentve.');
            else showMessage(data.message || 'Mentési hiba.', true);
            this._doLeave();
        };
        socket.on('action_result', onResult);
        // Fallback: leave after 3s even if no response
        setTimeout(() => {
            socket.off('action_result', onResult);
            this._doLeave();
        }, 3000);
    },

    leave() {
        this.hideDialog();
        this._doLeave();
    },

    _doLeave() {
        socket.emit('leave_room');
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
        document.getElementById('btn-profile-back').addEventListener('click', () => {
            showScreen('lobby-screen');
            socket.emit('get_rooms');
        });
    },

    async show() {
        try {
            const resp = await fetch('/api/auth/profile');
            const data = await resp.json();
            if (!data.success) {
                showMessage(data.message || 'Hiba a profil betöltésekor.', true);
                return;
            }

            this.renderStats(data.stats);
            this.renderHistory(data.history);
            const nameEl = document.getElementById('profile-user-name');
            if (nameEl) nameEl.textContent = AppState.currentUser?.display_name || '';
            showScreen('profile-screen');
        } catch {
            showMessage('Hiba a profil betöltésekor.', true);
        }
    },

    renderStats(stats) {
        const container = document.getElementById('profile-stats');
        container.innerHTML = '';
        const cards = [
            { label: 'Játszott', value: stats.games_played },
            { label: 'Győzelem', value: stats.games_won },
            { label: 'Nyerési arány', value: stats.win_rate + '%' },
            { label: 'Átl. pontszám', value: stats.avg_score },
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
            container.innerHTML = '<p class="empty-msg">Nincs még befejezett játék.</p>';
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
            date.textContent = h.created_at ? h.created_at.replace('T', ' ').substring(0, 16) : '';

            const name = document.createElement('span');
            name.className = 'history-room';
            name.textContent = h.room_name || 'Szoba';

            const score = document.createElement('span');
            score.className = 'history-score';
            score.textContent = h.final_score + ' pont';

            const result = document.createElement('span');
            result.className = 'history-result';
            result.textContent = h.is_winner ? 'Győzelem' : 'Vereség';

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
            btn.textContent = 'Visszajátszás';
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

    init() {
        document.getElementById('btn-replay-back').addEventListener('click', () => Profile.show());
        document.getElementById('btn-replay-prev').addEventListener('click', () => this.prev());
        document.getElementById('btn-replay-next').addEventListener('click', () => this.next());
    },

    async load(gameId) {
        try {
            const resp = await fetch(`/api/game/${gameId}/moves`);
            const data = await resp.json();
            if (!data.success) {
                showMessage(data.message || 'Hiba a lépések betöltésekor.', true);
                return;
            }

            this.moves = data.moves;
            this.currentIdx = -1;
            this.buildBoard();
            this.renderMove();
            showScreen('replay-screen');
        } catch {
            showMessage('Hiba a lépések betöltésekor.', true);
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
            info.textContent = 'Kezdő állapot (üres tábla)';
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
                if (details.score) text += ` (${details.score} pont)`;
                break;
            case 'exchange':
                text += 'Csere';
                break;
            case 'pass':
                text += 'Passz';
                break;
            case 'challenge_reject':
                text += 'Szavak elutasítva';
                break;
            default:
                text += move.action_type;
        }

        info.textContent = text;
        const snapshot = move.board_snapshot_json ? JSON.parse(move.board_snapshot_json) : null;
        this._renderSnapshot(snapshot);
    },

    _renderSnapshot(boardData) {
        const board = document.getElementById('replay-board');
        const cells = board.querySelectorAll('.cell');

        cells.forEach(cell => {
            const r = parseInt(cell.dataset.row);
            const c = parseInt(cell.dataset.col);
            const key = `${r},${c}`;

            cell.classList.remove('has-tile');

            if (boardData && boardData[r] && boardData[r][c]) {
                const tile = boardData[r][c];
                cell.classList.add('has-tile');
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
        { key: 'tile_place',      label: 'Betű lerakás',       desc: 'Kattanó hang zsetonok lerakásakor' },
        { key: 'vote',            label: 'Szavazás',            desc: 'Hangjelzés szavazógomb megnyomásakor' },
        { key: 'challenge_result',label: 'Szavazás eredménye',  desc: 'Elfogadva / elutasítva hangjelzés' },
        { key: 'your_turn',       label: 'Te következel',       desc: 'Értesítő hang kör váltáskor' },
        { key: 'chat',            label: 'Chat üzenet',         desc: 'Hangjelzés bejövő üzenetnél' },
        { key: 'game_events',     label: 'Játék események',     desc: 'Játék kezdés és vége hangok' },
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
            name.textContent = cat.label;
            const desc = document.createElement('span');
            desc.className = 'toggle-desc';
            desc.textContent = cat.desc;
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
            showMessage(data.message, !data.success);
            if (data.success) this.load();
        });

        socket.on('friend_request_received', (data) => {
            showMessage(`${data.from_name} barátkérést küldött!`);
            this.load();
        });

        socket.on('friend_request_accepted', (data) => {
            showMessage(`${data.display_name} elfogadta a barátkérésedet!`);
            this.load();
        });

        socket.on('invite_sent', (data) => {
            showMessage(data.message, !data.success);
        });

        socket.on('game_invite', (data) => this.showInvitePopup(data));

        socket.on('invite_accepted', (data) => {
            socket.emit('join_room', { code: data.join_code });
        });

        // Várakozó szobában barátok meghívása
        document.getElementById('btn-invite-friends')?.addEventListener('click', () => this.toggleInviteList());
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
            container.innerHTML = '<div class="empty-state"><i>👥</i><p>Még nincsenek barátaid.</p></div>';
            return;
        }

        container.innerHTML = this.friendsList.map(f => `
            <div class="friend-item">
                <div class="friend-item-name">
                    <span class="status-dot ${f.online ? 'online' : 'offline'}"></span>
                    ${escapeHtml(f.display_name)}
                </div>
                <div class="friend-item-actions">
                    <button class="small-btn danger" onclick="Friends.removeFriend(${f.id})">Törlés</button>
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
                    <button class="small-btn btn-primary" onclick="Friends.acceptRequest(${r.id})">Elfogad</button>
                    <button class="small-btn danger" onclick="Friends.declineRequest(${r.id})">Elutasít</button>
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
                <div class="text-muted text-sm">Folyamatban</div>
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
                    if (isFriend) actionHtml = '<span class="text-muted text-sm">Barát</span>';
                    else if (isPending) actionHtml = '<span class="text-muted text-sm">Kérés érkezett</span>';
                    else if (isSent) actionHtml = '<span class="text-muted text-sm">Kérés elküldve</span>';
                    else actionHtml = `<button class="small-btn" onclick="Friends.sendRequest(${u.id})">Kérés küldése</button>`;

                    return `
                        <div class="friend-item">
                            <div class="friend-item-name">${escapeHtml(u.display_name)}</div>
                            <div class="friend-item-actions">${actionHtml}</div>
                        </div>
                    `;
                }).join('');
                resultsEl.classList.remove('hidden');
            } else {
                resultsEl.innerHTML = '<div class="padding-3 text-muted">Nincs találat.</div>';
                resultsEl.classList.remove('hidden');
            }
        } catch {
            showMessage('Hiba a keresés során.', true);
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
        showConfirm('Barát törlése', 'Biztosan törölni szeretnéd a barátaid közül?', 'Törlés', () => {
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
                container.innerHTML = '<div class="padding-3 text-muted text-center text-sm">Nincs elérhető barátod jelenleg.</div>';
            } else {
                container.innerHTML = onlineFriends.map(f => `
                    <div class="friend-item">
                        <div class="friend-item-name text-sm">
                            <span class="status-dot online"></span>
                            ${escapeHtml(f.display_name)}
                        </div>
                        <button class="small-btn" onclick="Friends.inviteToRoom(${f.id})">Meghívás</button>
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
        toast.className = 'toast';
        toast.style.display = 'flex';
        toast.style.flexDirection = 'column';
        toast.style.gap = '12px';
        toast.style.borderLeft = '4px solid var(--accent)';
        
        const text = document.createElement('div');
        text.style.fontWeight = '500';
        text.textContent = `${data.from_name} meghívott ide: "${data.room_name}"`;
        
        const btnContainer = document.createElement('div');
        btnContainer.style.display = 'flex';
        btnContainer.style.gap = '8px';
        
        const acceptBtn = document.createElement('button');
        acceptBtn.className = 'btn-primary small-btn';
        acceptBtn.textContent = 'Csatlakozás';
        acceptBtn.style.flex = '1';
        
        const declineBtn = document.createElement('button');
        declineBtn.className = 'danger small-btn';
        declineBtn.textContent = 'Elutasítás';
        declineBtn.style.flex = '1';
        declineBtn.style.marginTop = '0'; // Override default danger margin-top
        
        btnContainer.appendChild(acceptBtn);
        btnContainer.appendChild(declineBtn);
        
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
            if (toast.parentElement) {
                socket.emit('respond_invite', { invite_id: data.invite_id, accept: false });
                dismiss();
            }
        }, 15000);

        SoundManager.play('chat');
    }
};

// ===== INITIALIZATION =====

SoundManager.init();
SoundSettings.init();
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
