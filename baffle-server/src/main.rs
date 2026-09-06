#[macro_use]
extern crate rocket;

use game::{ActionError, ClientAction, GameState, Phase};
use rocket::fairing::{Fairing, Info, Kind};
use rocket::fs::FileServer;
use rocket::futures::lock::Mutex;
use rocket::futures::{SinkExt, StreamExt};
use rocket::http::Header;
use rocket::tokio::sync::broadcast::{self, Sender};
use rocket::{tokio, State};
use rocket_ws::{Message, WebSocket};
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf, sync::Arc, time::Instant};

mod game;
mod words;

struct Room {
    state: GameState,
    sender: Sender<()>,
    last_activity: Instant,
    rematch_code: Option<String>,
}
struct Rooms(HashMap<String, Room>);
struct LobbySender(Sender<()>);

fn room_code() -> String {
    use rand::Rng;
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ";
    let mut rng = rand::thread_rng();
    (0..4)
        .map(|_| CHARS[rng.gen_range(0..CHARS.len())] as char)
        .collect()
}

fn snapshot(state: &GameState, seat: usize) -> String {
    serde_json::to_string(&state.to_client_state(seat)).expect("state serializes")
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
        let (seat, connection_id) = { let mut all = rooms.lock().await; let room = all.0.entry(code.clone()).or_insert_with(|| { let (tx, _) = broadcast::channel(16); Room { state: GameState::new(8), sender: tx, last_activity: Instant::now(), rematch_code: None } }); match room.state.join(&player) { Ok(connection) => { room.last_activity = Instant::now(); sender = room.sender.clone(); connection }, Err(err) => { let _ = stream.send(Message::Text(format!("{{\"Err\":\"{}\"}}", err))).await; return Ok(()); } } };
        // Notify every already-connected player in the room. The joining
        // player gets the initial snapshot below, so this broadcast is for
        // the host and other waiting players.
        let _ = sender.send(());
        let _ = lobby_sender.send(());
        let mut updates = sender.subscribe();
        if let Some(room) = rooms.lock().await.0.get(&code) { let _ = stream.send(Message::Text(snapshot(&room.state, seat))).await; }
        loop {
            tokio::select! {
            incoming = stream.next() => match incoming { Some(Ok(message)) => handle_message(message, &code, seat, rooms.clone(), &mut stream, &sender, &lobby_sender).await, _ => break },
                update = updates.recv() => { if update.is_err() { break; } if let Some(room) = rooms.lock().await.0.get(&code) { let _ = stream.send(Message::Text(snapshot(&room.state, seat))).await; } else { break; } }
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
        let new_code = match all.0.get(code) {
            Some(room) if room.state.phase == Phase::GameOver => {
                room.rematch_code.clone().unwrap_or_else(|| room_code())
            }
            _ => {
                let _ = stream
                    .send(Message::Text("{\"Err\":\"WrongPhase\"}".into()))
                    .await;
                return;
            }
        };
        if !all.0.contains_key(&new_code) {
            let (tx, _) = broadcast::channel(16);
            all.0.insert(
                new_code.clone(),
                Room {
                    state: GameState::new(8),
                    sender: tx,
                    last_activity: Instant::now(),
                    rematch_code: None,
                },
            );
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
    let mut accepted: Option<u32> = None;
    let mut started = false;
    let mut error: Option<ActionError> = None;
    {
        let mut all = rooms.lock().await;
        if let Some(room) = all.0.get_mut(code) {
            room.last_activity = Instant::now();
            room.state.tick();
            let result = match action {
                ClientAction::Start { mode } => {
                    started = true;
                    room.state.start(seat, mode).map(|_| 0)
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
    if let Some(points) = accepted {
        let _ = stream
            .send(Message::Text(format!(
                "{{\"word_accepted\":true,\"points\":{}}}",
                points
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
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                let mut all = rooms.lock().await;
                for room in all.0.values_mut() {
                    if room.state.tick() {
                        let _ = room.sender.send(());
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
    rocket::build()
        .manage(Arc::new(Mutex::new(Rooms(HashMap::new()))))
        .manage(LobbySender(lobby_sender))
        .attach(Headers)
        .attach(Expiry)
        .mount("/", routes![game_socket, lobby_socket, list_rooms, health])
        .mount("/", FileServer::from(PathBuf::from(client_dir)))
}
