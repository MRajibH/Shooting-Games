//! One room: an authoritative fixed-tick simulation running in its own task.

use std::collections::{HashMap, VecDeque};

use riftline_sim::ballistics::{point, ray_player, raycast_map, Bullet, Zone};
use riftline_sim::constants::{combat, slap, MAX_REWIND_MS, SNAPSHOT_RATE, TICK_DT, TICK_RATE};
use riftline_sim::map;
use riftline_sim::protocol::{
    GameEvent, InputFrame, Mode, PlayerState, RosterEntry, ServerMessage,
};
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
}

impl Game {
    pub fn new(room: String, mode: Mode) -> Self {
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
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Spawn at the point farthest from everyone alive (round-robin when alone).
    fn spawn(&mut self, id: u32) -> PlayerState {
        let living: Vec<(f32, f32)> = self
            .slots
            .iter()
            .filter(|(&k, s)| k != id && s.state.dead <= 0.0)
            .map(|(_, s)| (s.state.x, s.state.z))
            .collect();
        let n = map::spawn_count();
        let pick = if living.is_empty() {
            self.next_spawn % n
        } else {
            (0..n)
                .map(|k| (k + self.next_spawn) % n)
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
        spawn_state(id, pick, self.mode == Mode::Combat)
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
                    },
                );
                let state = self.spawn(id);
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
                    },
                );
                self.roster_dirty = true;
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
                }
            }
        }
    }

    pub fn step(&mut self) {
        self.tick += 1;
        let mut slappers = Vec::new();
        let mut respawns = Vec::new();
        let mut volleys = Vec::new();
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
            let fresh = self.spawn(id);
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
                })
                .collect();
            players.sort_by(|a, b| {
                b.kos
                    .cmp(&a.kos)
                    .then(b.slaps.cmp(&a.slaps))
                    .then(b.kills.cmp(&a.kills))
                    .then(a.deaths.cmp(&b.deaths))
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
            let (o, d, l) = b.advance(TICK_DT);
            let wall = raycast_map(o, d, l);
            let max = wall.map_or(l, |h| h.t);
            let mut hit: Option<(u32, f32, Zone)> = None;
            if let Some(states) = self.past(b.rewind) {
                for t in states {
                    if t.id == b.owner {
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
            if let Some(h) = wall {
                self.events.push(GameEvent::Impact {
                    p: point(o, d, h.t),
                    n: h.normal,
                    mat: h.mat,
                });
                return false;
            }
            b.travelled += l;
            !b.expired()
        });
        self.bullets = bullets;
    }

    fn damage(&mut self, from: u32, to: u32, w: u8, dmg: f32, zone: Zone, p: [f32; 3]) {
        let shooter = self.slots.get(&from).map(|s| (s.state.x, s.state.z));
        let Some(t) = self.slots.get_mut(&to) else {
            return;
        };
        let st = &mut t.state;
        st.hp -= dmg;
        st.regen_cd = combat::REGEN_DELAY;
        self.events.push(GameEvent::Hit {
            from,
            to,
            dmg,
            zone,
            p,
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
        let mut g = Game::new("T".into(), mode);
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
        let mut g = Game::new("C".into(), Mode::Combat);
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
    fn carbine_kills_in_four_torso_hits_then_respawns() {
        let mut g = duel();
        let mut seq = 0;
        while g.slots[&2].state.dead <= 0.0 && seq < 60 {
            seq += 1;
            tick(&mut g, seq, true, 1.15, weapons::RIFLE, 0.0);
        }
        let fired = 30 - g.slots[&1].state.mags[0];
        assert_eq!(fired, 4, "31 damage per torso hit, 100 hp");
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
}
