# RIFTLINE — an original browser multiplayer arena shooter

## Context

The reference (games.polinkhan.com/arena) is a three.js r128 third-person shooter: one weapon
(plasma blaster, 30 ammo), free-for-all kill count, WASD/sprint/jump, shoulder swap, auto-respawn,
plain script files, desktop-only controls. It is fun but shallow: no weapon variety, no pickups,
no modes, no bots (empty when nobody is online), no mobile play, and no server-side fairness.

Goal: an **original** game in the same genre (own name, theme, art, map, sounds — nothing copied)
that is deeper and fairer. Decisions made with you: same genre but deeper, authoritative Node
server, desktop + mobile touch, "polished core" scope.

## What makes it better

| Area | Reference | RIFTLINE |
|---|---|---|
| Weapons | 1 | 4: Pulse Rifle (hitscan auto), Scatter (shotgun), Arc Launcher (splash projectile), Rail (charged precision) |
| Movement | run, sprint, jump | + dash (cooldown), double jump, jump pads, slide |
| Map | flat arena | 1 multi-level map with cover, jump pads, high-risk centre |
| Pickups | none | health, armour, ammo, weapon spawns, timed overcharge (damage boost) |
| Modes | FFA kill count | Deathmatch (first to 20 / 8 min) and Team Control (hold the zone) |
| Empty servers | nobody to play | server-side bots fill rooms, leave as humans join |
| Fairness | client-trusting | server-authoritative sim, lag-compensated hits |
| Platforms | desktop | desktop + mobile twin-stick touch with auto-fire assist |
| Feedback | basic | hit markers, damage numbers, kill feed, killstreaks, end-of-match scoreboard + MVP |
| Rooms | one global arena | quick play + private room codes (max 8 players) |

Theme: bright low-poly orbital salvage yard (deliberately not neon-cyber), procedurally built
geometry and synthesized WebAudio sounds, so no asset licensing.

## Architecture

npm-workspaces monorepo in TypeScript, new folder `riftline/`:

```
riftline/
  shared/   # runs on BOTH client and server — the single source of truth
    constants.ts   tick rate, weapon table, pickup table, mode rules
    map.ts         static colliders (AABBs/ramps), spawn points, pickup + pad positions
    physics.ts     kinematic capsule vs static colliders, gravity, dash, jump pads
    simulate.ts    stepPlayer(state, input, dt) — deterministic, used for prediction + authority
    protocol.ts    message types + compact encode/decode
  server/   # Node + ws
    index.ts       HTTP (serves built client) + WebSocket upgrade
    rooms.ts       quick-play matchmaking, room codes, lifecycle
    game.ts        30 Hz tick loop, input queues, snapshots at 20 Hz
    combat.ts      hitscan with lag compensation (position history rewind), projectiles, splash
    modes.ts       deathmatch + team control scoring, round timer, restart
    bots.ts        waypoint navigation on map graph, target selection, aim error by difficulty
  client/   # Vite + three.js (current release)
    main.ts        boot, state machine: menu → lobby → match → results
    net.ts         socket, input sending, prediction + reconciliation, 100 ms interpolation buffer
    render/        scene, map builder, character rig + procedural animation, weapon/impact FX
    input/         keyboard+mouse (pointer lock), touch twin-stick, unified InputFrame
    ui/            HUD, kill feed, scoreboard, menus, settings (sensitivity, volume, quality)
    audio.ts       synthesized WebAudio SFX, positional panning
```

Key netcode rules:
- Client sends numbered `InputFrame`s (move axes, look yaw/pitch, buttons) each tick; never positions.
- Server applies inputs with `shared/simulate.ts`, owns health, ammo, hits, pickups, score.
- Client predicts its own player with the same function and replays unacknowledged inputs on each snapshot.
- Remote players are rendered ~100 ms in the past, interpolated between snapshots.
- Hitscan shots carry the client's render timestamp; server rewinds targets (cap 200 ms) to validate.
- Server validates fire rate, ammo and input ranges; rate-limits messages; sanitises callsigns.

## Build order (each milestone playable)

1. **Skeleton** — workspaces, Vite client, Node server, shared package, one room, cubes moving with prediction + reconciliation and interpolation.
2. **Movement + map** — capsule physics, the map, third-person camera with collision, dash/double jump/jump pads.
3. **Combat** — four weapons, lag-compensated hitscan, projectiles and splash, health/armour, death and respawn, pickups.
4. **Modes + rooms** — deathmatch and team control, round flow, quick play and room codes, scoreboard and results.
5. **Bots** — waypoint graph, combat behaviour, three difficulties, auto-fill.
6. **Mobile** — touch sticks, aim assist, responsive HUD, quality scaling.
7. **Polish** — character rig and animation, FX, audio, kill feed, hit markers, settings, tuning.

## Verification

- `npm test` — vitest unit tests for `shared/` (physics determinism, weapon damage, mode scoring) and server combat (lag-compensation rewind).
- `npm run dev` — server + client; open two browser tabs and confirm movement sync, hits, pickups, scoring and a full round to the results screen.
- Simulated bad network (server-side artificial latency/loss flag) at 150 ms / 5 % loss: own movement stays smooth, no rubber-banding on others.
- Bot soak test: 8 bots for a full match headless, no crashes, tick time under budget.
- Mobile viewport emulation in the browser: touch controls, HUD layout, frame rate.
- `npm run build` then `npm start` — single Node process serves the built client and WebSocket (deployable behind any reverse proxy).
