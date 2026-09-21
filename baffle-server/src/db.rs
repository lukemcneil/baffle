use crate::game::{FoundWord, GameState, Mode};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

pub type DbResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub struct Database {
    connection: Mutex<Connection>,
}

#[derive(Debug, Serialize)]
pub struct GameSettings {
    pub mode: String,
    pub board_size: usize,
    pub duration_secs: u64,
    pub cancel_shared_words: bool,
}

#[derive(Debug, Serialize)]
pub struct GameSummary {
    pub id: i64,
    pub room_code: String,
    pub series_id: String,
    pub round_number: u32,
    pub finished_at_ms: i64,
    pub settings: GameSettings,
    pub player_count: usize,
    pub winners: Vec<String>,
    pub winning_score: u32,
    pub perfect_score: u32,
}

#[derive(Debug, Serialize)]
pub struct StoredWord {
    pub word: String,
    pub points: u32,
    pub word_length: usize,
    pub submitted_ms: u64,
    pub finder_count: usize,
    pub is_shared: bool,
    pub unique_bonus: bool,
}

#[derive(Debug, Serialize)]
pub struct StoredPlayer {
    pub seat: usize,
    pub name: String,
    pub final_score: u32,
    pub placement: usize,
    pub accepted_words: usize,
    pub scoring_words: usize,
    pub canceled_words: usize,
    pub canceled_points: u32,
    pub total_attempts: u32,
    pub not_a_word_attempts: u32,
    pub not_on_board_attempts: u32,
    pub duplicate_attempts: u32,
    pub invalid_attempts: u32,
    pub words: Vec<StoredWord>,
}

#[derive(Debug, Serialize)]
pub struct GameDetail {
    pub id: i64,
    pub room_code: String,
    pub series_id: String,
    pub round_number: u32,
    pub started_at_ms: i64,
    pub finished_at_ms: i64,
    pub settings: GameSettings,
    pub board: Vec<String>,
    pub possible_words: Vec<FoundWord>,
    pub perfect_score: u32,
    pub players: Vec<StoredPlayer>,
}

#[derive(Debug, Serialize)]
pub struct PlayerGame {
    pub game_id: i64,
    pub finished_at_ms: i64,
    pub score: u32,
    pub placement: usize,
    pub player_count: usize,
    pub word_count: usize,
    pub efficiency: f64,
    pub settings: GameSettings,
}

#[derive(Debug, Serialize)]
pub struct ConfigStats {
    pub label: String,
    pub games: usize,
    pub wins: usize,
    pub avg_score: f64,
    pub high_score: u32,
    pub avg_efficiency: f64,
}

#[derive(Debug, Serialize)]
pub struct RivalStats {
    pub name: String,
    pub games: usize,
    pub wins: usize,
    pub losses: usize,
    pub ties: usize,
}

#[derive(Debug, Serialize)]
pub struct WordRecord {
    pub word: String,
    pub length: usize,
    pub game_id: i64,
}

#[derive(Debug, Serialize)]
pub struct PlayerStats {
    pub name: String,
    pub games_played: usize,
    pub wins: usize,
    pub win_rate: f64,
    pub total_points: u64,
    pub avg_score: f64,
    pub high_score: u32,
    pub high_score_game_id: Option<i64>,
    pub total_words: usize,
    pub unique_words: usize,
    pub avg_words: f64,
    pub avg_word_length: f64,
    pub longest_word: Option<WordRecord>,
    pub favorite_word: Option<String>,
    pub avg_efficiency: f64,
    pub submission_accuracy: f64,
    pub canceled_words: usize,
    pub canceled_points: u64,
    pub recent_games: Vec<PlayerGame>,
    pub by_config: Vec<ConfigStats>,
    pub rivals: Vec<RivalStats>,
}

#[derive(Debug, Serialize)]
pub struct LeaderboardEntry {
    pub rank: usize,
    pub name: String,
    pub game_id: i64,
    pub score: u32,
    pub efficiency: f64,
    pub finished_at_ms: i64,
    pub settings: GameSettings,
}

#[derive(Debug, Default)]
pub struct LeaderboardFilter<'a> {
    pub limit: usize,
    pub metric: &'a str,
    pub mode: Option<&'a str>,
    pub board_size: Option<usize>,
    pub duration_secs: Option<u64>,
    pub cancel_shared_words: Option<bool>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> DbResult<Self> {
        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    #[cfg(test)]
    pub fn in_memory() -> DbResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> DbResult<Self> {
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;",
        )?;
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn save_game(
        &self,
        room_code: &str,
        series_id: &str,
        round_number: u32,
        started_at_ms: i64,
        finished_at_ms: i64,
        state: &GameState,
    ) -> DbResult<i64> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned")?;
        let transaction = connection.transaction()?;
        let mode = mode_name(state.mode);
        let board_json = serde_json::to_string(
            &state
                .board
                .as_ref()
                .map(|board| board.letters.clone())
                .unwrap_or_default(),
        )?;
        let possible_words_json =
            serde_json::to_string(&state.possible_words.as_ref().cloned().unwrap_or_default())?;
        transaction.execute(
            "INSERT OR IGNORE INTO games
             (room_code, series_id, round_number, started_at_ms, finished_at_ms, mode,
              board_size, duration_secs, cancel_shared_words, board_json,
              possible_words_json, perfect_score, player_count, rules_version, dictionary_version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1, 1)",
            params![
                room_code,
                series_id,
                round_number,
                started_at_ms,
                finished_at_ms,
                mode,
                state.board_size as i64,
                state.duration_secs as i64,
                state.cancel_shared_words,
                board_json,
                possible_words_json,
                state.perfect_score.unwrap_or(0) as i64,
                state.players.len() as i64,
            ],
        )?;
        let game_id: i64 = transaction.query_row(
            "SELECT id FROM games WHERE series_id = ?1 AND round_number = ?2",
            params![series_id, round_number],
            |row| row.get(0),
        )?;

        let existing_players: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM game_players WHERE game_id = ?1",
            [game_id],
            |row| row.get(0),
        )?;
        if existing_players > 0 {
            transaction.commit()?;
            return Ok(game_id);
        }

        let mut finder_counts = HashMap::<String, usize>::new();
        for player in &state.players {
            for found in &player.words {
                *finder_counts.entry(found.word.clone()).or_default() += 1;
            }
        }
        let mut scores: Vec<u32> = state.players.iter().map(|player| player.score).collect();
        scores.sort_unstable_by(|a, b| b.cmp(a));
        scores.dedup();

        for player in &state.players {
            let placement = scores
                .iter()
                .position(|score| *score == player.score)
                .unwrap_or(0)
                + 1;
            let canceled: Vec<&FoundWord> = player
                .words
                .iter()
                .filter(|found| {
                    state.cancel_shared_words
                        && finder_counts.get(&found.word).copied().unwrap_or(1) > 1
                })
                .collect();
            let canceled_points: u32 = canceled
                .iter()
                .map(|found| base_score(state.mode, &found.word))
                .sum();
            let first_word_ms = player.words.iter().map(|word| word.submitted_ms).min();
            let last_word_ms = player.words.iter().map(|word| word.submitted_ms).max();
            transaction.execute(
                "INSERT INTO game_players
                 (game_id, seat, name, name_lower, final_score, placement, accepted_words,
                  scoring_words, canceled_words, canceled_points, total_attempts,
                  not_a_word_attempts, not_on_board_attempts, duplicate_attempts,
                  invalid_attempts, first_word_ms, last_word_ms)
                 VALUES (?1, ?2, ?3, lower(?3), ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                         ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    game_id,
                    player.seat as i64,
                    player.name,
                    player.score as i64,
                    placement as i64,
                    player.words.len() as i64,
                    player.words.iter().filter(|word| word.points > 0).count() as i64,
                    canceled.len() as i64,
                    canceled_points as i64,
                    player.attempts.total as i64,
                    player.attempts.not_a_word as i64,
                    player.attempts.not_on_board as i64,
                    player.attempts.duplicate as i64,
                    player.attempts.invalid as i64,
                    first_word_ms.map(|value| value as i64),
                    last_word_ms.map(|value| value as i64),
                ],
            )?;
            let game_player_id = transaction.last_insert_rowid();
            for found in &player.words {
                let finder_count = finder_counts.get(&found.word).copied().unwrap_or(1);
                transaction.execute(
                    "INSERT INTO player_words
                     (game_player_id, word, points, word_length, submitted_ms,
                      finder_count, is_shared, unique_bonus)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        game_player_id,
                        found.word,
                        found.points as i64,
                        found.word.len() as i64,
                        found.submitted_ms as i64,
                        finder_count as i64,
                        finder_count > 1,
                        state.mode == Mode::Party && state.players.len() > 1 && finder_count == 1,
                    ],
                )?;
            }
        }
        transaction.commit()?;
        Ok(game_id)
    }

    pub fn recent_games(&self, limit: usize) -> DbResult<Vec<GameSummary>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned")?;
        load_game_summaries(&connection, limit.clamp(1, 100))
    }

    pub fn game_detail(&self, id: i64) -> DbResult<Option<GameDetail>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned")?;
        let mut statement = connection.prepare(
            "SELECT room_code, series_id, round_number, started_at_ms, finished_at_ms,
                    mode, board_size, duration_secs, cancel_shared_words, board_json,
                    possible_words_json, perfect_score
             FROM games WHERE id = ?1",
        )?;
        let row = statement
            .query_row([id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, bool>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, String>(10)?,
                    row.get::<_, i64>(11)?,
                ))
            })
            .optional()?;
        let Some((
            room_code,
            series_id,
            round_number,
            started_at_ms,
            finished_at_ms,
            mode,
            board_size,
            duration_secs,
            cancel_shared_words,
            board_json,
            possible_words_json,
            perfect_score,
        )) = row
        else {
            return Ok(None);
        };
        drop(statement);

        let mut player_statement = connection.prepare(
            "SELECT id, seat, name, final_score, placement, accepted_words, scoring_words,
                    canceled_words, canceled_points, total_attempts, not_a_word_attempts,
                    not_on_board_attempts, duplicate_attempts, invalid_attempts
             FROM game_players WHERE game_id = ?1 ORDER BY placement, seat",
        )?;
        let player_rows = player_statement.query_map([id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, i64>(9)?,
                row.get::<_, i64>(10)?,
                row.get::<_, i64>(11)?,
                row.get::<_, i64>(12)?,
                row.get::<_, i64>(13)?,
            ))
        })?;
        let mut players = Vec::new();
        for player in player_rows {
            let (
                player_id,
                seat,
                name,
                final_score,
                placement,
                accepted_words,
                scoring_words,
                canceled_words,
                canceled_points,
                total_attempts,
                not_a_word_attempts,
                not_on_board_attempts,
                duplicate_attempts,
                invalid_attempts,
            ) = player?;
            players.push(StoredPlayer {
                seat: seat as usize,
                name,
                final_score: final_score as u32,
                placement: placement as usize,
                accepted_words: accepted_words as usize,
                scoring_words: scoring_words as usize,
                canceled_words: canceled_words as usize,
                canceled_points: canceled_points as u32,
                total_attempts: total_attempts as u32,
                not_a_word_attempts: not_a_word_attempts as u32,
                not_on_board_attempts: not_on_board_attempts as u32,
                duplicate_attempts: duplicate_attempts as u32,
                invalid_attempts: invalid_attempts as u32,
                words: load_words(&connection, player_id)?,
            });
        }
        Ok(Some(GameDetail {
            id,
            room_code,
            series_id,
            round_number: round_number as u32,
            started_at_ms,
            finished_at_ms,
            settings: GameSettings {
                mode,
                board_size: board_size as usize,
                duration_secs: duration_secs as u64,
                cancel_shared_words,
            },
            board: serde_json::from_str(&board_json)?,
            possible_words: serde_json::from_str(&possible_words_json)?,
            perfect_score: perfect_score as u32,
            players,
        }))
    }

    pub fn leaderboard(&self, filter: LeaderboardFilter<'_>) -> DbResult<Vec<LeaderboardEntry>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned")?;
        let mut statement = connection.prepare(
            "SELECT gp.name, gp.final_score, g.id, g.finished_at_ms, g.mode,
                    g.board_size, g.duration_secs, g.cancel_shared_words, g.perfect_score
             FROM game_players gp JOIN games g ON g.id = gp.game_id",
        )?;
        let rows = statement.query_map([], |row| {
            let score = row.get::<_, i64>(1)? as u32;
            let perfect = row.get::<_, i64>(8)? as u32;
            Ok(LeaderboardEntry {
                rank: 0,
                name: row.get(0)?,
                game_id: row.get(2)?,
                score,
                efficiency: efficiency(score, perfect),
                finished_at_ms: row.get(3)?,
                settings: GameSettings {
                    mode: row.get(4)?,
                    board_size: row.get::<_, i64>(5)? as usize,
                    duration_secs: row.get::<_, i64>(6)? as u64,
                    cancel_shared_words: row.get(7)?,
                },
            })
        })?;
        let mut entries = rows.collect::<Result<Vec<_>, _>>()?;
        entries.retain(|entry| {
            filter.mode.is_none_or(|mode| entry.settings.mode == mode)
                && filter
                    .board_size
                    .is_none_or(|size| entry.settings.board_size == size)
                && filter
                    .duration_secs
                    .is_none_or(|duration| entry.settings.duration_secs == duration)
                && filter
                    .cancel_shared_words
                    .is_none_or(|cancel| entry.settings.cancel_shared_words == cancel)
        });
        if filter.metric == "score" {
            entries.sort_by(|a, b| {
                b.score
                    .cmp(&a.score)
                    .then_with(|| b.finished_at_ms.cmp(&a.finished_at_ms))
            });
        } else {
            entries.sort_by(|a, b| {
                b.efficiency
                    .total_cmp(&a.efficiency)
                    .then_with(|| b.score.cmp(&a.score))
            });
        }
        let mut seen = HashSet::new();
        entries.retain(|entry| seen.insert(entry.name.to_ascii_lowercase()));
        entries.truncate(filter.limit.clamp(1, 100));
        for (index, entry) in entries.iter_mut().enumerate() {
            entry.rank = index + 1;
        }
        Ok(entries)
    }

    pub fn player_stats(&self, requested_name: &str) -> DbResult<Option<PlayerStats>> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned")?;
        let name_lower = requested_name.trim().to_ascii_lowercase();
        let name: Option<String> = connection
            .query_row(
                "SELECT name FROM game_players WHERE name_lower = ?1 ORDER BY id DESC LIMIT 1",
                [&name_lower],
                |row| row.get(0),
            )
            .optional()?;
        let Some(name) = name else { return Ok(None) };

        let aggregate = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(CASE WHEN placement = 1 THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(final_score), 0), COALESCE(AVG(final_score), 0),
                    COALESCE(MAX(final_score), 0), COALESCE(SUM(accepted_words), 0),
                    COALESCE(AVG(accepted_words), 0), COALESCE(SUM(total_attempts), 0),
                    COALESCE(SUM(canceled_words), 0), COALESCE(SUM(canceled_points), 0)
             FROM game_players WHERE name_lower = ?1",
            [&name_lower],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, f64>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                ))
            },
        )?;
        let (
            games,
            wins,
            total_points,
            avg_score,
            high_score,
            total_words,
            avg_words,
            attempts,
            canceled_words,
            canceled_points,
        ) = aggregate;
        let high_score_game_id = connection
            .query_row(
                "SELECT game_id FROM game_players WHERE name_lower = ?1 ORDER BY final_score DESC, id DESC LIMIT 1",
                [&name_lower],
                |row| row.get(0),
            )
            .optional()?;
        let unique_words: i64 = connection.query_row(
            "SELECT COUNT(DISTINCT pw.word) FROM player_words pw
             JOIN game_players gp ON gp.id = pw.game_player_id WHERE gp.name_lower = ?1",
            [&name_lower],
            |row| row.get(0),
        )?;
        let avg_word_length: f64 = connection.query_row(
            "SELECT COALESCE(AVG(pw.word_length), 0) FROM player_words pw
             JOIN game_players gp ON gp.id = pw.game_player_id WHERE gp.name_lower = ?1",
            [&name_lower],
            |row| row.get(0),
        )?;
        let longest_word = connection
            .query_row(
                "SELECT pw.word, pw.word_length, gp.game_id FROM player_words pw
                 JOIN game_players gp ON gp.id = pw.game_player_id
                 WHERE gp.name_lower = ?1 ORDER BY pw.word_length DESC, pw.word ASC LIMIT 1",
                [&name_lower],
                |row| {
                    Ok(WordRecord {
                        word: row.get(0)?,
                        length: row.get::<_, i64>(1)? as usize,
                        game_id: row.get(2)?,
                    })
                },
            )
            .optional()?;
        let favorite_word = connection
            .query_row(
                "SELECT pw.word FROM player_words pw JOIN game_players gp ON gp.id = pw.game_player_id
                 WHERE gp.name_lower = ?1 GROUP BY pw.word ORDER BY COUNT(*) DESC, pw.word ASC LIMIT 1",
                [&name_lower],
                |row| row.get(0),
            )
            .optional()?;
        let avg_efficiency: f64 = connection.query_row(
            "SELECT COALESCE(AVG(CASE WHEN g.perfect_score > 0
                        THEN gp.final_score * 100.0 / g.perfect_score ELSE 0 END), 0)
             FROM game_players gp JOIN games g ON g.id = gp.game_id WHERE gp.name_lower = ?1",
            [&name_lower],
            |row| row.get(0),
        )?;
        let recent_games = load_player_games(&connection, &name_lower, 10)?;
        let by_config = load_config_stats(&connection, &name_lower)?;
        let rivals = load_rivals(&connection, &name_lower)?;
        Ok(Some(PlayerStats {
            name,
            games_played: games as usize,
            wins: wins as usize,
            win_rate: percent(wins as u64, games as u64),
            total_points: total_points as u64,
            avg_score,
            high_score: high_score as u32,
            high_score_game_id,
            total_words: total_words as usize,
            unique_words: unique_words as usize,
            avg_words,
            avg_word_length,
            longest_word,
            favorite_word,
            avg_efficiency,
            submission_accuracy: percent(total_words as u64, attempts as u64),
            canceled_words: canceled_words as usize,
            canceled_points: canceled_points as u64,
            recent_games,
            by_config,
            rivals,
        }))
    }
}

fn migrate(connection: &Connection) -> DbResult<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS games (
            id INTEGER PRIMARY KEY,
            room_code TEXT NOT NULL,
            series_id TEXT NOT NULL,
            round_number INTEGER NOT NULL,
            started_at_ms INTEGER NOT NULL,
            finished_at_ms INTEGER NOT NULL,
            mode TEXT NOT NULL,
            board_size INTEGER NOT NULL,
            duration_secs INTEGER NOT NULL,
            cancel_shared_words INTEGER NOT NULL,
            board_json TEXT NOT NULL,
            possible_words_json TEXT NOT NULL,
            perfect_score INTEGER NOT NULL,
            player_count INTEGER NOT NULL,
            rules_version INTEGER NOT NULL,
            dictionary_version INTEGER NOT NULL,
            UNIQUE(series_id, round_number)
         );
         CREATE TABLE IF NOT EXISTS game_players (
            id INTEGER PRIMARY KEY,
            game_id INTEGER NOT NULL REFERENCES games(id) ON DELETE CASCADE,
            seat INTEGER NOT NULL,
            name TEXT NOT NULL,
            name_lower TEXT NOT NULL,
            final_score INTEGER NOT NULL,
            placement INTEGER NOT NULL,
            accepted_words INTEGER NOT NULL,
            scoring_words INTEGER NOT NULL,
            canceled_words INTEGER NOT NULL,
            canceled_points INTEGER NOT NULL,
            total_attempts INTEGER NOT NULL,
            not_a_word_attempts INTEGER NOT NULL,
            not_on_board_attempts INTEGER NOT NULL,
            duplicate_attempts INTEGER NOT NULL,
            invalid_attempts INTEGER NOT NULL,
            first_word_ms INTEGER,
            last_word_ms INTEGER,
            UNIQUE(game_id, seat)
         );
         CREATE TABLE IF NOT EXISTS player_words (
            id INTEGER PRIMARY KEY,
            game_player_id INTEGER NOT NULL REFERENCES game_players(id) ON DELETE CASCADE,
            word TEXT NOT NULL,
            points INTEGER NOT NULL,
            word_length INTEGER NOT NULL,
            submitted_ms INTEGER NOT NULL,
            finder_count INTEGER NOT NULL,
            is_shared INTEGER NOT NULL,
            unique_bonus INTEGER NOT NULL,
            UNIQUE(game_player_id, word)
         );
         CREATE INDEX IF NOT EXISTS idx_games_finished ON games(finished_at_ms DESC);
         CREATE INDEX IF NOT EXISTS idx_game_players_name ON game_players(name_lower);
         CREATE INDEX IF NOT EXISTS idx_game_players_score ON game_players(final_score DESC);
         CREATE INDEX IF NOT EXISTS idx_player_words_word ON player_words(word);
         UPDATE games SET mode = 'party' WHERE mode = 'netflix';
         PRAGMA user_version = 2;",
    )?;
    Ok(())
}

fn load_game_summaries(connection: &Connection, limit: usize) -> DbResult<Vec<GameSummary>> {
    let mut statement = connection.prepare(
        "SELECT id, room_code, series_id, round_number, finished_at_ms, mode, board_size,
                duration_secs, cancel_shared_words, player_count, perfect_score
         FROM games ORDER BY finished_at_ms DESC LIMIT ?1",
    )?;
    let rows = statement.query_map([limit as i64], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, bool>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
        ))
    })?;
    let mut games = Vec::new();
    for row in rows {
        let (
            id,
            room_code,
            series_id,
            round,
            finished,
            mode,
            size,
            duration,
            cancel,
            player_count,
            perfect,
        ) = row?;
        let winning_score: i64 = connection.query_row(
            "SELECT COALESCE(MAX(final_score), 0) FROM game_players WHERE game_id = ?1",
            [id],
            |row| row.get(0),
        )?;
        let mut winners_statement = connection.prepare(
            "SELECT name FROM game_players WHERE game_id = ?1 AND final_score = ?2 ORDER BY seat",
        )?;
        let winners = winners_statement
            .query_map(params![id, winning_score], |row| row.get(0))?
            .collect::<Result<Vec<String>, _>>()?;
        games.push(GameSummary {
            id,
            room_code,
            series_id,
            round_number: round as u32,
            finished_at_ms: finished,
            settings: GameSettings {
                mode,
                board_size: size as usize,
                duration_secs: duration as u64,
                cancel_shared_words: cancel,
            },
            player_count: player_count as usize,
            winners,
            winning_score: winning_score as u32,
            perfect_score: perfect as u32,
        });
    }
    Ok(games)
}

fn load_words(connection: &Connection, player_id: i64) -> DbResult<Vec<StoredWord>> {
    let mut statement = connection.prepare(
        "SELECT word, points, word_length, submitted_ms, finder_count, is_shared, unique_bonus
         FROM player_words WHERE game_player_id = ?1 ORDER BY submitted_ms, word",
    )?;
    let words = statement
        .query_map([player_id], |row| {
            Ok(StoredWord {
                word: row.get(0)?,
                points: row.get::<_, i64>(1)? as u32,
                word_length: row.get::<_, i64>(2)? as usize,
                submitted_ms: row.get::<_, i64>(3)? as u64,
                finder_count: row.get::<_, i64>(4)? as usize,
                is_shared: row.get(5)?,
                unique_bonus: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(words)
}

fn load_player_games(
    connection: &Connection,
    name: &str,
    limit: usize,
) -> DbResult<Vec<PlayerGame>> {
    let mut statement = connection.prepare(
        "SELECT gp.game_id, g.finished_at_ms, gp.final_score, gp.placement, g.player_count,
                gp.accepted_words, g.perfect_score, g.mode, g.board_size, g.duration_secs,
                g.cancel_shared_words
         FROM game_players gp JOIN games g ON g.id = gp.game_id
         WHERE gp.name_lower = ?1 ORDER BY g.finished_at_ms DESC LIMIT ?2",
    )?;
    let games = statement
        .query_map(params![name, limit as i64], |row| {
            let score = row.get::<_, i64>(2)? as u32;
            let perfect = row.get::<_, i64>(6)? as u32;
            Ok(PlayerGame {
                game_id: row.get(0)?,
                finished_at_ms: row.get(1)?,
                score,
                placement: row.get::<_, i64>(3)? as usize,
                player_count: row.get::<_, i64>(4)? as usize,
                word_count: row.get::<_, i64>(5)? as usize,
                efficiency: efficiency(score, perfect),
                settings: GameSettings {
                    mode: row.get(7)?,
                    board_size: row.get::<_, i64>(8)? as usize,
                    duration_secs: row.get::<_, i64>(9)? as u64,
                    cancel_shared_words: row.get(10)?,
                },
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(games)
}

fn load_config_stats(connection: &Connection, name: &str) -> DbResult<Vec<ConfigStats>> {
    let mut statement = connection.prepare(
        "SELECT g.mode, g.board_size, g.duration_secs, g.cancel_shared_words, COUNT(*),
                SUM(CASE WHEN gp.placement = 1 THEN 1 ELSE 0 END), AVG(gp.final_score),
                MAX(gp.final_score), AVG(CASE WHEN g.perfect_score > 0
                    THEN gp.final_score * 100.0 / g.perfect_score ELSE 0 END)
         FROM game_players gp JOIN games g ON g.id = gp.game_id
         WHERE gp.name_lower = ?1
         GROUP BY g.mode, g.board_size, g.duration_secs, g.cancel_shared_words
         ORDER BY COUNT(*) DESC",
    )?;
    let stats = statement
        .query_map([name], |row| {
            let mode: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            let duration: i64 = row.get(2)?;
            let cancel: bool = row.get(3)?;
            Ok(ConfigStats {
                label: format!(
                    "{} · {}×{} · {}s · shared {}",
                    mode_label(&mode),
                    size,
                    size,
                    duration,
                    if cancel { "cancel" } else { "score" }
                ),
                games: row.get::<_, i64>(4)? as usize,
                wins: row.get::<_, i64>(5)? as usize,
                avg_score: row.get(6)?,
                high_score: row.get::<_, i64>(7)? as u32,
                avg_efficiency: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(stats)
}

fn load_rivals(connection: &Connection, name: &str) -> DbResult<Vec<RivalStats>> {
    let mut statement = connection.prepare(
        "SELECT other.name, COUNT(*),
                SUM(CASE WHEN me.final_score > other.final_score THEN 1 ELSE 0 END),
                SUM(CASE WHEN me.final_score < other.final_score THEN 1 ELSE 0 END),
                SUM(CASE WHEN me.final_score = other.final_score THEN 1 ELSE 0 END)
         FROM game_players me JOIN game_players other ON me.game_id = other.game_id AND me.id != other.id
         WHERE me.name_lower = ?1
         GROUP BY other.name_lower ORDER BY COUNT(*) DESC, other.name LIMIT 12",
    )?;
    let rivals = statement
        .query_map([name], |row| {
            Ok(RivalStats {
                name: row.get(0)?,
                games: row.get::<_, i64>(1)? as usize,
                wins: row.get::<_, i64>(2)? as usize,
                losses: row.get::<_, i64>(3)? as usize,
                ties: row.get::<_, i64>(4)? as usize,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rivals)
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Classic => "classic",
        Mode::Party => "party",
    }
}

fn mode_label(mode: &str) -> &'static str {
    if mode == "party" {
        "Party"
    } else {
        "Classic"
    }
}

fn base_score(mode: Mode, word: &str) -> u32 {
    match mode {
        Mode::Classic => match word.len() {
            3 | 4 => 1,
            5 => 2,
            6 => 3,
            7 => 5,
            _ => 11,
        },
        Mode::Party => word.len().saturating_sub(2) as u32,
    }
}

fn efficiency(score: u32, perfect: u32) -> f64 {
    if perfect == 0 {
        0.0
    } else {
        score as f64 * 100.0 / perfect as f64
    }
}

fn percent(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 * 100.0 / denominator as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{AttemptStats, Board, Phase};

    fn completed_game(mode: Mode, cancel_shared: bool) -> GameState {
        let mut game = GameState::new(8);
        game.mode = mode;
        game.board_size = 4;
        game.duration_secs = 60;
        game.cancel_shared_words = cancel_shared;
        game.board = Some(Board {
            size: 4,
            letters: "ABCDEFGHIJKLMNOP"
                .chars()
                .map(|letter| letter.to_string())
                .collect(),
        });
        game.possible_words = Some(vec![
            FoundWord {
                word: "ABLE".into(),
                points: 2,
                submitted_ms: 0,
            },
            FoundWord {
                word: "BAKE".into(),
                points: 2,
                submitted_ms: 0,
            },
        ]);
        game.perfect_score = Some(4);
        game.join("Alice").unwrap();
        game.join("Bob").unwrap();
        game.phase = Phase::GameOver;
        game.players[0].score = 2;
        game.players[0].words = vec![FoundWord {
            word: "ABLE".into(),
            points: 2,
            submitted_ms: 4100,
        }];
        game.players[0].attempts = AttemptStats {
            total: 2,
            not_a_word: 1,
            ..Default::default()
        };
        game.players[1].score = 2;
        game.players[1].words = vec![FoundWord {
            word: "BAKE".into(),
            points: 2,
            submitted_ms: 5200,
        }];
        game.players[1].attempts = AttemptStats {
            total: 1,
            ..Default::default()
        };
        game
    }

    #[test]
    fn saves_once_and_reopens_the_exact_game() {
        let db = Database::in_memory().unwrap();
        let game = completed_game(Mode::Classic, false);
        let first = db
            .save_game("TEST", "series", 1, 1000, 61000, &game)
            .unwrap();
        let second = db
            .save_game("TEST", "series", 1, 1000, 61000, &game)
            .unwrap();
        assert_eq!(first, second);
        assert_eq!(db.recent_games(10).unwrap().len(), 1);
        let detail = db.game_detail(first).unwrap().unwrap();
        assert_eq!(detail.board, game.board.unwrap().letters);
        assert_eq!(detail.players.len(), 2);
        assert_eq!(detail.players[0].words[0].submitted_ms, 4100);
    }

    #[test]
    fn calculates_stats_and_filtered_records() {
        let db = Database::in_memory().unwrap();
        let game = completed_game(Mode::Classic, false);
        db.save_game("TEST", "series", 1, 1000, 61000, &game)
            .unwrap();
        let stats = db.player_stats("aLiCe").unwrap().unwrap();
        assert_eq!(stats.games_played, 1);
        assert_eq!(stats.wins, 1);
        assert_eq!(stats.submission_accuracy, 50.0);
        assert_eq!(stats.longest_word.unwrap().word, "ABLE");
        let records = db
            .leaderboard(LeaderboardFilter {
                limit: 10,
                metric: "efficiency",
                mode: Some("classic"),
                board_size: Some(4),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].efficiency, 50.0);
        assert!(db
            .leaderboard(LeaderboardFilter {
                limit: 10,
                metric: "score",
                mode: Some("party"),
                ..Default::default()
            })
            .unwrap()
            .is_empty());
    }

    #[test]
    fn preserves_shared_word_cancellations() {
        let db = Database::in_memory().unwrap();
        let mut game = completed_game(Mode::Party, true);
        game.players[0].score = 0;
        game.players[1].score = 0;
        game.players[0].words[0] = FoundWord {
            word: "ABLE".into(),
            points: 0,
            submitted_ms: 4100,
        };
        game.players[1].words[0] = FoundWord {
            word: "ABLE".into(),
            points: 0,
            submitted_ms: 5200,
        };
        let id = db
            .save_game("SHAR", "shared-series", 1, 1000, 61000, &game)
            .unwrap();
        let detail = db.game_detail(id).unwrap().unwrap();
        assert_eq!(detail.players[0].canceled_words, 1);
        assert_eq!(detail.players[0].canceled_points, 2);
        assert!(detail.players[0].words[0].is_shared);
        assert!(!detail.players[0].words[0].unique_bonus);
    }

    #[test]
    fn migrates_the_legacy_mode_name_to_party() {
        let db = Database::in_memory().unwrap();
        let game = completed_game(Mode::Party, false);
        let id = db
            .save_game("PRTY", "party-series", 1, 1000, 61000, &game)
            .unwrap();
        {
            let connection = db.connection.lock().unwrap();
            connection
                .execute("UPDATE games SET mode = 'netflix' WHERE id = ?1", [id])
                .unwrap();
            migrate(&connection).unwrap();
        }
        assert_eq!(db.game_detail(id).unwrap().unwrap().settings.mode, "party");
    }
}
