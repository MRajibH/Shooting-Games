//! Room registry. Player counts change only under the lock, so a room never
//! shuts down while someone is joining it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use riftline_sim::constants::MAX_PLAYERS;
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};

use riftline_sim::protocol::Mode;

use crate::game::{self, Game, RoomEvent};

struct Room {
    tx: UnboundedSender<RoomEvent>,
    count: usize,
    mode: Mode,
}

#[derive(Clone, Default)]
pub struct Registry(Arc<Mutex<Inner>>);

#[derive(Default)]
struct Inner {
    rooms: HashMap<String, Room>,
    next: u32,
}

pub struct Seat {
    pub room: String,
    pub tx: UnboundedSender<RoomEvent>,
}

impl Registry {
    /// Quick play fills the first open room; a code joins or creates that room.
    pub fn take_seat(&self, code: Option<&str>, mode: Mode) -> Seat {
        let mut g = self.0.lock().unwrap();
        let wanted = code
            .map(|c| {
                c.chars()
                    .filter(|c| c.is_ascii_alphanumeric())
                    .take(8)
                    .collect::<String>()
                    .to_uppercase()
            })
            .filter(|c| !c.is_empty());
        let key = match wanted {
            Some(k) if g.rooms.get(&k).is_none_or(|r| r.count < MAX_PLAYERS) => k,
            _ => match g
                .rooms
                .iter()
                .find(|(_, r)| r.count < MAX_PLAYERS && r.mode == mode)
            {
                Some((k, _)) => k.clone(),
                None => {
                    g.next += 1;
                    format!("R{}", g.next)
                }
            },
        };
        if !g.rooms.contains_key(&key) {
            let (tx, rx) = unbounded_channel();
            tokio::spawn(game::run(Game::new(key.clone(), mode), rx, self.clone()));
            g.rooms.insert(key.clone(), Room { tx, count: 0, mode });
        }
        let r = g.rooms.get_mut(&key).unwrap();
        r.count += 1;
        Seat {
            room: key,
            tx: r.tx.clone(),
        }
    }

    pub fn release(&self, room: &str) {
        if let Some(r) = self.0.lock().unwrap().rooms.get_mut(room) {
            r.count = r.count.saturating_sub(1);
        }
    }

    pub fn remove_if_empty(&self, room: &str) -> bool {
        let mut g = self.0.lock().unwrap();
        if g.rooms.get(room).is_some_and(|r| r.count == 0) {
            g.rooms.remove(room);
            true
        } else {
            false
        }
    }
}
