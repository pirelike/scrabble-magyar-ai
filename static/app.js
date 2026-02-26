const socket = io({
    reconnection: true,
    reconnectionAttempts: Infinity,
    reconnectionDelay: 1000,
    reconnectionDelayMax: 5000,
    timeout: 120000,
});

// Magyar betűk és pontértékek
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

// Premium mező elrendezés
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

const PREMIUM_LABELS = {
    'DL': 'DUPLA\nBETŰ',
    'TL': 'TRIPLA\nBETŰ',
    'DW': 'DUPLA\nSZÓ',
    'TW': 'TRIPLA\nSZÓ',
    'ST': '★',
};

// Állapot
let gameState = null;
let myPlayerId = null;
let isOwner = false;
let isGuest = true;
let currentUser = null;  // {id, email, display_name} ha be van jelentkezve
let selectedTileIdx = null;
let exchangeMode = false;
let exchangeIndices = new Set();
let placedTiles = []; // [{row, col, letter, is_blank, handIdx}]
let currentRoomCode = null;
let currentRoomId = null;
let reconnectToken = null;
let boardDragInitialized = false;
let challengeTimer = null;
let challengeTimeLeft = 0;
let chatMessages = [];
let challengeModeEnabled = false;
let wasVotingPhase = false;
let lastDropTarget = null;  // Aktuálisan kijelölt cella drag közben

// Képernyő váltás
function showScreen(screenId) {
    document.querySelectorAll('.screen').forEach(s => s.classList.add('hidden'));
    document.getElementById(screenId).classList.remove('hidden');
}

// ===== AUTH RENDSZER =====

// Tab váltás
document.querySelectorAll('.auth-tab').forEach(tab => {
    tab.addEventListener('click', () => {
        document.querySelectorAll('.auth-tab').forEach(t => t.classList.remove('active'));
        document.querySelectorAll('.auth-tab-content').forEach(c => c.classList.remove('active'));
        tab.classList.add('active');
        document.getElementById('tab-' + tab.dataset.tab).classList.add('active');
    });
});

// --- Bejelentkezés ---
document.getElementById('login-form').addEventListener('submit', async (e) => {
    e.preventDefault();
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
            currentUser = data.user;
            isGuest = false;
            enterLobby(data.user.display_name);
        } else {
            showAuthError(errorEl, data.message);
        }
    } catch {
        showAuthError(errorEl, 'Hálózati hiba. Próbáld újra.');
    }
});

// --- Regisztráció ---
let regEmail = '';

document.getElementById('btn-reg-send-code').addEventListener('click', async () => {
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
            regEmail = email;
            document.getElementById('reg-step-1').classList.add('hidden');
            document.getElementById('reg-step-2').classList.remove('hidden');
            if (data.dev_code) {
                document.getElementById('reg-code').value = data.dev_code;
                const info = document.querySelector('#reg-step-2 .step-info');
                info.textContent = 'Fejlesztői mód: a kód automatikusan kitöltve.';
            }
        } else {
            showAuthError(errorEl, data.message);
        }
    } catch {
        showAuthError(errorEl, 'Hálózati hiba. Próbáld újra.');
    }
});

document.getElementById('btn-reg-resend').addEventListener('click', async () => {
    const errorEl = document.getElementById('reg-error');
    errorEl.classList.add('hidden');

    try {
        const res = await fetch('/api/auth/request-code', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ email: regEmail }),
        });
        const data = await res.json();
        if (data.success) {
            showAuthError(errorEl, 'Új kód elküldve!');
            errorEl.classList.remove('hidden');
            errorEl.style.color = '#4CAF50';
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
});

document.getElementById('btn-reg-verify-code').addEventListener('click', async () => {
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
            body: JSON.stringify({ email: regEmail, code }),
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
});

document.getElementById('btn-reg-finish').addEventListener('click', async () => {
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
            body: JSON.stringify({ email: regEmail, password, display_name: displayName }),
        });
        const data = await res.json();

        if (data.success) {
            currentUser = data.user;
            isGuest = false;
            enterLobby(data.user.display_name);
        } else {
            showAuthError(errorEl, data.message);
        }
    } catch {
        showAuthError(errorEl, 'Hálózati hiba. Próbáld újra.');
    }
});

// --- Vendég belépés ---
document.getElementById('btn-guest-enter').addEventListener('click', () => {
    const name = document.getElementById('guest-name').value.trim();
    const errorEl = document.getElementById('guest-error');
    if (errorEl) errorEl.classList.add('hidden');
    if (!name) {
        if (errorEl) showAuthError(errorEl, 'Add meg a neved a belépéshez.');
        return;
    }
    currentUser = null;
    isGuest = true;
    enterLobby(name);
});

document.getElementById('guest-name').addEventListener('keypress', (e) => {
    if (e.key === 'Enter') document.getElementById('btn-guest-enter').click();
});

// --- Közös belépés a lobbyba ---
function enterLobby(displayName) {
    socket.emit('set_name', {
        name: displayName,
        is_guest: isGuest,
        user_id: currentUser ? currentUser.id : null,
    });
    myPlayerId = socket.id;

    // Lobby UI beállítás
    document.getElementById('lobby-user-name').textContent = displayName + (isGuest ? ' (vendég)' : '');

    // Szoba létrehozás szekció megjelenítése
    document.getElementById('create-room-section').classList.remove('hidden');

    showScreen('lobby-screen');
    socket.emit('get_rooms');
}

// --- Kijelentkezés ---
document.getElementById('btn-logout').addEventListener('click', async () => {
    if (!isGuest) {
        try {
            await fetch('/api/auth/logout', { method: 'POST' });
        } catch { /* ignore */ }
    }
    currentUser = null;
    isGuest = true;
    currentRoomCode = null;
    // Reset regisztráció lépések
    document.getElementById('reg-step-1').classList.remove('hidden');
    document.getElementById('reg-step-2').classList.add('hidden');
    document.getElementById('reg-step-3').classList.add('hidden');
    document.getElementById('reg-email').value = '';
    document.getElementById('reg-code').value = '';
    document.getElementById('reg-display-name').value = '';
    document.getElementById('reg-password').value = '';
    document.getElementById('reg-password2').value = '';
    showScreen('auth-screen');
});

// --- Auto-login (oldal betöltéskor) ---
async function checkSession() {
    try {
        const res = await fetch('/api/auth/me');
        const data = await res.json();
        if (data.success) {
            currentUser = data.user;
            isGuest = false;
            enterLobby(data.user.display_name);
        }
    } catch { /* no session */ }
}

function showAuthError(el, msg) {
    el.textContent = msg;
    el.classList.remove('hidden');
}

// ===== Lobby =====
document.getElementById('btn-create-room').addEventListener('click', () => {
    const name = document.getElementById('room-name').value.trim() || 'Szoba';
    const maxPlayers = document.getElementById('room-max-players').value;
    const challengeMode = document.getElementById('room-challenge-mode').checked;
    socket.emit('create_room', { name, max_players: maxPlayers, challenge_mode: challengeMode });
});

// Csatlakozás kóddal
document.getElementById('btn-join-by-code').addEventListener('click', () => {
    const code = document.getElementById('join-code-input').value.trim();
    if (!code || code.length !== 6) {
        showMessage('6 számjegyű kódot adj meg.', true);
        return;
    }
    socket.emit('join_room', { code });
});

document.getElementById('join-code-input').addEventListener('keypress', (e) => {
    if (e.key === 'Enter') document.getElementById('btn-join-by-code').click();
});

socket.on('rooms_list', (rooms) => {
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
            btn.addEventListener('click', () => {
                socket.emit('join_room', { room_id: room.id });
            });
            card.appendChild(btn);
        }

        container.appendChild(card);
    });
});

// ===== Szoba =====
socket.on('room_joined', (data) => {
    isOwner = data.is_owner;
    currentRoomId = data.room_id;
    if (data.reconnect_token) reconnectToken = data.reconnect_token;
    challengeModeEnabled = data.challenge_mode || false;
    document.getElementById('waiting-room-name').textContent = data.room_name;

    // Challenge mód badge
    const challengeBadge = document.getElementById('waiting-challenge-mode');
    if (challengeModeEnabled) {
        challengeBadge.classList.remove('hidden');
    } else {
        challengeBadge.classList.add('hidden');
    }

    // Kód megjelenítés reset
    const codeSection = document.getElementById('room-code-display');
    if (isOwner) {
        // A kódot a 'room_code' event-ben kapjuk
        codeSection.classList.remove('hidden');
    } else {
        codeSection.classList.add('hidden');
    }

    showScreen('waiting-screen');
    updateWaitingRoom();
});

// Csatlakozási kód fogadása (csak tulajdonos kapja)
socket.on('room_code', (data) => {
    currentRoomCode = data.code;
    const codeEl = document.getElementById('room-code-value');
    codeEl.textContent = data.code;
    document.getElementById('room-code-display').classList.remove('hidden');
});

// Kód másolása
document.getElementById('btn-copy-code').addEventListener('click', () => {
    if (currentRoomCode) {
        navigator.clipboard.writeText(currentRoomCode).then(() => {
            const btn = document.getElementById('btn-copy-code');
            btn.textContent = 'Másolva!';
            setTimeout(() => { btn.textContent = 'Másolás'; }, 2000);
        }).catch(() => {
            showMessage('Másolás sikertelen.', true);
        });
    }
});

socket.on('room_left', () => {
    currentRoomCode = null;
    currentRoomId = null;
    reconnectToken = null;
    chatMessages = [];
    stopChallengeCountdown();
    showScreen('lobby-screen');
    socket.emit('get_rooms');
});

document.getElementById('btn-leave-room').addEventListener('click', () => {
    socket.emit('leave_room');
});

document.getElementById('btn-start-game').addEventListener('click', () => {
    socket.emit('start_game');
});

function updateWaitingRoom() {
    if (!gameState) return;
    const container = document.getElementById('waiting-players');
    container.innerHTML = gameState.players.map((p, i) => `
        <div class="player-item ${i === 0 ? 'owner' : ''}">${escapeHtml(p.name)}</div>
    `).join('');

    const startBtn = document.getElementById('btn-start-game');
    if (isOwner && gameState.players.length >= 1) {
        startBtn.classList.remove('hidden');
    } else {
        startBtn.classList.add('hidden');
    }
}

// ===== Játék =====
socket.on('game_started', () => {
    showScreen('game-screen');
    buildBoard();
});

socket.on('game_state', (state) => {
    gameState = state;
    myPlayerId = socket.id;

    if (state.started) {
        if (document.getElementById('game-screen').classList.contains('hidden')) {
            showScreen('game-screen');
            buildBoard();
        }
        renderBoard();
        renderHand();
        renderScoreboard();
        renderGameInfo();
        renderChallengeSection();
        updateButtons();

        if (state.finished) {
            stopChallengeCountdown();
            showGameOver();
        }
    } else {
        updateWaitingRoom();
    }
});

socket.on('action_result', (data) => {
    if (!data.success) {
        showMessage(data.message, true);
    } else {
        placedTiles = [];
        selectedTileIdx = null;
        exchangeMode = false;
        exchangeIndices.clear();
        // Újra renderelünk, mert a game_state előbb érkezik mint az action_result,
        // így a renderHand() a régi placedTiles indexekkel szűrt — most hogy töröltük,
        // az új kéz helyesen jelenik meg az összes újonnan húzott zsetonnal.
        renderBoard();
        renderHand();
        updateButtons();
    }
});

socket.on('error', (data) => {
    showMessage(data.message, true);
});

function showMessage(msg, isError = false) {
    const container = document.getElementById('toast-container');
    const toast = document.createElement('div');
    toast.className = 'toast' + (isError ? ' toast-error' : '');
    toast.textContent = msg;
    container.appendChild(toast);

    // Auto-eltüntetés
    setTimeout(() => {
        toast.classList.add('toast-out');
        toast.addEventListener('animationend', () => toast.remove());
    }, 3000);

    // Kattintásra eltüntetés
    toast.addEventListener('click', () => {
        toast.classList.add('toast-out');
        toast.addEventListener('animationend', () => toast.remove());
    });
}

// Touch-eszköz felismerés
const isTouchDevice = ('ontouchstart' in window) || (navigator.maxTouchPoints > 0);

// Touch drag állapot
let touchDragTile = null;
let touchDragGhost = null;

function createDragGhost(tile, x, y) {
    const ghost = document.createElement('div');
    ghost.className = 'hand-tile drag-ghost';
    ghost.style.cssText = `position:fixed;left:${x - 22}px;top:${y - 22}px;z-index:150;pointer-events:none;opacity:0.85;`;
    ghost.innerHTML = tile.innerHTML;
    document.body.appendChild(ghost);
    return ghost;
}

function removeDragGhost() {
    if (touchDragGhost) {
        touchDragGhost.remove();
        touchDragGhost = null;
    }
    touchDragTile = null;
    if (lastDropTarget) {
        lastDropTarget.classList.remove('drop-target');
        lastDropTarget = null;
    }
}

// Tábla felépítése
function buildBoard() {
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
            }

            cell.addEventListener('click', () => onCellClick(r, c));
            board.appendChild(cell);
        }
    }

    // Board-level drag event delegation (robust against child elements like premium labels)
    if (!boardDragInitialized) {
        boardDragInitialized = true;

        board.addEventListener('dragover', (e) => {
            e.preventDefault();
            e.dataTransfer.dropEffect = 'move';
            const cell = e.target.closest('.cell');
            if (cell !== lastDropTarget) {
                if (lastDropTarget) lastDropTarget.classList.remove('drop-target');
                if (cell) cell.classList.add('drop-target');
                lastDropTarget = cell;
            }
        });

        board.addEventListener('dragleave', (e) => {
            if (!board.contains(e.relatedTarget)) {
                if (lastDropTarget) {
                    lastDropTarget.classList.remove('drop-target');
                    lastDropTarget = null;
                }
            }
        });

        board.addEventListener('drop', (e) => {
            e.preventDefault();
            if (lastDropTarget) {
                lastDropTarget.classList.remove('drop-target');
                lastDropTarget = null;
            }
            const cell = e.target.closest('.cell');
            if (cell) {
                const r = parseInt(cell.dataset.row);
                const c = parseInt(cell.dataset.col);
                const handIdx = parseInt(e.dataTransfer.getData('text/plain'));
                if (!isNaN(handIdx)) {
                    placeTileOnBoard(handIdx, r, c);
                }
            }
        });
    }
}

function renderBoard() {
    if (!gameState) return;
    const cells = document.querySelectorAll('.cell');
    const hasSelected = selectedTileIdx !== null;
    const pendingTiles = gameState.pending_challenge ? gameState.pending_challenge.tiles : [];

    // Map-ek a O(1) kereséshez O(n*m) find() helyett
    const placedMap = new Map();
    for (const t of placedTiles) {
        placedMap.set(`${t.row},${t.col}`, t);
    }
    const pendingMap = new Map();
    for (const t of pendingTiles) {
        pendingMap.set(`${t.row},${t.col}`, t);
    }

    cells.forEach(cell => {
        const r = parseInt(cell.dataset.row);
        const c = parseInt(cell.dataset.col);
        const key = `${r},${c}`;
        const boardCell = gameState.board[r][c];
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
            if (hasSelected) {
                cell.classList.add('can-place');
            }
            const premium = PREMIUM_MAP[key];
            if (premium) {
                cell.innerHTML = `<span class="premium-label">${PREMIUM_LABELS[premium]}</span>`;
            } else {
                cell.innerHTML = '';
            }
        }
    });
}

function renderHand() {
    if (!gameState) return;
    const handContainer = document.getElementById('hand');
    const myPlayer = gameState.players.find(p => p.id === myPlayerId);
    if (!myPlayer || !myPlayer.hand) return;

    // Kizárjuk a már lerakott zsetonokat
    const placedHandIndices = new Set(placedTiles.map(t => t.handIdx));

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

        if (selectedTileIdx === idx) {
            el.classList.add('selected');
        }
        if (exchangeMode && exchangeIndices.has(idx)) {
            el.classList.add('exchange-selected');
        }

        el.draggable = !isTouchDevice;
        el.addEventListener('dragstart', (e) => {
            e.dataTransfer.setData('text/plain', idx.toString());
            selectedTileIdx = idx;
        });

        // Touch drag support
        if (isTouchDevice) {
            let touchStartTimer = null;
            let touchMoved = false;

            el.addEventListener('touchstart', (e) => {
                touchMoved = false;
                touchStartTimer = setTimeout(() => {
                    // Hosszú érintés: drag indítás
                    e.preventDefault();
                    touchDragTile = idx;
                    selectedTileIdx = idx;
                    const touch = e.touches[0];
                    touchDragGhost = createDragGhost(el, touch.clientX, touch.clientY);
                    el.classList.add('selected');
                }, 200);
            }, { passive: false });

            el.addEventListener('touchmove', (e) => {
                touchMoved = true;
                if (touchDragTile !== null) {
                    e.preventDefault();
                    const touch = e.touches[0];
                    if (touchDragGhost) {
                        touchDragGhost.style.left = (touch.clientX - 22) + 'px';
                        touchDragGhost.style.top = (touch.clientY - 22) + 'px';
                    }
                    // Highlight cella alatta
                    const rawEl = document.elementFromPoint(touch.clientX, touch.clientY);
                    const targetCell = rawEl && rawEl.closest('.cell');
                    if (targetCell !== lastDropTarget) {
                        if (lastDropTarget) lastDropTarget.classList.remove('drop-target');
                        if (targetCell) targetCell.classList.add('drop-target');
                        lastDropTarget = targetCell;
                    }
                }
            }, { passive: false });

            el.addEventListener('touchend', (e) => {
                clearTimeout(touchStartTimer);
                if (touchDragTile !== null && touchMoved) {
                    // Drop a célcellára
                    const touch = e.changedTouches[0];
                    const rawEl = document.elementFromPoint(touch.clientX, touch.clientY);
                    const targetCell = rawEl && rawEl.closest('.cell');
                    if (targetCell) {
                        const r = parseInt(targetCell.dataset.row);
                        const c = parseInt(targetCell.dataset.col);
                        placeTileOnBoard(touchDragTile, r, c);
                    }
                    removeDragGhost();
                    return;
                }
                removeDragGhost();
            });
        }

        el.addEventListener('click', () => {
            if (exchangeMode) {
                if (exchangeIndices.has(idx)) {
                    exchangeIndices.delete(idx);
                } else {
                    exchangeIndices.add(idx);
                }
                renderHand();
            } else {
                selectedTileIdx = (selectedTileIdx === idx) ? null : idx;
                renderHand();
            }
        });

        handContainer.appendChild(el);
    });
}

function renderScoreboard() {
    if (!gameState) return;
    const sb = document.getElementById('scoreboard');
    sb.innerHTML = gameState.players.map(p => `
        <div class="score-item ${p.id === gameState.current_player ? 'active' : ''}">
            <span>${escapeHtml(p.name)}</span>
            <span>${p.score}</span>
        </div>
    `).join('');
}

function renderGameInfo() {
    if (!gameState) return;
    document.getElementById('tiles-remaining').textContent = `Zsák: ${gameState.tiles_remaining} zseton`;

    const isMyTurn = gameState.current_player === myPlayerId;
    document.getElementById('current-turn').textContent = isMyTurn ?
        'Te következel!' :
        `${gameState.current_player_name || '?'} következik`;
    document.getElementById('current-turn').style.color = isMyTurn ? '#e8b930' : '#aaa';

    if (gameState.last_action) {
        document.getElementById('last-action').textContent = gameState.last_action;
    }
}

function updateButtons() {
    if (!gameState) return;
    const isMyTurn = gameState.current_player === myPlayerId && !gameState.finished;
    const hasPending = !!gameState.pending_challenge;

    document.getElementById('btn-place').disabled = !isMyTurn || placedTiles.length === 0 || hasPending;
    document.getElementById('btn-exchange').disabled = !isMyTurn || hasPending;
    document.getElementById('btn-pass').disabled = !isMyTurn || hasPending;

    if (exchangeMode) {
        document.getElementById('btn-exchange').textContent = `Csere (${exchangeIndices.size})`;
    } else {
        document.getElementById('btn-exchange').textContent = 'Csere';
    }
}

function onCellClick(row, col) {
    if (!gameState || gameState.current_player !== myPlayerId) return;

    // Ha van kiválasztott zseton, lerakjuk
    if (selectedTileIdx !== null) {
        placeTileOnBoard(selectedTileIdx, row, col);
        return;
    }

    // Ha ez egy ebben a körben lerakott zseton, visszavesszük
    const placedIdx = placedTiles.findIndex(t => t.row === row && t.col === col);
    if (placedIdx !== -1) {
        placedTiles.splice(placedIdx, 1);
        renderBoard();
        renderHand();
        updateButtons();
    }
}

function placeTileOnBoard(handIdx, row, col) {
    if (!gameState) return;
    const myPlayer = gameState.players.find(p => p.id === myPlayerId);
    if (!myPlayer || !myPlayer.hand) return;

    // Ellenőrzések
    if (gameState.board[row][col] !== null) return;
    if (placedTiles.find(t => t.row === row && t.col === col)) return;
    if (placedTiles.find(t => t.handIdx === handIdx)) return;

    const tile = myPlayer.hand[handIdx];

    if (tile === '') {
        // Joker - betű választás
        showBlankDialog(handIdx, row, col);
    } else {
        placedTiles.push({ row, col, letter: tile, is_blank: false, handIdx });
        selectedTileIdx = null;
        renderBoard();
        renderHand();
        updateButtons();
    }
}

function showBlankDialog(handIdx, row, col) {
    const dialog = document.getElementById('blank-dialog');
    const lettersContainer = document.getElementById('blank-letters');

    lettersContainer.innerHTML = '';
    ALL_LETTERS.forEach(l => {
        const btn = document.createElement('button');
        btn.textContent = l;
        btn.addEventListener('click', () => {
            placedTiles.push({ row, col, letter: l, is_blank: true, handIdx });
            selectedTileIdx = null;
            dialog.classList.add('hidden');
            renderBoard();
            renderHand();
            updateButtons();
        });
        lettersContainer.appendChild(btn);
    });

    dialog.classList.remove('hidden');
}

// Akció gombok
document.getElementById('btn-place').addEventListener('click', () => {
    if (!placedTiles.length) return;

    const tiles = placedTiles.map(t => ({
        row: t.row,
        col: t.col,
        letter: t.letter,
        is_blank: t.is_blank,
    }));

    socket.emit('place_tiles', { tiles });
});

document.getElementById('btn-exchange').addEventListener('click', () => {
    if (exchangeMode) {
        if (exchangeIndices.size > 0) {
            socket.emit('exchange_tiles', { indices: Array.from(exchangeIndices) });
            exchangeMode = false;
            exchangeIndices.clear();
        }
    } else {
        // Visszavonjuk a lerakott zsetonokat
        placedTiles = [];
        selectedTileIdx = null;
        exchangeMode = true;
        exchangeIndices.clear();
        renderBoard();
        renderHand();
        updateButtons();
        showMessage('Jelöld ki a cserélendő zsetonokat, majd nyomd meg újra a Csere gombot.');
    }
});

document.getElementById('btn-pass').addEventListener('click', () => {
    socket.emit('pass_turn');
});

document.getElementById('btn-recall').addEventListener('click', () => {
    placedTiles = [];
    selectedTileIdx = null;
    exchangeMode = false;
    exchangeIndices.clear();
    renderBoard();
    renderHand();
    updateButtons();
});

// ===== Challenge rendszer =====

function startChallengeCountdown() {
    stopChallengeCountdown();
    challengeTimeLeft = 30;
    challengeTimer = setInterval(() => {
        challengeTimeLeft--;
        if (challengeTimeLeft <= 0) {
            stopChallengeCountdown();
        }
        renderChallengeSection();
    }, 1000);
}

function stopChallengeCountdown() {
    if (challengeTimer) {
        clearInterval(challengeTimer);
        challengeTimer = null;
    }
    challengeTimeLeft = 0;
}

function renderChallengeSection() {
    if (!gameState) return;
    const section = document.getElementById('challenge-section');
    const infoEl = document.getElementById('challenge-info');
    const timerEl = document.getElementById('challenge-timer');
    const buttonsEl = document.getElementById('challenge-buttons');

    if (!gameState.pending_challenge) {
        section.classList.add('hidden');
        if (challengeTimer) stopChallengeCountdown();
        wasVotingPhase = false;
        return;
    }

    section.classList.remove('hidden');
    const pc = gameState.pending_challenge;
    const isMyPlacement = pc.player_id === myPlayerId;
    const isVotingPhase = pc.voting_phase || false;
    const playerCount = pc.player_count || 0;
    const myAccepted = (pc.accepted_players || []).includes(myPlayerId);
    const myVote = (pc.votes || {})[myPlayerId];
    const isChallenger = pc.challenger_id === myPlayerId;

    // Timer indítása ha még nem fut, vagy újraindítása ha szavazási fázis kezdődött
    if (!challengeTimer) {
        startChallengeCountdown();
        wasVotingPhase = isVotingPhase;
    } else if (isVotingPhase && !wasVotingPhase) {
        // Szavazási fázis most kezdődött: timer újraindítás
        startChallengeCountdown();
        wasVotingPhase = true;
    }

    // --- Info szekció: lerakott szavak ---
    infoEl.innerHTML = '';
    const infoText = document.createElement('div');
    infoText.textContent = `${escapeHtml(pc.player_name)}: ${pc.words.join(', ')} (${pc.score} pont)`;
    infoEl.appendChild(infoText);

    // Szavazási fázisban: státusz megjelenítése
    if (isVotingPhase) {
        const voteStatus = document.createElement('div');
        voteStatus.className = 'vote-status';
        const challengerName = pc.challenger_name || '?';
        voteStatus.textContent = `${escapeHtml(challengerName)} megtámadta — szavazás folyamatban`;
        infoEl.appendChild(voteStatus);

        // Szavazatok megjelenítése
        const votes = pc.votes || {};
        const voteList = document.createElement('div');
        voteList.className = 'vote-list';
        for (const player of gameState.players) {
            if (player.id === pc.player_id) continue; // Lerakó nem szavaz
            if (player.id === pc.challenger_id) continue; // Megtámadó nem szavaz
            const vote = votes[player.id];
            const voteItem = document.createElement('span');
            voteItem.className = 'vote-item';
            if (vote === 'accept') {
                voteItem.textContent = `${escapeHtml(player.name)}: Elfogad`;
                voteItem.classList.add('vote-accept');
            } else if (vote === 'reject') {
                voteItem.textContent = `${escapeHtml(player.name)}: Elutasít`;
                voteItem.classList.add('vote-reject');
            } else {
                voteItem.textContent = `${escapeHtml(player.name)}: ...`;
                voteItem.classList.add('vote-pending');
            }
            voteList.appendChild(voteItem);
        }
        infoEl.appendChild(voteList);
    }

    timerEl.textContent = challengeTimeLeft > 0 ? `${challengeTimeLeft} mp` : '';

    // --- Gombok ---
    buttonsEl.innerHTML = '';

    if (isMyPlacement) {
        // Lerakó: várakozás
        const waitText = document.createElement('div');
        waitText.className = 'challenge-wait';
        waitText.textContent = isVotingPhase
            ? 'Szavazás folyamatban...'
            : 'Várakozás elfogadásra...';
        buttonsEl.appendChild(waitText);
    } else if (isVotingPhase) {
        // Szavazási fázis: szavazó gombok
        if (isChallenger) {
            const waitText = document.createElement('div');
            waitText.className = 'challenge-wait';
            waitText.textContent = 'Te megtámadtad — várakozás a szavazásra...';
            buttonsEl.appendChild(waitText);
        } else if (myVote) {
            const waitText = document.createElement('div');
            waitText.className = 'challenge-wait';
            waitText.textContent = myVote === 'accept'
                ? 'Elfogadtad — várakozás...'
                : 'Elutasítottad — várakozás...';
            buttonsEl.appendChild(waitText);
        } else {
            const acceptBtn = document.createElement('button');
            acceptBtn.className = 'btn-accept';
            acceptBtn.textContent = 'Elfogad';
            acceptBtn.addEventListener('click', () => {
                socket.emit('cast_vote', { vote: 'accept' });
            });
            buttonsEl.appendChild(acceptBtn);

            const rejectBtn = document.createElement('button');
            rejectBtn.className = 'btn-challenge';
            rejectBtn.textContent = 'Elutasít';
            rejectBtn.addEventListener('click', () => {
                socket.emit('cast_vote', { vote: 'reject' });
            });
            buttonsEl.appendChild(rejectBtn);
        }
    } else if (playerCount <= 2) {
        // 2 játékos: Elfogad + Elutasít gombok
        if (myAccepted) {
            const waitText = document.createElement('div');
            waitText.className = 'challenge-wait';
            waitText.textContent = 'Elfogadva — várakozás...';
            buttonsEl.appendChild(waitText);
        } else {
            const rejectBtn = document.createElement('button');
            rejectBtn.className = 'btn-challenge';
            rejectBtn.textContent = 'Elutasít';
            rejectBtn.addEventListener('click', () => {
                socket.emit('reject_words');
            });
            buttonsEl.appendChild(rejectBtn);

            const acceptBtn = document.createElement('button');
            acceptBtn.className = 'btn-accept';
            acceptBtn.textContent = 'Elfogad';
            acceptBtn.addEventListener('click', () => {
                socket.emit('accept_words');
            });
            buttonsEl.appendChild(acceptBtn);
        }
    } else {
        // 3+ játékos, megtámadási ablak: Megtámad + Elfogad gombok
        if (myAccepted) {
            const waitText = document.createElement('div');
            waitText.className = 'challenge-wait';
            waitText.textContent = 'Elfogadtad — várakozás...';
            buttonsEl.appendChild(waitText);
        } else {
            const challengeBtn = document.createElement('button');
            challengeBtn.className = 'btn-challenge';
            challengeBtn.textContent = 'Megtámad';
            challengeBtn.addEventListener('click', () => {
                socket.emit('challenge');
            });
            buttonsEl.appendChild(challengeBtn);

            const acceptBtn = document.createElement('button');
            acceptBtn.className = 'btn-accept';
            acceptBtn.textContent = 'Elfogad';
            acceptBtn.addEventListener('click', () => {
                socket.emit('accept_words');
            });
            buttonsEl.appendChild(acceptBtn);
        }
    }
}

socket.on('challenge_result', (data) => {
    showMessage(data.message, false);
    stopChallengeCountdown();
});

// ===== Chat =====

socket.on('chat_message', (msg) => {
    chatMessages.push(msg);
    const container = document.getElementById('chat-messages');
    if (!container) return;

    // Max 100 üzenet: DOM-ból is töröljük a legrégebbit
    if (chatMessages.length > 100) {
        chatMessages.shift();
        if (container.firstChild) container.removeChild(container.firstChild);
    }

    // Csak az új üzenetet fűzzük hozzá
    _appendChatMsg(container, msg);
    container.scrollTop = container.scrollHeight;
});

function _appendChatMsg(container, msg) {
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
}

function renderChatMessages() {
    const container = document.getElementById('chat-messages');
    if (!container) return;
    container.innerHTML = '';
    chatMessages.forEach(msg => _appendChatMsg(container, msg));
    container.scrollTop = container.scrollHeight;
}

document.getElementById('btn-send-chat').addEventListener('click', () => {
    const input = document.getElementById('chat-input');
    const message = input.value.trim();
    if (!message) return;
    socket.emit('send_chat', { message });
    input.value = '';
});

document.getElementById('chat-input').addEventListener('keypress', (e) => {
    if (e.key === 'Enter') document.getElementById('btn-send-chat').click();
});

// Játék vége
function showGameOver() {
    const dialog = document.getElementById('game-over-dialog');
    const scoresContainer = document.getElementById('final-scores');

    const sorted = [...gameState.players].sort((a, b) => b.score - a.score);
    scoresContainer.innerHTML = sorted.map((p, i) => `
        <div class="score-final ${i === 0 ? 'winner' : ''}">
            <span>${i === 0 ? '&#x1F3C6; ' : ''}${escapeHtml(p.name)}</span>
            <span>${p.score} pont</span>
        </div>
    `).join('');

    dialog.classList.remove('hidden');
}

document.getElementById('btn-back-lobby').addEventListener('click', () => {
    document.getElementById('game-over-dialog').classList.add('hidden');
    currentRoomCode = null;
    currentRoomId = null;
    reconnectToken = null;
    chatMessages = [];
    stopChallengeCountdown();
    socket.emit('leave_room');
    showScreen('lobby-screen');
    socket.emit('get_rooms');
});

// Segéd
function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

// ===== Reconnect kezelés =====

socket.on('connect', () => {
    // Újracsatlakozás után: ha volt aktív szoba, megpróbáljuk visszakapni
    if (reconnectToken) {
        socket.emit('rejoin_room', { token: reconnectToken });
    }
});

socket.on('rejoin_failed', () => {
    // Reconnect sikertelen: visszadobjuk a lobbyba
    reconnectToken = null;
    currentRoomId = null;
    currentRoomCode = null;
    chatMessages = [];
    stopChallengeCountdown();
    showScreen('lobby-screen');
    socket.emit('get_rooms');
});

socket.on('player_disconnected', (data) => {
    showMessage(`${data.name} lecsatlakozott, várakozás újracsatlakozásra...`, false);
});

socket.on('player_reconnected', (data) => {
    showMessage(`${data.name} újracsatlakozott!`, false);
});

// Amikor az oldal visszakerül előtérbe (telefon feloldás, tab váltás),
// ellenőrizzük a kapcsolatot és ha kell, reconnectelünk
document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') {
        if (!socket.connected) {
            socket.connect();
        }
    }
});

// Oldal betöltéskor: session ellenőrzés
checkSession();
