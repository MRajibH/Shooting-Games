import {
  SNAPSHOT_RATE, TICK_DT, TICK_RATE, encode, spawnState, stepPlayer,
  type InputFrame, type PlayerState, type ServerMessage,
} from '@riftline/shared';

export interface Client {
  id: number;
  name: string;
  send(data: string): void;
}

interface Slot {
  client: Client;
  state: PlayerState;
  queue: InputFrame[];
  lastInput: InputFrame | null;
  ack: number;
}

/** One room: authoritative fixed-tick simulation plus snapshot broadcast. */
export class Game {
  private slots = new Map<number, Slot>();
  private tick = 0;
  private snapAcc = 0;
  private timer: NodeJS.Timeout | null = null;

  constructor(readonly room: string) {}

  get size() { return this.slots.size; }

  add(client: Client): void {
    this.slots.set(client.id, { client, state: spawnState(client.id), queue: [], lastInput: null, ack: 0 });
    client.send(encode({ t: 'welcome', id: client.id, room: this.room, tickRate: TICK_RATE }));
    if (!this.timer) this.timer = setInterval(() => this.step(), 1000 / TICK_RATE);
  }

  remove(id: number): void {
    if (!this.slots.delete(id)) return;
    this.broadcast({ t: 'left', id });
    if (this.slots.size === 0 && this.timer) { clearInterval(this.timer); this.timer = null; }
  }

  pushInputs(id: number, frames: InputFrame[]): void {
    const s = this.slots.get(id);
    if (!s) return;
    for (const f of frames) {
      if (f.seq > s.ack && s.queue.length < TICK_RATE) s.queue.push(f);
    }
    s.queue.sort((a, b) => a.seq - b.seq);
  }

  private step(): void {
    this.tick++;
    for (const s of this.slots.values()) {
      // one input per tick; repeat the last one if the client is late
      const f = s.queue.shift() ?? s.lastInput;
      if (!f) continue;
      if (f.seq > s.ack) s.ack = f.seq;
      s.lastInput = f;
      stepPlayer(s.state, f, TICK_DT);
    }
    this.snapAcc += SNAPSHOT_RATE;
    if (this.snapAcc < TICK_RATE) return;
    this.snapAcc -= TICK_RATE;
    const players = [...this.slots.values()].map((s) => s.state);
    for (const s of this.slots.values()) {
      s.client.send(encode({ t: 'snapshot', tick: this.tick, ack: s.ack, players }));
    }
  }

  private broadcast(m: ServerMessage): void {
    const raw = encode(m);
    for (const s of this.slots.values()) s.client.send(raw);
  }
}
