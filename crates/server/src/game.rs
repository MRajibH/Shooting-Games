//! One room: an authoritative fixed-tick simulation running in its own task.

use std::collections::HashMap;

use riftline_sim::constants::{SNAPSHOT_RATE, TICK_DT, TICK_RATE};
use riftline_sim::protocol::{InputFrame, PlayerState, ServerMessage};
use riftline_sim::sim::{spawn_state, step_player};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

pub type Outbox = UnboundedSender<String>;

pub enum RoomEvent {
    Join { id: u32, name: String, out: Outbox },
    Input { id: u32, frames: Vec<InputFrame> },
    Leave { id: u32 },
}

struct Slot {
    #[allow(dead_code)] // shown on the scoreboard from milestone 4
    name: String,
    out: Outbox,
    state: PlayerState,
    queue: Vec<InputFrame>,
    last: Option<InputFrame>,
    ack: u32,
}

pub struct Game {
    room: String,
    slots: HashMap<u32, Slot>,
    tick: u64,
    snap_acc: u32,
}

impl Game {
    pub fn new(room: String) -> Self {
        Self {
            room,
            slots: HashMap::new(),
            tick: 0,
            snap_acc: 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
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
                    },
                );
                self.slots.insert(
                    id,
                    Slot {
                        name,
                        out,
                        state: spawn_state(id),
                        queue: Vec::new(),
                        last: None,
                        ack: 0,
                    },
                );
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
                }
            }
        }
    }

    pub fn step(&mut self) {
        self.tick += 1;
        for s in self.slots.values_mut() {
            // One input per tick; repeat the last one if the client's packet is late.
            let f = if s.queue.is_empty() {
                s.last
            } else {
                Some(s.queue.remove(0))
            };
            let Some(f) = f else { continue };
            s.ack = s.ack.max(f.seq);
            s.last = Some(f);
            step_player(&mut s.state, &f, TICK_DT);
        }
        self.snap_acc += SNAPSHOT_RATE;
        if self.snap_acc < TICK_RATE {
            return;
        }
        self.snap_acc -= TICK_RATE;
        let players: Vec<PlayerState> = self.slots.values().map(|s| s.state.clone()).collect();
        for s in self.slots.values() {
            send(
                &s.out,
                &ServerMessage::Snapshot {
                    tick: self.tick,
                    ack: s.ack,
                    players: players.clone(),
                },
            );
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
