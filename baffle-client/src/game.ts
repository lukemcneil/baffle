type Mode = 'classic' | 'netflix';
type Phase = 'waiting' | 'playing' | 'game_over';

interface Board { size: number; letters: string[]; }
interface FoundWord { word: string; points: number; submitted_ms?: number; }
interface RecentFind { player: string; word: string; points: number; word_length: number; at_ms: number; }
interface Player { seat: number; name: string; score: number; word_count: number; connected: boolean; is_me: boolean; words?: FoundWord[]; }
interface GameState { phase: Phase; mode: Mode; board_size: number; cancel_shared_words: boolean; board: Board | null; duration_secs: number; ends_at_ms: number | null; my_seat: number; my_score: number; my_words: FoundWord[]; possible_words?: FoundWord[]; perfect_score?: number; recent_activity?: RecentFind[]; players: Player[]; game_record_id?: number | null; }
interface AcceptedWord { word_accepted: boolean; points: number; shared_cancelled: boolean; unique_bonus: boolean; }
interface Room { code: string; players: string[]; player_count: number; max_players: number; }
interface GameSettings { mode: Mode; board_size: number; duration_secs: number; cancel_shared_words: boolean; }
interface GameSummary { id: number; room_code: string; series_id: string; round_number: number; finished_at_ms: number; settings: GameSettings; player_count: number; winners: string[]; winning_score: number; perfect_score: number; }
interface StoredWord { word: string; points: number; word_length: number; submitted_ms: number; finder_count: number; is_shared: boolean; unique_bonus: boolean; }
interface StoredPlayer { seat: number; name: string; final_score: number; placement: number; accepted_words: number; scoring_words: number; canceled_words: number; canceled_points: number; total_attempts: number; not_a_word_attempts: number; not_on_board_attempts: number; duplicate_attempts: number; invalid_attempts: number; words: StoredWord[]; }
interface GameDetail { id: number; room_code: string; series_id: string; round_number: number; started_at_ms: number; finished_at_ms: number; settings: GameSettings; board: string[]; possible_words: FoundWord[]; perfect_score: number; players: StoredPlayer[]; }
interface PlayerGame { game_id: number; finished_at_ms: number; score: number; placement: number; player_count: number; word_count: number; efficiency: number; settings: GameSettings; }
interface ConfigStats { label: string; games: number; wins: number; avg_score: number; high_score: number; avg_efficiency: number; }
interface RivalStats { name: string; games: number; wins: number; losses: number; ties: number; }
interface PlayerStats { name: string; games_played: number; wins: number; win_rate: number; total_points: number; avg_score: number; high_score: number; high_score_game_id?: number; total_words: number; unique_words: number; avg_words: number; avg_word_length: number; longest_word?: { word: string; length: number; game_id: number }; favorite_word?: string; avg_efficiency: number; submission_accuracy: number; canceled_words: number; canceled_points: number; recent_games: PlayerGame[]; by_config: ConfigStats[]; rivals: RivalStats[]; }
interface LeaderboardEntry { rank: number; name: string; game_id: number; score: number; efficiency: number; finished_at_ms: number; settings: GameSettings; }

const $ = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
const lobby = $('lobby');
const waitingRoom = $('waiting-room');
const gameScreen = $('game-screen');
const gameOver = $('game-over');
const statsScreen = $('stats-screen');
const statsContent = $('stats-content');
const playerName = $('player-name') as HTMLInputElement;
const roomCodeInput = $('room-code') as HTMLInputElement;
const lobbyStatus = $('lobby-status');
const createButton = $('create-btn') as HTMLButtonElement;
const joinButton = $('join-btn') as HTMLButtonElement;
const startButton = $('start-btn') as HTMLButtonElement;
const modeSelect = $('mode-select') as HTMLSelectElement;
const boardSizeSelect = $('board-size-select') as HTMLSelectElement;
const timerSelect = $('timer-select') as HTMLSelectElement;
const cancelSharedToggle = $('cancel-shared-toggle') as HTMLInputElement;
const modeDescription = $('mode-description');
const boardEl = $('board');
const boardWrap = $('board-wrap');
const pathLines = $('path-lines') as unknown as SVGElement;
const selectionLine = $('selection-line') as unknown as SVGPolylineElement;
const currentWordEl = $('current-word');
const submitButton = $('submit-btn') as HTMLButtonElement;
const toastEl = $('toast');
const possibleSearch = $('possible-search') as HTMLInputElement;
const possibleWordList = $('possible-word-list');
const possibleMapWrap = $('possible-map-wrap');
const possiblePathLine = $('possible-path-line') as unknown as SVGElement;
const possibleSelectionLine = $('possible-selection-line') as unknown as SVGPolylineElement;
const possibleBoardEl = $('possible-board');

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
let dragStartIndex: number | null = null;
let pathBeforeDrag: number[] = [];
let lastDragX = 0;
let lastDragY = 0;
let clickGuardUntil = 0;
let pendingWords: string[] = [];
let selectedPossibleWord = '';
let statsOpen = false;
let statsTab = 'mine';

function wsUrl(path: string): string {
  const params = new URLSearchParams(location.search);
  const host = params.get('server') || location.host;
  const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
  return `${protocol}//${host}${path}`;
}

function apiUrl(path: string): string {
  const params = new URLSearchParams(location.search);
  const host = params.get('server');
  if (!host) return path;
  return `${location.protocol === 'https:' ? 'https:' : 'http:'}//${host}${path}`;
}

function showScreen(screen: HTMLElement): void {
  [lobby, waitingRoom, gameScreen, gameOver, statsScreen].forEach(item => item.classList.add('hidden'));
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
  statsOpen = false;
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
    const message = JSON.parse(event.data) as GameState | { Err: string } | AcceptedWord | { rematch_code: string };
    if ('Err' in message) {
      const submissionErrors = ['NotAWord', 'NotOnBoard', 'DuplicateWord', 'InvalidWord', 'GameOver'];
      const rejectedWord = state?.phase === 'playing' && submissionErrors.includes(message.Err) ? pendingWords.shift() : undefined;
      const tracedWord = rejectedWord?.toUpperCase();
      const labels: Record<string, string> = {
        RoomFull: 'That room is full.',
        GameAlreadyStarted: 'That game has already started.',
        NotHost: 'Only the host can start the game.',
        NotEnoughPlayers: 'Add at least one player first.',
        NotAWord: tracedWord ? `${tracedWord} isn’t in the dictionary.` : 'That one is not in my dictionary.',
        NotOnBoard: tracedWord ? `${tracedWord} is not a connected path.` : 'Those letters are not connected on the grid.',
        DuplicateWord: tracedWord ? `${tracedWord} was already found — try another!` : 'Already found — try another!',
        InvalidWord: tracedWord ? `${tracedWord} needs to be 3–25 letters.` : 'Words need 3–25 letters.',
        InvalidSettings: 'Choose a 4×4–6×6 board and a listed timer.',
        GameOver: 'Time is up!'
      };
      if (state?.phase === 'playing') {
        showToast(labels[message.Err] || `Could not do that: ${message.Err}`, true);
        if (message.Err === 'DuplicateWord') {
          const finds = document.querySelector('.finds-panel');
          finds?.classList.remove('shake');
          void finds?.clientWidth;
          finds?.classList.add('shake');
        }
      } else lobbyStatus.textContent = labels[message.Err] || `Could not do that: ${message.Err}`;
      if (message.Err === 'RoomExpired') backToLobby();
      return;
    }
    if ('word_accepted' in message) {
      const word = pendingWords.shift() || 'Nice!';
      celebrateWord(word, message.points, message.shared_cancelled ? 'shared · canceled' : message.unique_bonus ? 'unique word!' : undefined);
      if (message.shared_cancelled) showToast(`${word.toUpperCase()} is shared — canceled for everyone.`, true);
      else if (message.unique_bonus) showToast(`+${message.points} points — unique word bonus!`);
      else showToast(`+${message.points} point${message.points === 1 ? '' : 's'} — nice find!`);
      return;
    }
    if ('rematch_code' in message) { connect(message.rematch_code); return; }
    state = message as GameState;
    if (state.phase === 'waiting') renderWaiting();
    else if (state.phase === 'playing') renderGame();
    else if (!statsOpen) renderGameOver();
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
  const activeRooms = rooms.filter(room => room.player_count > 0);
  if (!activeRooms.length) { list.innerHTML = '<div class="no-games">No open rooms yet. Be the first!</div>'; return; }
  list.innerHTML = '';
  activeRooms.forEach(room => {
    const row = document.createElement('button'); row.className = 'active-game-row'; row.type = 'button';
    row.innerHTML = `<span class="game-players">${escapeHtml(room.players.join(', '))}</span><span class="game-count">${room.player_count}/${room.max_players} →</span>`;
    row.addEventListener('click', () => connect(room.code)); list.appendChild(row);
  });
}

function renderWaiting(): void {
  if (!state) return;
  statsOpen = false;
  showScreen(waitingRoom);
  $('room-code-display').textContent = currentRoom;
  $('player-count').textContent = `${state.players.filter(player => player.connected).length}/${8}`;
  const list = $('waiting-players'); list.innerHTML = '';
  state.players.filter(player => player.connected).forEach(player => { const item = document.createElement('div'); item.className = `waiting-player${player.seat === 0 ? ' host' : ''}`; item.textContent = `${player.name}${player.seat === 0 ? ' · host' : ''}`; list.appendChild(item); });
  const isHost = state.my_seat === 0;
  startButton.disabled = !isHost;
  [modeSelect, boardSizeSelect, timerSelect, cancelSharedToggle].forEach(control => { control.disabled = !isHost; });
  $('waiting-hint').textContent = isHost ? 'You are the host. Start when everyone is ready.' : 'Waiting for the host to start the hunt…';
}

function renderGame(): void {
  if (!state || !state.board) return;
  statsOpen = false;
  showScreen(gameScreen);
  $('connection-state').innerHTML = '<span class="live-dot"></span> Live';
  $('game-room-code').textContent = currentRoom;
  $('game-mode-pill').textContent = `${state.mode === 'netflix' ? 'PARTY' : 'CLASSIC'} · ${state.board.size}×${state.board.size}`;
  renderBoard(); renderScoreboard(); renderLivePulse(); renderFinds(); updateTimer();
  if (timerHandle === null) timerHandle = window.setInterval(updateTimer, 500);
}

function renderBoard(): void {
  if (!state?.board) return;
  const board = state.board; boardEl.className = `board${board.size >= 5 ? ' mega' : ''}`; boardEl.style.gridTemplateColumns = `repeat(${board.size}, 1fr)`; boardEl.innerHTML = '';
  board.letters.forEach((letter, index) => {
    const tile = document.createElement('button'); tile.type = 'button'; tile.className = `tile${letter.length > 1 ? ' multi-letter' : ''}`; tile.textContent = letter; tile.dataset.index = String(index); tile.dataset.order = String(selectedPath.indexOf(index) + 1); tile.setAttribute('aria-label', `${letter.length > 1 ? 'Letters' : 'Letter'} ${letter}, position ${index + 1}`);
    if (selectedPath.includes(index)) tile.classList.add('selected');
    tile.addEventListener('click', () => { if (performance.now() < clickGuardUntil) return; chooseTile(index); }); boardEl.appendChild(tile);
  });
  currentWordEl.innerHTML = selectedPath.length ? selectedPath.map(index => board.letters[index]).join('') + '<span class="cursor"></span>' : 'Drag letters to begin<span class="cursor"></span>';
  submitButton.disabled = selectedWord().length < 3;
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
  const boardRect = boardEl.getBoundingClientRect();
  const edgeTolerance = 12;
  if (x < boardRect.left - edgeTolerance || x > boardRect.right + edgeTolerance || y < boardRect.top - edgeTolerance || y > boardRect.bottom + edgeTolerance) return null;
  let nearest: number | null = null;
  let nearestDistance = Number.POSITIVE_INFINITY;
  tiles.forEach(tile => {
    const index = Number(tile.dataset.index);
    const centerX = boardRect.left + tile.offsetLeft + tile.offsetWidth / 2;
    const centerY = boardRect.top + tile.offsetTop + tile.offsetHeight / 2;
    const distance = Math.hypot(x - centerX, y - centerY);
    if (distance < nearestDistance) {
      nearest = index;
      nearestDistance = distance;
    }
  });
  return nearest;
}

function nextTileForDrag(x: number, y: number): number | null {
  const lastIndex = selectedPath[selectedPath.length - 1];
  if (lastIndex === undefined || !state?.board) return null;
  const lastTile = boardEl.children[lastIndex] as HTMLElement | undefined;
  if (!lastTile) return null;
  const boardRect = boardEl.getBoundingClientRect();
  const style = getComputedStyle(boardEl);
  const columnGap = Number.parseFloat(style.columnGap) || 0;
  const rowGap = Number.parseFloat(style.rowGap) || columnGap;
  const centerX = boardRect.left + lastTile.offsetLeft + lastTile.offsetWidth / 2;
  const centerY = boardRect.top + lastTile.offsetTop + lastTile.offsetHeight / 2;
  const horizontalTravel = Math.abs(x - centerX) / (lastTile.offsetWidth + columnGap);
  const verticalTravel = Math.abs(y - centerY) / (lastTile.offsetHeight + rowGap);
  const largerTravel = Math.max(horizontalTravel, verticalTravel);
  const smallerTravel = Math.min(horizontalTravel, verticalTravel);
  if (largerTravel < 0.56) return null;

  let rowStep = 0;
  let colStep = 0;
  const isDiagonal = smallerTravel >= 0.34 && largerTravel / smallerTravel <= 2.75;
  if (isDiagonal) {
    rowStep = y > centerY ? 1 : -1;
    colStep = x > centerX ? 1 : -1;
  } else {
    if (largerTravel < 0.64) return null;
    if (horizontalTravel > verticalTravel) colStep = x > centerX ? 1 : -1;
    else rowStep = y > centerY ? 1 : -1;
  }

  const row = Math.floor(lastIndex / state.board.size) + rowStep;
  const col = lastIndex % state.board.size + colStep;
  if (row < 0 || row >= state.board.size || col < 0 || col >= state.board.size) return null;
  return row * state.board.size + col;
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
  dragStartIndex = index;
  pathBeforeDrag = [...selectedPath];
  lastDragX = event.clientX;
  lastDragY = event.clientY;
  boardEl.setPointerCapture(event.pointerId);
  selectedPath = [index];
  renderBoard();
  event.preventDefault();
}

function appendDragTile(index: number): void {
  const last = selectedPath[selectedPath.length - 1];
  if (index === last) return;
  dragMoved = true;
  if (!selectedPath.includes(index) && (last === undefined || isNeighbor(last, index, state?.board?.size || 4))) {
    selectedPath.push(index);
    renderBoard();
  }
}

function moveDrag(event: PointerEvent): void {
  if (!isDragging || event.pointerId !== dragPointerId) return;
  const coalesced = event.getCoalescedEvents?.() || [];
  const samples = [...coalesced];
  const lastCoalesced = samples[samples.length - 1];
  if (!lastCoalesced || lastCoalesced.clientX !== event.clientX || lastCoalesced.clientY !== event.clientY) samples.push(event);
  samples.forEach(point => {
    const distance = Math.hypot(point.clientX - lastDragX, point.clientY - lastDragY);
    const steps = Math.max(1, Math.ceil(distance / 12));
    for (let step = 1; step <= steps; step += 1) {
      const progress = step / steps;
      const index = nextTileForDrag(
        lastDragX + (point.clientX - lastDragX) * progress,
        lastDragY + (point.clientY - lastDragY) * progress
      );
      if (index !== null) appendDragTile(index);
    }
    lastDragX = point.clientX;
    lastDragY = point.clientY;
  });
  event.preventDefault();
}

function endDrag(event: PointerEvent): void {
  if (!isDragging || event.pointerId !== dragPointerId) return;
  const cancelled = event.type === 'pointercancel';
  isDragging = false;
  clickGuardUntil = performance.now() + 300;
  if (boardEl.hasPointerCapture(event.pointerId)) boardEl.releasePointerCapture(event.pointerId);
  if (!cancelled && !dragMoved && dragStartIndex !== null) {
    selectedPath = pathBeforeDrag;
    chooseTile(dragStartIndex);
  } else if (!cancelled && dragMoved && selectedWord().length >= 3) submitSelectedWord();
  else if (cancelled || dragMoved) { selectedPath = []; renderBoard(); }
  dragPointerId = null;
  dragStartIndex = null;
  pathBeforeDrag = [];
}

function selectedWord(): string { return state?.board ? selectedPath.map(index => state!.board!.letters[index]).join('') : ''; }

function submitSelectedWord(): void { if (!state?.board || selectedWord().length < 3) return; if (socket?.readyState !== WebSocket.OPEN) { showToast('Reconnecting — try that word again.', true); return; } const word = selectedWord(); pendingWords.push(word); send({ action: 'submit_word', word }); selectedPath = []; renderBoard(); }

function celebrateWord(word: string, points: number, note?: string): void { const burst = document.createElement('div'); burst.className = `word-burst${points === 0 ? ' canceled' : ''}`; burst.innerHTML = `<strong>${escapeHtml(word)}</strong><span>${note ? escapeHtml(note) : `+${points} point${points === 1 ? '' : 's'}`}</span><i>✦</i><i>✦</i><i>✦</i>`; boardWrap.appendChild(burst); boardWrap.classList.remove('word-win'); void boardWrap.clientWidth; boardWrap.classList.add('word-win'); window.setTimeout(() => { burst.remove(); boardWrap.classList.remove('word-win'); }, 1200); }

function send(payload: object): void { if (socket?.readyState === WebSocket.OPEN) socket.send(JSON.stringify(payload)); }

function renderScoreboard(): void {
  if (!state) return;
  const scoreboard = $('scoreboard'); scoreboard.innerHTML = ''; $('live-player-count').textContent = `${state.players.length} playing`;
  [...state.players].sort((a, b) => b.score - a.score).forEach((player, rank) => { const row = document.createElement('div'); row.className = `score-row${player.is_me ? ' me' : ''}`; row.innerHTML = `<span class="score-rank">${rank + 1}</span><span class="avatar">${escapeHtml(player.name.slice(0, 1).toUpperCase())}</span><span class="score-name">${escapeHtml(player.name)}</span><span class="score-value">${player.score}</span>`; scoreboard.appendChild(row); });
}

function renderFinds(): void {
  if (!state) return;
  $('your-score').innerHTML = `${state.my_score} <small>pts</small>`; $('found-count').textContent = String(state.my_words.length);
  const list = $('word-list'); list.innerHTML = '';
  if (!state.my_words.length) { list.innerHTML = '<p class="empty-finds">Your first find is hiding in there.</p>'; return; }
  [...state.my_words].reverse().forEach(found => { const chip = document.createElement('span'); chip.className = `word-chip${found.points === 0 ? ' canceled' : ''}`; chip.innerHTML = `<span>${escapeHtml(found.word)}</span><b>${wordPointsLabel(found)}</b>`; list.appendChild(chip); });
}

function renderLivePulse(): void {
  if (!state) return;
  const players = state.players.filter(player => player.connected);
  const leader = [...players].sort((a, b) => b.score - a.score)[0];
  $('live-leader').textContent = leader ? `${leader.name} leads with ${pointsLabel(leader.score)}` : 'Waiting for the first score';
  const activity = (state.recent_activity || []).filter(find => Date.now() - find.at_ms <= 15_000).sort((a, b) => b.at_ms - a.at_ms);
  const recentTotals = new Map<string, { points: number; finds: number }>();
  activity.forEach(find => { const total = recentTotals.get(find.player) || { points: 0, finds: 0 }; total.points += find.points; total.finds += 1; recentTotals.set(find.player, total); });
  const hot = [...recentTotals.entries()].sort((a, b) => b[1].points - a[1].points || b[1].finds - a[1].finds)[0];
  $('live-momentum').textContent = hot ? `${hot[0]} is hot · +${hot[1].points} in the last 15 seconds` : 'No recent finds · make the next move';
  const feed = $('recent-activity'); feed.innerHTML = '';
  if (!activity.length) { feed.innerHTML = '<span class="activity-empty">The room is warming up.</span>'; return; }
  activity.slice(0, 3).forEach(find => { const item = document.createElement('span'); item.className = 'activity-item'; item.innerHTML = `<strong>${escapeHtml(find.player)}</strong><span>${find.word_length} letters · +${find.points}</span>`; feed.appendChild(item); });
}

function updateTimer(): void {
  if (!state?.ends_at_ms) return;
  const left = Math.max(0, state.ends_at_ms - Date.now()); const seconds = Math.ceil(left / 1000); const mins = Math.floor(seconds / 60); const secs = seconds % 60;
  $('timer').textContent = `${mins}:${String(secs).padStart(2, '0')}`; $('timer-bar').setAttribute('style', `width:${Math.min(100, left / (state.duration_secs * 1000) * 100)}%`);
  $('timer').style.color = seconds <= 10 ? 'var(--coral)' : 'var(--cream)';
  renderLivePulse();
}

function pointsLabel(points: number): string { return `${points} point${points === 1 ? '' : 's'}`; }
function wordPointsLabel(found: FoundWord): string { return found.points === 0 ? 'shared' : `+${found.points}`; }

function renderGameOver(): void {
  if (!state) return;
  statsOpen = false;
  if (timerHandle !== null) { window.clearInterval(timerHandle); timerHandle = null; }
  showScreen(gameOver);
  const ordered = [...state.players].sort((a, b) => b.score - a.score); const winner = ordered[0];
  $('results-headline').textContent = winner?.is_me ? 'You baffled them all.' : `${winner?.name || 'Someone'} took the crown.`;
  const modeLabel = state.mode === 'netflix' ? 'Netflix-style Party' : 'Classic';
  const sharedLabel = state.cancel_shared_words ? 'shared words canceled' : state.mode === 'netflix' ? 'unique words doubled' : 'shared words allowed';
  $('results-subtitle').textContent = `${state.my_words.length} word${state.my_words.length === 1 ? '' : 's'} found by you · ${modeLabel} · ${state.board_size}×${state.board_size} · ${sharedLabel}`;
  const allFinds = ordered.flatMap(player => (player.words || []).map(found => ({ ...found, player })));
  const longestLength = allFinds.reduce((longest, found) => Math.max(longest, found.word.length), 0);
  const longestFinds = allFinds.filter(found => found.word.length === longestLength);
  const longestLabel = longestFinds.length ? longestFinds.map(found => `${escapeHtml(found.word)} · ${escapeHtml(found.player.name)} (${wordPointsLabel(found)})`).join(' · ') : 'No words found';
  $('results-insights').innerHTML = `<div class="insight-card"><span>TOP SCORE</span><strong>${escapeHtml(winner?.name || '—')}</strong><small>${pointsLabel(winner?.score || 0)}</small></div><div class="insight-card longest"><span>LONGEST FIND</span><strong>${longestLabel}</strong><small>${longestFinds.length > 1 ? 'Tied for longest' : longestLength ? `${longestLength} letters` : 'Keep hunting'}</small></div><div class="insight-card"><span>WORDS FOUND</span><strong>${allFinds.length}</strong><small>Across ${ordered.length} player${ordered.length === 1 ? '' : 's'}</small></div>`;
  const scoreboard = $('final-scoreboard'); scoreboard.innerHTML = '';
  ordered.forEach((player, index) => { const row = document.createElement('div'); row.className = `final-row${index === 0 ? ' winner' : ''}`; row.innerHTML = `<span class="final-rank">${index === 0 ? '★' : index + 1}</span><span class="avatar">${escapeHtml(player.name.slice(0, 1).toUpperCase())}</span><span class="final-name"><strong>${escapeHtml(player.name)}${player.is_me ? ' · you' : ''}</strong><small>${player.word_count} word${player.word_count === 1 ? '' : 's'} found · ${pointsLabel(player.score)}</small></span><span class="final-score">${player.score} pts</span>`; scoreboard.appendChild(row); });
  const wordGroups = $('results-word-groups'); wordGroups.innerHTML = '<p class="results-section-label">EVERYONE\'S FINDS</p>';
  ordered.forEach(player => { const group = document.createElement('section'); group.className = 'results-word-group'; const words = player.words || []; group.innerHTML = `<div class="results-player-heading"><span class="avatar">${escapeHtml(player.name.slice(0, 1).toUpperCase())}</span><div><strong>${escapeHtml(player.name)}${player.is_me ? ' · you' : ''}</strong><small>${words.length} word${words.length === 1 ? '' : 's'} · ${pointsLabel(player.score)}</small></div></div>`; const wordsEl = document.createElement('div'); wordsEl.className = 'results-word-list'; if (!words.length) wordsEl.innerHTML = '<span class="no-results-words">No finds this round.</span>'; else words.forEach(found => { const chip = document.createElement('span'); chip.className = `results-word-chip${found.word.length === longestLength ? ' longest' : ''}${found.points === 0 ? ' canceled' : ''}`; chip.innerHTML = `<strong>${escapeHtml(found.word)}</strong><b>${wordPointsLabel(found)}</b>${found.word.length === longestLength ? '<i>longest</i>' : ''}`; wordsEl.appendChild(chip); }); group.appendChild(wordsEl); wordGroups.appendChild(group); });
  possibleSearch.value = '';
  const possibleWords = state.possible_words || [];
  selectedPossibleWord = possibleWords[0]?.word || '';
  $('possible-note').textContent = state.mode === 'netflix'
    ? `Values show the maximum Netflix-style score${state.players.length > 1 ? ' with the unique-word bonus' : ''}. Green words were found by someone in the room.`
    : 'Values use traditional Boggle scoring. Green words were found by someone in the room.';
  $('possible-summary').textContent = `${possibleWords.length} word${possibleWords.length === 1 ? '' : 's'} on this board · perfect play is ${pointsLabel(state.perfect_score || 0)}`;
  const historyButton = $('view-history-btn') as HTMLButtonElement;
  historyButton.classList.remove('hidden');
  historyButton.disabled = !state.game_record_id;
  historyButton.textContent = state.game_record_id ? 'View saved game' : 'Saving to history…';
  renderWordMap();
  renderPossibleWords();
}

function renderPossibleWords(): void {
  if (!state) return;
  const query = possibleSearch.value.trim().toLowerCase();
  const possibleWords = (state.possible_words || []).filter(found => !query || found.word.toLowerCase().includes(query));
  const foundBy = new Map<string, string[]>();
  state.players.forEach(player => (player.words || []).forEach(found => { const key = found.word.toLowerCase(); const owners = foundBy.get(key) || []; owners.push(player.name); foundBy.set(key, owners); }));
  possibleWordList.innerHTML = '';
  if (!possibleWords.length) { possibleWordList.innerHTML = '<p class="no-results-words">No possible words match that filter.</p>'; return; }
  possibleWords.forEach(found => { const owners = foundBy.get(found.word.toLowerCase()) || []; const chip = document.createElement('button'); chip.type = 'button'; chip.className = `possible-word-chip${owners.length ? ' found' : ''}${found.word === selectedPossibleWord ? ' selected' : ''}`; chip.title = owners.length ? `Found by ${owners.join(', ')}` : 'Nobody found this word'; chip.innerHTML = `<strong>${escapeHtml(found.word)}</strong><b>+${found.points}</b><i>${owners.length ? `✓ ${escapeHtml(owners.join(', '))}` : 'missed'}`; chip.addEventListener('click', () => { selectedPossibleWord = found.word; renderWordMap(); renderPossibleWords(); }); possibleWordList.appendChild(chip); });
  $('possible-summary').textContent = query ? `${possibleWords.length} matching word${possibleWords.length === 1 ? '' : 's'} · perfect play is ${pointsLabel(state.perfect_score || 0)}` : `${possibleWords.length} word${possibleWords.length === 1 ? '' : 's'} on this board · perfect play is ${pointsLabel(state.perfect_score || 0)}`;
}

function findWordPath(board: Board, word: string): number[] | null {
  const target = word.toUpperCase();
  const used = new Array(board.letters.length).fill(false) as boolean[];
  const path: number[] = [];
  function visit(index: number, position: number): boolean {
    const tile = board.letters[index].toUpperCase();
    if (used[index] || !target.startsWith(tile, position)) return false;
    path.push(index);
    const nextPosition = position + tile.length;
    if (nextPosition === target.length) return true;
    used[index] = true;
    const row = Math.floor(index / board.size);
    const col = index % board.size;
    for (let rowDelta = -1; rowDelta <= 1; rowDelta += 1) {
      for (let colDelta = -1; colDelta <= 1; colDelta += 1) {
        if (!rowDelta && !colDelta) continue;
        const nextRow = row + rowDelta;
        const nextCol = col + colDelta;
        if (nextRow >= 0 && nextRow < board.size && nextCol >= 0 && nextCol < board.size && visit(nextRow * board.size + nextCol, nextPosition)) return true;
      }
    }
    used[index] = false;
    path.pop();
    return false;
  }
  for (let start = 0; start < board.letters.length; start += 1) if (visit(start, 0)) return path;
  return null;
}

function renderWordMap(): void {
  if (!state?.board) return;
  const board = state.board;
  const word = selectedPossibleWord;
  const path = word ? findWordPath(board, word) : null;
  $('possible-map-word').textContent = word || 'No playable words';
  $('possible-map-hint').textContent = path ? `${word.length} letters · one valid path highlighted` : 'No word path to show on this board.';
  possibleBoardEl.className = `word-map-board${board.size >= 5 ? ' mega' : ''}`;
  possibleBoardEl.style.gridTemplateColumns = `repeat(${board.size}, 1fr)`;
  possibleBoardEl.innerHTML = '';
  board.letters.forEach((letter, index) => { const tile = document.createElement('span'); tile.className = `word-map-tile${letter.length > 1 ? ' multi-letter' : ''}${path?.includes(index) ? ' active' : ''}`; tile.textContent = letter; if (path) tile.dataset.order = String(path.indexOf(index) + 1); possibleBoardEl.appendChild(tile); });
  if (!path) { possibleSelectionLine.setAttribute('points', ''); return; }
  const wrapRect = possibleMapWrap.getBoundingClientRect(); possiblePathLine.setAttribute('viewBox', `0 0 ${wrapRect.width} ${wrapRect.height}`);
  possibleSelectionLine.setAttribute('points', path.map(index => { const rect = (possibleBoardEl.children[index] as HTMLElement).getBoundingClientRect(); return `${rect.left - wrapRect.left + rect.width / 2},${rect.top - wrapRect.top + rect.height / 2}`; }).join(' '));
}

async function fetchJson<T>(path: string): Promise<T> {
  const response = await fetch(apiUrl(path), { headers: { Accept: 'application/json' } });
  if (!response.ok) {
    let message = `Request failed (${response.status})`;
    try { message = ((await response.json()) as { error?: string }).error || message; } catch { /* use status */ }
    throw new Error(message);
  }
  return response.json() as Promise<T>;
}

function settingsLabel(settings: GameSettings): string {
  return `${settings.mode === 'netflix' ? 'Party' : 'Classic'} · ${settings.board_size}×${settings.board_size} · ${formatDuration(settings.duration_secs)} · shared ${settings.cancel_shared_words ? 'cancel' : 'score'}`;
}

function formatDuration(seconds: number): string { return seconds >= 60 && seconds % 60 === 0 ? `${seconds / 60}m` : `${seconds}s`; }
function formatDate(timestamp: number): string { return new Intl.DateTimeFormat(undefined, { month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit' }).format(new Date(timestamp)); }
function formatPercent(value: number): string { return `${value.toFixed(value >= 10 ? 0 : 1)}%`; }

function updateStatsTabs(active: string): void {
  document.querySelectorAll<HTMLButtonElement>('#stats-tabs button').forEach(button => button.classList.toggle('active', button.dataset.tab === active));
}

function openStats(tab = 'mine', gameId?: number): void {
  statsOpen = true;
  statsTab = tab;
  disconnectLobby();
  showScreen(statsScreen);
  window.scrollTo(0, 0);
  if (gameId !== undefined) {
    location.hash = `stats/game/${gameId}`;
    updateStatsTabs('');
    void renderGameDetail(gameId);
  } else {
    location.hash = `stats/${tab}`;
    updateStatsTabs(tab);
    void renderStatsTab(tab);
  }
}

function closeStats(): void {
  statsOpen = false;
  if (state?.phase === 'game_over' && currentRoom) {
    location.hash = currentRoom;
    renderGameOver();
  } else {
    location.hash = '';
    showScreen(lobby);
    connectLobby();
  }
}

async function renderStatsTab(tab: string): Promise<void> {
  statsContent.innerHTML = '<div class="stats-loading">Opening the record book…</div>';
  try {
    if (tab === 'records') await renderLeaderboard();
    else if (tab === 'recent') await renderRecentGames();
    else await renderPlayerStats(localStorage.getItem('baffle_name') || playerName.value.trim());
  } catch (error) {
    statsContent.innerHTML = `<div class="stats-empty"><strong>Couldn’t open the record book.</strong><p>${escapeHtml(error instanceof Error ? error.message : String(error))}</p></div>`;
  }
}

async function renderPlayerStats(name: string): Promise<void> {
  const search = `<form id="player-stats-search" class="stats-search"><label for="stats-player-name">Player</label><div><input id="stats-player-name" maxlength="20" value="${escapeHtml(name)}" placeholder="Enter a player name"><button class="secondary-button" type="submit">Look up</button></div></form>`;
  if (!name) {
    statsContent.innerHTML = `${search}<div class="stats-empty"><strong>Who are we looking for?</strong><p>Enter the same name you use when joining a game.</p></div>`;
    bindPlayerSearch();
    return;
  }
  let stats: PlayerStats;
  try {
    stats = await fetchJson<PlayerStats>(`/api/stats/player/${encodeURIComponent(name)}`);
  } catch (error) {
    statsContent.innerHTML = `${search}<div class="stats-empty"><strong>No finished games yet.</strong><p>${escapeHtml(error instanceof Error ? error.message : String(error))}</p></div>`;
    bindPlayerSearch();
    return;
  }
  const longest = stats.longest_word ? `${escapeHtml(stats.longest_word.word)} · ${stats.longest_word.length} letters` : '—';
  statsContent.innerHTML = `${search}
    <div class="stat-hero"><div><p class="results-section-label">CAREER CARD</p><h3>${escapeHtml(stats.name)}</h3><p>${stats.games_played} game${stats.games_played === 1 ? '' : 's'} · ${stats.wins} win${stats.wins === 1 ? '' : 's'}</p></div><strong>${formatPercent(stats.win_rate)}<small>win rate</small></strong></div>
    <div class="metric-grid">
      <button class="metric-card history-open" data-game-id="${stats.high_score_game_id || ''}"><span>PERSONAL BEST</span><strong>${stats.high_score}</strong><small>points · tap to reopen</small></button>
      <div class="metric-card"><span>AVERAGE</span><strong>${stats.avg_score.toFixed(1)}</strong><small>points per game</small></div>
      <div class="metric-card"><span>BOARD COVERAGE</span><strong>${formatPercent(stats.avg_efficiency)}</strong><small>of the perfect score</small></div>
      <div class="metric-card"><span>ACCURACY</span><strong>${formatPercent(stats.submission_accuracy)}</strong><small>accepted attempts</small></div>
      <div class="metric-card"><span>WORDS</span><strong>${stats.total_words}</strong><small>${stats.unique_words} different</small></div>
      <div class="metric-card"><span>LONGEST</span><strong class="metric-word">${longest}</strong><small>favorite: ${escapeHtml(stats.favorite_word || '—')}</small></div>
    </div>
    <section class="stats-section"><div class="stats-section-heading"><p class="results-section-label">RECENT FORM</p><h3>Last rounds</h3></div><div class="history-list">${stats.recent_games.length ? stats.recent_games.map(game => playerGameRow(game)).join('') : '<div class="stats-empty compact">No games yet.</div>'}</div></section>
    <div class="stats-two-column">
      <section class="stats-section"><div class="stats-section-heading"><p class="results-section-label">BY FORMAT</p><h3>Where you shine</h3></div>${stats.by_config.map(config => `<div class="breakdown-row"><div><strong>${escapeHtml(config.label)}</strong><small>${config.games} game${config.games === 1 ? '' : 's'} · ${config.wins} wins</small></div><b>${config.avg_score.toFixed(1)} avg</b></div>`).join('') || '<div class="stats-empty compact">Play a few formats to compare them.</div>'}</section>
      <section class="stats-section"><div class="stats-section-heading"><p class="results-section-label">RIVALS</p><h3>Head to head</h3></div>${stats.rivals.map(rival => `<div class="breakdown-row"><div><strong>${escapeHtml(rival.name)}</strong><small>${rival.games} together · ${rival.ties} ties</small></div><b>${rival.wins}–${rival.losses}</b></div>`).join('') || '<div class="stats-empty compact">Bring a rival next round.</div>'}</section>
    </div>`;
  bindPlayerSearch();
  bindHistoryLinks();
}

function bindPlayerSearch(): void {
  const form = document.getElementById('player-stats-search') as HTMLFormElement | null;
  form?.addEventListener('submit', event => {
    event.preventDefault();
    const input = document.getElementById('stats-player-name') as HTMLInputElement;
    const name = input.value.trim();
    if (name) { playerName.value = name; localStorage.setItem('baffle_name', name); }
    statsContent.innerHTML = '<div class="stats-loading">Finding that player…</div>';
    void renderPlayerStats(name);
  });
}

function playerGameRow(game: PlayerGame): string {
  const result = game.placement === 1 ? 'Win' : `#${game.placement}`;
  return `<button class="history-row history-open" data-game-id="${game.game_id}"><span class="history-result${game.placement === 1 ? ' win' : ''}">${result}</span><span><strong>${game.score} points · ${game.word_count} words</strong><small>${formatDate(game.finished_at_ms)} · ${escapeHtml(settingsLabel(game.settings))}</small></span><b>${formatPercent(game.efficiency)} →</b></button>`;
}

async function renderLeaderboard(): Promise<void> {
  statsContent.innerHTML = `<div class="records-controls"><label>Rank by<select id="record-metric"><option value="efficiency">Board coverage</option><option value="score">Raw score</option></select></label><label>Mode<select id="record-mode"><option value="">All modes</option><option value="classic">Classic</option><option value="netflix">Party</option></select></label><label>Board<select id="record-board"><option value="">All sizes</option><option value="4">4×4</option><option value="5">5×5</option><option value="6">6×6</option></select></label></div><p id="records-explainer" class="stats-explainer">Board coverage compares your score with the best possible score on that exact board, so different formats stay fair.</p><div id="leaderboard-list" class="leaderboard-list"><div class="stats-loading">Ranking the wordsmiths…</div></div>`;
  const controls = ['record-metric', 'record-mode', 'record-board'];
  controls.forEach(id => $(id).addEventListener('change', () => { void loadLeaderboardRows(); }));
  await loadLeaderboardRows();
}

async function loadLeaderboardRows(): Promise<void> {
  const metric = ($('record-metric') as HTMLSelectElement).value;
  const mode = ($('record-mode') as HTMLSelectElement).value;
  const board = ($('record-board') as HTMLSelectElement).value;
  $('records-explainer').textContent = metric === 'efficiency'
    ? 'Board coverage compares your score with the best possible score on that exact board, so different formats stay fair.'
    : 'Raw score is best compared within one mode, board size, and timer. Use the filters for a fair race.';
  const query = new URLSearchParams({ metric, limit: '50' });
  if (mode) query.set('mode', mode);
  if (board) query.set('board_size', board);
  const list = $('leaderboard-list');
  list.innerHTML = '<div class="stats-loading">Ranking the wordsmiths…</div>';
  const entries = await fetchJson<LeaderboardEntry[]>(`/api/stats/leaderboard?${query}`);
  list.innerHTML = entries.length ? entries.map(entry => `<button class="leaderboard-row history-open" data-game-id="${entry.game_id}"><span class="leaderboard-rank">${entry.rank <= 3 ? ['★', 'Ⅱ', 'Ⅲ'][entry.rank - 1] : entry.rank}</span><span class="avatar">${escapeHtml(entry.name.slice(0, 1).toUpperCase())}</span><span><strong>${escapeHtml(entry.name)}</strong><small>${formatDate(entry.finished_at_ms)} · ${escapeHtml(settingsLabel(entry.settings))}</small></span><b>${metric === 'efficiency' ? formatPercent(entry.efficiency) : `${entry.score} pts`}</b></button>`).join('') : '<div class="stats-empty"><strong>No records match those filters.</strong><p>Finish a game in this format to claim the first spot.</p></div>';
  bindHistoryLinks();
}

async function renderRecentGames(): Promise<void> {
  const games = await fetchJson<GameSummary[]>('/api/stats/games?limit=50');
  statsContent.innerHTML = `<div class="stats-section-heading recent-heading"><div><p class="results-section-label">LATEST ROUNDS</p><h3>${games.length} saved game${games.length === 1 ? '' : 's'}</h3></div><p>Completed rounds are saved automatically.</p></div><div class="game-history-grid">${games.length ? games.map(game => `<button class="game-history-card history-open" data-game-id="${game.id}"><div><span>${formatDate(game.finished_at_ms)}</span><b>Round ${game.round_number}</b></div><h3>${escapeHtml(game.winners.join(' & ') || 'No winner')}</h3><p>${game.winning_score} points · ${game.player_count} player${game.player_count === 1 ? '' : 's'}</p><small>${escapeHtml(settingsLabel(game.settings))}</small><i>Open game →</i></button>`).join('') : '<div class="stats-empty"><strong>The record book is blank.</strong><p>Finish a round and it will appear here automatically.</p></div>'}</div>`;
  bindHistoryLinks();
}

function bindHistoryLinks(): void {
  document.querySelectorAll<HTMLElement>('.history-open').forEach(element => element.addEventListener('click', () => {
    const gameId = Number(element.dataset.gameId);
    if (gameId) openStats('detail', gameId);
  }));
}

async function renderGameDetail(gameId: number): Promise<void> {
  statsContent.innerHTML = '<div class="stats-loading">Rebuilding that grid…</div>';
  try {
    const game = await fetchJson<GameDetail>(`/api/stats/games/${gameId}`);
    const foundBy = new Map<string, string[]>();
    game.players.forEach(player => player.words.forEach(word => { const key = word.word.toLowerCase(); const owners = foundBy.get(key) || []; owners.push(player.name); foundBy.set(key, owners); }));
    const board = `<div class="history-board" style="grid-template-columns:repeat(${game.settings.board_size},1fr)">${game.board.map(letter => `<span class="history-tile${letter.length > 1 ? ' multi-letter' : ''}">${escapeHtml(letter)}</span>`).join('')}</div>`;
    statsContent.innerHTML = `<button id="detail-back-btn" class="text-button detail-back">← ${statsTab === 'detail' ? 'Recent games' : 'Back to stats'}</button>
      <div class="game-detail-title"><div><p class="results-section-label">SAVED GAME #${game.id}</p><h3>${escapeHtml(game.players.filter(player => player.placement === 1).map(player => player.name).join(' & '))} ${game.players.filter(player => player.placement === 1).length > 1 ? 'tied' : 'won'}</h3><p>${formatDate(game.finished_at_ms)} · ${escapeHtml(settingsLabel(game.settings))}</p></div><strong>${game.perfect_score}<small>perfect score</small></strong></div>
      <div class="game-detail-grid"><section class="stats-section"><div class="stats-section-heading"><p class="results-section-label">THE GRID</p><h3>Round ${game.round_number}</h3></div>${board}</section><section class="stats-section"><div class="stats-section-heading"><p class="results-section-label">FINAL STANDINGS</p><h3>${game.players.length} player${game.players.length === 1 ? '' : 's'}</h3></div>${game.players.map(player => `<div class="detail-player"><span class="history-result${player.placement === 1 ? ' win' : ''}">${player.placement === 1 ? '★' : `#${player.placement}`}</span><div><strong>${escapeHtml(player.name)}</strong><small>${player.accepted_words}/${player.total_attempts} accepted · ${player.canceled_words} canceled</small></div><b>${player.final_score} pts</b></div>`).join('')}</section></div>
      <section class="stats-section"><div class="stats-section-heading"><p class="results-section-label">EVERY FIND</p><h3>Who found what</h3></div><div class="detail-word-groups">${game.players.map(player => `<div><strong>${escapeHtml(player.name)}</strong><p>${player.words.length ? player.words.map(word => `<span class="detail-word${word.points === 0 ? ' canceled' : ''}">${escapeHtml(word.word)} <b>${word.points ? `+${word.points}` : 'shared'}</b></span>`).join('') : '<small>No accepted words</small>'}</p></div>`).join('')}</div></section>
      <section class="stats-section"><div class="stats-section-heading"><p class="results-section-label">THE WHOLE GRID</p><h3>${game.possible_words.length} possible words · ${game.perfect_score} points</h3></div><div class="detail-possible-words">${game.possible_words.map(word => { const owners = foundBy.get(word.word.toLowerCase()) || []; return `<span class="detail-word${owners.length ? ' found' : ''}" title="${owners.length ? `Found by ${escapeHtml(owners.join(', '))}` : 'Missed'}">${escapeHtml(word.word)} <b>+${word.points}</b><i>${owners.length ? `✓ ${escapeHtml(owners.join(', '))}` : 'missed'}</i></span>`; }).join('')}</div></section>`;
    $('detail-back-btn').addEventListener('click', () => openStats('recent'));
  } catch (error) {
    statsContent.innerHTML = `<button id="detail-back-btn" class="text-button detail-back">← Recent games</button><div class="stats-empty"><strong>That game couldn’t be opened.</strong><p>${escapeHtml(error instanceof Error ? error.message : String(error))}</p></div>`;
    $('detail-back-btn').addEventListener('click', () => openStats('recent'));
  }
}

function showToast(message: string, error = false): void { if (toastHandle !== null) window.clearTimeout(toastHandle); toastEl.textContent = message; toastEl.className = `toast show${error ? ' error' : ''}`; toastHandle = window.setTimeout(() => { toastEl.className = 'toast'; }, 2200); }
function escapeHtml(text: string): string { const div = document.createElement('div'); div.textContent = text; return div.innerHTML; }
function backToLobby(): void { statsOpen = false; if (socket) { socket.close(); socket = null; } state = null; currentRoom = ''; localStorage.removeItem('baffle_room'); location.hash = ''; showScreen(lobby); connectLobby(); }

createButton.addEventListener('click', () => connect(roomCode()));
joinButton.addEventListener('click', () => connect(roomCodeInput.value.trim()));
roomCodeInput.addEventListener('keydown', event => { if (event.key === 'Enter') connect(roomCodeInput.value.trim()); });
startButton.addEventListener('click', () => send({
  action: 'start',
  mode: modeSelect.value,
  board_size: Number(boardSizeSelect.value),
  duration_secs: Number(timerSelect.value),
  cancel_shared_words: cancelSharedToggle.checked
}));
modeSelect.addEventListener('change', () => {
  const netflix = modeSelect.value === 'netflix';
  cancelSharedToggle.checked = !netflix;
  modeDescription.textContent = netflix
    ? 'Netflix-style points: 3 letters score 1, then +1 per letter. A word only you found scores double.'
    : 'Traditional point values. Unique words score normally; shared words cancel by default.';
});
submitButton.addEventListener('click', submitSelectedWord);
$('clear-btn').addEventListener('click', () => { selectedPath = []; renderBoard(); });
$('copy-link-btn').addEventListener('click', async () => { await navigator.clipboard?.writeText(`${location.origin}${location.pathname}?room=${currentRoom}`); showToast('Invite link copied.'); });
$('back-to-lobby-btn').addEventListener('click', backToLobby); $('results-lobby-btn').addEventListener('click', backToLobby);
$('rematch-btn').addEventListener('click', () => send({ action: 'rematch' }));
$('stats-btn').addEventListener('click', () => openStats('mine'));
$('stats-back-btn').addEventListener('click', closeStats);
$('view-history-btn').addEventListener('click', () => { if (state?.game_record_id) openStats('detail', state.game_record_id); });
document.querySelectorAll<HTMLButtonElement>('#stats-tabs button').forEach(button => button.addEventListener('click', () => openStats(button.dataset.tab || 'mine')));
$('rules-btn').addEventListener('click', () => $('rules-modal').classList.remove('hidden')); $('close-rules-btn').addEventListener('click', () => $('rules-modal').classList.add('hidden')); $('rules-modal').addEventListener('click', event => { if (event.target === $('rules-modal')) $('rules-modal').classList.add('hidden'); });
possibleSearch.addEventListener('input', renderPossibleWords);

boardEl.addEventListener('pointerdown', beginDrag);
boardEl.addEventListener('pointermove', moveDrag);
boardEl.addEventListener('pointerup', endDrag);
boardEl.addEventListener('pointercancel', endDrag);
window.addEventListener('resize', drawPath);
window.addEventListener('resize', renderWordMap);
window.addEventListener('blur', () => { if (isDragging) { isDragging = false; dragPointerId = null; dragStartIndex = null; pathBeforeDrag = []; selectedPath = []; renderBoard(); } });

const savedName = localStorage.getItem('baffle_name'); if (savedName) playerName.value = savedName;
const inviteRoom = new URLSearchParams(location.search).get('room');
const savedRoom = localStorage.getItem('baffle_room');
const hashParts = location.hash.replace('#', '').split('/').filter(Boolean);
const hash = hashParts[0] || '';
const inviteCode = inviteRoom?.toUpperCase();
const rememberedRoom = hash || savedRoom;
if (hash === 'stats') {
  if (hashParts[1] === 'game' && Number(hashParts[2])) openStats('detail', Number(hashParts[2]));
  else openStats(hashParts[1] || 'mine');
}
else if (savedName && rememberedRoom && (!inviteCode || inviteCode === rememberedRoom)) connect(rememberedRoom);
else if (inviteCode) { roomCodeInput.value = inviteCode; connectLobby(); }
else connectLobby();
