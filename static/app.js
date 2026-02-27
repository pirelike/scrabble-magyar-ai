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
    'DL': 'DUPLA\nBETŰ',
    'TL': 'TRIPLA\nBETŰ',
    'DW': 'DUPLA\nSZÓ',
    'TW': 'TRIPLA\nSZÓ',
    'ST': '★',
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

    reset() {
        this.currentRoomCode = null;
        this.currentRoomId = null;
        this.reconnectToken = null;
        this.chatMessages = [];
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
        ghost.className = 'hand-tile drag-ghost';
        ghost.style.cssText = `position:fixed;left:${x - 22}px;top:${y - 22}px;z-index:150;pointer-events:none;opacity:0.85;`;
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
        errorEl.classList.add('hidden');

        if (!email || !password) {
            showAuthError(errorEl, 'Minden mező kitöltése kötelező.');
            return;
        }

        try {
            const res = await fetch('/api/auth/login', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ email, password }),
            });
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
        }
    },

    async sendCode() {
        const email = document.getElementById('reg-email').value.trim();
        const errorEl = document.getElementById('reg-error');
        errorEl.classList.add('hidden');

        if (!email) {
            showAuthError(errorEl, 'Email cím megadása kötelező.');
            return;
        }

        try {
            const res = await fetch('/api/auth/request-code', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ email }),
            });
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
            const data = await res.json();
            if (data.success) {
                showAuthError(errorEl, 'Új kód elküldve!');
                errorEl.classList.remove('hidden');
                errorEl.style.color = 'var(--success)';
                setTimeout(() => { errorEl.style.color = ''; }, 3000);
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

        try {
            const res = await fetch('/api/auth/register', {
                method: 'POST',
                headers: { 'Content-Type': 'application/json' },
                body: JSON.stringify({ email: this.regEmail, password, display_name: displayName }),
            });
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

        socket.on('rooms_list', (rooms) => this.renderRoomsList(rooms));
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
        document.getElementById('create-room-section').classList.remove('hidden');

        showScreen('lobby-screen');
        socket.emit('get_rooms');
    },

    createRoom() {
        const name = document.getElementById('room-name').value.trim() || 'Szoba';
        const maxPlayers = document.getElementById('room-max-players').value;
        const challengeMode = document.getElementById('room-challenge-mode').checked;
        const isPrivate = document.getElementById('room-private').checked;
        socket.emit('create_room', {
            name, max_players: maxPlayers,
            challenge_mode: challengeMode, is_private: isPrivate,
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

    renderRoomsList(rooms) {
        const container = document.getElementById('rooms-container');
        if (!rooms.length) {
            container.innerHTML = '<p class="empty-msg">Nincs elérhető szoba.</p>';
            return;
        }
        container.innerHTML = '';
        rooms.forEach(room => {
            const card = document.createElement('div');
            card.className = 'room-card';

            const info = document.createElement('div');
            info.className = 'room-info';

            const nameDiv = document.createElement('div');
            nameDiv.className = 'room-name';
            nameDiv.textContent = room.name;

            const details = document.createElement('div');
            details.className = 'room-details';
            details.textContent = `${room.players}/${room.max_players} játékos | Tulajdonos: ${room.owner} | ${room.started ? 'Folyamatban' : 'Várakozik'}${room.challenge_mode ? ' | Megtámadás' : ''}`;

            info.appendChild(nameDiv);
            info.appendChild(details);
            card.appendChild(info);

            if (!room.started && room.players < room.max_players) {
                const btn = document.createElement('button');
                btn.textContent = 'Csatlakozás';
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
        if (data.reconnect_token) AppState.reconnectToken = data.reconnect_token;
        AppState.challengeModeEnabled = data.challenge_mode || false;
        document.getElementById('waiting-room-name').textContent = data.room_name;

        document.getElementById('waiting-challenge-mode').classList.toggle('hidden', !AppState.challengeModeEnabled);
        document.getElementById('waiting-private-mode').classList.toggle('hidden', !data.is_private);

        const codeSection = document.getElementById('room-code-display');
        codeSection.classList.toggle('hidden', !AppState.isOwner);

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
        ChallengeUI.stopCountdown();
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
        container.innerHTML = gs.players.map((p, i) => `
            <div class="player-item ${i === 0 ? 'owner' : ''}">${escapeHtml(p.name)}</div>
        `).join('');

        const startBtn = document.getElementById('btn-start-game');
        startBtn.classList.toggle('hidden', !(AppState.isOwner && gs.players.length >= 1));
    },
};

// ===== GAME BOARD =====

const GameBoard = {
    init() {
        socket.on('game_started', () => {
            showScreen('game-screen');
            this.build();
            BoardZoom.init();
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
        AppState.gameState = state;
        AppState.myPlayerId = socket.id;

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
            this.updateButtons();

            if (state.finished) {
                ChallengeUI.stopCountdown();
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
                    label.textContent = PREMIUM_LABELS[premium];
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
                cell.innerHTML = `${pending.letter}<span class="tile-value">${pending.is_blank ? 0 : (TILE_VALUES[pending.letter] || 0)}</span>`;
            } else if (placed) {
                cell.classList.add('has-tile', 'placed-this-turn');
                cell.innerHTML = `${placed.letter}<span class="tile-value">${placed.is_blank ? 0 : (TILE_VALUES[placed.letter] || 0)}</span>`;
            } else if (boardCell) {
                cell.classList.add('has-tile');
                cell.innerHTML = `${boardCell.letter}<span class="tile-value">${boardCell.is_blank ? 0 : (TILE_VALUES[boardCell.letter] || 0)}</span>`;
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
                el.innerHTML = `${tile}<span class="tile-value">${TILE_VALUES[tile] || 0}</span>`;
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

        el.addEventListener('touchstart', (e) => {
            touchMoved = false;
            touchStartTimer = setTimeout(() => {
                e.preventDefault();
                TouchDrag.tileIdx = idx;
                BoardState.selectedTileIdx = idx;
                const touch = e.touches[0];
                TouchDrag.createGhost(el, touch.clientX, touch.clientY);
                el.classList.add('selected');
            }, 200);
        }, { passive: false });

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
        turnEl.style.color = isMyTurn ? 'var(--accent)' : 'var(--text-secondary)';
        turnEl.style.fontWeight = isMyTurn ? '700' : '';

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
        if (!gs || gs.current_player !== AppState.myPlayerId) return;

        if (BoardState.selectedTileIdx !== null) {
            this.placeTileOnBoard(BoardState.selectedTileIdx, row, col);
            return;
        }

        const placedIdx = BoardState.placedTiles.findIndex(t => t.row === row && t.col === col);
        if (placedIdx !== -1) {
            BoardState.placedTiles.splice(placedIdx, 1);
            this.renderBoard();
            this.renderHand();
            this.updateButtons();
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
                BoardState.exchangeMode = false;
                BoardState.exchangeIndices.clear();
            }
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
                GameBoard.renderBoard();
                GameBoard.renderHand();
                GameBoard.updateButtons();
            });
            container.appendChild(btn);
        });

        dialog.classList.remove('hidden');
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
        });
    },

    startCountdown() {
        this.stopCountdown();
        this.timeLeft = 30;
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
            this.wasVotingPhase = false;
            return;
        }

        section.classList.remove('hidden');
        const pc = gs.pending_challenge;
        const isMyPlacement = pc.player_id === AppState.myPlayerId;
        const isVotingPhase = pc.voting_phase || false;
        const playerCount = pc.player_count || 0;
        const myAccepted = (pc.accepted_players || []).includes(AppState.myPlayerId);
        const myVote = (pc.votes || {})[AppState.myPlayerId];
        const isChallenger = pc.challenger_id === AppState.myPlayerId;

        // Timer management
        if (!this.timer) {
            this.startCountdown();
            this.wasVotingPhase = isVotingPhase;
        } else if (isVotingPhase && !this.wasVotingPhase) {
            this.startCountdown();
            this.wasVotingPhase = true;
        }

        // Info
        this._renderInfo(infoEl, pc, gs, isVotingPhase);

        // Timer display
        timerEl.textContent = this.timeLeft > 0 ? `${this.timeLeft} mp` : '';

        // Buttons
        buttonsEl.innerHTML = '';
        if (isMyPlacement) {
            this._addWaitText(buttonsEl, isVotingPhase ? 'Szavazás folyamatban...' : 'Várakozás elfogadásra...');
        } else if (isVotingPhase) {
            this._renderVotingButtons(buttonsEl, isChallenger, myVote);
        } else if (playerCount <= 2) {
            this._render2PlayerButtons(buttonsEl, myAccepted);
        } else {
            this._render3PlusButtons(buttonsEl, myAccepted);
        }
    },

    _renderInfo(infoEl, pc, gs, isVotingPhase) {
        infoEl.innerHTML = '';
        const infoText = document.createElement('div');
        infoText.textContent = `${escapeHtml(pc.player_name)}: ${pc.words.join(', ')} (${pc.score} pont)`;
        infoEl.appendChild(infoText);

        if (isVotingPhase) {
            const voteStatus = document.createElement('div');
            voteStatus.className = 'vote-status';
            voteStatus.textContent = `${escapeHtml(pc.challenger_name || '?')} megtámadta — szavazás folyamatban`;
            infoEl.appendChild(voteStatus);

            const voteList = document.createElement('div');
            voteList.className = 'vote-list';
            const votes = pc.votes || {};
            for (const player of gs.players) {
                if (player.id === pc.player_id || player.id === pc.challenger_id) continue;
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

    _renderVotingButtons(buttonsEl, isChallenger, myVote) {
        if (isChallenger) {
            this._addWaitText(buttonsEl, 'Te megtámadtad — várakozás a szavazásra...');
        } else if (myVote) {
            this._addWaitText(buttonsEl, myVote === 'accept' ? 'Elfogadtad — várakozás...' : 'Elutasítottad — várakozás...');
        } else {
            this._addAcceptRejectButtons(buttonsEl, 'cast_vote');
        }
    },

    _render2PlayerButtons(buttonsEl, myAccepted) {
        if (myAccepted) {
            this._addWaitText(buttonsEl, 'Elfogadva — várakozás...');
        } else {
            const rejectBtn = document.createElement('button');
            rejectBtn.className = 'btn-challenge';
            rejectBtn.textContent = 'Elutasít';
            rejectBtn.addEventListener('click', () => socket.emit('reject_words'));
            buttonsEl.appendChild(rejectBtn);

            const acceptBtn = document.createElement('button');
            acceptBtn.className = 'btn-accept';
            acceptBtn.textContent = 'Elfogad';
            acceptBtn.addEventListener('click', () => socket.emit('accept_words'));
            buttonsEl.appendChild(acceptBtn);
        }
    },

    _render3PlusButtons(buttonsEl, myAccepted) {
        if (myAccepted) {
            this._addWaitText(buttonsEl, 'Elfogadtad — várakozás...');
        } else {
            const challengeBtn = document.createElement('button');
            challengeBtn.className = 'btn-challenge';
            challengeBtn.textContent = 'Megtámad';
            challengeBtn.addEventListener('click', () => socket.emit('challenge'));
            buttonsEl.appendChild(challengeBtn);

            const acceptBtn = document.createElement('button');
            acceptBtn.className = 'btn-accept';
            acceptBtn.textContent = 'Elfogad';
            acceptBtn.addEventListener('click', () => socket.emit('accept_words'));
            buttonsEl.appendChild(acceptBtn);
        }
    },

    _addAcceptRejectButtons(buttonsEl, eventType) {
        const acceptBtn = document.createElement('button');
        acceptBtn.className = 'btn-accept';
        acceptBtn.textContent = 'Elfogad';
        acceptBtn.addEventListener('click', () => socket.emit(eventType, { vote: 'accept' }));
        buttonsEl.appendChild(acceptBtn);

        const rejectBtn = document.createElement('button');
        rejectBtn.className = 'btn-challenge';
        rejectBtn.textContent = 'Elutasít';
        rejectBtn.addEventListener('click', () => socket.emit(eventType, { vote: 'reject' }));
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

    send() {
        const input = document.getElementById('chat-input');
        const message = input.value.trim();
        if (!message) return;
        socket.emit('send_chat', { message });
        input.value = '';
    },

    onMessage(msg) {
        AppState.chatMessages.push(msg);
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
        const gs = AppState.gameState;
        const dialog = document.getElementById('game-over-dialog');
        const scoresContainer = document.getElementById('final-scores');

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
        ChallengeUI.stopCountdown();
        socket.emit('leave_room');
        showScreen('lobby-screen');
        socket.emit('get_rooms');
    },
};

// ===== RECONNECTION =====

const Reconnection = {
    init() {
        socket.on('connect', () => {
            if (AppState.reconnectToken) {
                socket.emit('rejoin_room', { token: AppState.reconnectToken });
            }
        });

        socket.on('rejoin_failed', () => {
            AppState.reset();
            ChallengeUI.stopCountdown();
            showScreen('lobby-screen');
            socket.emit('get_rooms');
        });

        socket.on('player_disconnected', (data) => {
            showMessage(`${data.name} lecsatlakozott, várakozás újracsatlakozásra...`, false);
        });

        socket.on('player_reconnected', (data) => {
            showMessage(`${data.name} újracsatlakozott!`, false);
        });

        document.addEventListener('visibilitychange', () => {
            if (document.visibilityState === 'visible' && !socket.connected) {
                socket.connect();
            }
        });
    },
};

// ===== INITIALIZATION =====

Auth.init();
Lobby.init();
WaitingRoom.init();
GameBoard.init();
ChallengeUI.init();
Chat.init();
GameOver.init();
Reconnection.init();

// Auto-login on page load
Auth.checkSession();
