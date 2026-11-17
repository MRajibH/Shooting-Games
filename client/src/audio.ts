/** Gunshot character per weapon: crack, body (freq, length), thump (freq, gain), tail (length, gain). */
const SHOT = [
  { crack: 0.9, body: 1300, bodyDur: 0.09, thump: 110, thumpGain: 0.9, tail: 0.8, tailGain: 0.32 }, // carbine
  { crack: 0.55, body: 1800, bodyDur: 0.06, thump: 150, thumpGain: 0.55, tail: 0.5, tailGain: 0.22 }, // SMG
  { crack: 0.7, body: 750, bodyDur: 0.16, thump: 75, thumpGain: 1.4, tail: 1.1, tailGain: 0.45 }, // shotgun
  { crack: 1.0, body: 1000, bodyDur: 0.15, thump: 65, thumpGain: 1.5, tail: 1.8, tailGain: 0.5 }, // bolt rifle
];

/** Synthesised sound effects (no audio files): gunfire, impacts, reloads, slaps. */
export class Sfx {
  private ctx: AudioContext | null = null;
  private noise: AudioBuffer | null = null;
  private master: AudioNode | null = null;
  volume = 0.8;

  unlock() {
    if (!this.ctx) {
      this.ctx = new AudioContext();
      const len = this.ctx.sampleRate;
      this.noise = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
      const d = this.noise.getChannelData(0);
      for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
      // a limiter so a firefight does not clip
      const comp = this.ctx.createDynamicsCompressor();
      comp.threshold.value = -10;
      comp.ratio.value = 12;
      comp.connect(this.ctx.destination);
      this.master = comp;
    }
    void this.ctx.resume();
  }

  private out(pan: number, gain: number) {
    const c = this.ctx!;
    const g = c.createGain();
    g.gain.value = gain * this.volume;
    const p = c.createStereoPanner();
    p.pan.value = Math.max(-1, Math.min(1, pan));
    g.connect(p).connect(this.master ?? c.destination);
    return g;
  }

  private burst(dest: AudioNode, t: number, dur: number, type: BiquadFilterType, freq: number, q: number, peak: number) {
    const c = this.ctx!;
    const src = c.createBufferSource();
    src.buffer = this.noise;
    const f = c.createBiquadFilter();
    f.type = type; f.frequency.value = freq; f.Q.value = q;
    const g = c.createGain();
    g.gain.setValueAtTime(0, t);
    g.gain.linearRampToValueAtTime(peak, t + 0.002);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    src.connect(f).connect(g).connect(dest);
    src.start(t, Math.random() * 0.5);
    src.stop(t + dur + 0.02);
    return f;
  }

  /** Open-hand impact: sharp crack plus a soft body thump. */
  slapHit(pan = 0, gain = 1) {
    if (!this.ctx) return;
    const t = this.ctx.currentTime;
    const o = this.out(pan, gain);
    this.burst(o, t, 0.09, 'bandpass', 2200, 0.9, 1.0);
    this.burst(o, t, 0.05, 'highpass', 4500, 0.7, 0.6);
    const osc = this.ctx.createOscillator();
    const g = this.ctx.createGain();
    osc.frequency.setValueAtTime(180, t);
    osc.frequency.exponentialRampToValueAtTime(70, t + 0.12);
    g.gain.setValueAtTime(0.5, t);
    g.gain.exponentialRampToValueAtTime(0.0001, t + 0.14);
    osc.connect(g).connect(o);
    osc.start(t);
    osc.stop(t + 0.16);
  }

  /** Arm swinging through the air. */
  whoosh(pan = 0, gain = 1) {
    if (!this.ctx) return;
    const t = this.ctx.currentTime;
    const f = this.burst(this.out(pan, gain * 0.5), t, 0.22, 'bandpass', 500, 1.2, 0.8);
    f.frequency.setValueAtTime(400, t);
    f.frequency.exponentialRampToValueAtTime(1600, t + 0.18);
  }

  /** Body hitting the ground. */
  knockout(pan = 0, gain = 1) {
    if (!this.ctx) return;
    const t = this.ctx.currentTime + 0.35;
    const o = this.out(pan, gain);
    this.burst(o, t, 0.3, 'lowpass', 300, 0.8, 1.2);
    this.burst(o, t + 0.12, 0.15, 'lowpass', 500, 0.8, 0.5);
  }

  private tone(dest: AudioNode, t: number, f0: number, f1: number, dur: number, peak: number, type: OscillatorType = 'sine') {
    const c = this.ctx!;
    const osc = c.createOscillator();
    osc.type = type;
    const g = c.createGain();
    osc.frequency.setValueAtTime(f0, t);
    osc.frequency.exponentialRampToValueAtTime(Math.max(1, f1), t + dur);
    g.gain.setValueAtTime(peak, t);
    g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
    osc.connect(g).connect(dest);
    osc.start(t);
    osc.stop(t + dur + 0.02);
  }

  /**
   * A gunshot heard from `dist` metres: arrives late (speed of sound), quieter,
   * and duller (air absorbs the highs), while the echo tail carries further.
   */
  gunshot(weapon: number, pan = 0, dist = 0) {
    if (!this.ctx) return;
    const c = this.ctx;
    const p = SHOT[weapon] ?? SHOT[0]!;
    const t = c.currentTime + dist / 343;
    const near = 1 / (1 + dist / 12);
    const lp = c.createBiquadFilter();
    lp.type = 'lowpass';
    lp.frequency.value = 16000 / (1 + dist / 35);
    const o = this.out(pan * Math.min(1, 0.4 + dist / 20), 1);
    lp.connect(o);
    this.burst(lp, t, 0.03, 'highpass', 3500, 0.7, p.crack * near * 1.3);
    this.burst(lp, t, p.bodyDur, 'bandpass', p.body, 0.8, 1.4 * near);
    this.tone(lp, t, p.thump * 1.8, p.thump * 0.5, 0.14, p.thumpGain * near);
    const tail = this.burst(o, t + 0.02, p.tail, 'lowpass', 900, 0.5, p.tailGain * (0.35 + 0.65 * Math.sqrt(near)));
    tail.frequency.setValueAtTime(1400, t);
    tail.frequency.exponentialRampToValueAtTime(250, t + p.tail);
  }

  /** Supersonic bullet passing close by. */
  snap(pan = 0, gain = 1) {
    if (!this.ctx) return;
    const t = this.ctx.currentTime;
    this.burst(this.out(pan, gain), t, 0.025, 'highpass', 5000, 0.6, 0.9);
  }

  /** Bullet striking a surface nearby. */
  impact(pan: number, gain: number, metal: boolean) {
    if (!this.ctx || gain < 0.03) return;
    const t = this.ctx.currentTime;
    const o = this.out(pan, gain);
    if (metal) this.tone(o, t, 2400 + Math.random() * 1200, 1800, 0.12, 0.25, 'triangle');
    this.burst(o, t, 0.05, 'bandpass', metal ? 3000 : 1200, 1, 0.6);
  }

  dryFire() {
    if (!this.ctx) return;
    const t = this.ctx.currentTime;
    this.burst(this.out(0, 0.6), t, 0.02, 'bandpass', 3500, 2, 1);
  }

  /** Reload foley: 'out' magazine out, 'in' magazine seated, 'rack' bolt or pump, 'shell'. */
  foley(kind: 'out' | 'in' | 'rack' | 'shell', pan = 0, gain = 0.7) {
    if (!this.ctx) return;
    const t = this.ctx.currentTime;
    const o = this.out(pan, gain);
    if (kind === 'out') { this.burst(o, t, 0.04, 'bandpass', 1800, 2, 0.8); this.burst(o, t + 0.05, 0.06, 'bandpass', 900, 1, 0.4); }
    if (kind === 'in') { this.burst(o, t, 0.03, 'bandpass', 2500, 2, 1); this.burst(o, t + 0.01, 0.05, 'lowpass', 600, 1, 0.8); }
    if (kind === 'rack') { this.burst(o, t, 0.05, 'bandpass', 2000, 1.5, 0.9); this.burst(o, t + 0.12, 0.05, 'bandpass', 2800, 1.5, 1); }
    if (kind === 'shell') { this.burst(o, t, 0.04, 'bandpass', 1500, 2, 0.8); this.tone(o, t, 900, 700, 0.05, 0.1, 'triangle'); }
  }

  /** Confirmation that my bullet hit someone. */
  hitMarker(head: boolean, kill: boolean) {
    if (!this.ctx) return;
    const t = this.ctx.currentTime;
    const o = this.out(0, 0.5);
    this.burst(o, t, 0.03, 'bandpass', head ? 4200 : 2600, 4, 0.9);
    if (head) this.tone(o, t, 2900, 2700, 0.12, 0.12, 'triangle');
    if (kill) this.tone(o, t + 0.04, 220, 90, 0.18, 0.5);
  }

  /** I was hit. */
  hurt() {
    if (!this.ctx) return;
    const t = this.ctx.currentTime;
    const o = this.out(0, 0.8);
    this.burst(o, t, 0.12, 'lowpass', 400, 1, 1.2);
    this.tone(o, t, 120, 50, 0.2, 0.6);
  }
}
