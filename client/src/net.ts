import {
  INTERP_DELAY_MS, TICK_DT, decodeServer, encode, spawnState, stepPlayer,
  type InputFrame, type PlayerState, type ServerMessage,
} from '@riftline/shared';

interface Snap { time: number; players: Map<number, PlayerState>; }

/** Socket + client-side prediction, reconciliation and remote interpolation. */
export class Net {
  id = 0;
  room = '';
  me: PlayerState = spawnState(0);
  onStatus: (s: string) => void = () => {};
  private ws: WebSocket;
  private pending: InputFrame[] = [];
  private snaps: Snap[] = [];

  constructor(name: string, room?: string) {
    const proto = location.protocol === 'https:' ? 'wss' : 'ws';
    this.ws = new WebSocket(`${proto}://${location.host}/ws`);
    this.ws.onopen = () => this.ws.send(encode({ t: 'join', name, room }));
    this.ws.onmessage = (e) => this.handle(decodeServer(e.data));
    this.ws.onclose = () => this.onStatus('disconnected');
  }

  get ready() { return this.id !== 0; }

  /** Apply an input locally right away and queue it for the server. */
  sendInput(f: InputFrame): void {
    if (!this.ready) return;
    stepPlayer(this.me, f, TICK_DT);
    this.pending.push(f);
    if (this.pending.length > 120) this.pending.shift();
    // resend the last few frames so a dropped packet does not lose input
    this.ws.send(encode({ t: 'input', frames: this.pending.slice(-3) }));
  }

  private handle(m: ServerMessage): void {
    if (m.t === 'welcome') {
      this.id = m.id;
      this.room = m.room;
      this.me = spawnState(m.id);
      this.onStatus(`room ${m.room} · click to capture mouse · WASD / Shift / Space`);
    } else if (m.t === 'snapshot') {
      const players = new Map(m.players.map((p) => [p.id, p]));
      this.snaps.push({ time: performance.now(), players });
      if (this.snaps.length > 30) this.snaps.shift();
      const auth = players.get(this.id);
      if (auth) {
        // reconcile: rewind to the server state, replay inputs it has not seen yet
        this.me = { ...auth };
        this.pending = this.pending.filter((f) => f.seq > m.ack);
        for (const f of this.pending) stepPlayer(this.me, f, TICK_DT);
      }
    }
  }

  /** Other players, rendered slightly in the past between two snapshots. */
  remotes(): PlayerState[] {
    const t = performance.now() - INTERP_DELAY_MS;
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
      out.push({
        ...pa,
        x: pa.x + (pb.x - pa.x) * k,
        y: pa.y + (pb.y - pa.y) * k,
        z: pa.z + (pb.z - pa.z) * k,
        yaw: pa.yaw + Math.atan2(Math.sin(pb.yaw - pa.yaw), Math.cos(pb.yaw - pa.yaw)) * k,
      });
    }
    return out;
  }
}
