# RIFTLINE

An original browser multiplayer arena shooter: authoritative Node server, three.js client,
shared deterministic simulation. Full design and roadmap: [docs/PLAN.md](docs/PLAN.md).

## Status

Milestone 1 (skeleton): one shared simulation, rooms, client prediction + reconciliation,
remote interpolation, placeholder avatars on a flat ground. Weapons, map, modes, bots and
touch controls follow in the order listed in the plan.

## Layout

| Package | Role |
|---|---|
| `shared/` | Constants, wire protocol and `stepPlayer` — run by both server (authority) and client (prediction) |
| `server/` | Node + `ws`: rooms, 30 Hz tick, 20 Hz snapshots, serves the built client |
| `client/` | Vite + three.js: input, netcode, rendering |

## Run

```bash
npm install
npm run dev        # server on :8080, client on Vite's port (proxying /ws)
npm test           # shared simulation tests
npm run typecheck
npm run build && npm start   # single process on :8080
```

Open two tabs to see multiplayer. `?room=CODE&name=You` joins a private room.
