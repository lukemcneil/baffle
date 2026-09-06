"use strict";
const $ = (id) => document.getElementById(id);
const lobby = $('lobby');
const waitingRoom = $('waiting-room');
const gameScreen = $('game-screen');
const gameOver = $('game-over');
const playerName = $('player-name');
const roomCodeInput = $('room-code');
const lobbyStatus = $('lobby-status');
const createButton = $('create-btn');
const joinButton = $('join-btn');
const startButton = $('start-btn');
const modeSelect = $('mode-select');
const modeDescription = $('mode-description');
const boardEl = $('board');
const currentWordEl = $('current-word');
const submitButton = $('submit-btn');
const toastEl = $('toast');
let socket = null;
let lobbySocket = null;
let state = null;
let currentRoom = '';
let selectedPath = [];
let timerHandle = null;
let toastHandle = null;
function wsUrl(path) {
    const params = new URLSearchParams(location.search);
    const host = params.get('server') || location.host;
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
    return `${protocol}//${host}${path}`;
}
function showScreen(screen) {
    [lobby, waitingRoom, gameScreen, gameOver].forEach(item => item.classList.add('hidden'));
    screen.classList.remove('hidden');
}
function roomCode() {
    const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZ';
    return Array.from({ length: 4 }, () => chars[Math.floor(Math.random() * chars.length)]).join('');
}
function connect(code) {
    const name = playerName.value.trim();
    if (!name) {
        lobbyStatus.textContent = 'Give yourself a name first.';
        playerName.focus();
        return;
    }
    currentRoom = code.toUpperCase();
    localStorage.setItem('baffle_name', name);
    lobbyStatus.textContent = 'Joining the room…';
    if (socket)
        socket.close();
    disconnectLobby();
    socket = new WebSocket(wsUrl(`/game/${encodeURIComponent(currentRoom)}?player=${encodeURIComponent(name)}`));
    socket.addEventListener('open', () => { lobbyStatus.textContent = ''; });
    socket.addEventListener('message', event => {
        const message = JSON.parse(event.data);
        if ('Err' in message) {
            const labels = { RoomFull: 'That room is full.', GameAlreadyStarted: 'That game has already started.', NotHost: 'Only the host can start the game.', NotEnoughPlayers: 'Add at least one player first.', NotAWord: 'That one is not in my dictionary.', NotOnBoard: 'Those letters are not connected on the grid.', DuplicateWord: 'You already found that one.', InvalidWord: 'Words need 3–25 letters.', GameOver: 'Time is up!' };
            if (state?.phase === 'playing')
                showToast(labels[message.Err] || `Could not do that: ${message.Err}`, true);
            else
                lobbyStatus.textContent = labels[message.Err] || `Could not do that: ${message.Err}`;
            if (message.Err === 'RoomExpired')
                backToLobby();
            return;
        }
        if ('word_accepted' in message) {
            showToast(`+${message.points} point${message.points === 1 ? '' : 's'} — nice find!`);
            selectedPath = [];
            return;
        }
        if ('rematch_code' in message) {
            connect(message.rematch_code);
            return;
        }
        state = message;
        if (state.phase === 'waiting')
            renderWaiting();
        else if (state.phase === 'playing')
            renderGame();
        else
            renderGameOver();
    });
    socket.addEventListener('close', () => { if (state?.phase === 'playing')
        $('connection-state').innerHTML = '⚠ Disconnected'; });
    socket.addEventListener('error', () => { lobbyStatus.textContent = 'Could not reach the server. Is it running?'; });
}
function disconnectLobby() { if (lobbySocket) {
    lobbySocket.close();
    lobbySocket = null;
} }
function connectLobby() {
    if (lobbySocket)
        return;
    lobbySocket = new WebSocket(wsUrl('/lobby'));
    lobbySocket.addEventListener('message', event => renderRooms(JSON.parse(event.data)));
    lobbySocket.addEventListener('error', () => { $('active-games-list').innerHTML = '<div class="no-games">Open rooms are taking a nap.</div>'; });
    lobbySocket.addEventListener('close', () => { lobbySocket = null; });
}
function renderRooms(rooms) {
    const list = $('active-games-list');
    if (!rooms.length) {
        list.innerHTML = '<div class="no-games">No open rooms yet. Be the first!</div>';
        return;
    }
    list.innerHTML = '';
    rooms.forEach(room => {
        const row = document.createElement('button');
        row.className = 'active-game-row';
        row.type = 'button';
        row.innerHTML = `<span class="game-players">${escapeHtml(room.players.join(', '))}</span><span class="game-count">${room.player_count}/${room.max_players} →</span>`;
        row.addEventListener('click', () => connect(room.code));
        list.appendChild(row);
    });
}
function renderWaiting() {
    if (!state)
        return;
    showScreen(waitingRoom);
    $('room-code-display').textContent = currentRoom;
    $('player-count').textContent = `${state.players.length}/${8}`;
    const list = $('waiting-players');
    list.innerHTML = '';
    state.players.forEach(player => { const item = document.createElement('div'); item.className = `waiting-player${player.seat === 0 ? ' host' : ''}`; item.textContent = `${player.name}${player.seat === 0 ? ' · host' : ''}`; list.appendChild(item); });
    const isHost = state.my_seat === 0;
    startButton.disabled = !isHost;
    modeSelect.disabled = !isHost;
    $('waiting-hint').textContent = isHost ? 'You are the host. Start when everyone is ready.' : 'Waiting for the host to start the hunt…';
}
function renderGame() {
    if (!state || !state.board)
        return;
    showScreen(gameScreen);
    toastEl.className = 'toast';
    $('game-room-code').textContent = currentRoom;
    $('game-mode-pill').textContent = state.mode === 'mega' ? 'MEGA GRID' : state.mode.toUpperCase();
    renderBoard();
    renderScoreboard();
    renderFinds();
    updateTimer();
    if (timerHandle === null)
        timerHandle = window.setInterval(updateTimer, 500);
}
function renderBoard() {
    if (!state?.board)
        return;
    const board = state.board;
    boardEl.className = `board${board.size === 5 ? ' mega' : ''}`;
    boardEl.style.gridTemplateColumns = `repeat(${board.size}, 1fr)`;
    boardEl.innerHTML = '';
    board.letters.forEach((letter, index) => {
        const tile = document.createElement('button');
        tile.type = 'button';
        tile.className = 'tile';
        tile.textContent = letter;
        tile.dataset.order = String(selectedPath.indexOf(index) + 1);
        tile.setAttribute('aria-label', `Letter ${letter}, position ${index + 1}`);
        if (selectedPath.includes(index))
            tile.classList.add('selected');
        tile.addEventListener('click', () => chooseTile(index));
        boardEl.appendChild(tile);
    });
    currentWordEl.innerHTML = selectedPath.length ? selectedPath.map(index => board.letters[index]).join('') + '<span class="cursor"></span>' : 'Tap letters to begin<span class="cursor"></span>';
    submitButton.disabled = selectedPath.length < 3;
}
function chooseTile(index) {
    if (!state?.board)
        return;
    const last = selectedPath[selectedPath.length - 1];
    if (index === last)
        selectedPath.pop();
    else if (selectedPath.includes(index)) {
        selectedPath = [index];
    }
    else if (last === undefined || isNeighbor(last, index, state.board.size))
        selectedPath.push(index);
    else
        selectedPath = [index];
    renderBoard();
}
function isNeighbor(a, b, size) { const ar = Math.floor(a / size), ac = a % size, br = Math.floor(b / size), bc = b % size; return Math.max(Math.abs(ar - br), Math.abs(ac - bc)) <= 1; }
function send(payload) { if (socket?.readyState === WebSocket.OPEN)
    socket.send(JSON.stringify(payload)); }
function renderScoreboard() {
    if (!state)
        return;
    const scoreboard = $('scoreboard');
    scoreboard.innerHTML = '';
    $('live-player-count').textContent = `${state.players.length} playing`;
    [...state.players].sort((a, b) => b.score - a.score).forEach((player, rank) => { const row = document.createElement('div'); row.className = `score-row${player.is_me ? ' me' : ''}`; row.innerHTML = `<span class="score-rank">${rank + 1}</span><span class="avatar">${escapeHtml(player.name.slice(0, 1).toUpperCase())}</span><span class="score-name">${escapeHtml(player.name)}</span><span class="score-value">${player.score}</span>`; scoreboard.appendChild(row); });
}
function renderFinds() {
    if (!state)
        return;
    $('your-score').innerHTML = `${state.my_score} <small>pts</small>`;
    $('found-count').textContent = String(state.my_words.length);
    const streak = $('streak-badge');
    streak.classList.toggle('hidden', state.my_streak < 3);
    streak.textContent = `🔥 ${state.my_streak} streak`;
    const list = $('word-list');
    list.innerHTML = '';
    if (!state.my_words.length) {
        list.innerHTML = '<p class="empty-finds">Your first find is hiding in there.</p>';
        return;
    }
    [...state.my_words].reverse().forEach(word => { const chip = document.createElement('span'); chip.className = 'word-chip'; chip.textContent = word; list.appendChild(chip); });
}
function updateTimer() {
    if (!state?.ends_at_ms)
        return;
    const left = Math.max(0, state.ends_at_ms - Date.now());
    const seconds = Math.ceil(left / 1000);
    const mins = Math.floor(seconds / 60);
    const secs = seconds % 60;
    $('timer').textContent = `${mins}:${String(secs).padStart(2, '0')}`;
    $('timer-bar').setAttribute('style', `width:${Math.min(100, left / (state.duration_secs * 1000) * 100)}%`);
    if (seconds <= 10)
        $('timer').style.color = 'var(--coral)';
}
function renderGameOver() {
    if (!state)
        return;
    if (timerHandle !== null) {
        window.clearInterval(timerHandle);
        timerHandle = null;
    }
    showScreen(gameOver);
    const ordered = [...state.players].sort((a, b) => b.score - a.score);
    const winner = ordered[0];
    $('results-headline').textContent = winner?.is_me ? 'You baffled them all.' : `${winner?.name || 'Someone'} took the crown.`;
    $('results-subtitle').textContent = `${state.my_words.length} word${state.my_words.length === 1 ? '' : 's'} found by you · ${state.mode === 'mega' ? 'Mega Grid' : state.mode[0].toUpperCase() + state.mode.slice(1)}`;
    const scoreboard = $('final-scoreboard');
    scoreboard.innerHTML = '';
    ordered.forEach((player, index) => { const row = document.createElement('div'); row.className = `final-row${index === 0 ? ' winner' : ''}`; row.innerHTML = `<span class="final-rank">${index === 0 ? '★' : index + 1}</span><span class="avatar">${escapeHtml(player.name.slice(0, 1).toUpperCase())}</span><span class="final-name"><strong>${escapeHtml(player.name)}${player.is_me ? ' · you' : ''}</strong><small>${player.word_count} word${player.word_count === 1 ? '' : 's'} found</small></span><span class="final-score">${player.score} pts</span>`; scoreboard.appendChild(row); });
}
function showToast(message, error = false) { if (toastHandle !== null)
    window.clearTimeout(toastHandle); toastEl.textContent = message; toastEl.className = `toast show${error ? ' error' : ''}`; toastHandle = window.setTimeout(() => { toastEl.className = 'toast'; }, 2200); }
function escapeHtml(text) { const div = document.createElement('div'); div.textContent = text; return div.innerHTML; }
function backToLobby() { if (socket) {
    socket.close();
    socket = null;
} state = null; currentRoom = ''; showScreen(lobby); connectLobby(); }
createButton.addEventListener('click', () => connect(roomCode()));
joinButton.addEventListener('click', () => connect(roomCodeInput.value.trim()));
roomCodeInput.addEventListener('keydown', event => { if (event.key === 'Enter')
    connect(roomCodeInput.value.trim()); });
startButton.addEventListener('click', () => send({ action: 'start', mode: modeSelect.value }));
modeSelect.addEventListener('change', () => { const descriptions = { classic: 'The original pressure cooker. Plenty of time for a big score.', blitz: 'One minute. Zero mercy. Every second counts.', mega: 'A roomier 5×5 grid for explorers who like their chaos extra large.' }; modeDescription.textContent = descriptions[modeSelect.value]; });
submitButton.addEventListener('click', () => { if (!state?.board || selectedPath.length < 3)
    return; send({ action: 'submit_word', word: selectedPath.map(index => state.board.letters[index]).join('') }); });
$('clear-btn').addEventListener('click', () => { selectedPath = []; renderBoard(); });
$('copy-link-btn').addEventListener('click', async () => { await navigator.clipboard?.writeText(`${location.origin}${location.pathname}?room=${currentRoom}`); showToast('Invite link copied.'); });
$('back-to-lobby-btn').addEventListener('click', backToLobby);
$('results-lobby-btn').addEventListener('click', backToLobby);
$('rematch-btn').addEventListener('click', () => send({ action: 'rematch' }));
$('rules-btn').addEventListener('click', () => $('rules-modal').classList.remove('hidden'));
$('close-rules-btn').addEventListener('click', () => $('rules-modal').classList.add('hidden'));
$('rules-modal').addEventListener('click', event => { if (event.target === $('rules-modal'))
    $('rules-modal').classList.add('hidden'); });
const savedName = localStorage.getItem('baffle_name');
if (savedName)
    playerName.value = savedName;
const inviteRoom = new URLSearchParams(location.search).get('room');
const hash = location.hash.replace('#', '').split('/')[0];
if (inviteRoom) {
    roomCodeInput.value = inviteRoom.toUpperCase();
    connectLobby();
}
else if (hash && savedName)
    connect(hash);
else
    connectLobby();
