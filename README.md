# Baffle

A friendly, fast, multiplayer Boggle-style word hunt. Find connected words on the same letter grid, race the clock, and see who can baffle their friends the fastest.

## Run locally

The project mirrors the shape of Far Away: a browser client folder and a Rust WebSocket server folder.

```sh
cd baffle-client
npm install
npm run build

cd ../baffle-server
cargo run
```

Open <http://localhost:8000>. Share the URL and a room code with friends on the same hosted server. For a different frontend host, point the browser URL at the Rust server with `?server=host:8000`.

## Play from a phone on the same Wi-Fi

Bind the server to the local network, then use your computer's Wi-Fi address:

```sh
ROCKET_ADDRESS=0.0.0.0 cargo run
```

On this machine, the current address is <http://192.168.0.105:8000>. The phone and computer must be on the same Wi-Fi, and macOS may ask you to allow incoming connections for the server.

## How to play

1. Create a room and share the four-letter code.
2. Pick a mode: Classic (3 minutes), Blitz (60 seconds), or Mega Grid (5×5 for more chaos).
3. Click neighboring letters to trace a word. A tile can only be used once in a word.
4. Submit words of three or more letters. Every valid word scores for everyone in the room in real time.

Words score 1 point for 3–4 letters, 2 for 5, 3 for 6, 5 for 7, and 11 for 8+. Duplicate words do not score twice for the same player.
