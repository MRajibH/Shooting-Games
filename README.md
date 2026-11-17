# RIFTLINE

An original, realistic multiplayer shooter that runs entirely in the browser.
Rust authoritative server; the same Rust simulation compiled to WebAssembly for client
prediction; Vite + TypeScript + three.js for rendering. Design and roadmap: [docs/PLAN.md](docs/PLAN.md).

## Status

Milestone 1 (skeleton) done: realistic movement (walk 3 m/s, sprint 6 m/s, crouch, 9.81 m/s²
gravity, ~0.45 m jump, acceleration), rooms with quick play and room codes, 60 Hz server tick,
30 Hz snapshots, WASM prediction + reconciliation, interpolated remote players, physical sky,
filmic tone mapping and soft shadows. Map, weapons, ballistics, modes, bots and touch controls
come next, in the order in the plan.

## Layout

| Path | Role |
|---|---|
| `crates/sim` | Shared Rust: constants, protocol, movement. Native for the server, WASM for the browser |
| `crates/server` | axum + tokio: `/ws`, room registry, one task per room, serves `client/dist` |
| `client/` | Vite + three.js: input, netcode, rendering. `src/wasm/` is generated |

## Requirements

Rust (stable) with the wasm target, `wasm-bindgen-cli` matching the crate version, Node 20+.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.130 --locked
npm install
```

## Run

```bash
npm run dev                 # builds WASM, starts the Rust server on :8080 and Vite (proxies /ws)
npm test                    # cargo test
npm run typecheck
npm run build && npm start  # one Rust binary on :8080 serving the game
```

Open two tabs to see multiplayer. `?room=CODE&name=You` joins a private room.
Controls: click to capture the mouse, WASD, Shift sprint, C / Ctrl crouch, Space jump, right mouse aim.
