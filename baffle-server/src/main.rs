#[macro_use]
extern crate rocket;

use db::{Database, LeaderboardFilter};
use game::{ActionError, ClientAction, GameState, Phase, SubmissionResult};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::fs::FileServer;
use rocket::futures::lock::Mutex;
use rocket::futures::{SinkExt, StreamExt};
use rocket::http::Header;
use rocket::http::Status;
use rocket::response::status;
use rocket::serde::json::Json;
use rocket::tokio::sync::broadcast::{self, Sender};
use rocket::{tokio, State};
use rocket_ws::{Message, WebSocket};
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};

mod db;
mod game;
mod words;

struct Room {
    state: GameState,
    sender: Sender<()>,
    last_activity: Instant,
    rematch_code: Option<String>,
    series_id: String,
    round_number: u32,
    started_at_ms: Option<i64>,
    game_record_id: Option<i64>,
    persisting: bool,
    last_persist_attempt: Option<Instant>,
}
struct Rooms(HashMap<String, Room>);
struct LobbySender(Sender<()>);

impl Room {
    fn new(series_id: String, round_number: u32) -> Self {
        let (sender, _) = broadcast::channel(16);
        Self {
            state: GameState::new(8),
            sender,
            last_activity: Instant::now(),
            rematch_code: None,
            series_id,
            round_number,
            started_at_ms: None,
            game_record_id: None,
            persisting: false,
            last_persist_attempt: None,
        }
    }
}

fn room_code() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let mut rng = rand::thread_rng();
    (0..4)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn snapshot(room: &Room, seat: usize) -> String {
    let mut state = room.state.to_client_state(seat);
    state.game_record_id = room.game_record_id;
    serde_json::to_string(&state).expect("state serializes")
}

#[get("/game/<code>?<player>")]
async fn game_socket(
    ws: WebSocket,
    code: &str,
    player: Option<&str>,
    rooms: &State<Arc<Mutex<Rooms>>>,
    lobby: &State<LobbySender>,
) -> rocket_ws::Channel<'static> {
    let code = code.to_ascii_uppercase();
    let player = player.unwrap_or("Anonymous").trim().to_string();
    let rooms = Arc::clone(rooms);
    let lobby_sender = lobby.0.clone();
    ws.channel(move |mut stream| Box::pin(async move {
        let sender;
        let (seat, connection_id) = { let mut all = rooms.lock().await; let room = all.0.entry(code.clone()).or_insert_with(|| Room::new(format!("{}-{}", code, epoch_ms()), 1)); match room.state.join(&player) { Ok(connection) => { room.last_activity = Instant::now(); sender = room.sender.clone(); connection }, Err(err) => { let _ = stream.send(Message::Text(format!("{{\"Err\":\"{}\"}}", err))).await; return Ok(()); } } };
        // Notify every already-connected player in the room. The joining
        // player gets the initial snapshot below, so this broadcast is for
        // the host and other waiting players.
        let _ = sender.send(());
        let _ = lobby_sender.send(());
        let mut updates = sender.subscribe();
        if let Some(room) = rooms.lock().await.0.get(&code) { let _ = stream.send(Message::Text(snapshot(room, seat))).await; }
        loop {
            tokio::select! {
            incoming = stream.next() => match incoming { Some(Ok(message)) => handle_message(message, &code, seat, rooms.clone(), &mut stream, &sender, &lobby_sender).await, _ => break },
                update = updates.recv() => { if update.is_err() { break; } else if let Some(room) = rooms.lock().await.0.get(&code) { let _ = stream.send(Message::Text(snapshot(room, seat))).await; } else { break; } }
            }
        }
        let mut all = rooms.lock().await; if let Some(room) = all.0.get_mut(&code) { room.state.disconnect(seat, connection_id); room.last_activity = Instant::now(); let _ = room.sender.send(()); }
        Ok(())
    }))
}

async fn handle_message(
    message: Message,
    code: &str,
    seat: usize,
    rooms: Arc<Mutex<Rooms>>,
    stream: &mut rocket_ws::stream::DuplexStream,
    sender: &Sender<()>,
    lobby: &Sender<()>,
) {
    let Message::Text(text) = message else {
        return;
    };
    if text == "ping" {
        return;
    }
    let action = match serde_json::from_str::<ClientAction>(&text) {
        Ok(action) => action,
        Err(_) => {
            let _ = stream
                .send(Message::Text("{\"Err\":\"MalformedAction\"}".into()))
                .await;
            return;
        }
    };
    if matches!(action, ClientAction::Rematch) {
        let mut all = rooms.lock().await;
        let (new_code, series_id, round_number) = match all.0.get(code) {
            Some(room) if room.state.phase == Phase::GameOver => (
                room.rematch_code.clone().unwrap_or_else(room_code),
                room.series_id.clone(),
                room.round_number + 1,
            ),
            _ => {
                let _ = stream
                    .send(Message::Text("{\"Err\":\"WrongPhase\"}".into()))
                    .await;
                return;
            }
        };
        if !all.0.contains_key(&new_code) {
            all.0
                .insert(new_code.clone(), Room::new(series_id, round_number));
        }
        if let Some(room) = all.0.get_mut(code) {
            room.rematch_code = Some(new_code.clone());
        }
        let _ = stream
            .send(Message::Text(format!(
                "{{\"rematch_code\":\"{}\"}}",
                new_code
            )))
            .await;
        let _ = lobby.send(());
        return;
    }
    let mut accepted: Option<SubmissionResult> = None;
    let mut started = false;
    let mut error: Option<ActionError> = None;
    {
        let mut all = rooms.lock().await;
        if let Some(room) = all.0.get_mut(code) {
            room.last_activity = Instant::now();
            room.state.tick();
            let result = match action {
                ClientAction::Start {
                    mode,
                    board_size,
                    duration_secs,
                    cancel_shared_words,
                } => {
                    started = true;
                    let result = room
                        .state
                        .start(seat, mode, board_size, duration_secs, cancel_shared_words)
                        .map(|_| SubmissionResult {
                            points: 0,
                            shared_cancelled: false,
                            unique_bonus: false,
                        });
                    if result.is_ok() {
                        room.started_at_ms = room.state.started_at_ms.map(|value| value as i64);
                        room.game_record_id = None;
                        room.persisting = false;
                        room.last_persist_attempt = None;
                    }
                    result
                }
                ClientAction::SubmitWord { word } => room.state.submit_word(seat, &word),
                ClientAction::Rematch => unreachable!(),
            };
            match result {
                Ok(points) => {
                    if !started {
                        accepted = Some(points);
                    }
                }
                Err(err) => error = Some(err),
            }
        } else {
            error = Some(ActionError::InvalidSeat);
        }
    }
    if let Some(err) = error {
        let _ = stream
            .send(Message::Text(format!("{{\"Err\":\"{}\"}}", err)))
            .await;
        return;
    }
    if let Some(result) = accepted {
        let _ = stream
            .send(Message::Text(format!(
                "{{\"word_accepted\":true,\"points\":{},\"shared_cancelled\":{},\"unique_bonus\":{}}}",
                result.points, result.shared_cancelled, result.unique_bonus
            )))
            .await;
    }
    let _ = sender.send(());
    if started {
        let _ = lobby.send(());
    }
}

#[derive(Serialize)]
struct RoomInfo {
    code: String,
    players: Vec<String>,
    player_count: usize,
    max_players: usize,
}
fn room_list(all: &Rooms) -> Vec<RoomInfo> {
    all.0
        .iter()
        .filter_map(|(code, room)| {
            if room.state.phase == Phase::Waiting {
                Some(RoomInfo {
                    code: code.clone(),
                    players: room
                        .state
                        .players
                        .iter()
                        .filter(|p| p.connected)
                        .map(|p| p.name.clone())
                        .collect(),
                    player_count: room.state.players.iter().filter(|p| p.connected).count(),
                    max_players: room.state.max_players,
                })
            } else {
                None
            }
        })
        .collect()
}

#[get("/api/rooms")]
async fn list_rooms(rooms: &State<Arc<Mutex<Rooms>>>) -> rocket::serde::json::Json<Vec<RoomInfo>> {
    rocket::serde::json::Json(room_list(&*rooms.lock().await))
}

#[get("/lobby")]
async fn lobby_socket(
    ws: WebSocket,
    rooms: &State<Arc<Mutex<Rooms>>>,
    lobby: &State<LobbySender>,
) -> rocket_ws::Channel<'static> {
    let rooms = Arc::clone(rooms);
    let mut updates = lobby.0.subscribe();
    ws.channel(move |mut stream| Box::pin(async move { let current = room_list(&*rooms.lock().await); let _ = stream.send(Message::Text(serde_json::to_string(&current).unwrap())).await; loop { tokio::select! { update = updates.recv() => { if update.is_err() { break; } let list = room_list(&*rooms.lock().await); let _ = stream.send(Message::Text(serde_json::to_string(&list).unwrap())).await; }, incoming = stream.next() => { if incoming.is_none() { break; } } } } Ok(()) }))
}

#[get("/health")]
fn health() -> &'static str {
    "baffle-server ok"
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

type ApiResult<T> = Result<Json<T>, status::Custom<Json<ApiError>>>;

fn api_error(status_code: Status, error: impl ToString) -> status::Custom<Json<ApiError>> {
    status::Custom(
        status_code,
        Json(ApiError {
            error: error.to_string(),
        }),
    )
}

#[get("/api/stats/games?<limit>")]
fn recent_games(
    limit: Option<usize>,
    database: &State<Arc<Database>>,
) -> ApiResult<Vec<db::GameSummary>> {
    database
        .recent_games(limit.unwrap_or(20))
        .map(Json)
        .map_err(|error| api_error(Status::InternalServerError, error))
}

#[get("/api/stats/games/<id>")]
fn game_detail(id: i64, database: &State<Arc<Database>>) -> ApiResult<db::GameDetail> {
    match database.game_detail(id) {
        Ok(Some(game)) => Ok(Json(game)),
        Ok(None) => Err(api_error(Status::NotFound, "Game not found")),
        Err(error) => Err(api_error(Status::InternalServerError, error)),
    }
}

#[get("/api/stats/player/<name>")]
fn player_stats(name: &str, database: &State<Arc<Database>>) -> ApiResult<db::PlayerStats> {
    match database.player_stats(name) {
        Ok(Some(stats)) => Ok(Json(stats)),
        Ok(None) => Err(api_error(
            Status::NotFound,
            "No completed games for that player",
        )),
        Err(error) => Err(api_error(Status::InternalServerError, error)),
    }
}

#[get("/api/stats/leaderboard?<limit>&<metric>&<mode>&<board_size>&<duration_secs>&<cancel_shared_words>")]
fn leaderboard(
    limit: Option<usize>,
    metric: Option<&str>,
    mode: Option<&str>,
    board_size: Option<usize>,
    duration_secs: Option<u64>,
    cancel_shared_words: Option<bool>,
    database: &State<Arc<Database>>,
) -> ApiResult<Vec<db::LeaderboardEntry>> {
    let metric = metric.unwrap_or("efficiency");
    if !matches!(metric, "efficiency" | "score") {
        return Err(api_error(
            Status::BadRequest,
            "metric must be efficiency or score",
        ));
    }
    database
        .leaderboard(LeaderboardFilter {
            limit: limit.unwrap_or(20),
            metric,
            mode,
            board_size,
            duration_secs,
            cancel_shared_words,
        })
        .map(Json)
        .map_err(|error| api_error(Status::InternalServerError, error))
}

struct Headers;
#[rocket::async_trait]
impl Fairing for Headers {
    fn info(&self) -> Info {
        Info {
            name: "Baffle response headers",
            kind: Kind::Response,
        }
    }
    async fn on_response<'r>(&self, req: &'r rocket::Request<'_>, res: &mut rocket::Response<'r>) {
        if req.uri().path().as_str().starts_with("/api/") {
            res.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        }
    }
}

struct Expiry;

struct PersistJob {
    code: String,
    series_id: String,
    round_number: u32,
    started_at_ms: i64,
    finished_at_ms: i64,
    state: GameState,
}

#[rocket::async_trait]
impl Fairing for Expiry {
    fn info(&self) -> Info {
        Info {
            name: "Baffle game clock",
            kind: Kind::Liftoff,
        }
    }
    async fn on_liftoff(&self, rocket: &rocket::Rocket<rocket::Orbit>) {
        let rooms = Arc::clone(rocket.state::<Arc<Mutex<Rooms>>>().unwrap());
        let database = Arc::clone(rocket.state::<Arc<Database>>().unwrap());
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let mut jobs = Vec::new();
                {
                    let mut all = rooms.lock().await;
                    for (code, room) in &mut all.0 {
                        if room.state.tick() {
                            let _ = room.sender.send(());
                        }
                        let retry_ready = room
                            .last_persist_attempt
                            .is_none_or(|attempt| attempt.elapsed().as_secs() >= 10);
                        if room.state.phase == Phase::GameOver
                            && room.game_record_id.is_none()
                            && !room.persisting
                            && retry_ready
                        {
                            room.persisting = true;
                            room.last_persist_attempt = Some(Instant::now());
                            jobs.push(PersistJob {
                                code: code.clone(),
                                series_id: room.series_id.clone(),
                                round_number: room.round_number,
                                started_at_ms: room.started_at_ms.unwrap_or_else(|| {
                                    epoch_ms() as i64 - room.state.duration_secs as i64 * 1000
                                }),
                                finished_at_ms: epoch_ms() as i64,
                                state: room.state.clone(),
                            });
                        }
                    }
                }
                for job in jobs {
                    let database = Arc::clone(&database);
                    let code = job.code.clone();
                    let series_id = job.series_id.clone();
                    let round_number = job.round_number;
                    let result = tokio::task::spawn_blocking(move || {
                        database.save_game(
                            &job.code,
                            &job.series_id,
                            job.round_number,
                            job.started_at_ms,
                            job.finished_at_ms,
                            &job.state,
                        )
                    })
                    .await;
                    let mut all = rooms.lock().await;
                    if let Some(room) = all.0.get_mut(&code) {
                        if room.series_id == series_id && room.round_number == round_number {
                            room.persisting = false;
                            match result {
                                Ok(Ok(game_id)) => {
                                    room.game_record_id = Some(game_id);
                                    let _ = room.sender.send(());
                                }
                                Ok(Err(error)) => eprintln!("could not save game {code}: {error}"),
                                Err(error) => {
                                    eprintln!("game save task failed for {code}: {error}")
                                }
                            }
                        }
                    }
                }
            }
        });
    }
}

#[launch]
fn rocket() -> _ {
    let (lobby_sender, _) = broadcast::channel(16);
    let client_dir =
        std::env::var("BAFFLE_CLIENT_DIR").unwrap_or_else(|_| "../baffle-client".into());
    let database_path = std::env::var("BAFFLE_DB_PATH").unwrap_or_else(|_| "baffle.db".into());
    let database = Arc::new(Database::open(&database_path).unwrap_or_else(|error| {
        panic!("could not open Baffle database at {database_path}: {error}")
    }));
    rocket::build()
        .configure(rocket::Config {
            address: std::env::var("ROCKET_ADDRESS")
                .ok()
                .and_then(|address| address.parse().ok())
                .unwrap_or_else(|| "0.0.0.0".parse().unwrap()),
            port: std::env::var("ROCKET_PORT")
                .ok()
                .and_then(|port| port.parse().ok())
                .unwrap_or(8088),
            ..Default::default()
        })
        .manage(Arc::new(Mutex::new(Rooms(HashMap::new()))))
        .manage(database)
        .manage(LobbySender(lobby_sender))
        .attach(Headers)
        .attach(Expiry)
        .mount(
            "/",
            routes![
                game_socket,
                lobby_socket,
                list_rooms,
                health,
                recent_games,
                game_detail,
                player_stats,
                leaderboard
            ],
        )
        .mount("/", FileServer::from(PathBuf::from(client_dir)))
}
