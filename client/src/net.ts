import { Predictor, interpDelayMs } from './wasm/riftline_sim.js';
import type { ClientMessage, GameEvent, InputFrame, Mode, PlayerState, RosterEntry, ServerMessage } from './protocol';

interface Snap { time: number; players: Map<number, PlayerState>; }

const send = (ws: WebSocket, m: ClientMessage) => ws.readyState === WebSocket.OPEN && ws.send(JSON.stringify(m));
const lerpAngle = (a: number, b: number, k: number) => a + Math.atan2(Math.sin(b - a), Math.cos(b - a)) * k;

/**
 * Socket plus client-side prediction (the server's own Rust code, compiled to
 * WebAssembly), reconciliation, and interpolation of other players.
 */
export class Net {
  id = 0;
  room = '';
  mode: Mode = 'combat';
  me = new Predictor(0);
  roster: RosterEntry[] = [];
  onStatus: (s: string) => void = () => {};
  onWelcome: () => void = () => {};
  onEvent: (e: GameEvent) => void = () => {};
  onRoster: () => void = () => {};
  private ws: WebSocket;
  private pending: InputFrame[] = [];
  private snaps: Snap[] = [];
  private readonly delay = interpDelayMs();

  constructor(name: string, room: string | undefined, mode: Mode) {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    this.ws = new WebSocket(`${proto}://${location.host}/ws`);
    this.ws.onopen = () => send(this.ws, { t: 'join', name, room, mode });
    this.ws.onmessage = (e) => this.handle(JSON.parse(e.data) as ServerMessage);
    this.ws.onclose = () => this.onStatus('disconnected — reload to rejoin');
  }

  get ready() { return this.id !== 0; }

  /** Apply an input locally right away and send it. Returns true if a slap was thrown. */
  sendInput(f: InputFrame): boolean {
    if (!this.ready) return false;
    const slapped = this.step(f);
    this.pending.push(f);
    if (this.pending.length > 240) this.pending.shift();
    // resend the last few frames so one dropped packet does not lose input
    send(this.ws, { t: 'input', frames: this.pending.slice(-3) });
    return slapped;
  }

  private step(f: InputFrame) {
    return this.me.step(f.seq, f.moveX, f.moveZ, f.yaw, f.pitch, f.buttons);
  }

  private handle(m: ServerMessage): void {
    if (m.t === 'welcome') {
      this.id = m.id;
      this.room = m.room;
      this.mode = m.mode;
      this.me.free();
      this.me = new Predictor(m.id);
      this.onStatus(`${m.mode === 'slap' ? 'Slap Arena' : 'Combat'} · room ${m.room}`);
      this.onWelcome();
    } else if (m.t === 'snapshot') {
      const players = new Map(m.players.map((p) => [p.id, p]));
      this.snaps.push({ time: performance.now(), players });
      if (this.snaps.length > 60) this.snaps.shift();
      const a = players.get(this.id);
      if (a) {
        // reconcile: take the server state, replay inputs it has not processed yet
        this.me.setJson(JSON.stringify(a));
        this.pending = this.pending.filter((f) => f.seq > m.ack);
        for (const f of this.pending) this.step(f);
      }
      for (const e of m.events) this.onEvent(e);
    } else if (m.t === 'roster') {
      this.roster = m.players;
      this.onRoster();
    } else if (m.t === 'left') {
      for (const s of this.snaps) s.players.delete(m.id);
    }
  }

  /** Other players, rendered slightly in the past between two snapshots. */
  remotes(): PlayerState[] {
    const t = performance.now() - this.delay;
    const s = this.snaps;
    if (s.length === 0) return [];
    let i = s.length - 1;
    while (i > 0 && s[i]!.time > t) i--;
    const a = s[i]!;
    const b = s[Math.min(i + 1, s.length - 1)]!;
    const k = b.time > a.time ? Math.min(1, Math.max(0, (t - a.time) / (b.time - a.time))) : 0;
    const out: PlayerState[] = [];
    for (const [id, pa] of a.players) {
      if (id === this.id) continue;
      const pb = b.players.get(id) ?? pa;
      const teleported = Math.hypot(pb.x - pa.x, pb.z - pa.z) > 4;
      const kk = teleported ? 1 : k;
      out.push({
        ...pb,
        x: pa.x + (pb.x - pa.x) * kk,
        y: pa.y + (pb.y - pa.y) * kk,
        z: pa.z + (pb.z - pa.z) * kk,
        vx: pa.vx + (pb.vx - pa.vx) * kk,
        vz: pa.vz + (pb.vz - pa.vz) * kk,
        lean: pa.lean + (pb.lean - pa.lean) * kk,
        yaw: lerpAngle(pa.yaw, pb.yaw, kk),
        pitch: pa.pitch + (pb.pitch - pa.pitch) * kk,
      });
    }
    return out;
  }

  nameOf(id: number) {
    return this.roster.find((r) => r.id === id)?.name ?? `#${id}`;
  }
}
