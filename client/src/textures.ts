import * as THREE from 'three';

/**
 * Procedural PBR materials (albedo + normal map from a height field), generated at
 * load so the repo ships no texture files. Each texture tiles over `tile` metres.
 */

type Draw = (px: (x: number, y: number) => [number, number, number, number], size: number) => void;

function rng(seed: number) {
  return () => {
    seed |= 0; seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Tileable value noise with octaves. */
function makeNoise(seed: number, cells: number) {
  const r = rng(seed);
  const grid = Array.from({ length: cells * cells }, r);
  const at = (x: number, y: number) => grid[((y % cells) + cells) % cells * cells + (((x % cells) + cells) % cells)]!;
  const smooth = (t: number) => t * t * (3 - 2 * t);
  const one = (u: number, v: number) => {
    const x = u * cells, y = v * cells;
    const x0 = Math.floor(x), y0 = Math.floor(y);
    const fx = smooth(x - x0), fy = smooth(y - y0);
    const a = at(x0, y0), b = at(x0 + 1, y0), c = at(x0, y0 + 1), d = at(x0 + 1, y0 + 1);
    return a + (b - a) * fx + (c - a) * fy + (a - b - c + d) * fx * fy;
  };
  return (u: number, v: number, oct = 4) => {
    let s = 0, amp = 0.5, f = 1, norm = 0;
    for (let o = 0; o < oct; o++) { s += one(u * f, v * f) * amp; norm += amp; amp *= 0.5; f *= 2; }
    return s / norm;
  };
}

function bake(size: number, draw: (x: number, y: number) => [number, number, number, number], normalStrength: number) {
  const albedo = new Uint8ClampedArray(new ArrayBuffer(size * size * 4));
  const height = new Float32Array(size * size);
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const [r, g, b, h] = draw(x, y);
      const i = y * size + x;
      albedo[i * 4] = r; albedo[i * 4 + 1] = g; albedo[i * 4 + 2] = b; albedo[i * 4 + 3] = 255;
      height[i] = h;
    }
  }
  const normal = new Uint8ClampedArray(new ArrayBuffer(size * size * 4));
  const H = (x: number, y: number) => height[((y + size) % size) * size + ((x + size) % size)]!;
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const dx = (H(x + 1, y) - H(x - 1, y)) * normalStrength;
      const dy = (H(x, y + 1) - H(x, y - 1)) * normalStrength;
      const l = Math.hypot(dx, dy, 1);
      const i = (y * size + x) * 4;
      normal[i] = (-dx / l * 0.5 + 0.5) * 255;
      normal[i + 1] = (dy / l * 0.5 + 0.5) * 255;
      normal[i + 2] = (1 / l * 0.5 + 0.5) * 255;
      normal[i + 3] = 255;
    }
  }
  const tex = (data: Uint8ClampedArray<ArrayBuffer>, srgb: boolean) => {
    const t = new THREE.DataTexture(data, size, size, THREE.RGBAFormat);
    t.wrapS = t.wrapT = THREE.RepeatWrapping;
    t.magFilter = THREE.LinearFilter;
    t.minFilter = THREE.LinearMipmapLinearFilter;
    t.generateMipmaps = true;
    t.anisotropy = 8;
    if (srgb) t.colorSpace = THREE.SRGBColorSpace;
    t.needsUpdate = true;
    return t;
  };
  return { map: tex(albedo, true), normalMap: tex(normal, false) };
}

const clamp255 = (v: number) => Math.max(0, Math.min(255, v));
const mix = (a: number, b: number, t: number) => a + (b - a) * t;

export interface MatSpec { material: THREE.MeshStandardMaterial; tile: number }

function concrete(seed: number, base: [number, number, number], size = 512) {
  const n = makeNoise(seed, 8), fine = makeNoise(seed + 1, 64), stain = makeNoise(seed + 2, 4);
  return bake(size, (x, y) => {
    const u = x / size, v = y / size;
    const big = n(u, v), f = fine(u, v, 2), s = stain(u, v, 3);
    const pit = f > 0.78 ? -0.5 : 0;
    const seam = (y % (size / 2)) < 2 ? -0.6 : 0; // formwork seam every metre
    const k = 0.82 + big * 0.25 + (f - 0.5) * 0.12 - Math.max(0, s - 0.55) * 0.5 + pit * 0.2 + seam * 0.15;
    return [clamp255(base[0] * k), clamp255(base[1] * k), clamp255(base[2] * k), big * 0.3 + f * 0.5 + pit + seam];
  }, 3);
}

function brick(seed: number, size = 512) {
  const r = rng(seed), n = makeNoise(seed + 3, 32), dirt = makeNoise(seed + 4, 4);
  const rows = 30, cols = 9; // 2 m tile: 6.7 cm courses, 22 cm bricks
  const tints = Array.from({ length: rows * cols * 2 }, () => [0.8 + r() * 0.35, r()]);
  return bake(size, (x, y) => {
    const u = x / size, v = y / size;
    const row = Math.floor(v * rows);
    const off = row % 2 ? 0.5 : 0;
    const cu = u * cols + off, col = Math.floor(cu);
    const fu = cu - col, fv = v * rows - row;
    const mortar = fu < 0.05 || fv < 0.14;
    const [t, hue] = tints[(row * cols * 2 + ((col % (cols * 2)) + cols * 2) % (cols * 2))]!;
    const grain = n(u, v, 3);
    const d = Math.max(0, dirt(u, v) - 0.5) * 0.6;
    if (mortar) {
      const m = 150 + grain * 30 - d * 120;
      return [m, m * 0.97, m * 0.92, -1 + grain * 0.2];
    }
    const k = t * (0.85 + grain * 0.3) * (1 - d);
    return [clamp255(mix(150, 120, hue) * k), clamp255(mix(62, 55, hue) * k), clamp255(mix(44, 40, hue) * k), grain * 0.4];
  }, 4);
}

function plaster(seed: number, size = 512) {
  const n = makeNoise(seed, 16), fine = makeNoise(seed + 1, 128), dirt = makeNoise(seed + 2, 4);
  return bake(size, (x, y) => {
    const u = x / size, v = y / size;
    const f = fine(u, v, 2), b = n(u, v), d = Math.max(0, dirt(u, v) - 0.45);
    const k = 0.9 + b * 0.12 + (f - 0.5) * 0.06 - d * 0.5;
    return [clamp255(214 * k), clamp255(203 * k), clamp255(182 * k), f * 0.6 + b * 0.3];
  }, 2);
}

function wood(seed: number, size = 512) {
  const n = makeNoise(seed, 8), grain = makeNoise(seed + 1, 64), r = rng(seed + 5);
  const planks = 5;
  const tones = Array.from({ length: planks }, () => 0.8 + r() * 0.35);
  return bake(size, (x, y) => {
    const u = x / size, v = y / size;
    const p = Math.floor(v * planks), fv = v * planks - p;
    const gap = fv < 0.03 || fv > 0.97;
    const g = Math.sin((u * 40 + grain(u * 0.3, v * 3, 3) * 18) * Math.PI) * 0.5 + 0.5;
    const k = tones[p]! * (0.75 + g * 0.2 + n(u, v) * 0.15);
    if (gap) return [40, 28, 18, -1];
    return [clamp255(158 * k), clamp255(118 * k), clamp255(76 * k), g * 0.25];
  }, 3);
}

function container(seed: number, paint: [number, number, number], size = 512) {
  const rust = makeNoise(seed, 6), fine = makeNoise(seed + 1, 64), streak = makeNoise(seed + 2, 24);
  return bake(size, (x, y) => {
    const u = x / size, v = y / size;
    // trapezoidal corrugation, ~27 cm pitch over a 2.44 m tile
    const c = (u * 9) % 1;
    const ridge = c < 0.2 ? c / 0.2 : c < 0.5 ? 1 : c < 0.7 ? 1 - (c - 0.5) / 0.2 : 0;
    const rr = rust(u, v, 4) + (streak(u * 4, v * 0.25, 2) - 0.5) * 0.3;
    const rusty = Math.max(0, Math.min(1, (rr - 0.58) * 6));
    const f = fine(u, v, 2);
    const k = 0.85 + f * 0.2 + ridge * 0.06;
    const r = mix(paint[0] * k, 105 * k, rusty), g = mix(paint[1] * k, 58 * k, rusty), b = mix(paint[2] * k, 32 * k, rusty);
    return [clamp255(r), clamp255(g), clamp255(b), ridge * 2.2 + rusty * 0.3];
  }, 2.5);
}

function steel(seed: number, size = 256) {
  const n = makeNoise(seed, 8), fine = makeNoise(seed + 1, 128);
  return bake(size, (x, y) => {
    const u = x / size, v = y / size;
    const f = fine(u * 3, v * 0.2, 2);
    const k = 0.75 + n(u, v) * 0.3 + (f - 0.5) * 0.15;
    return [clamp255(96 * k), clamp255(99 * k), clamp255(102 * k), f * 0.3];
  }, 2);
}

function asphalt(seed: number, size = 1024) {
  const n = makeNoise(seed, 8), fine = makeNoise(seed + 1, 256), crack = makeNoise(seed + 2, 12), patch = makeNoise(seed + 3, 3);
  return bake(size, (x, y) => {
    const u = x / size, v = y / size;
    const f = fine(u, v, 2), b = n(u, v);
    const cr = Math.abs(crack(u, v, 3) - 0.5) < 0.006 ? 1 : 0;
    const pa = patch(u, v) > 0.62 ? 0.12 : 0;
    const stone = f > 0.7 ? 0.18 : 0;
    const k = 0.75 + b * 0.25 + stone - cr * 0.5 - pa;
    return [clamp255(68 * k), clamp255(67 * k), clamp255(65 * k), f * 0.8 - cr * 1.5];
  }, 3);
}

function std(maps: { map: THREE.Texture; normalMap: THREE.Texture }, roughness: number, metalness = 0, normal = 1) {
  return new THREE.MeshStandardMaterial({ ...maps, roughness, metalness, normalScale: new THREE.Vector2(normal, normal) });
}

/** Materials indexed by the map's material ids (crates/sim/src/map.rs `mat`). */
export function mapMaterials(): MatSpec[] {
  return [
    { material: std(concrete(11, [150, 148, 142]), 0.92), tile: 2 },
    { material: std(brick(21), 0.88, 0, 1.2), tile: 2 },
    { material: std(plaster(31), 0.95, 0, 0.6), tile: 3 },
    { material: std(wood(41), 0.8), tile: 1.2 },
    { material: std(container(51, [150, 42, 34]), 0.55, 0.35), tile: 2.44 },
    { material: std(container(61, [38, 72, 118]), 0.55, 0.35), tile: 2.44 },
    { material: std(steel(71), 0.45, 0.8), tile: 2 },
  ];
}

export function groundMaterial(): MatSpec {
  return { material: std(asphalt(81), 0.95, 0, 0.8), tile: 6 };
}
