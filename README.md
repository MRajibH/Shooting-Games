# RIFTLINE

An original, realistic multiplayer shooter that runs entirely in the browser.
Rust authoritative server; the same Rust simulation compiled to WebAssembly for client
prediction; Vite + TypeScript + three.js for rendering. Design and roadmap: [docs/PLAN.md](docs/PLAN.md).

## Status

**Slap Arena, made real:** the slapping arm is steered with IK so the open palm sweeps across the
victim's face (with a step and lean into the swing); reach is an arm's length (1.1 m centre to
centre). The head snaps along the swing, blood sprays from the face and splats on the ground, about
every other slap knocks a tooth out and a knockout blow takes two or three; teeth bounce, click and
lie on the ground. Slaps are lag-compensated like bullets: reach and facing are checked against where
the target was on the slapper's screen (up to 250 ms back). Settings has a "Blood and gore" switch (on by default) that turns all of it, and
bullet blood, off.

**Milestone 7 done: polish.** (6: phones and tablets. 5: smarter AI players. 4: modes, match flow, first bots. 3: combat. 2: movement, map, character, Slap mode.)

- **Arrow keys** move as well as WASD; right Shift / right Ctrl sprint and crouch for arrow-key players.
- **Armour plates:** everyone spawns with a 50-point plate (Combat modes) that soaks 60 % of torso
  damage until it is used up; head and leg hits go around it. A plated torso takes 5 carbine hits
  instead of 4, and a bolt-rifle body shot no longer kills outright (a headshot still does).
  Plate hits clank instead of drawing blood; the HUD shows the plate.
- **Penetration:** rounds punch through thin steel and wood (walkway railings and deck) at reduced
  power, leaving a hole on the way.
- **Footsteps** for everyone, per surface (asphalt and concrete, steel deck and containers, wood),
  quieter crouched and louder sprinting, plus a thud on landing.
- **Shell casings** eject from the right of the receiver (brass, or red shells from the shotgun when
  it is pumped) and bounce, roll and tink on the ground.
- **Camera:** gentle head bob while moving (almost none when aiming) and a dip on landing.
- **Post-processing by quality:** High adds ground-truth ambient occlusion (contact shadows) and a
  restrained bloom on very bright things; Medium bloom only; Low none. Rendering goes through a
  multisampled HDR target so edges stay smooth. Auto quality now steps back up when there is headroom.
- **Settings:** field of view (60–90°) and invert look, alongside graphics, sensitivity and volume.


- **Touch controls** (switch on automatically when the main pointer is a finger; `?touch=1` or
  `?touch=0` to force): a floating movement stick on the left half (push past the edge to
  sprint), drag anywhere on the right to look, and buttons for fire (drag on it to keep aiming),
  aim and crouch (toggles), jump, reload, weapon swap and lean. In Slap Arena the fire button
  becomes SLAP and the gun buttons go away. A SCORE button shows the scoreboard.
- **Aim assist (touch only):** when an enemy's chest is near the crosshair and in plain sight,
  looking slows down over them and the aim drifts gently onto them. Mouse players get none.
- **Phone layout:** compact HUD, full screen and landscape lock on deploy where the browser allows,
  a "turn your device sideways" screen in portrait, safe-area aware buttons.
- **Quality scaling:** Auto (default) starts at High on desktop and Medium on phones, scales the
  render resolution every second to hold about 55 fps (down to 60 %), and drops a tier if that is
  not enough. High / Medium / Low set the resolution cap and shadow resolution (4096 / 2048 / 1024).
- **Settings** in the menu: graphics, look sensitivity, volume (remembered).


- **Difficulty** (menu, or `?ai=easy|normal|hard`): reaction time, aim error and how fast it
  settles, turn speed, burst length, hearing range and whether bots use cover. Measured against a
  person standing still 20 m away, the average time to kill is about 8.5 s (Easy), 1.7 s
  (Normal) and 1.1 s (Hard).
- **Cover:** when hurt or low on ammo (Normal and Hard), a bot picks a nearby spot the enemy cannot
  see (crouching behind low walls if that is all there is), reloads and recovers there, fights back
  if found, then peeks out again.
- **Hearing and memory:** bots no longer know where everyone is. They react to gunfire within
  earshot, go to where they last saw or heard someone, and otherwise wander the map; only after a
  long quiet spell do they get a rough steer toward the nearest enemy, so a lone player is never
  left waiting.
- **Upper level:** the navigation includes the steel walkway and both staircases, so bots climb up
  and come back down.
- **Hard bots** crouch to steady long shots.


- **Four modes:** Deathmatch (first to 25 kills), Team Deathmatch (Blue vs Red, first team to 50),
  Domination (hold capture points A, B, C; one point per second per point held, first to 200),
  Slap Arena (first to 10 knockouts). Every match is 10 minutes at most.
- **Teams:** auto-balanced on join, team colours on the soldiers, teammates' names over their heads
  (enemies get none), spawns on your own half, no friendly fire.
- **Domination:** standing on a point alone takes it in 6 s (faster with teammates, frozen while
  an enemy is on it too); flags and ground rings in the world, A / B / C markers with distance
  on screen, ownership and capture progress in the match bar.
- **Match flow:** match bar with clock and scores, results screen (victory / defeat / draw, final
  score, MVP, full table) for 12 s, then a fresh match with everyone respawned.
- **AI players:** whenever you are the only person in a room, two bots join; they leave as soon as
  a second person arrives and come back if you are alone again. Bots use the same simulation as
  players (speed, stamina, recoil, ammo, reloads), see only what is in line of sight, path around
  the map on a navigation grid, react with a human delay, aim with an error that settles while they
  track you, fire in bursts, strafe, capture points in Domination and slap in Slap Arena.


- **Four weapons** (table in `crates/sim/src/weapons.rs`, shared by server and client):

  | Weapon | Mag | Rate | Muzzle velocity | Damage (torso / head) |
  |---|---|---|---|---|
  | Carbine (5.56) | 30 + 1 | 750 rpm, auto / semi | 880 m/s | 31 / 108 |
  | SMG (9 mm) | 30 | 900 rpm, auto / semi | 380 m/s | 23 / 57 |
  | Pump shotgun (00 buck) | 7 shells | 70 rpm | 400 m/s | 9 × 15 / pellet |
  | Bolt rifle (.338, scoped) | 5 | 45 rpm | 850 m/s | 105 / 262 |

  Magazines and spare ammo, tactical reload keeps a round chambered, empty reloads are slower,
  shells load one at a time (and firing interrupts), trigger discipline (semi needs a release),
  fire-mode toggle, 0.6 s weapon swap, sprinting lowers the weapon.
- **Ballistics:** every round is a projectile with muzzle velocity, air drag and gravity drop,
  flying across ticks; damage falls off with distance. Map walls, floors and cover stop rounds.
- **Hitboxes:** head sphere, torso and legs capsules that follow crouch and lean; headshot and
  limb multipliers per weapon. 100 hp, regeneration after 5 s without damage, 4 s respawn at the
  spawn point farthest from enemies.
- **Lag compensation:** each input frame carries the server tick the player was looking at;
  hits are tested against players as they were then (capped at 250 ms). The server also drains
  a backed-up input queue so latency cannot build up.
- **Aiming:** hip spread vs aimed spread (the crosshair gap shows the real cone), movement and
  airborne penalties, bloom, deterministic spread and recoil computed in the shared sim, view
  kick that settles back about halfway, weapon sway (crouch and holding breath steady it),
  per-weapon aimed zoom and a scope for the bolt rifle.
- **Look and sound:** procedural gun models at real sizes held with two-hand arm IK (hands
  turned and fingers curled onto the grip and handguard), reload animation that carries the
  magazine to the pouch and back, pump and bolt cycling, recoil on the model. Tracers, muzzle
  flash with light, bullet holes, dust or sparks by material, hit puffs. Synthesised gunshots
  per weapon with speed-of-sound delay and distance filtering, supersonic snaps near you,
  impact sounds, reload foley, hit marker sounds.
- **HUD:** ammo / weapon / fire mode / slots, health bar, damage direction arcs, hit markers
  (head and kill variants), kill feed with weapon and headshot, death screen (killer, weapon,
  distance, countdown), K / D scoreboard.

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
- **Camera:** over-the-shoulder with aim zoom, lean offset and wall collision.

Debug handle in the browser console: `__riftline` (fps, renderer, scene, net).

## Layout

| Path | Role |
|---|---|
| `crates/sim` | Shared Rust: constants, protocol, map, movement and collision, weapons, ballistics, hitboxes. Native for the server, WASM for the browser |
| `crates/server` | axum + tokio: `/ws`, room registry, one task per room (bullets, lag-compensated hits, damage, respawn, teams, match flow, capture points), `nav.rs` walkable grid + A*, `bots.rs` AI players, serves `client/dist` |
| `client/` | Vite + three.js: menu, HUD, input (recoil, sway), netcode, world, character (weapon IK), guns, effects, audio. `src/wasm/` is generated |

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
Controls: click to capture the mouse, WASD or arrow keys, Shift sprint, Space jump / vault, C / Ctrl crouch,
Q / E lean, right mouse aim, left mouse fire (slap in Slap mode), 1–4 or wheel weapons, R reload,
B fire mode, Shift while aimed holds breath. `?mode=combat|tdm|domination|slap` preselects the mode.
