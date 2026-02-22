const socket = io();

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
let selectedTileIdx = null;
let exchangeMode = false;
let exchangeIndices = new Set();
let placedTiles = []; // [{row, col, letter, is_blank, handIdx}]

// Képernyő váltás
function showScreen(screenId) {
    document.querySelectorAll('.screen').forEach(s => s.classList.add('hidden'));
    document.getElementById(screenId).classList.remove('hidden');
}

// ===== Név képernyő =====
document.getElementById('btn-set-name').addEventListener('click', () => {
    const name = document.getElementById('player-name').value.trim();
    if (!name) return;
    socket.emit('set_name', { name });
    myPlayerId = socket.id;
    showScreen('lobby-screen');
    socket.emit('get_rooms');
});

document.getElementById('player-name').addEventListener('keypress', (e) => {
    if (e.key === 'Enter') document.getElementById('btn-set-name').click();
});

// ===== Lobby =====
document.getElementById('btn-create-room').addEventListener('click', () => {
    const name = document.getElementById('room-name').value.trim() || 'Szoba';
    const maxPlayers = document.getElementById('room-max-players').value;
    socket.emit('create_room', { name, max_players: maxPlayers });
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
        details.textContent = `${room.players}/${room.max_players} játékos | Tulajdonos: ${room.owner} | ${room.started ? 'Folyamatban' : 'Várakozik'}`;

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
    document.getElementById('waiting-room-name').textContent = data.room_name;
    showScreen('waiting-screen');
    updateWaitingRoom();
});

socket.on('room_left', () => {
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

    if (state.started && !document.getElementById('game-screen').classList.contains('hidden') === false) {
        showScreen('game-screen');
        buildBoard();
    }

    if (state.started) {
        if (document.getElementById('game-screen').classList.contains('hidden')) {
            showScreen('game-screen');
            buildBoard();
        }
        renderBoard();
        renderHand();
        renderScoreboard();
        renderGameInfo();
        updateButtons();

        if (state.finished) {
            showGameOver();
        }
    } else {
        updateWaitingRoom();
    }
});

socket.on('action_result', (data) => {
    if (!data.success) {
        showMessage(data.message);
    } else {
        placedTiles = [];
        selectedTileIdx = null;
        exchangeMode = false;
        exchangeIndices.clear();
    }
});

socket.on('error', (data) => {
    showMessage(data.message);
});

function showMessage(msg) {
    // Egyszerű alert - lehetne szebb is
    alert(msg);
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
            cell.addEventListener('dragover', (e) => {
                e.preventDefault();
                cell.classList.add('drop-target');
            });
            cell.addEventListener('dragleave', () => {
                cell.classList.remove('drop-target');
            });
            cell.addEventListener('drop', (e) => {
                e.preventDefault();
                cell.classList.remove('drop-target');
                const handIdx = parseInt(e.dataTransfer.getData('text/plain'));
                placeTileOnBoard(handIdx, r, c);
            });

            board.appendChild(cell);
        }
    }
}

function renderBoard() {
    if (!gameState) return;
    const cells = document.querySelectorAll('.cell');
    cells.forEach(cell => {
        const r = parseInt(cell.dataset.row);
        const c = parseInt(cell.dataset.col);
        const boardCell = gameState.board[r][c];

        // Lerakott zseton ebben a körben?
        const placed = placedTiles.find(t => t.row === r && t.col === c);

        cell.classList.remove('has-tile', 'placed-this-turn');

        if (placed) {
            cell.classList.add('has-tile', 'placed-this-turn');
            cell.innerHTML = `${placed.letter}<span class="tile-value">${placed.is_blank ? 0 : (TILE_VALUES[placed.letter] || 0)}</span>`;
        } else if (boardCell) {
            cell.classList.add('has-tile');
            cell.innerHTML = `${boardCell.letter}<span class="tile-value">${boardCell.is_blank ? 0 : (TILE_VALUES[boardCell.letter] || 0)}</span>`;
        } else {
            const premium = PREMIUM_MAP[`${r},${c}`];
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

        el.draggable = true;
        el.addEventListener('dragstart', (e) => {
            e.dataTransfer.setData('text/plain', idx.toString());
            selectedTileIdx = idx;
        });

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

    document.getElementById('btn-place').disabled = !isMyTurn || placedTiles.length === 0;
    document.getElementById('btn-exchange').disabled = !isMyTurn;
    document.getElementById('btn-pass').disabled = !isMyTurn;

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
        showMessage('Kattints a cserélendő zsetonokra, majd nyomd meg újra a Csere gombot.');
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

// Játék vége
function showGameOver() {
    const dialog = document.getElementById('game-over-dialog');
    const scoresContainer = document.getElementById('final-scores');

    const sorted = [...gameState.players].sort((a, b) => b.score - a.score);
    scoresContainer.innerHTML = sorted.map((p, i) => `
        <div class="score-final ${i === 0 ? 'winner' : ''}">
            <span>${i === 0 ? '🏆 ' : ''}${escapeHtml(p.name)}</span>
            <span>${p.score} pont</span>
        </div>
    `).join('');

    dialog.classList.remove('hidden');
}

document.getElementById('btn-back-lobby').addEventListener('click', () => {
    document.getElementById('game-over-dialog').classList.add('hidden');
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
