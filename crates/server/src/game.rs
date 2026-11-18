//! One room: an authoritative fixed-tick simulation running in its own task.

use std::collections::{HashMap, VecDeque};

use riftline_sim::ballistics::{penetrate, point, ray_player, raycast_map, Bullet, Zone};
use riftline_sim::constants::{
    combat, rules, slap, MAX_REWIND_MS, SNAPSHOT_RATE, TICK_DT, TICK_RATE,
};
use riftline_sim::map;
use riftline_sim::protocol::{
    Difficulty, FlagState, GameEvent, InputFrame, MatchState, Mode, Phase, PlayerState,
    RosterEntry, ServerMessage,
};

use crate::bots::{self, Brain, Noise, View};
use riftline_sim::sim::{spawn_state, step_player};

/// Input frames buffered beyond this are run two per tick until the backlog clears.
const MAX_QUEUED: usize = 3;

/// Ticks of player history kept for lag compensation.
const MAX_REWIND: u32 = (MAX_REWIND_MS * TICK_RATE as f64 / 1000.0) as u32;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub type Outbox = UnboundedSender<String>;

pub enum RoomEvent {
    Join { id: u32, name: String, out: Outbox },
    Input { id: u32, frames: Vec<InputFrame> },
    Leave { id: u32 },
}

struct Slot {
    name: String,
    out: Outbox,
    state: PlayerState,
    queue: Vec<InputFrame>,
    last: Option<InputFrame>,
    ack: u32,
    slaps: u32,
    kos: u32,
    kills: u32,
    deaths: u32,
    caps: u32,
    /// Some for an AI player.
    brain: Option<Box<Brain>>,
}

impl Slot {
    fn score(&self, mode: Mode) -> u32 {
        if mode == Mode::Slap {
            self.kos * 100 + self.slaps * 10
        } else {
            self.kills * 100 + self.caps * 150
        }
    }
}

/// Bot ids live far above the ids handed to connections.
const FIRST_BOT_ID: u32 = 1_000_000;

fn fresh_match(mode: Mode) -> MatchState {
    MatchState {
        phase: Phase::Playing,
        time_left: rules::MATCH_TIME,
        limit: match mode {
            Mode::Combat => rules::FFA_KILLS,
            Mode::Tdm => rules::TDM_KILLS,
            Mode::Domination => rules::DOM_POINTS,
            Mode::Slap => rules::SLAP_KOS,
        },
        flags: if mode == Mode::Domination {
            map::FLAGS
                .iter()
                .map(|&(x, z)| FlagState {
                    x,
                    z,
                    ..Default::default()
                })
                .collect()
        } else {
            Vec::new()
        },
        ..Default::default()
    }
}

pub struct Game {
    room: String,
    mode: Mode,
    slots: HashMap<u32, Slot>,
    tick: u64,
    snap_acc: u32,
    events: Vec<GameEvent>,
    next_spawn: usize,
    roster_dirty: bool,
    bullets: Vec<Bullet>,
    /// Player states after each recent tick, newest last, for rewinding hitboxes.
    history: VecDeque<(u64, Vec<PlayerState>)>,
    game: MatchState,
    point_acc: f32,
    next_bot: u32,
    difficulty: Difficulty,
    /// Recent gunshots (with the tick they happened), for bots to hear.
    noises: Vec<(u64, Noise)>,
}

impl Game {
    pub fn new(room: String, mode: Mode, difficulty: Difficulty) -> Self {
        Self {
            room,
            mode,
            slots: HashMap::new(),
            tick: 0,
            snap_acc: 0,
            events: Vec::new(),
            next_spawn: 0,
            roster_dirty: false,
            bullets: Vec::new(),
            history: VecDeque::new(),
            game: fresh_match(mode),
            point_acc: 0.0,
            next_bot: FIRST_BOT_ID,
            difficulty,
            noises: Vec::new(),
        }
    }

    /// No people left (bots alone do not keep a room open).
    pub fn is_empty(&self) -> bool {
        self.humans() == 0
    }

    fn humans(&self) -> usize {
        self.slots.values().filter(|s| s.brain.is_none()).count()
    }

    /// The smaller team (team 1 on a tie), or 0 in free-for-all modes.
    fn pick_team(&self) -> u8 {
        if !self.mode.teams() {
            return 0;
        }
        let n = |t: u8| self.slots.values().filter(|s| s.state.team == t).count();
        if n(2) < n(1) {
            2
        } else {
            1
        }
    }

    fn add_player(&mut self, id: u32, name: String, out: Outbox, brain: Option<Box<Brain>>) {
        let team = self.pick_team();
        let state = self.spawn(id, team);
        self.slots.insert(
            id,
            Slot {
                name,
                out,
                state,
                queue: Vec::new(),
                last: None,
                ack: 0,
                slaps: 0,
                kos: 0,
                kills: 0,
                deaths: 0,
                caps: 0,
                brain,
            },
        );
        self.roster_dirty = true;
    }

    /// A lone person gets AI company; bots leave as soon as a second person is here.
    fn fill_bots(&mut self) {
        let humans = self.humans();
        let want = if humans == 1 {
            rules::BOTS_FOR_LONE_PLAYER
        } else {
            0
        };
        let mut bots: Vec<u32> = self
            .slots
            .iter()
            .filter(|(_, s)| s.brain.is_some())
            .map(|(&k, _)| k)
            .collect();
        bots.sort_unstable();
        while bots.len() > want {
            let id = bots.pop().unwrap();
            self.slots.remove(&id);
            self.broadcast(&ServerMessage::Left { id });
            self.roster_dirty = true;
        }
        while bots.len() < want {
            let id = self.next_bot;
            self.next_bot += 1;
            let used: Vec<&str> = self.slots.values().map(|s| s.name.as_str()).collect();
            let name = bots::NAMES
                .iter()
                .find(|n| !used.contains(n))
                .map_or_else(|| format!("Bot-{id}"), |n| n.to_string());
            // nobody listens to a bot's outbox
            let (out, _) = tokio::sync::mpsc::unbounded_channel();
            self.add_player(
                id,
                name,
                out,
                Some(Box::new(Brain::new(id, self.difficulty))),
            );
            bots.push(id);
        }
    }

    /// Spawn at the point farthest from living enemies (round-robin when alone);
    /// in team modes only on the team's own half.
    fn spawn(&mut self, id: u32, team: u8) -> PlayerState {
        let living: Vec<(f32, f32)> = self
            .slots
            .iter()
            .filter(|(&k, s)| k != id && s.state.dead <= 0.0 && (team == 0 || s.state.team != team))
            .map(|(_, s)| (s.state.x, s.state.z))
            .collect();
        let n = map::spawn_count();
        let side = |i: &usize| team == 0 || map::spawn_side(*i) == team;
        let pick = if living.is_empty() {
            (0..n)
                .map(|k| (k + self.next_spawn) % n)
                .find(side)
                .unwrap_or(0)
        } else {
            (0..n)
                .map(|k| (k + self.next_spawn) % n)
                .filter(side)
                .max_by(|&a, &b| {
                    let near = |i: usize| {
                        let (x, z, _) = map::spawn(i);
                        living
                            .iter()
                            .map(|&(px, pz)| libm::hypotf(px - x, pz - z))
                            .fold(f32::MAX, f32::min)
                    };
                    near(a).total_cmp(&near(b))
                })
                .unwrap_or(0)
        };
        self.next_spawn += 1;
        let mut s = spawn_state(id, pick, self.mode.armed());
        s.team = team;
        s
    }

    pub fn handle(&mut self, ev: RoomEvent) {
        match ev {
            RoomEvent::Join { id, name, out } => {
                send(
                    &out,
                    &ServerMessage::Welcome {
                        id,
                        room: self.room.clone(),
                        tick_rate: TICK_RATE,
                        mode: self.mode,
                        difficulty: self.difficulty,
                    },
                );
                self.add_player(id, name, out, None);
                self.fill_bots();
            }
            RoomEvent::Input { id, frames } => {
                let Some(s) = self.slots.get_mut(&id) else {
                    return;
                };
                for f in frames {
                    let dup = s.queue.iter().any(|q| q.seq == f.seq);
                    if f.seq > s.ack && !dup && s.queue.len() < TICK_RATE as usize {
                        s.queue.push(f);
                    }
                }
                s.queue.sort_by_key(|f| f.seq);
            }
            RoomEvent::Leave { id } => {
                if self.slots.remove(&id).is_some() {
                    self.broadcast(&ServerMessage::Left { id });
                    self.roster_dirty = true;
                    self.fill_bots();
                }
            }
        }
    }

    pub fn step(&mut self) {
        self.tick += 1;
        let mut slappers = Vec::new();
        let mut respawns = Vec::new();
        let mut volleys = Vec::new();

        // bots decide this tick's input from what they can see
        let states: Vec<PlayerState> = self.slots.values().map(|s| s.state.clone()).collect();
        let tick = self.tick;
        self.noises.retain(|(t, _)| tick - *t < 20);
        let noises: Vec<Noise> = self.noises.iter().map(|(_, n)| *n).collect();
        let view = View {
            players: &states,
            mode: self.mode,
            flags: &self.game.flags,
            noises: &noises,
            dt: TICK_DT,
        };
        for s in self.slots.values_mut() {
            if let Some(b) = s.brain.as_mut() {
                let f = b.think(&s.state, &view);
                s.queue.clear();
                s.queue.push(f);
            }
        }
        for (&id, s) in self.slots.iter_mut() {
            // One input per tick; repeat the last one if the client's packet is late,
            // and run an extra one when frames pile up so latency cannot build up.
            let runs = if s.queue.len() > MAX_QUEUED { 2 } else { 1 };
            for _ in 0..runs {
                let f = if s.queue.is_empty() {
                    s.last
                } else {
                    Some(s.queue.remove(0))
                };
                let Some(f) = f else { continue };
                s.ack = s.ack.max(f.seq);
                s.last = Some(f);
                let was_down = s.state.ko > 0.0 || s.state.dead > 0.0;
                let r = step_player(&mut s.state, &f, TICK_DT);
                if r.slap {
                    slappers.push(id);
                }
                if !r.shots.is_empty() {
                    // trust the client's view time only within the rewind cap
                    let behind = self.tick as f64 - f.view.round();
                    let rewind = if f.view > 0.0 && behind > 0.0 {
                        (behind as u32).min(MAX_REWIND)
                    } else {
                        0
                    };
                    volleys.push((id, s.state.weapon, r.shots, rewind));
                }
                if was_down && s.state.ko <= 0.0 && s.state.dead <= 0.0 {
                    respawns.push(id);
                }
            }
        }
        for id in respawns {
            let team = self.slots.get(&id).map_or(0, |s| s.state.team);
            let fresh = self.spawn(id, team);
            if let Some(s) = self.slots.get_mut(&id) {
                s.state = fresh;
            }
            self.events.push(GameEvent::Respawn { id });
        }
        if self.mode == Mode::Slap {
            for id in slappers {
                self.resolve_slap(id);
            }
        }

        self.history.push_back((
            self.tick,
            self.slots.values().map(|s| s.state.clone()).collect(),
        ));
        while self.history.len() > MAX_REWIND as usize + 2 {
            self.history.pop_front();
        }
        for (id, w, shots, rewind) in volleys {
            let first = shots[0];
            self.noises.push((
                self.tick,
                Noise {
                    at: first.origin,
                    by: id,
                },
            ));
            self.events.push(GameEvent::Fire {
                id,
                w,
                o: first.origin,
                d: first.dir,
            });
            for sh in shots {
                self.bullets
                    .push(Bullet::new(id, w, sh.origin, sh.dir, rewind));
            }
        }
        self.fly_bullets();
        self.update_match(TICK_DT);

        if self.roster_dirty {
            self.roster_dirty = false;
            let mut players: Vec<RosterEntry> = self
                .slots
                .iter()
                .map(|(&id, s)| RosterEntry {
                    id,
                    name: s.name.clone(),
                    slaps: s.slaps,
                    kos: s.kos,
                    kills: s.kills,
                    deaths: s.deaths,
                    caps: s.caps,
                    score: s.score(self.mode),
                    team: s.state.team,
                    bot: s.brain.is_some(),
                })
                .collect();
            players.sort_by(|a, b| {
                a.team
                    .cmp(&b.team)
                    .then(b.score.cmp(&a.score))
                    .then(a.deaths.cmp(&b.deaths))
                    .then(a.id.cmp(&b.id))
            });
            self.broadcast(&ServerMessage::Roster { players });
        }

        self.snap_acc += SNAPSHOT_RATE;
        if self.snap_acc < TICK_RATE {
            return;
        }
        self.snap_acc -= TICK_RATE;
        let players: Vec<PlayerState> = self.slots.values().map(|s| s.state.clone()).collect();
        let events = std::mem::take(&mut self.events);
        for s in self.slots.values() {
            send(
                &s.out,
                &ServerMessage::Snapshot {
                    tick: self.tick,
                    ack: s.ack,
                    players: players.clone(),
                    events: events.clone(),
                    game: self.game.clone(),
                },
            );
        }
    }

    /// Player states as they were `rewind` ticks ago (the shooter's view of the world).
    fn past(&self, rewind: u32) -> Option<&Vec<PlayerState>> {
        let want = self.tick.saturating_sub(rewind as u64);
        self.history
            .iter()
            .rev()
            .find(|(t, _)| *t <= want)
            .or(self.history.front())
            .map(|(_, v)| v)
    }

    /// Move every bullet one tick; the first thing it crosses (wall, floor or a
    /// player's hitbox as the shooter saw them) stops it.
    fn fly_bullets(&mut self) {
        let mut bullets = std::mem::take(&mut self.bullets);
        bullets.retain_mut(|b| {
            let (mut o, d, mut l) = b.advance(TICK_DT);
            let team = self.slots.get(&b.owner).map_or(0, |s| s.state.team);
            // a round can punch through thin steel or wood several times in one tick
            loop {
                let wall = raycast_map(o, d, l);
                let max = wall.map_or(l, |h| h.t);
                let mut hit: Option<(u32, f32, Zone)> = None;
                if let Some(states) = self.past(b.rewind) {
                    for t in states {
                        // no friendly fire: rounds pass teammates
                        if t.id == b.owner || (team != 0 && t.team == team) {
                            continue;
                        }
                        if let Some((at, zone)) = ray_player(o, d, max, t) {
                            if hit.is_none_or(|h| at < h.1) {
                                hit = Some((t.id, at, zone));
                            }
                        }
                    }
                }
                if let Some((to, at, zone)) = hit {
                    b.travelled += at;
                    let alive = self.slots.get(&to).is_some_and(|s| s.state.dead <= 0.0);
                    if alive {
                        self.damage(b.owner, to, b.weapon, b.damage(zone), zone, point(o, d, at));
                    }
                    return false;
                }
                let Some(h) = wall else { break };
                self.events.push(GameEvent::Impact {
                    p: point(o, d, h.t),
                    n: h.normal,
                    mat: h.mat,
                });
                match penetrate(&h) {
                    Some(keep) if b.power * keep > 0.2 => {
                        let through = h.t + h.thickness + 0.01;
                        b.power *= keep;
                        b.travelled += through;
                        o = point(o, d, through);
                        l -= through;
                        if l <= 0.0 {
                            b.pos = o;
                            return !b.expired();
                        }
                    }
                    _ => return false,
                }
            }
            b.travelled += l;
            !b.expired()
        });
        self.bullets = bullets;
    }

    fn damage(&mut self, from: u32, to: u32, w: u8, dmg: f32, zone: Zone, p: [f32; 3]) {
        if self.game.phase != Phase::Playing {
            return;
        }
        let shooter = self.slots.get(&from).map(|s| (s.state.x, s.state.z));
        let Some(t) = self.slots.get_mut(&to) else {
            return;
        };
        let st = &mut t.state;
        // a plate soaks part of a torso hit until its points are gone
        let mut dmg = dmg;
        let plate = zone == Zone::Torso && st.armor > 0.0;
        if plate {
            let soak = (dmg * combat::PLATE_ABSORB).min(st.armor);
            st.armor -= soak;
            dmg -= soak;
        }
        st.hp -= dmg;
        st.regen_cd = combat::REGEN_DELAY;
        self.events.push(GameEvent::Hit {
            from,
            to,
            dmg,
            zone,
            p,
            plate,
        });
        if st.hp > 0.0 {
            return;
        }
        st.hp = 0.0;
        st.dead = combat::RESPAWN_TIME;
        st.vx = 0.0;
        st.vz = 0.0;
        st.reload = 0.0;
        st.ads = 0.0;
        let dist = shooter.map_or(0.0, |(x, z)| libm::hypotf(x - st.x, z - st.z));
        t.deaths += 1;
        if let Some(s) = self.slots.get_mut(&from) {
            s.kills += 1;
            if self.mode == Mode::Tdm && s.state.team > 0 {
                self.game.scores[s.state.team as usize - 1] += 1;
            }
        }
        self.events.push(GameEvent::Kill {
            from,
            to,
            w,
            head: zone == Zone::Head,
            dist,
        });
        self.roster_dirty = true;
    }

    /// Nearest standing player inside the slapper's reach and frontal cone gets
    /// knocked back, staggered and dazed; full daze is a knockout.
    fn resolve_slap(&mut self, from: u32) {
        if self.game.phase != Phase::Playing {
            return;
        }
        let Some(a) = self.slots.get(&from).map(|s| s.state.clone()) else {
            return;
        };
        let (fx, fz) = (-libm::sinf(a.yaw), -libm::cosf(a.yaw));
        let mut best: Option<(u32, f32, f32, f32)> = None;
        for (&id, s) in &self.slots {
            let t = &s.state;
            if id == from || t.ko > 0.0 {
                continue;
            }
            let (dx, dz) = (t.x - a.x, t.z - a.z);
            let d = libm::hypotf(dx, dz);
            if !(1e-3..=slap::RANGE).contains(&d) || (t.y - a.y).abs() > 1.2 {
                continue;
            }
            if (dx * fx + dz * fz) / d < slap::CONE_COS {
                continue;
            }
            if best.is_none_or(|b| d < b.1) {
                best = Some((id, d, dx / d, dz / d));
            }
        }
        let Some((to, _, nx, nz)) = best else {
            self.events.push(GameEvent::Slap { from, to: None });
            return;
        };
        let knocked_out = {
            let t = &mut self.slots.get_mut(&to).unwrap().state;
            t.vx = nx * slap::KNOCKBACK;
            t.vz = nz * slap::KNOCKBACK;
            t.vy = slap::LIFT;
            t.grounded = false;
            t.stun = slap::STUN;
            t.daze += slap::DAZE_PER_HIT;
            if t.daze >= 100.0 {
                t.daze = 100.0;
                t.ko = slap::KO_TIME;
                true
            } else {
                false
            }
        };
        let s = self.slots.get_mut(&from).unwrap();
        s.slaps += 1;
        self.events.push(GameEvent::Slap { from, to: Some(to) });
        if knocked_out {
            s.kos += 1;
            self.events.push(GameEvent::Ko { from, to });
        }
        self.roster_dirty = true;
    }

    /// Clock, capture points, win check, results screen and the next match.
    fn update_match(&mut self, dt: f32) {
        if self.game.phase == Phase::Ended {
            self.game.next_in -= dt;
            if self.game.next_in <= 0.0 {
                self.new_match();
            }
            return;
        }
        self.game.time_left = (self.game.time_left - dt).max(0.0);
        if self.mode == Mode::Domination {
            self.update_flags(dt);
        }
        let limit = self.game.limit;
        let best = self
            .slots
            .iter()
            .map(|(&id, s)| {
                (
                    id,
                    if self.mode == Mode::Slap {
                        s.kos
                    } else {
                        s.kills
                    },
                )
            })
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)));
        let [t1, t2] = self.game.scores;
        let over = self.game.time_left <= 0.0
            || if self.mode.teams() {
                t1 >= limit || t2 >= limit
            } else {
                best.is_some_and(|b| b.1 >= limit)
            };
        if !over {
            return;
        }
        if self.mode.teams() {
            self.game.winner_team = match t1.cmp(&t2) {
                std::cmp::Ordering::Greater => 1,
                std::cmp::Ordering::Less => 2,
                std::cmp::Ordering::Equal => 0,
            };
        } else {
            self.game.winner_id = best.filter(|b| b.1 > 0).map_or(0, |b| b.0);
        }
        let mode = self.mode;
        let wt = self.game.winner_team;
        self.game.mvp = self
            .slots
            .iter()
            .filter(|(_, s)| wt == 0 || s.state.team == wt)
            .max_by(|a, b| a.1.score(mode).cmp(&b.1.score(mode)).then(b.0.cmp(a.0)))
            .map_or(0, |(&id, _)| id);
        self.game.phase = Phase::Ended;
        self.game.next_in = rules::RESULTS_TIME;
        self.roster_dirty = true;
    }

    fn new_match(&mut self) {
        self.game = fresh_match(self.mode);
        self.point_acc = 0.0;
        self.bullets.clear();
        let ids: Vec<u32> = self.slots.keys().copied().collect();
        for id in ids {
            let team = self.slots[&id].state.team;
            let fresh = self.spawn(id, team);
            let s = self.slots.get_mut(&id).unwrap();
            s.state = fresh;
            s.queue.clear();
            (s.kills, s.deaths, s.slaps, s.kos, s.caps) = (0, 0, 0, 0, 0);
            self.events.push(GameEvent::Respawn { id });
        }
        self.roster_dirty = true;
    }

    /// Standing on a point alone moves it toward your team; enemies there too stop it.
    fn update_flags(&mut self, dt: f32) {
        for (k, flag) in self.game.flags.iter_mut().enumerate() {
            let mut on = [Vec::new(), Vec::new()];
            for (&id, s) in &self.slots {
                let p = &s.state;
                if p.dead <= 0.0
                    && p.team > 0
                    && libm::hypotf(p.x - flag.x, p.z - flag.z) <= rules::FLAG_RADIUS
                    && p.y < 2.5
                {
                    on[p.team as usize - 1].push(id);
                }
            }
            let rate = |n: usize| dt / rules::CAPTURE_TIME * (1.0 + 0.5 * (n.min(3) as f32 - 1.0));
            let before = flag.owner;
            match (on[0].len(), on[1].len()) {
                (n, 0) if n > 0 => flag.progress = (flag.progress - rate(n)).max(-1.0),
                (0, n) if n > 0 => flag.progress = (flag.progress + rate(n)).min(1.0),
                (0, 0) => {
                    // drifts back to whoever holds it
                    let home = match flag.owner {
                        1 => -1.0,
                        2 => 1.0,
                        _ => 0.0,
                    };
                    let d = home - flag.progress;
                    flag.progress += d.clamp(-dt / rules::CAPTURE_TIME, dt / rules::CAPTURE_TIME);
                }
                _ => {} // contested
            }
            if flag.progress <= -1.0 {
                flag.owner = 1;
            } else if flag.progress >= 1.0 {
                flag.owner = 2;
            } else if (flag.owner == 1 && flag.progress > 0.0)
                || (flag.owner == 2 && flag.progress < 0.0)
            {
                flag.owner = 0;
            }
            if flag.owner != before && flag.owner != 0 {
                self.events.push(GameEvent::Capture {
                    flag: k as u8,
                    team: flag.owner,
                });
                for id in &on[flag.owner as usize - 1] {
                    if let Some(s) = self.slots.get_mut(id) {
                        s.caps += 1;
                    }
                }
                self.roster_dirty = true;
            }
        }
        self.point_acc += dt;
        while self.point_acc >= rules::POINTS_EVERY {
            self.point_acc -= rules::POINTS_EVERY;
            for f in &self.game.flags {
                if f.owner > 0 {
                    self.game.scores[f.owner as usize - 1] += 1;
                }
            }
        }
    }

    fn broadcast(&self, m: &ServerMessage) {
        for s in self.slots.values() {
            send(&s.out, m);
        }
    }
}

fn send(out: &Outbox, m: &ServerMessage) {
    if let Ok(raw) = serde_json::to_string(m) {
        let _ = out.send(raw);
    }
}

/// Room task: tick on a fixed interval, apply events between ticks.
pub async fn run(
    mut game: Game,
    mut rx: UnboundedReceiver<RoomEvent>,
    registry: crate::rooms::Registry,
) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs_f32(TICK_DT));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Burst);
    loop {
        tokio::select! {
            _ = interval.tick() => game.step(),
            ev = rx.recv() => {
                let Some(ev) = ev else { return };
                let leaving = matches!(ev, RoomEvent::Leave { .. });
                game.handle(ev);
                if leaving && game.is_empty() && registry.remove_if_empty(&game.room) { return; }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use riftline_sim::protocol::button;
    use tokio::sync::mpsc::unbounded_channel;

    fn two_players(mode: Mode) -> Game {
        let mut g = Game::new("T".into(), mode, Difficulty::Normal);
        for id in [1, 2] {
            let (out, _rx) = unbounded_channel();
            std::mem::forget(_rx);
            g.handle(RoomEvent::Join {
                id,
                name: format!("p{id}"),
                out,
            });
        }
        // 1 at the origin facing -z, 2 standing 1.2 m in front of it
        g.slots.get_mut(&1).unwrap().state = PlayerState {
            id: 1,
            x: 0.0,
            z: 10.0,
            grounded: true,
            stamina: 1.0,
            ..Default::default()
        };
        g.slots.get_mut(&2).unwrap().state = PlayerState {
            id: 2,
            x: 0.0,
            z: 8.8,
            grounded: true,
            stamina: 1.0,
            ..Default::default()
        };
        g
    }

    fn slap_once(g: &mut Game, seq: u32) {
        g.handle(RoomEvent::Input {
            id: 1,
            frames: vec![InputFrame {
                seq,
                buttons: button::FIRE,
                ..Default::default()
            }],
        });
        g.handle(RoomEvent::Input {
            id: 2,
            frames: vec![InputFrame {
                seq,
                ..Default::default()
            }],
        });
        g.step();
    }

    #[test]
    fn slap_knocks_back_and_scores() {
        let mut g = two_players(Mode::Slap);
        slap_once(&mut g, 1);
        let t = &g.slots[&2].state;
        assert!(t.vz < -5.0 && t.stun > 0.0 && t.daze > 30.0, "{t:?}");
        assert_eq!(g.slots[&1].slaps, 1);
    }

    #[test]
    fn three_slaps_knock_out_then_respawn() {
        let mut g = two_players(Mode::Slap);
        for k in 0..3u32 {
            // put the target back in reach and let the cooldown expire
            let st = &mut g.slots.get_mut(&2).unwrap().state;
            st.x = 0.0;
            st.z = 8.8;
            st.y = 0.0;
            st.vx = 0.0;
            st.vz = 0.0;
            st.vy = 0.0;
            st.grounded = true;
            g.slots.get_mut(&1).unwrap().state.slap_cd = 0.0;
            slap_once(&mut g, k * 100 + 1);
        }
        assert!(g.slots[&2].state.ko > 0.0);
        assert_eq!(g.slots[&1].kos, 1);
        for i in 0..(slap::KO_TIME / TICK_DT) as u32 + 5 {
            g.handle(RoomEvent::Input {
                id: 2,
                frames: vec![InputFrame {
                    seq: 1000 + i,
                    ..Default::default()
                }],
            });
            g.step();
        }
        let t = &g.slots[&2].state;
        assert!(t.ko == 0.0 && t.daze == 0.0, "{t:?}");
    }

    #[test]
    fn no_slap_damage_in_combat_mode_or_from_behind() {
        let mut g = two_players(Mode::Combat);
        slap_once(&mut g, 1);
        assert_eq!(g.slots[&1].slaps, 0);

        let mut g = two_players(Mode::Slap);
        g.slots.get_mut(&2).unwrap().state.z = 11.2; // behind the slapper
        slap_once(&mut g, 1);
        assert_eq!(g.slots[&1].slaps, 0);
    }

    // ---- combat ----

    use riftline_sim::weapons;

    /// A clear firing lane on the map.
    const LANE: f32 = -3.0;

    /// Shooter 1 at z = 10 facing -z; target 2 at z = 0 in line with the shooter's
    /// right shoulder (where shots leave from).
    fn duel() -> Game {
        let mut g = Game::new("C".into(), Mode::Combat, Difficulty::Normal);
        for id in [1, 2] {
            let (out, rx) = unbounded_channel();
            std::mem::forget(rx);
            g.handle(RoomEvent::Join {
                id,
                name: format!("p{id}"),
                out,
            });
        }
        for (id, x, z) in [(1, LANE, 10.0), (2, LANE + 0.5, 0.0)] {
            let mut st = spawn_state(id, 0, true);
            st.x = x;
            st.z = z;
            st.yaw = 0.0;
            st.trigger_spent = false;
            st.ads = 1.0;
            g.slots.get_mut(&id).unwrap().state = st;
        }
        g
    }

    /// One tick: player 1 aims at height `h` on the target and maybe fires.
    fn tick(g: &mut Game, seq: u32, fire: bool, h: f32, weapon: u8, view: f64) {
        let tz = g.slots[&2].state.z;
        let pitch = libm::atanf((h - 1.65) / (10.0 - tz));
        let b = button::AIM | if fire { button::FIRE } else { 0 };
        g.handle(RoomEvent::Input {
            id: 1,
            frames: vec![InputFrame {
                seq,
                pitch,
                buttons: b,
                weapon,
                view,
                ..Default::default()
            }],
        });
        g.handle(RoomEvent::Input {
            id: 2,
            frames: vec![InputFrame {
                seq,
                ..Default::default()
            }],
        });
        g.step();
    }

    #[test]
    fn carbine_kills_in_five_plated_torso_hits_then_respawns() {
        let mut g = duel();
        let mut seq = 0;
        while g.slots[&2].state.dead <= 0.0 && seq < 60 {
            seq += 1;
            tick(&mut g, seq, true, 1.15, weapons::RIFLE, 0.0);
        }
        let fired = 30 - g.slots[&1].state.mags[0];
        // the 50-point plate soaks 60 % of the first hits: 12.4 + 12.4 + 18.2 + 31 + 31
        assert_eq!(fired, 5, "31 damage per torso hit, 100 hp behind a plate");
        assert_eq!((g.slots[&1].kills, g.slots[&2].deaths), (1, 1));
        for _ in 0..(combat::RESPAWN_TIME / TICK_DT) as u32 + 3 {
            seq += 1;
            tick(&mut g, seq, false, 1.15, weapons::RIFLE, 0.0);
        }
        let t = &g.slots[&2].state;
        assert!(t.dead == 0.0 && t.hp == combat::MAX_HP, "{t:?}");
        assert!(
            libm::hypotf(t.x - LANE - 0.5, t.z) > 5.0,
            "respawned somewhere else"
        );
    }

    #[test]
    fn bolt_rifle_headshot_and_leg_shot() {
        let mut g = duel();
        for seq in 1..=60 {
            tick(&mut g, seq, false, 1.62, weapons::SNIPER, 0.0); // swap to the rifle
        }
        g.slots.get_mut(&1).unwrap().state.ads = 1.0;
        tick(&mut g, 61, true, 1.62, weapons::SNIPER, 0.0);
        assert!(g.slots[&2].state.dead > 0.0);

        let mut g = duel();
        for seq in 1..=60 {
            tick(&mut g, seq, false, 0.5, weapons::SNIPER, 0.0);
        }
        g.slots.get_mut(&1).unwrap().state.ads = 1.0;
        tick(&mut g, 61, true, 0.5, weapons::SNIPER, 0.0);
        let t = &g.slots[&2].state;
        assert!(
            t.dead == 0.0 && (t.hp - (100.0 - 105.0 * 0.7)).abs() < 0.5,
            "{t:?}"
        );
    }

    #[test]
    fn walls_stop_bullets() {
        let mut g = duel();
        // a 0.9 m wall between them hides the legs but not the chest
        for (id, z) in [(1, 1.0), (2, -9.0)] {
            g.slots.get_mut(&id).unwrap().state.z = z;
            g.slots.get_mut(&id).unwrap().state.x = if id == 1 { -14.5 } else { -14.0 };
        }
        let aim = |g: &mut Game, seq: u32, h: f32| {
            let pitch = libm::atanf((h - 1.65) / 10.0);
            g.handle(RoomEvent::Input {
                id: 1,
                frames: vec![InputFrame {
                    seq,
                    pitch,
                    buttons: button::AIM | button::FIRE,
                    ..Default::default()
                }],
            });
            g.step();
        };
        aim(&mut g, 1, 0.4);
        for k in 2..6 {
            aim(&mut g, k, 0.4);
        }
        assert_eq!(
            g.slots[&2].state.hp,
            combat::MAX_HP,
            "legs are behind the wall"
        );
        for k in 7..17 {
            aim(&mut g, k, 1.2);
        }
        assert!(g.slots[&2].state.hp < combat::MAX_HP, "chest is above it");
    }

    #[test]
    fn lag_compensation_hits_where_the_shooter_saw_the_target() {
        let run = |view_behind: u64, after_move: u32| {
            let mut g = duel();
            for seq in 1..=20 {
                tick(&mut g, seq, false, 1.15, weapons::RIFLE, 0.0);
            }
            // the target steps 2 m sideways; the shooter's screen still shows the old spot
            g.slots.get_mut(&2).unwrap().state.x = LANE + 2.5;
            for seq in 21..21 + after_move {
                tick(&mut g, seq, false, 1.15, weapons::RIFLE, 0.0);
            }
            let view = (g.tick as f64 + 1.0 - view_behind as f64).max(1.0);
            g.slots.get_mut(&1).unwrap().state.ads = 1.0;
            tick(&mut g, 100, true, 1.15, weapons::RIFLE, view);
            g.slots[&2].state.hp < combat::MAX_HP
        };
        assert!(run(9, 6), "seen 9 ticks ago, before the step: hit");
        assert!(!run(1, 6), "seen now: miss");
        // the step happened 20 ticks (333 ms) ago: older than the 250 ms cap allows
        assert!(!run(25, 20), "rewind is capped at 250 ms");
    }

    #[test]
    fn slap_mode_has_no_guns() {
        let mut g = two_players(Mode::Slap);
        slap_once(&mut g, 1);
        assert!(g.bullets.is_empty() && !g.slots[&1].state.armed);
    }

    // ---- modes, match flow, bots ----

    fn join(g: &mut Game, id: u32) {
        let (out, rx) = unbounded_channel();
        std::mem::forget(rx);
        g.handle(RoomEvent::Join {
            id,
            name: format!("p{id}"),
            out,
        });
    }

    fn bots(g: &Game) -> usize {
        g.slots.values().filter(|s| s.brain.is_some()).count()
    }

    #[test]
    fn bots_keep_a_lone_player_company_and_leave_for_people() {
        let mut g = Game::new("B".into(), Mode::Combat, Difficulty::Normal);
        join(&mut g, 1);
        assert_eq!(bots(&g), 2);
        join(&mut g, 2);
        assert_eq!(bots(&g), 0);
        g.handle(RoomEvent::Leave { id: 2 });
        assert_eq!(bots(&g), 2);
        g.handle(RoomEvent::Leave { id: 1 });
        assert!(g.is_empty() && g.slots.is_empty());
    }

    #[test]
    fn teams_are_balanced_and_spawn_on_their_own_half() {
        let mut g = Game::new("T".into(), Mode::Tdm, Difficulty::Normal);
        join(&mut g, 1);
        let teams: Vec<u8> = g.slots.values().map(|s| s.state.team).collect();
        assert_eq!(teams.iter().filter(|&&t| t == 1).count(), 2);
        assert_eq!(teams.iter().filter(|&&t| t == 2).count(), 1);
        for s in g.slots.values() {
            let side = if s.state.x - s.state.z < 0.0 { 1 } else { 2 };
            assert_eq!(side, s.state.team, "{} spawned on the wrong half", s.name);
        }
    }

    #[test]
    fn no_friendly_fire_in_team_modes() {
        let mut g = duel();
        g.mode = Mode::Tdm;
        for id in [1, 2] {
            g.slots.get_mut(&id).unwrap().state.team = 1;
        }
        for seq in 1..30 {
            tick(&mut g, seq, true, 1.15, weapons::RIFLE, 0.0);
        }
        assert_eq!(g.slots[&2].state.hp, combat::MAX_HP);
        g.slots.get_mut(&2).unwrap().state.team = 2;
        for seq in 30..60 {
            tick(&mut g, seq, true, 1.15, weapons::RIFLE, 0.0);
        }
        assert!(g.slots[&2].state.hp < combat::MAX_HP || g.slots[&2].state.dead > 0.0);
        assert!(g.game.scores[0] <= 1);
    }

    #[test]
    fn domination_capture_and_points() {
        let mut g = duel();
        g.mode = Mode::Domination;
        g.game = fresh_match(Mode::Domination);
        let (fx, fz) = map::FLAGS[0];
        {
            let st = &mut g.slots.get_mut(&1).unwrap().state;
            (st.team, st.x, st.z) = (1, fx, fz);
            let st = &mut g.slots.get_mut(&2).unwrap().state;
            (st.team, st.x, st.z) = (2, map::FLAGS[2].0, map::FLAGS[2].1 + 6.0);
        }
        let idle = |g: &mut Game, seq: u32| {
            for id in [1, 2] {
                g.handle(RoomEvent::Input {
                    id,
                    frames: vec![InputFrame {
                        seq,
                        ..Default::default()
                    }],
                });
            }
            g.step();
        };
        let secs = |t: f32| (t / TICK_DT) as u32;
        for seq in 1..secs(rules::CAPTURE_TIME - 0.5) {
            idle(&mut g, seq);
        }
        assert_eq!(g.game.flags[0].owner, 0, "not yet");
        for seq in 1000..1000 + secs(1.0) {
            idle(&mut g, seq);
        }
        assert_eq!(g.game.flags[0].owner, 1);
        assert_eq!(g.slots[&1].caps, 1);
        for seq in 2000..2000 + secs(10.0) {
            idle(&mut g, seq);
        }
        assert!((9..=11).contains(&g.game.scores[0]), "{:?}", g.game.scores);
        assert_eq!(g.game.scores[1], 0);
        // an enemy on the point alongside blocks it from moving
        let p = g.game.flags[0].progress;
        {
            let st = &mut g.slots.get_mut(&2).unwrap().state;
            (st.x, st.z) = (fx + 1.0, fz);
        }
        for seq in 3000..3000 + secs(2.0) {
            idle(&mut g, seq);
        }
        assert_eq!(g.game.flags[0].progress, p, "contested");
    }

    #[test]
    fn match_ends_at_the_limit_then_restarts() {
        let mut g = duel();
        g.slots.get_mut(&1).unwrap().kills = rules::FFA_KILLS - 1;
        let mut seq = 0;
        while g.game.phase == Phase::Playing && seq < 120 {
            seq += 1;
            tick(&mut g, seq, true, 1.15, weapons::RIFLE, 0.0);
        }
        assert_eq!(g.game.phase, Phase::Ended);
        assert_eq!((g.game.winner_id, g.game.mvp), (1, 1));
        let hp = g.slots[&2].state.hp;
        for _ in 0..(rules::RESULTS_TIME / TICK_DT) as u32 + 2 {
            seq += 1;
            tick(&mut g, seq, true, 1.15, weapons::RIFLE, 0.0);
            if g.game.phase == Phase::Ended {
                assert!(
                    g.slots[&2].state.hp >= hp,
                    "no damage on the results screen"
                );
            }
        }
        assert_eq!(g.game.phase, Phase::Playing);
        assert_eq!(g.slots[&1].kills, 0);
        assert!(g.game.time_left > rules::MATCH_TIME - 1.0);
    }

    #[test]
    fn bots_hunt_and_fight() {
        for mode in [Mode::Combat, Mode::Domination, Mode::Slap] {
            let mut g = Game::new("S".into(), mode, Difficulty::Normal);
            join(&mut g, 1); // a person who stands still at their spawn
            let ids: Vec<u32> = g.slots.keys().copied().filter(|&k| k != 1).collect();
            let start: Vec<(f32, f32)> = ids
                .iter()
                .map(|k| (g.slots[k].state.x, g.slots[k].state.z))
                .collect();
            let mut moved = vec![0.0f32; ids.len()];
            for t in 0..(90.0 / TICK_DT) as u32 {
                g.step();
                if t % 60 == 0 {
                    for (i, k) in ids.iter().enumerate() {
                        let s = &g.slots[k].state;
                        moved[i] = moved[i].max(libm::hypotf(s.x - start[i].0, s.z - start[i].1));
                    }
                }
            }
            let kills: u32 = g
                .slots
                .values()
                .map(|s| if mode == Mode::Slap { s.kos } else { s.kills })
                .sum();
            if mode.armed() {
                let covers: u32 = g
                    .slots
                    .values()
                    .filter_map(|s| s.brain.as_ref().map(|b| b.covers))
                    .sum();
                assert!(covers > 0, "{mode:?}: bots never used cover");
            }
            assert!(
                moved.iter().all(|&m| m > 10.0),
                "{mode:?}: bots barely moved {moved:?}"
            );
            if mode == Mode::Domination {
                assert!(
                    g.game.scores.iter().sum::<u32>() > 20,
                    "{mode:?}: no points {:?}",
                    g.game.scores
                );
            } else {
                assert!(kills >= 2, "{mode:?}: only {kills} kills in 90 s");
            }
        }
    }

    /// Seconds a lone bot needs to kill a person standing still 20 m away in the open.
    fn time_to_kill(d: Difficulty, seed: u32) -> f32 {
        let mut g = Game::new("K".into(), Mode::Combat, d);
        g.next_bot = FIRST_BOT_ID + seed * 7;
        join(&mut g, 1);
        let mut bots: Vec<u32> = g.slots.keys().copied().filter(|&k| k != 1).collect();
        bots.sort_unstable();
        g.slots.remove(&bots[1]);
        let b = bots[0];
        {
            let st = &mut g.slots.get_mut(&1).unwrap().state;
            (st.x, st.z) = (LANE, -10.0);
            let st = &mut g.slots.get_mut(&b).unwrap().state;
            (st.x, st.z, st.yaw) = (LANE, 10.0, 0.0);
        }
        for t in 0..(30.0 / TICK_DT) as u32 {
            g.step();
            if g.slots[&1].state.dead > 0.0 {
                return t as f32 * TICK_DT;
            }
        }
        30.0
    }

    #[test]
    fn difficulty_changes_how_fast_bots_kill() {
        let avg = |d| (0..4).map(|k| time_to_kill(d, k)).sum::<f32>() / 4.0;
        let (easy, normal, hard) = (
            avg(Difficulty::Easy),
            avg(Difficulty::Normal),
            avg(Difficulty::Hard),
        );
        eprintln!("time to kill at 20 m: easy {easy:.2}s normal {normal:.2}s hard {hard:.2}s");
        assert!(
            hard < normal && normal < easy,
            "easy {easy} normal {normal} hard {hard}"
        );
        assert!(hard < 3.0 && easy < 30.0);
    }

    #[test]
    fn plate_soaks_torso_hits_but_not_legs() {
        let mut g = duel();
        g.damage(1, 2, weapons::RIFLE, 30.0, Zone::Torso, [0.0; 3]);
        let t = &g.slots[&2].state;
        assert!(
            (t.hp - 88.0).abs() < 1e-3 && (t.armor - 32.0).abs() < 1e-3,
            "{t:?}"
        );
        g.damage(1, 2, weapons::RIFLE, 30.0, Zone::Limb, [0.0; 3]);
        let t = &g.slots[&2].state;
        assert!((t.hp - 58.0).abs() < 1e-3 && (t.armor - 32.0).abs() < 1e-3);
    }

    #[test]
    fn shots_through_the_walkway_railing_still_hit() {
        let mut g = duel();
        // shooter on the walkway, target on the ground beyond the railing
        {
            let st = &mut g.slots.get_mut(&1).unwrap().state;
            (st.x, st.y, st.z, st.yaw) = (-0.5, 3.0, -14.0, 0.0);
            let st = &mut g.slots.get_mut(&2).unwrap().state;
            (st.x, st.y, st.z) = (0.0, 0.0, -20.0);
        }
        let pitch = libm::atan2f(1.15 - 4.65, 6.0);
        // the line of fire really does cross the 8 cm steel railing first
        let o = riftline_sim::sim::muzzle_origin(&g.slots[&1].state);
        let d = riftline_sim::sim::aim_dir(0.0, pitch);
        let first = raycast_map(o, d, 10.0).expect("railing in the way");
        assert!(
            first.mat == map::mat::STEEL && first.thickness < 0.1,
            "{first:?}"
        );
        for seq in 1..20 {
            g.handle(RoomEvent::Input {
                id: 1,
                frames: vec![InputFrame {
                    seq,
                    pitch,
                    buttons: button::AIM | button::FIRE,
                    ..Default::default()
                }],
            });
            g.step();
        }
        let t = &g.slots[&2].state;
        assert!(
            t.hp < combat::MAX_HP || t.dead > 0.0,
            "railing stopped every round: {t:?}"
        );
    }
}
