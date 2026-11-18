import * as THREE from 'three';

export type QualityLevel = 'auto' | 'high' | 'medium' | 'low';

export interface Settings {
  quality: QualityLevel;
  /** Look sensitivity multiplier, 0.25..3. */
  sensitivity: number;
  /** 0..1 */
  volume: number;
  /** Base field of view in degrees (60..90). */
  fov: number;
  invertY: boolean;
}

const KEY = 'riftline.settings';

export function loadSettings(): Settings {
  const d: Settings = { quality: 'auto', sensitivity: 1, volume: 0.8, fov: 72, invertY: false };
  try {
    const s = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Partial<Settings>;
    return {
      quality: ['auto', 'high', 'medium', 'low'].includes(s.quality as string) ? s.quality! : d.quality,
      sensitivity: typeof s.sensitivity === 'number' ? Math.min(3, Math.max(0.25, s.sensitivity)) : d.sensitivity,
      volume: typeof s.volume === 'number' ? Math.min(1, Math.max(0, s.volume)) : d.volume,
      fov: typeof s.fov === 'number' ? Math.min(90, Math.max(60, s.fov)) : d.fov,
      invertY: s.invertY === true,
    };
  } catch {
    return d;
  }
}

export function saveSettings(s: Settings) {
  try { localStorage.setItem(KEY, JSON.stringify(s)); } catch { /* storage blocked */ }
}

const TIERS = {
  high: { ratio: 2, shadow: 4096 },
  medium: { ratio: 1.5, shadow: 2048 },
  low: { ratio: 1, shadow: 1024 },
} as const;

/**
 * Render quality: a tier sets the pixel-ratio cap and the sun's shadow resolution.
 * In auto, resolution scales every second to hold ~55 fps (down to 60 % of the cap),
 * and the tier itself drops when even that is not enough.
 */
export class Quality {
  tier: keyof typeof TIERS = 'high';
  private scale = 1;
  private frames = 0;
  private since = performance.now();
  private slowSeconds = 0;
  private fastSeconds = 0;
  /** Best tier auto may climb back to. */
  private top: keyof typeof TIERS = 'high';
  fps = 0;
  /** Called after the tier or resolution changes. */
  onApply: (tier: 'high' | 'medium' | 'low') => void = () => {};

  constructor(private renderer: THREE.WebGLRenderer, private sun: THREE.DirectionalLight, private level: QualityLevel, mobile: boolean) {
    this.set(level, mobile);
  }

  get label() { return `${this.level === 'auto' ? 'auto/' : ''}${this.tier} ${Math.round(this.scale * 100)}%`; }

  set(level: QualityLevel, mobile = false) {
    this.level = level;
    this.tier = level === 'auto' ? (mobile ? 'medium' : 'high') : level;
    this.top = this.tier;
    this.scale = 1;
    this.apply();
  }

  private apply() {
    const t = TIERS[this.tier];
    this.renderer.setPixelRatio(Math.min(devicePixelRatio, t.ratio) * this.scale);
    if (this.sun.shadow.mapSize.x !== t.shadow) {
      this.sun.shadow.mapSize.set(t.shadow, t.shadow);
      this.sun.shadow.map?.dispose();
      this.sun.shadow.map = null;
    }
    this.onApply(this.tier);
  }

  /** Call once per rendered frame. */
  frame() {
    this.frames++;
    const now = performance.now();
    if (now - this.since < 1000) return;
    this.fps = (this.frames * 1000) / (now - this.since);
    this.frames = 0;
    this.since = now;
    // a hidden tab is throttled: its frame rate says nothing about the device
    if (this.level !== 'auto' || document.hidden || this.fps < 5) return;
    if (this.fps < 48 && this.scale > 0.6) {
      this.scale = Math.max(0.6, this.scale - 0.1);
      this.apply();
    } else if (this.fps > 58 && this.scale < 1) {
      this.scale = Math.min(1, this.scale + 0.05);
      this.apply();
    }
    this.slowSeconds = this.fps < 40 && this.scale <= 0.6 ? this.slowSeconds + 1 : 0;
    // plenty of headroom for a while (e.g. after the window was in the background): step back up
    this.fastSeconds = this.fps > 58 && this.scale >= 1 ? this.fastSeconds + 1 : 0;
    if (this.fastSeconds >= 5 && this.tier !== this.top) {
      this.tier = this.tier === 'low' ? 'medium' : 'high';
      this.scale = 0.8;
      this.fastSeconds = 0;
      this.apply();
      return;
    }
    if (this.slowSeconds >= 3 && this.tier !== 'low') {
      this.tier = this.tier === 'high' ? 'medium' : 'low';
      this.scale = 0.8;
      this.slowSeconds = 0;
      this.apply();
    }
  }
}
