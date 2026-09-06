use rand::{seq::SliceRandom, thread_rng};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::words;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Classic,
    Blitz,
    Mega,
}

impl Mode {
    pub fn size(&self) -> usize {
        if matches!(self, Mode::Mega) {
            5
        } else {
            4
        }
    }
    pub fn duration_secs(&self) -> u64 {
        if matches!(self, Mode::Blitz) {
            60
        } else {
            180
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Board {
    pub size: usize,
    pub letters: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Player {
    pub seat: usize,
    pub name: String,
    pub score: u32,
    pub words: Vec<String>,
    pub streak: u32,
    pub connected: bool,
    connection_id: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Phase {
    Waiting,
    Playing,
    GameOver,
}

#[derive(Debug, Clone)]
pub struct GameState {
    pub phase: Phase,
    pub mode: Mode,
    pub board: Option<Board>,
    pub players: Vec<Player>,
    pub duration_secs: u64,
    pub ends_at: Option<Instant>,
    pub ends_at_ms: Option<u64>,
    pub max_players: usize,
    next_connection_id: u64,
}

#[derive(Debug, Serialize)]
pub struct ClientState {
    pub phase: &'static str,
    pub mode: Mode,
    pub board: Option<Board>,
    pub duration_secs: u64,
    pub ends_at_ms: Option<u64>,
    pub my_seat: usize,
    pub my_score: u32,
    pub my_words: Vec<String>,
    pub my_streak: u32,
    pub players: Vec<ClientPlayer>,
}

#[derive(Debug, Serialize)]
pub struct ClientPlayer {
    pub seat: usize,
    pub name: String,
    pub score: u32,
    pub word_count: usize,
    pub connected: bool,
    pub is_me: bool,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ClientAction {
    Start { mode: Mode },
    SubmitWord { word: String },
    Rematch,
}

#[derive(Debug)]
pub enum ActionError {
    GameAlreadyStarted,
    RoomFull,
    NotHost,
    NotEnoughPlayers,
    WrongPhase,
    InvalidWord,
    NotAWord,
    NotOnBoard,
    DuplicateWord,
    GameOver,
    InvalidSeat,
}

impl std::fmt::Display for ActionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            ActionError::GameAlreadyStarted => "GameAlreadyStarted",
            ActionError::RoomFull => "RoomFull",
            ActionError::NotHost => "NotHost",
            ActionError::NotEnoughPlayers => "NotEnoughPlayers",
            ActionError::WrongPhase => "WrongPhase",
            ActionError::InvalidWord => "InvalidWord",
            ActionError::NotAWord => "NotAWord",
            ActionError::NotOnBoard => "NotOnBoard",
            ActionError::DuplicateWord => "DuplicateWord",
            ActionError::GameOver => "GameOver",
            ActionError::InvalidSeat => "InvalidSeat",
        };
        f.write_str(text)
    }
}

impl GameState {
    pub fn new(max_players: usize) -> Self {
        Self {
            phase: Phase::Waiting,
            mode: Mode::Classic,
            board: None,
            players: Vec::new(),
            duration_secs: 180,
            ends_at: None,
            ends_at_ms: None,
            max_players,
            next_connection_id: 0,
        }
    }

    pub fn join(&mut self, name: &str) -> Result<(usize, u64), ActionError> {
        self.next_connection_id = self.next_connection_id.wrapping_add(1);
        let connection_id = self.next_connection_id;
        if let Some(player) = self
            .players
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(name))
        {
            player.connected = true;
            player.connection_id = connection_id;
            return Ok((player.seat, connection_id));
        }
        if self.phase != Phase::Waiting {
            return Err(ActionError::GameAlreadyStarted);
        }
        if let Some(player) = self.players.iter_mut().find(|p| !p.connected) {
            player.name = name.to_string();
            player.score = 0;
            player.words.clear();
            player.streak = 0;
            player.connected = true;
            player.connection_id = connection_id;
            return Ok((player.seat, connection_id));
        }
        if self.players.iter().filter(|p| p.connected).count() >= self.max_players {
            return Err(ActionError::RoomFull);
        }
        let seat = self.players.len();
        self.players.push(Player {
            seat,
            name: name.to_string(),
            score: 0,
            words: Vec::new(),
            streak: 0,
            connected: true,
            connection_id,
        });
        Ok((seat, connection_id))
    }

    pub fn disconnect(&mut self, seat: usize, connection_id: u64) {
        if let Some(player) = self.players.get_mut(seat) {
            if player.connection_id == connection_id {
                player.connected = false;
            }
        }
    }

    pub fn start(&mut self, seat: usize, mode: Mode) -> Result<(), ActionError> {
        if seat != 0 {
            return Err(ActionError::NotHost);
        }
        if self.phase != Phase::Waiting {
            return Err(ActionError::GameAlreadyStarted);
        }
        if self.players.is_empty() {
            return Err(ActionError::NotEnoughPlayers);
        }
        self.mode = mode;
        self.duration_secs = self.mode.duration_secs();
        self.board = Some(make_board(self.mode.size()));
        for p in &mut self.players {
            p.score = 0;
            p.words.clear();
            p.streak = 0;
        }
        self.ends_at = Some(Instant::now() + Duration::from_secs(self.duration_secs));
        self.ends_at_ms = Some(epoch_ms() + self.duration_secs * 1000);
        self.phase = Phase::Playing;
        Ok(())
    }

    pub fn tick(&mut self) -> bool {
        if self.phase == Phase::Playing
            && self
                .ends_at
                .map(|end| Instant::now() >= end)
                .unwrap_or(false)
        {
            self.phase = Phase::GameOver;
            true
        } else {
            false
        }
    }

    pub fn submit_word(&mut self, seat: usize, raw_word: &str) -> Result<u32, ActionError> {
        if self.phase != Phase::Playing {
            return Err(if self.phase == Phase::GameOver {
                ActionError::GameOver
            } else {
                ActionError::WrongPhase
            });
        }
        let word = raw_word.trim().to_ascii_uppercase();
        if word.len() < 3 || word.len() > 25 || !word.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(ActionError::InvalidWord);
        }
        let player = self.players.get_mut(seat).ok_or(ActionError::InvalidSeat)?;
        if player.words.iter().any(|w| w == &word) {
            return Err(ActionError::DuplicateWord);
        }
        let board = self.board.as_ref().ok_or(ActionError::WrongPhase)?;
        if !words::is_word(&word) {
            return Err(ActionError::NotAWord);
        }
        if !can_trace(board, &word) {
            return Err(ActionError::NotOnBoard);
        }
        let base = match word.len() {
            3 | 4 => 1,
            5 => 2,
            6 => 3,
            7 => 5,
            _ => 11,
        };
        player.streak += 1;
        let combo = if player.streak >= 3 { 1 } else { 0 };
        let points = base + combo;
        player.score += points;
        player.words.push(word);
        Ok(points)
    }

    pub fn to_client_state(&self, my_seat: usize) -> ClientState {
        let me = self.players.get(my_seat);
        ClientState {
            phase: match self.phase {
                Phase::Waiting => "waiting",
                Phase::Playing => "playing",
                Phase::GameOver => "game_over",
            },
            mode: self.mode.clone(),
            board: self.board.clone(),
            duration_secs: self.duration_secs,
            ends_at_ms: self.ends_at_ms,
            my_seat,
            my_score: me.map(|p| p.score).unwrap_or(0),
            my_words: me.map(|p| p.words.clone()).unwrap_or_default(),
            my_streak: me.map(|p| p.streak).unwrap_or(0),
            players: self
                .players
                .iter()
                .map(|p| ClientPlayer {
                    seat: p.seat,
                    name: p.name.clone(),
                    score: p.score,
                    word_count: p.words.len(),
                    connected: p.connected,
                    is_me: p.seat == my_seat,
                })
                .collect(),
        }
    }
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn make_board(size: usize) -> Board {
    let mut rng = thread_rng();
    let distribution: Vec<char> = "EEEEEEEEEEEEAAAAAAAAAIIIIIIIIOOOOOOOONNNNNNRRRRRRTTTTTTLLLLSSSSUUUUDDDDGGGBBCCMMPPFFHHVVWWYYKJXZ".chars().collect();
    let mut letters: Vec<String> = (0..size * size)
        .map(|_| distribution.choose(&mut rng).unwrap().to_string())
        .collect();
    letters.shuffle(&mut rng);
    Board { size, letters }
}

fn can_trace(board: &Board, word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    let mut used = vec![false; board.letters.len()];
    for start in 0..board.letters.len() {
        if board.letters[start].chars().next() == Some(chars[0])
            && trace_from(board, &chars, 0, start, &mut used)
        {
            return true;
        }
    }
    false
}

fn trace_from(board: &Board, chars: &[char], pos: usize, index: usize, used: &mut [bool]) -> bool {
    if used[index] || board.letters[index].chars().next() != Some(chars[pos]) {
        return false;
    }
    if pos == chars.len() - 1 {
        return true;
    }
    used[index] = true;
    let row = index / board.size;
    let col = index % board.size;
    for dr in -1i32..=1 {
        for dc in -1i32..=1 {
            if dr == 0 && dc == 0 {
                continue;
            }
            let nr = row as i32 + dr;
            let nc = col as i32 + dc;
            if nr >= 0 && nr < board.size as i32 && nc >= 0 && nc < board.size as i32 {
                let next = nr as usize * board.size + nc as usize;
                if trace_from(board, chars, pos + 1, next, used) {
                    used[index] = false;
                    return true;
                }
            }
        }
    }
    used[index] = false;
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traces_diagonal_and_rejects_reused_tiles() {
        let board = Board {
            size: 2,
            letters: vec!["C".into(), "A".into(), "T".into(), "S".into()],
        };
        assert!(can_trace(&board, "CAT"));
        assert!(!can_trace(&board, "CC"));
    }

    #[test]
    fn accepted_word_scores_and_duplicates_are_rejected() {
        let mut game = GameState::new(2);
        let (seat, _) = game.join("Tester").unwrap();
        game.start(seat, Mode::Blitz).unwrap();
        game.board = Some(Board {
            size: 2,
            letters: vec!["S".into(), "U".into(), "E".into(), "X".into()],
        });
        assert_eq!(game.submit_word(seat, "sue").unwrap(), 1);
        assert!(matches!(
            game.submit_word(seat, "SUE"),
            Err(ActionError::DuplicateWord)
        ));
    }

    #[test]
    fn stale_disconnect_cannot_hide_a_reconnected_player() {
        let mut game = GameState::new(2);
        let (seat, first_connection) = game.join("Luke").unwrap();
        let (same_seat, second_connection) = game.join("Luke").unwrap();
        assert_eq!(seat, same_seat);
        game.disconnect(seat, first_connection);
        assert!(game.players[seat].connected);
        game.disconnect(seat, second_connection);
        assert!(!game.players[seat].connected);
    }

    #[test]
    fn waiting_room_reuses_disconnected_slots() {
        let mut game = GameState::new(2);
        let (_, _) = game.join("Luke").unwrap();
        let (wife_seat, wife_connection) = game.join("Wife").unwrap();
        game.disconnect(wife_seat, wife_connection);
        let (new_seat, _) = game.join("Friend").unwrap();
        assert_eq!(new_seat, wife_seat);
        assert_eq!(game.players.len(), 2);
        assert_eq!(
            game.players
                .iter()
                .filter(|player| player.connected)
                .count(),
            2
        );
    }
}
