type Mode = 'classic' | 'blitz' | 'mega';
type Phase = 'waiting' | 'playing' | 'game_over';

interface Board { size: number; letters: string[]; }
interface Player { seat: number; name: string; score: number; word_count: number; connected: boolean; is_me: boolean; }
interface GameState { phase: Phase; mode: Mode; board: Board | null; duration_secs: number; ends_at_ms: number | null; my_seat: number; my_score: number; my_words: string[]; my_streak: number; players: Player[]; }
interface Room { code: string; players: string[]; player_count: number; max_players: number; }

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
const lobby = $('lobby');
const waitingRoom = $('waiting-room');
const gameScreen = $('game-screen');
const gameOver = $('game-over');
const playerName = $('player-name') as HTMLInputElement;
const roomCodeInput = $('room-code') as HTMLInputElement;
const lobbyStatus = $('lobby-status');
const createButton = $('create-btn') as HTMLButtonElement;
const joinButton = $('join-btn') as HTMLButtonElement;
const startButton = $('start-btn') as HTMLButtonElement;
const modeSelect = $('mode-select') as HTMLSelectElement;
const modeDescription = $('mode-description');
const boardEl = $('board');
const boardWrap = $('board-wrap');
const pathLines = $('path-lines') as unknown as SVGElement;
const selectionLine = $('selection-line') as unknown as SVGPolylineElement;
const currentWordEl = $('current-word');
const submitButton = $('submit-btn') as HTMLButtonElement;
const toastEl = $('toast');

let socket: WebSocket | null = null;
let lobbySocket: WebSocket | null = null;
let state: GameState | null = null;
let currentRoom = '';
let selectedPath: number[] = [];
let timerHandle: number | null = null;
let toastHandle: number | null = null;
let isDragging = false;
let dragMoved = false;
let dragPointerId: number | null = null;
let clickGuardUntil = 0;
let pendingWords: string[] = [];

function wsUrl(path: string): string {
  const params = new URLSearchParams(location.search);
  const host = params.get('server') || location.host;
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${host}${path}`;
}

function showScreen(screen: HTMLElement): void {
  [lobby, waitingRoom, gameScreen, gameOver].forEach(item => item.classList.add('hidden'));
  screen.classList.remove('hidden');
}

function roomCode(): string {
  const chars = 'ABCDEFGHJKLMNPQRSTUVWXYZ';
  return Array.from({ length: 4 }, () => chars[Math.floor(Math.random() * chars.length)]).join('');
}

function connect(code: string): void {
  const name = playerName.value.trim();
  if (!name) { lobbyStatus.textContent = 'Give yourself a name first.'; playerName.focus(); return; }
  currentRoom = code.toUpperCase();
  pendingWords = [];
  localStorage.setItem('baffle_name', name);
  localStorage.setItem('baffle_room', currentRoom);
  location.hash = currentRoom;
  lobbyStatus.textContent = 'Joining the room…';
  if (socket) socket.close();
  disconnectLobby();
  socket = new WebSocket(wsUrl(`/game/${encodeURIComponent(currentRoom)}?player=${encodeURIComponent(name)}`));
  socket.addEventListener('open', () => { lobbyStatus.textContent = ''; });
  socket.addEventListener('message', event => {
    const message = JSON.parse(event.data) as GameState | { Err: string } | { word_accepted: boolean; points: number } | { rematch_code: string };
    if ('Err' in message) { const labels: Record<string, string> = { RoomFull: 'That room is full.', GameAlreadyStarted: 'That game has already started.', NotHost: 'Only the host can start the game.', NotEnoughPlayers: 'Add at least one player first.', NotAWord: 'That one is not in my dictionary.', NotOnBoard: 'Those letters are not connected on the grid.', DuplicateWord: 'Already found — try another!', InvalidWord: 'Words need 3–25 letters.', GameOver: 'Time is up!' }; if (state?.phase === 'playing') { if (['NotAWord', 'NotOnBoard', 'DuplicateWord', 'InvalidWord', 'GameOver'].includes(message.Err)) pendingWords.shift(); showToast(labels[message.Err] || `Could not do that: ${message.Err}`, true); if (message.Err === 'DuplicateWord') { const finds = document.querySelector('.finds-panel'); finds?.classList.remove('shake'); void finds?.clientWidth; finds?.classList.add('shake'); } } else lobbyStatus.textContent = labels[message.Err] || `Could not do that: ${message.Err}`; if (message.Err === 'RoomExpired') backToLobby(); return; }
    if ('word_accepted' in message) { celebrateWord(pendingWords.shift() || 'Nice!', message.points); showToast(`+${message.points} point${message.points === 1 ? '' : 's'} — nice find!`); return; }
    if ('rematch_code' in message) { connect(message.rematch_code); return; }
    state = message as GameState;
    if (state.phase === 'waiting') renderWaiting();
    else if (state.phase === 'playing') renderGame();
    else renderGameOver();
  });
  socket.addEventListener('close', () => { if (state?.phase === 'playing') $('connection-state').innerHTML = '⚠ Disconnected'; });
  socket.addEventListener('error', () => { lobbyStatus.textContent = 'Could not reach the server. Is it running?'; });
}

function disconnectLobby(): void { if (lobbySocket) { lobbySocket.close(); lobbySocket = null; } }

function connectLobby(): void {
  if (lobbySocket) return;
  lobbySocket = new WebSocket(wsUrl('/lobby'));
  lobbySocket.addEventListener('message', event => renderRooms(JSON.parse(event.data) as Room[]));
  lobbySocket.addEventListener('error', () => { $('active-games-list').innerHTML = '<div class="no-games">Open rooms are taking a nap.</div>'; });
  lobbySocket.addEventListener('close', () => { lobbySocket = null; });
}

function renderRooms(rooms: Room[]): void {
  const list = $('active-games-list');
  if (!rooms.length) { list.innerHTML = '<div class="no-games">No open rooms yet. Be the first!</div>'; return; }
  list.innerHTML = '';
  rooms.forEach(room => {
    const row = document.createElement('button'); row.className = 'active-game-row'; row.type = 'button';
    row.innerHTML = `<span class="game-players">${escapeHtml(room.players.join(', '))}</span><span class="game-count">${room.player_count}/${room.max_players} →</span>`;
    row.addEventListener('click', () => connect(room.code)); list.appendChild(row);
  });
}

function renderWaiting(): void {
  if (!state) return;
  showScreen(waitingRoom);
  $('room-code-display').textContent = currentRoom;
  $('player-count').textContent = `${state.players.filter(player => player.connected).length}/${8}`;
  const list = $('waiting-players'); list.innerHTML = '';
  state.players.filter(player => player.connected).forEach(player => { const item = document.createElement('div'); item.className = `waiting-player${player.seat === 0 ? ' host' : ''}`; item.textContent = `${player.name}${player.seat === 0 ? ' · host' : ''}`; list.appendChild(item); });
  const isHost = state.my_seat === 0; startButton.disabled = !isHost; modeSelect.disabled = !isHost;
  $('waiting-hint').textContent = isHost ? 'You are the host. Start when everyone is ready.' : 'Waiting for the host to start the hunt…';
}

function renderGame(): void {
  if (!state || !state.board) return;
  showScreen(gameScreen);
  $('connection-state').innerHTML = '<span class="live-dot"></span> Live';
  $('game-room-code').textContent = currentRoom;
  $('game-mode-pill').textContent = state.mode === 'mega' ? 'MEGA GRID' : state.mode.toUpperCase();
  renderBoard(); renderScoreboard(); renderFinds(); updateTimer();
  if (timerHandle === null) timerHandle = window.setInterval(updateTimer, 500);
}

function renderBoard(): void {
  if (!state?.board) return;
  const board = state.board; boardEl.className = `board${board.size === 5 ? ' mega' : ''}`; boardEl.style.gridTemplateColumns = `repeat(${board.size}, 1fr)`; boardEl.innerHTML = '';
  board.letters.forEach((letter, index) => {
    const tile = document.createElement('button'); tile.type = 'button'; tile.className = 'tile'; tile.textContent = letter; tile.dataset.index = String(index); tile.dataset.order = String(selectedPath.indexOf(index) + 1); tile.setAttribute('aria-label', `Letter ${letter}, position ${index + 1}`);
    if (selectedPath.includes(index)) tile.classList.add('selected');
    tile.addEventListener('click', () => { if (performance.now() < clickGuardUntil) return; chooseTile(index); }); boardEl.appendChild(tile);
  });
  currentWordEl.innerHTML = selectedPath.length ? selectedPath.map(index => board.letters[index]).join('') + '<span class="cursor"></span>' : 'Drag letters to begin<span class="cursor"></span>';
  submitButton.disabled = selectedPath.length < 3;
  drawPath();
}

function chooseTile(index: number): void {
  if (!state?.board) return;
  const last = selectedPath[selectedPath.length - 1];
  if (index === last) selectedPath.pop();
  else if (selectedPath.includes(index)) { selectedPath = selectedPath.slice(0, selectedPath.indexOf(index) + 1); }
  else if (last === undefined || isNeighbor(last, index, state.board.size)) selectedPath.push(index);
  else selectedPath = [index];
  renderBoard();
}

function isNeighbor(a: number, b: number, size: number): boolean { const ar = Math.floor(a / size), ac = a % size, br = Math.floor(b / size), bc = b % size; return Math.max(Math.abs(ar - br), Math.abs(ac - bc)) <= 1; }

function tileAtPoint(x: number, y: number): number | null {
  const tiles = Array.from(boardEl.querySelectorAll<HTMLButtonElement>('.tile'));
  let nearest: number | null = null;
  let nearestDistance = Number.POSITIVE_INFINITY;
  tiles.forEach(tile => {
    const rect = tile.getBoundingClientRect();
    const centerX = rect.left + rect.width / 2;
    const centerY = rect.top + rect.height / 2;
    const distance = Math.hypot(x - centerX, y - centerY);
    const hitRadius = Math.max(rect.width * 0.82, 36);
    if (distance <= hitRadius && distance < nearestDistance) {
      nearest = Number(tile.dataset.index);
      nearestDistance = distance;
    }
  });
  return nearest;
}

function drawPath(): void {
  if (!state?.board || !selectedPath.length) { selectionLine.setAttribute('points', ''); return; }
  const wrapRect = boardWrap.getBoundingClientRect(); pathLines.setAttribute('viewBox', `0 0 ${wrapRect.width} ${wrapRect.height}`);
  const points = selectedPath.map(index => { const rect = (boardEl.children[index] as HTMLElement).getBoundingClientRect(); return `${rect.left - wrapRect.left + rect.width / 2},${rect.top - wrapRect.top + rect.height / 2}`; }).join(' ');
  selectionLine.setAttribute('points', points);
}

function beginDrag(event: PointerEvent): void {
  const index = tileAtPoint(event.clientX, event.clientY);
  if (index === null) return;
  isDragging = true;
  dragMoved = false;
  dragPointerId = event.pointerId;
  boardEl.setPointerCapture(event.pointerId);
  chooseTile(index);
  event.preventDefault();
}

function moveDrag(event: PointerEvent): void {
  if (!isDragging || event.pointerId !== dragPointerId) return;
  const index = tileAtPoint(event.clientX, event.clientY);
  const last = selectedPath[selectedPath.length - 1];
  if (index !== null && index !== last) {
    dragMoved = true;
    if (!selectedPath.includes(index) && (last === undefined || isNeighbor(last, index, state?.board?.size || 4))) {
      selectedPath.push(index);
      renderBoard();
    }
  }
  event.preventDefault();
}

function endDrag(event: PointerEvent): void {
  if (!isDragging || event.pointerId !== dragPointerId) return;
  isDragging = false;
  clickGuardUntil = performance.now() + 300;
  if (boardEl.hasPointerCapture(event.pointerId)) boardEl.releasePointerCapture(event.pointerId);
  if (dragMoved && selectedPath.length >= 3) submitSelectedWord();
  else if (dragMoved) { selectedPath = []; renderBoard(); }
  dragPointerId = null;
}

function submitSelectedWord(): void { if (!state?.board || selectedPath.length < 3) return; if (socket?.readyState !== WebSocket.OPEN) { showToast('Reconnecting — try that word again.', true); return; } const word = selectedPath.map(index => state!.board!.letters[index]).join(''); pendingWords.push(word); send({ action: 'submit_word', word }); selectedPath = []; renderBoard(); }

function celebrateWord(word: string, points: number): void { const burst = document.createElement('div'); burst.className = 'word-burst'; burst.innerHTML = `<strong>${escapeHtml(word)}</strong><span>+${points} point${points === 1 ? '' : 's'}</span><i>✦</i><i>✦</i><i>✦</i>`; boardWrap.appendChild(burst); boardWrap.classList.remove('word-win'); void boardWrap.clientWidth; boardWrap.classList.add('word-win'); window.setTimeout(() => { burst.remove(); boardWrap.classList.remove('word-win'); }, 1200); }

function send(payload: object): void { if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(payload)); }

function renderScoreboard(): void {
  if (!state) return;
  const scoreboard = $('scoreboard'); scoreboard.innerHTML = ''; $('live-player-count').textContent = `${state.players.length} playing`;
  [...state.players].sort((a, b) => b.score - a.score).forEach((player, rank) => { const row = document.createElement('div'); row.className = `score-row${player.is_me ? ' me' : ''}`; row.innerHTML = `<span class="score-rank">${rank + 1}</span><span class="avatar">${escapeHtml(player.name.slice(0, 1).toUpperCase())}</span><span class="score-name">${escapeHtml(player.name)}</span><span class="score-value">${player.score}</span>`; scoreboard.appendChild(row); });
}

function renderFinds(): void {
  if (!state) return;
  $('your-score').innerHTML = `${state.my_score} <small>pts</small>`; $('found-count').textContent = String(state.my_words.length);
  const streak = $('streak-badge'); streak.classList.toggle('hidden', state.my_streak < 3); streak.textContent = `🔥 ${state.my_streak} streak`;
  const list = $('word-list'); list.innerHTML = '';
  if (!state.my_words.length) { list.innerHTML = '<p class="empty-finds">Your first find is hiding in there.</p>'; return; }
  [...state.my_words].reverse().forEach(word => { const chip = document.createElement('span'); chip.className = 'word-chip'; chip.textContent = word; list.appendChild(chip); });
}

function updateTimer(): void {
  if (!state?.ends_at_ms) return;
  const left = Math.max(0, state.ends_at_ms - Date.now()); const seconds = Math.ceil(left / 1000); const mins = Math.floor(seconds / 60); const secs = seconds % 60;
  $('timer').textContent = `${mins}:${String(secs).padStart(2, '0')}`; $('timer-bar').setAttribute('style', `width:${Math.min(100, left / (state.duration_secs * 1000) * 100)}%`);
  $('timer').style.color = seconds <= 10 ? 'var(--coral)' : 'var(--cream)';
}

function renderGameOver(): void {
  if (!state) return;
  if (timerHandle !== null) { window.clearInterval(timerHandle); timerHandle = null; }
  showScreen(gameOver);
  const ordered = [...state.players].sort((a, b) => b.score - a.score); const winner = ordered[0];
  $('results-headline').textContent = winner?.is_me ? 'You baffled them all.' : `${winner?.name || 'Someone'} took the crown.`;
  $('results-subtitle').textContent = `${state.my_words.length} word${state.my_words.length === 1 ? '' : 's'} found by you · ${state.mode === 'mega' ? 'Mega Grid' : state.mode[0].toUpperCase() + state.mode.slice(1)}`;
  const scoreboard = $('final-scoreboard'); scoreboard.innerHTML = '';
  ordered.forEach((player, index) => { const row = document.createElement('div'); row.className = `final-row${index === 0 ? ' winner' : ''}`; row.innerHTML = `<span class="final-rank">${index === 0 ? '★' : index + 1}</span><span class="avatar">${escapeHtml(player.name.slice(0, 1).toUpperCase())}</span><span class="final-name"><strong>${escapeHtml(player.name)}${player.is_me ? ' · you' : ''}</strong><small>${player.word_count} word${player.word_count === 1 ? '' : 's'} found</small></span><span class="final-score">${player.score} pts</span>`; scoreboard.appendChild(row); });
}

function showToast(message: string, error = false): void { if (toastHandle !== null) window.clearTimeout(toastHandle); toastEl.textContent = message; toastEl.className = `toast show${error ? ' error' : ''}`; toastHandle = window.setTimeout(() => { toastEl.className = 'toast'; }, 2200); }
function escapeHtml(text: string): string { const div = document.createElement('div'); div.textContent = text; return div.innerHTML; }
function backToLobby(): void { if (socket) { socket.close(); socket = null; } state = null; currentRoom = ''; localStorage.removeItem('baffle_room'); location.hash = ''; showScreen(lobby); connectLobby(); }

createButton.addEventListener('click', () => connect(roomCode()));
joinButton.addEventListener('click', () => connect(roomCodeInput.value.trim()));
roomCodeInput.addEventListener('keydown', event => { if (event.key === 'Enter') connect(roomCodeInput.value.trim()); });
startButton.addEventListener('click', () => send({ action: 'start', mode: modeSelect.value }));
modeSelect.addEventListener('change', () => { const descriptions: Record<string, string> = { classic: 'The original pressure cooker. Plenty of time for a big score.', blitz: 'One minute. Zero mercy. Every second counts.', mega: 'A roomier 5×5 grid for explorers who like their chaos extra large.' }; modeDescription.textContent = descriptions[modeSelect.value]; });
submitButton.addEventListener('click', submitSelectedWord);
$('clear-btn').addEventListener('click', () => { selectedPath = []; renderBoard(); });
$('copy-link-btn').addEventListener('click', async () => { await navigator.clipboard?.writeText(`${location.origin}${location.pathname}?room=${currentRoom}`); showToast('Invite link copied.'); });
$('back-to-lobby-btn').addEventListener('click', backToLobby); $('results-lobby-btn').addEventListener('click', backToLobby);
$('rematch-btn').addEventListener('click', () => send({ action: 'rematch' }));
$('rules-btn').addEventListener('click', () => $('rules-modal').classList.remove('hidden')); $('close-rules-btn').addEventListener('click', () => $('rules-modal').classList.add('hidden')); $('rules-modal').addEventListener('click', event => { if (event.target === $('rules-modal')) $('rules-modal').classList.add('hidden'); });

boardEl.addEventListener('pointerdown', beginDrag);
boardEl.addEventListener('pointermove', moveDrag);
boardEl.addEventListener('pointerup', endDrag);
boardEl.addEventListener('pointercancel', endDrag);
window.addEventListener('resize', drawPath);

const savedName = localStorage.getItem('baffle_name'); if (savedName) playerName.value = savedName;
const inviteRoom = new URLSearchParams(location.search).get('room');
const savedRoom = localStorage.getItem('baffle_room');
const hash = location.hash.replace('#', '').split('/')[0];
const inviteCode = inviteRoom?.toUpperCase();
const rememberedRoom = hash || savedRoom;
if (savedName && rememberedRoom && (!inviteCode || inviteCode === rememberedRoom)) connect(rememberedRoom);
else if (inviteCode) { roomCodeInput.value = inviteCode; connectLobby(); }
else connectLobby();
