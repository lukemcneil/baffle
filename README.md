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

Open <http://localhost:8088>. Share the URL and a room code with friends on the same hosted server. For a different frontend host, point the browser URL at the Rust server with `?server=host:8088`.

## Play from a phone on the same Wi-Fi

The server binds to the local network on port 8088 by default:

```sh
cargo run
```

On this machine, the current address is <http://192.168.0.105:8088>. The phone and computer must be on the same Wi-Fi, and macOS may ask you to allow incoming connections for the server. Set `ROCKET_PORT` or `ROCKET_ADDRESS` to override either default.

## How to play

1. Create a room and share the four-letter code.
2. Pick a mode: Classic (3 minutes), Blitz (60 seconds), or Mega Grid (5×5 for more chaos).
3. Drag across neighboring letters to trace a word, then release to submit it. Diagonals count, and the trace samples fast movement so you do not need to hit every pixel. Tapping still works with the Submit button as a fallback. A tile can only be used once in a word.
4. Valid words score automatically for you in real time. Duplicate, disconnected, and unknown words explain what went wrong.
5. When the clock ends, the recap shows every player’s words, each word’s points, the top score, and the longest find.

Words score 1 point for 3–4 letters, 2 for 5, 3 for 6, 5 for 7, and 11 for 8+. Duplicate words do not score twice for the same player.
