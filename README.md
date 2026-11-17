# RIFTLINE

An original, realistic multiplayer shooter that runs entirely in the browser.
Rust authoritative server; the same Rust simulation compiled to WebAssembly for client
prediction; Vite + TypeScript + three.js for rendering. Design and roadmap: [docs/PLAN.md](docs/PLAN.md).

## Status

**Milestone 2 done: movement, map, character, Slap mode.**

- **Map:** 64 x 64 m industrial yard (shared Rust data): brick and plaster buildings, shipping
  containers (one stacked), a 3 m steel walkway with two 15-step stairs, concrete barriers,
  crates, a vaultable low wall. Procedural PBR materials (albedo + normal maps), physical sky,
  image-based lighting, 4K soft shadows, filmic tone mapping, fog.
- **Movement (Rust, server-authoritative, WASM-predicted):** collision, stairs and kerbs,
  vaulting obstacles 0.45 to 1.25 m, crouch with headroom check, lean (Q/E), stamina with
  exhaustion lockout, forward-only sprint, real-world speeds and gravity.
- **Character:** rigged, textured soldier with motion-captured Idle / Walk / Run blended by speed,
  plus procedural layers: crouch (legs fold, feet stay planted), strafe hip twist, aim pitch,
  lean, slap swing, stagger wobble, knockout fall. Each player gets a subtle tint.
- **Slap Arena mode:** left mouse slaps. A hit in reach (1.8 m, front cone) knocks the target
  back, staggers them and adds daze; full daze (three quick slaps) is a 3.5 s knockout, then a
  respawn. Scoreboard (KOs, slaps), knockout feed, hit marker, daze meter and vignette,
  synthesised slap / whoosh / thud sounds with stereo position.
- **Combat mode:** movement only; weapons and ballistics are milestone 3.
- **Camera:** over-the-shoulder with aim zoom, lean offset and wall collision.

Debug handle in the browser console: `__riftline` (fps, renderer, scene, net).

## Layout

| Path | Role |
|---|---|
| `crates/sim` | Shared Rust: constants, protocol, map, movement and collision. Native for the server, WASM for the browser |
| `crates/server` | axum + tokio: `/ws`, room registry, one task per room, serves `client/dist` |
| `client/` | Vite + three.js: menu, HUD, input, netcode, world, character, audio. `src/wasm/` is generated |

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
Controls: click to capture the mouse, WASD, Shift sprint, Space jump / vault, C / Ctrl crouch,
Q / E lean, right mouse aim, left mouse slap. `?mode=slap|combat` preselects the mode.
