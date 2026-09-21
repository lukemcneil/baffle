use rand::{seq::SliceRandom, thread_rng, Rng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::words;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Classic,
    Netflix,
}

#[derive(Debug, Clone, Serialize)]
pub struct Board {
    pub size: usize,
    pub letters: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FoundWord {
    pub word: String,
    pub points: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecentFind {
    pub player: String,
    #[serde(skip_serializing)]
    pub word: String,
    pub points: u32,
    pub word_length: usize,
    pub at_ms: u64,
}

#[derive(Debug, Clone)]
pub struct Player {
    pub seat: usize,
    pub name: String,
    pub score: u32,
    pub words: Vec<FoundWord>,
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
    pub board_size: usize,
    pub cancel_shared_words: bool,
    pub board: Option<Board>,
    pub players: Vec<Player>,
    pub duration_secs: u64,
    pub ends_at: Option<Instant>,
    pub ends_at_ms: Option<u64>,
    pub max_players: usize,
    pub possible_words: Option<Vec<FoundWord>>,
    pub perfect_score: Option<u32>,
    pub recent_activity: Vec<RecentFind>,
    next_connection_id: u64,
}

#[derive(Debug, Serialize)]
pub struct ClientState {
    pub phase: &'static str,
    pub mode: Mode,
    pub board_size: usize,
    pub cancel_shared_words: bool,
    pub board: Option<Board>,
    pub duration_secs: u64,
    pub ends_at_ms: Option<u64>,
    pub my_seat: usize,
    pub my_score: u32,
    pub my_words: Vec<FoundWord>,
    pub possible_words: Option<Vec<FoundWord>>,
    pub perfect_score: Option<u32>,
    pub recent_activity: Vec<RecentFind>,
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
    pub words: Option<Vec<FoundWord>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum ClientAction {
    Start {
        mode: Mode,
        board_size: usize,
        duration_secs: u64,
        cancel_shared_words: bool,
    },
    SubmitWord {
        word: String,
    },
    Rematch,
}

#[derive(Debug)]
pub struct SubmissionResult {
    pub points: u32,
    pub shared_cancelled: bool,
    pub unique_bonus: bool,
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
    InvalidSettings,
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
            ActionError::InvalidSettings => "InvalidSettings",
        };
        f.write_str(text)
    }
}

impl GameState {
    pub fn new(max_players: usize) -> Self {
        Self {
            phase: Phase::Waiting,
            mode: Mode::Classic,
            board_size: 4,
            cancel_shared_words: true,
            board: None,
            players: Vec::new(),
            duration_secs: 180,
            ends_at: None,
            ends_at_ms: None,
            max_players,
            possible_words: None,
            perfect_score: None,
            recent_activity: Vec::new(),
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

    pub fn start(
        &mut self,
        seat: usize,
        mode: Mode,
        board_size: usize,
        duration_secs: u64,
        cancel_shared_words: bool,
    ) -> Result<(), ActionError> {
        if seat != 0 {
            return Err(ActionError::NotHost);
        }
        if self.phase != Phase::Waiting {
            return Err(ActionError::GameAlreadyStarted);
        }
        if self.players.is_empty() {
            return Err(ActionError::NotEnoughPlayers);
        }
        if !matches!(board_size, 4..=6) || !matches!(duration_secs, 30 | 60 | 90 | 120 | 180) {
            return Err(ActionError::InvalidSettings);
        }
        self.mode = mode;
        self.board_size = board_size;
        self.duration_secs = duration_secs;
        self.cancel_shared_words = cancel_shared_words;
        self.board = Some(make_board(self.board_size));
        self.possible_words = None;
        self.perfect_score = None;
        for p in &mut self.players {
            p.score = 0;
            p.words.clear();
        }
        self.recent_activity.clear();
        self.ends_at = Some(Instant::now() + Duration::from_secs(self.duration_secs));
        self.ends_at_ms = Some(epoch_ms() + self.duration_secs * 1000);
        self.phase = Phase::Playing;
        Ok(())
    }

    pub fn tick(&mut self) -> bool {
        self.prune_recent_activity();
        if self.phase == Phase::Playing
            && self
                .ends_at
                .map(|end| Instant::now() >= end)
                .unwrap_or(false)
        {
            self.phase = Phase::GameOver;
            self.build_recap();
            true
        } else {
            false
        }
    }

    pub fn submit_word(
        &mut self,
        seat: usize,
        raw_word: &str,
    ) -> Result<SubmissionResult, ActionError> {
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
        let board = self.board.as_ref().ok_or(ActionError::WrongPhase)?;
        if !words::is_word(&word) {
            return Err(ActionError::NotAWord);
        }
        if !can_trace(board, &word) {
            return Err(ActionError::NotOnBoard);
        }
        let player = self.players.get(seat).ok_or(ActionError::InvalidSeat)?;
        if player.words.iter().any(|found| found.word == word) {
            return Err(ActionError::DuplicateWord);
        }
        let player_name = player.name.clone();
        let word_length = word.len();
        self.players[seat].words.push(FoundWord {
            word: word.clone(),
            points: 0,
        });
        self.recent_activity.push(RecentFind {
            player: player_name,
            word: word.clone(),
            points: 0,
            word_length,
            at_ms: epoch_ms(),
        });
        self.recalculate_scores();
        let points = self.players[seat]
            .words
            .iter()
            .find(|found| found.word == word)
            .map(|found| found.points)
            .unwrap_or(0);
        let finders = self
            .players
            .iter()
            .filter(|player| player.words.iter().any(|found| found.word == word))
            .count();
        Ok(SubmissionResult {
            points,
            shared_cancelled: self.cancel_shared_words && finders > 1,
            unique_bonus: self.mode == Mode::Netflix && finders == 1 && self.players.len() > 1,
        })
    }

    pub fn to_client_state(&self, my_seat: usize) -> ClientState {
        let me = self.players.get(my_seat);
        ClientState {
            phase: match self.phase {
                Phase::Waiting => "waiting",
                Phase::Playing => "playing",
                Phase::GameOver => "game_over",
            },
            mode: self.mode,
            board_size: self.board_size,
            cancel_shared_words: self.cancel_shared_words,
            board: self.board.clone(),
            duration_secs: self.duration_secs,
            ends_at_ms: self.ends_at_ms,
            my_seat,
            my_score: me.map(|p| p.score).unwrap_or(0),
            my_words: me.map(|p| p.words.clone()).unwrap_or_default(),
            possible_words: if self.phase == Phase::GameOver {
                self.possible_words.clone()
            } else {
                None
            },
            recent_activity: self.recent_activity.clone(),
            perfect_score: if self.phase == Phase::GameOver {
                self.perfect_score
            } else {
                None
            },
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
                    words: (self.phase == Phase::GameOver).then(|| p.words.clone()),
                })
                .collect(),
        }
    }

    fn build_recap(&mut self) {
        let Some(board) = self.board.as_ref() else {
            self.possible_words = Some(Vec::new());
            self.perfect_score = Some(0);
            return;
        };
        let possible_words: Vec<_> = words::possible_words(board)
            .into_iter()
            .map(|word| FoundWord {
                points: score_word(
                    self.mode,
                    &word,
                    1,
                    self.cancel_shared_words,
                    self.players.len() > 1,
                ),
                word,
            })
            .collect();
        let base_total: u32 = possible_words.iter().map(|found| found.points).sum();
        self.perfect_score = Some(base_total);
        self.possible_words = Some(possible_words);
    }

    fn prune_recent_activity(&mut self) {
        let cutoff = epoch_ms().saturating_sub(15_000);
        self.recent_activity.retain(|find| find.at_ms >= cutoff);
    }

    fn recalculate_scores(&mut self) {
        let mut finder_counts: HashMap<String, usize> = HashMap::new();
        for player in &self.players {
            for found in &player.words {
                *finder_counts.entry(found.word.clone()).or_default() += 1;
            }
        }
        let multiplayer = self.players.len() > 1;
        for player in &mut self.players {
            for found in &mut player.words {
                found.points = score_word(
                    self.mode,
                    &found.word,
                    *finder_counts.get(&found.word).unwrap_or(&1),
                    self.cancel_shared_words,
                    multiplayer,
                );
            }
            player.score = player.words.iter().map(|found| found.points).sum();
        }
        for find in &mut self.recent_activity {
            find.points = score_word(
                self.mode,
                &find.word,
                *finder_counts.get(&find.word).unwrap_or(&1),
                self.cancel_shared_words,
                multiplayer,
            );
        }
    }
}

fn score_word(
    mode: Mode,
    word: &str,
    finder_count: usize,
    cancel_shared_words: bool,
    multiplayer: bool,
) -> u32 {
    if cancel_shared_words && finder_count > 1 {
        return 0;
    }
    let base = match mode {
        Mode::Classic => match word.len() {
            3 | 4 => 1,
            5 => 2,
            6 => 3,
            7 => 5,
            _ => 11,
        },
        Mode::Netflix => word.len().saturating_sub(2) as u32,
    };
    if mode == Mode::Netflix && multiplayer && finder_count == 1 {
        base * 2
    } else {
        base
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
    make_board_with_rng(size, &mut rng)
}

const CLASSIC_DICE: [&str; 16] = [
    "AAEEGN", "ABBJOO", "ACHOPS", "AFFKPS", "AOOTTW", "CIMOTU", "DEILRX", "DELRVY", "DISTTY",
    "EEGHNW", "EEINSU", "EHRTVW", "EIOSST", "ELRTTY", "HIMNQU", "HLNNRZ",
];

const BIG_DICE: [&str; 25] = [
    "AAAFRS", "AAEEEE", "AAFIRS", "ADENNN", "AEEEEM", "AEEGMU", "AEGMNN", "AFIRSY", "BJKQXZ",
    "CCNSTW", "CEIILT", "CEILPT", "CEIPST", "DDLNOR", "DDHNOT", "DHHLOR", "DHLNOR", "EIIITT",
    "EMOTTT", "ENSSSU", "FIPRSY", "GORRVW", "HIPRRY", "NOOTUW", "OOOTTU",
];

// Super Big Boggle has one double-letter cube and one cube with three stop
// faces. Baffle rerolls those stop faces as E/I/O so all 36 digital tiles stay
// playable while retaining the physical game's letter balance.
const SUPER_DICE: [&str; 34] = [
    "AAAFRS", "AAEEEE", "AAEEOO", "AAFIRS", "ABDEIO", "ADENNN", "AEEEEM", "AEEGMU", "AEGMNN",
    "AEILMN", "AEINOU", "AFIRSY", "BBJKXZ", "CCENST", "CDDLNN", "CEIITT", "CEIPST", "CFGNUY",
    "DDHNOT", "DHHLOR", "DHHNOW", "DHLNOR", "EHILRS", "EIILST", "EILPST", "EMTTTO", "ENSSSU",
    "GORRVW", "HIRSTV", "HOPRST", "IPRSYY", "JKQWXZ", "NOOTUW", "OOOTTU",
];
const SUPER_DOUBLE_FACES: [&str; 6] = ["AN", "ER", "HE", "IN", "QU", "TH"];
const SUPER_VOWEL_FACES: [&str; 3] = ["E", "I", "O"];

fn make_board_with_rng<R: Rng + ?Sized>(size: usize, rng: &mut R) -> Board {
    let mut letters: Vec<String> = match size {
        4 => roll_dice(&CLASSIC_DICE, rng),
        5 => roll_dice(&BIG_DICE, rng),
        6 => {
            let mut rolled = roll_dice(&SUPER_DICE, rng);
            rolled.push((*SUPER_DOUBLE_FACES.choose(rng).unwrap()).to_string());
            rolled.push((*SUPER_VOWEL_FACES.choose(rng).unwrap()).to_string());
            rolled
        }
        _ => unreachable!("board size is validated before generation"),
    };
    letters.shuffle(rng);
    Board { size, letters }
}

fn roll_dice<R: Rng + ?Sized>(dice: &[&str], rng: &mut R) -> Vec<String> {
    dice.iter()
        .map(|die| die.as_bytes().choose(rng).copied().unwrap() as char)
        .map(tile_for_letter)
        .collect()
}

fn tile_for_letter(letter: char) -> String {
    if letter == 'Q' {
        "QU".to_string()
    } else {
        letter.to_string()
    }
}

fn can_trace(board: &Board, word: &str) -> bool {
    let mut used = vec![false; board.letters.len()];
    for start in 0..board.letters.len() {
        if trace_from(board, word, 0, start, &mut used) {
            return true;
        }
    }
    false
}

fn trace_from(board: &Board, word: &str, pos: usize, index: usize, used: &mut [bool]) -> bool {
    let tile = board.letters[index].as_str();
    if used[index] || !word[pos..].starts_with(tile) {
        return false;
    }
    let next_pos = pos + tile.len();
    if next_pos == word.len() {
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
                if trace_from(board, word, next_pos, next, used) {
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
    use rand::{rngs::StdRng, SeedableRng};

    fn start_game(game: &mut GameState, seat: usize, mode: Mode, cancel_shared_words: bool) {
        game.start(seat, mode, 4, 60, cancel_shared_words).unwrap();
    }

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
    fn qu_tile_consumes_two_letters_without_reusing_a_tile() {
        let board = Board {
            size: 2,
            letters: vec!["QU".into(), "I".into(), "T".into(), "S".into()],
        };
        assert!(can_trace(&board, "QUIT"));
        assert!(!can_trace(&board, "QIT"));
    }

    #[test]
    fn accepted_word_scores_and_duplicates_are_rejected() {
        let mut game = GameState::new(2);
        let (seat, _) = game.join("Tester").unwrap();
        start_game(&mut game, seat, Mode::Classic, false);
        game.board = Some(Board {
            size: 2,
            letters: vec!["S".into(), "U".into(), "E".into(), "X".into()],
        });
        assert_eq!(game.submit_word(seat, "sue").unwrap().points, 1);
        assert!(matches!(
            game.submit_word(seat, "SUE"),
            Err(ActionError::DuplicateWord)
        ));
        assert_eq!(game.players[seat].words[0].word, "SUE");
        assert_eq!(game.players[seat].words[0].points, 1);
    }

    #[test]
    fn accepted_words_use_standard_points_without_a_combo_bonus() {
        let mut game = GameState::new(1);
        let (seat, _) = game.join("Tester").unwrap();
        start_game(&mut game, seat, Mode::Classic, false);
        game.board = Some(Board {
            size: 2,
            letters: vec!["A".into(), "L".into(), "E".into(), "T".into()],
        });
        assert_eq!(game.submit_word(seat, "ALE").unwrap().points, 1);
        assert_eq!(game.submit_word(seat, "LEA").unwrap().points, 1);
        assert_eq!(game.submit_word(seat, "LET").unwrap().points, 1);
        assert_eq!(game.players[seat].score, 3);
        assert_eq!(game.recent_activity.len(), 3);
    }

    #[test]
    fn game_over_payload_includes_word_scores_for_recap() {
        let mut game = GameState::new(2);
        let (seat, _) = game.join("Tester").unwrap();
        let (other_seat, _) = game.join("Wife").unwrap();
        start_game(&mut game, seat, Mode::Classic, false);
        game.board = Some(Board {
            size: 2,
            letters: vec!["S".into(), "U".into(), "E".into(), "X".into()],
        });
        game.submit_word(seat, "SUE").unwrap();
        game.submit_word(other_seat, "SUE").unwrap();
        game.ends_at = Some(Instant::now() - Duration::from_secs(1));
        assert!(game.tick());
        let client = game.to_client_state(seat);
        assert_eq!(client.my_words[0].word, "SUE");
        assert_eq!(client.my_words[0].points, 1);
        assert_eq!(client.players[0].words.as_ref().unwrap()[0].word, "SUE");
        assert_eq!(client.players[1].words.as_ref().unwrap()[0].points, 1);
    }

    #[test]
    fn shared_word_cancellation_recalculates_every_players_score() {
        let mut game = GameState::new(2);
        let (first, _) = game.join("Luke").unwrap();
        let (second, _) = game.join("Wife").unwrap();
        start_game(&mut game, first, Mode::Classic, true);
        game.board = Some(Board {
            size: 2,
            letters: vec!["S".into(), "U".into(), "E".into(), "X".into()],
        });

        assert_eq!(game.submit_word(first, "SUE").unwrap().points, 1);
        let result = game.submit_word(second, "SUE").unwrap();

        assert!(result.shared_cancelled);
        assert_eq!(result.points, 0);
        assert_eq!(game.players[first].score, 0);
        assert_eq!(game.players[second].score, 0);
        assert_eq!(game.players[first].words[0].points, 0);
    }

    #[test]
    fn netflix_unique_bonus_becomes_base_points_when_word_is_shared() {
        let mut game = GameState::new(2);
        let (first, _) = game.join("Luke").unwrap();
        let (second, _) = game.join("Wife").unwrap();
        start_game(&mut game, first, Mode::Netflix, false);
        game.board = Some(Board {
            size: 2,
            letters: vec!["S".into(), "U".into(), "E".into(), "X".into()],
        });

        let unique = game.submit_word(first, "SUE").unwrap();
        assert!(unique.unique_bonus);
        assert_eq!(unique.points, 2);

        let shared = game.submit_word(second, "SUE").unwrap();
        assert!(!shared.unique_bonus);
        assert_eq!(shared.points, 1);
        assert_eq!(game.players[first].score, 1);
        assert_eq!(game.players[second].score, 1);
    }

    #[test]
    fn netflix_points_follow_the_published_length_curve() {
        assert_eq!(score_word(Mode::Netflix, "CAT", 1, false, false), 1);
        assert_eq!(score_word(Mode::Netflix, "WORD", 1, false, false), 2);
        assert_eq!(score_word(Mode::Netflix, "FIVES", 1, false, false), 3);
        assert_eq!(score_word(Mode::Netflix, "LONGER", 1, false, false), 4);
    }

    #[test]
    fn validates_configurable_board_and_timer() {
        let mut game = GameState::new(1);
        let (seat, _) = game.join("Tester").unwrap();
        game.start(seat, Mode::Netflix, 6, 120, false).unwrap();
        assert_eq!(game.board_size, 6);
        assert_eq!(game.board.as_ref().unwrap().letters.len(), 36);
        assert_eq!(game.duration_secs, 120);
        let mut invalid = GameState::new(1);
        let (invalid_seat, _) = invalid.join("Tester").unwrap();
        assert!(matches!(
            invalid.start(invalid_seat, Mode::Classic, 7, 180, true),
            Err(ActionError::InvalidSettings)
        ));
    }

    #[test]
    fn every_board_size_uses_balanced_dice_with_qu_support() {
        let mut rng = StdRng::seed_from_u64(0xBAFF1E);

        for size in 4..=6 {
            let mut vowels = 0;
            let mut qu_tiles = 0;
            let mut double_tiles = 0;
            let board_count = 256;
            for _ in 0..board_count {
                let board = make_board_with_rng(size, &mut rng);
                assert_eq!(board.letters.len(), size * size);
                for tile in board.letters {
                    assert!(tile.bytes().all(|byte| byte.is_ascii_uppercase()));
                    if matches!(tile.as_bytes()[0], b'A' | b'E' | b'I' | b'O' | b'U') {
                        vowels += 1;
                    }
                    if tile == "QU" {
                        qu_tiles += 1;
                    }
                    if tile.len() > 1 {
                        double_tiles += 1;
                    }
                }
            }
            let vowel_ratio = vowels as f64 / (board_count * size * size) as f64;
            assert!((0.25..0.55).contains(&vowel_ratio));
            assert!(qu_tiles > 10, "size {size} did not roll enough Qu tiles");
            assert!(double_tiles >= qu_tiles);
        }
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
