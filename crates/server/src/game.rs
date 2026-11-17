//! One room: an authoritative fixed-tick simulation running in its own task.

use std::collections::HashMap;

use riftline_sim::constants::{slap, SNAPSHOT_RATE, TICK_DT, TICK_RATE};
use riftline_sim::protocol::{
    GameEvent, InputFrame, Mode, PlayerState, RosterEntry, ServerMessage,
};
use riftline_sim::sim::{spawn_state, step_player};
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
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    fn spawn(&mut self, id: u32) -> PlayerState {
        let s = spawn_state(id, self.next_spawn);
        self.next_spawn += 1;
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
        for (&id, s) in self.slots.iter_mut() {
            // One input per tick; repeat the last one if the client's packet is late.
            let f = if s.queue.is_empty() {
                s.last
            } else {
                Some(s.queue.remove(0))
            };
            let Some(f) = f else { continue };
            s.ack = s.ack.max(f.seq);
            s.last = Some(f);
            let was_ko = s.state.ko > 0.0;
            if step_player(&mut s.state, &f, TICK_DT).slap {
                slappers.push(id);
            }
            if was_ko && s.state.ko <= 0.0 {
                respawns.push(id);
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
                })
                .collect();
            players.sort_by(|a, b| b.kos.cmp(&a.kos).then(b.slaps.cmp(&a.slaps)));
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
}
