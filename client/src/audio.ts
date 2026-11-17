/** Synthesised sound effects (no audio files): slaps, whooshes, knockouts. */
export class Sfx {
  private ctx: AudioContext | null = null;
  private noise: AudioBuffer | null = null;
  volume = 0.8;

  unlock() {
    if (!this.ctx) {
      this.ctx = new AudioContext();
      const len = this.ctx.sampleRate;
      this.noise = this.ctx.createBuffer(1, len, this.ctx.sampleRate);
      const d = this.noise.getChannelData(0);
      for (let i = 0; i < len; i++) d[i] = Math.random() * 2 - 1;
    }
    void this.ctx.resume();
  }

  private out(pan: number, gain: number) {
    const c = this.ctx!;
    const g = c.createGain();
    g.gain.value = gain * this.volume;
    const p = c.createStereoPanner();
    p.pan.value = Math.max(-1, Math.min(1, pan));
    g.connect(p).connect(c.destination);
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
}
