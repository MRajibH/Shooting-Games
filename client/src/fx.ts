import * as THREE from 'three';

/**
 * Combat effects from fixed pools (no per-shot allocation, no light count changes
 * that would recompile shaders): tracers, muzzle flash + light, bullet holes,
 * impact dust / sparks, and hit puffs.
 */

function radial(stops: [number, string][], size = 64) {
  const c = document.createElement('canvas');
  c.width = c.height = size;
  const g = c.getContext('2d')!;
  const gr = g.createRadialGradient(size / 2, size / 2, 0, size / 2, size / 2, size / 2);
  for (const [o, col] of stops) gr.addColorStop(o, col);
  g.fillStyle = gr;
  g.fillRect(0, 0, size, size);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

function flashTexture() {
  const s = 128;
  const c = document.createElement('canvas');
  c.width = c.height = s;
  const g = c.getContext('2d')!;
  g.translate(s / 2, s / 2);
  const gr = g.createRadialGradient(0, 0, 0, 0, 0, s / 2);
  gr.addColorStop(0, 'rgba(255,250,225,1)');
  gr.addColorStop(0.25, 'rgba(255,190,90,0.9)');
  gr.addColorStop(1, 'rgba(255,120,30,0)');
  g.fillStyle = gr;
  // a few uneven prongs, like a real flash
  for (let i = 0; i < 7; i++) {
    g.rotate((Math.PI * 2) / 7 + Math.random() * 0.3);
    g.beginPath();
    g.moveTo(0, -s * 0.06);
    g.lineTo(s * (0.3 + Math.random() * 0.2), 0);
    g.lineTo(0, s * 0.06);
    g.fill();
  }
  g.beginPath();
  g.arc(0, 0, s * 0.16, 0, Math.PI * 2);
  g.fill();
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

function holeTexture() {
  const s = 64;
  const c = document.createElement('canvas');
  c.width = c.height = s;
  const g = c.getContext('2d')!;
  const gr = g.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
  gr.addColorStop(0, 'rgba(8,8,8,1)');
  gr.addColorStop(0.28, 'rgba(15,14,13,0.95)');
  gr.addColorStop(0.45, 'rgba(40,38,35,0.55)');
  gr.addColorStop(1, 'rgba(60,58,55,0)');
  g.fillStyle = gr;
  g.fillRect(0, 0, s, s);
  const t = new THREE.CanvasTexture(c);
  t.colorSpace = THREE.SRGBColorSpace;
  return t;
}

/** Puff colour per material id (map mats 0..6, ground 7). */
const DUST = [0xb8b4ab, 0x9a5a44, 0xd8d0c0, 0x8a6a48, 0x7a4a3a, 0x5a6878, 0x9a9a9a, 0x6a665e];
const METAL = new Set([4, 5, 6]);

interface Tracer { mesh: THREE.Mesh; from: THREE.Vector3; dir: THREE.Vector3; dist: number; travelled: number; speed: number; alive: boolean }
interface Particle { sprite: THREE.Sprite; vel: THREE.Vector3; life: number; age: number; grow: number; size: number; gravity: number; alpha: number; alive: boolean }
interface Flash { mesh: THREE.Object3D; age: number; alive: boolean }

export class Fx {
  private tracers: Tracer[] = [];
  private particles: Particle[] = [];
  private flashes: Flash[] = [];
  private holes: THREE.Mesh[] = [];
  private holeNext = 0;
  private lights: { light: THREE.PointLight; age: number }[] = [];
  private lightNext = 0;
  private flashTex = flashTexture();
  private softTex = radial([[0, 'rgba(255,255,255,1)'], [0.4, 'rgba(255,255,255,0.5)'], [1, 'rgba(255,255,255,0)']]);

  constructor(private scene: THREE.Scene, private camera: THREE.Camera) {
    const tracerMat = new THREE.MeshBasicMaterial({ color: 0xffd9a0, transparent: true, opacity: 0.85,
      blending: THREE.AdditiveBlending, depthWrite: false });
    const unit = new THREE.BoxGeometry(1, 1, 1).translate(0, 0, -0.5);
    for (let i = 0; i < 64; i++) {
      const mesh = new THREE.Mesh(unit, tracerMat);
      mesh.visible = false;
      mesh.frustumCulled = false;
      scene.add(mesh);
      this.tracers.push({ mesh, from: new THREE.Vector3(), dir: new THREE.Vector3(), dist: 0, travelled: 0, speed: 0, alive: false });
    }
    for (let i = 0; i < 160; i++) {
      const sprite = new THREE.Sprite(new THREE.SpriteMaterial({ map: this.softTex, transparent: true, depthWrite: false }));
      sprite.visible = false;
      scene.add(sprite);
      this.particles.push({ sprite, vel: new THREE.Vector3(), life: 1, age: 0, grow: 0, size: 0, gravity: 0, alpha: 1, alive: false });
    }
    const flashMat = new THREE.MeshBasicMaterial({ map: this.flashTex, transparent: true, blending: THREE.AdditiveBlending,
      depthWrite: false, side: THREE.DoubleSide });
    const plane = new THREE.PlaneGeometry(1, 1);
    for (let i = 0; i < 12; i++) {
      const mesh = new THREE.Group();
      const a = new THREE.Mesh(plane, flashMat);
      const b = new THREE.Mesh(plane, flashMat);
      b.rotation.y = Math.PI / 2;
      b.scale.set(1.6, 0.55, 1);
      b.position.z = 0.35;
      const c = new THREE.Mesh(plane, flashMat);
      c.rotation.x = Math.PI / 2;
      c.scale.set(0.55, 1.6, 1);
      c.position.z = 0.35;
      mesh.add(a, b, c);
      mesh.visible = false;
      scene.add(mesh);
      this.flashes.push({ mesh, age: 0, alive: false });
    }
    const holeMat = new THREE.MeshBasicMaterial({ map: holeTexture(), transparent: true, depthWrite: false,
      polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 });
    const holeGeo = new THREE.PlaneGeometry(1, 1);
    for (let i = 0; i < 220; i++) {
      const m = new THREE.Mesh(holeGeo, holeMat);
      m.visible = false;
      scene.add(m);
      this.holes.push(m);
    }
    // a fixed set of muzzle lights that stay in the scene (intensity 0 when idle)
    for (let i = 0; i < 3; i++) {
      const light = new THREE.PointLight(0xffb060, 0, 9, 2);
      scene.add(light);
      this.lights.push({ light, age: 1 });
    }
  }

  /** A visible round from `from` to `to` (stops where the bullet stopped). */
  tracer(from: THREE.Vector3, to: THREE.Vector3, speed: number) {
    const t = this.tracers.find((x) => !x.alive) ?? this.tracers[0]!;
    t.from.copy(from);
    t.dir.subVectors(to, from);
    t.dist = t.dir.length();
    if (t.dist < 0.5) return;
    t.dir.divideScalar(t.dist);
    t.travelled = 0;
    t.speed = Math.min(speed, 600); // slowed down a little so the eye can catch it
    t.alive = true;
    t.mesh.visible = true;
  }

  muzzleFlash(at: THREE.Vector3, dir: THREE.Vector3, size: number) {
    const f = this.flashes.find((x) => !x.alive) ?? this.flashes[0]!;
    f.mesh.position.copy(at);
    f.mesh.lookAt(at.clone().add(dir));
    f.mesh.rotateZ(Math.random() * Math.PI);
    f.mesh.scale.setScalar(size * (0.8 + Math.random() * 0.4));
    f.age = 0;
    f.alive = true;
    f.mesh.visible = true;
    const l = this.lights[this.lightNext++ % this.lights.length]!;
    l.light.position.copy(at);
    l.age = 0;
    // a little smoke
    this.puff(at.clone().addScaledVector(dir, 0.1), dir.clone().multiplyScalar(0.6), 0x9a958c, 0.08, 0.5, 0.9, 0.35);
  }

  private puff(at: THREE.Vector3, vel: THREE.Vector3, color: number, size: number, grow: number, life: number, alpha: number, gravity = -0.2) {
    const p = this.particles.find((x) => !x.alive);
    if (!p) return;
    p.sprite.position.copy(at);
    p.vel.copy(vel);
    p.size = size;
    p.grow = grow;
    p.life = life;
    p.age = 0;
    p.alpha = alpha;
    p.gravity = gravity;
    p.alive = true;
    const m = p.sprite.material;
    m.color.setHex(color);
    m.blending = gravity > 1 ? THREE.AdditiveBlending : THREE.NormalBlending;
    m.rotation = Math.random() * Math.PI * 2;
    p.sprite.visible = true;
  }

  impact(p: THREE.Vector3, n: THREE.Vector3, mat: number) {
    // bullet hole
    const h = this.holes[this.holeNext++ % this.holes.length]!;
    h.position.copy(p).addScaledVector(n, 0.004);
    h.lookAt(p.clone().add(n));
    h.rotateZ(Math.random() * Math.PI);
    h.scale.setScalar(METAL.has(mat) ? 0.025 : 0.045);
    h.visible = true;
    // dust or sparks
    const base = DUST[mat] ?? DUST[0]!;
    for (let i = 0; i < 3; i++) {
      const v = n.clone().multiplyScalar(0.8 + Math.random() * 1.2)
        .add(new THREE.Vector3(Math.random() - 0.5, Math.random() * 0.5, Math.random() - 0.5).multiplyScalar(0.8));
      this.puff(p.clone().addScaledVector(n, 0.03), v, base, 0.06, 0.6 + Math.random() * 0.4, 0.5 + Math.random() * 0.5, 0.55);
    }
    if (METAL.has(mat) || Math.random() < 0.15) {
      for (let i = 0; i < 5; i++) {
        const v = n.clone().multiplyScalar(2 + Math.random() * 3)
          .add(new THREE.Vector3(Math.random() - 0.5, Math.random(), Math.random() - 0.5).multiplyScalar(3));
        this.puff(p, v, 0xffc070, 0.025, 0, 0.18 + Math.random() * 0.15, 1, 9.8);
      }
    }
  }

  blood(p: THREE.Vector3, dir: THREE.Vector3) {
    for (let i = 0; i < 4; i++) {
      const v = dir.clone().multiplyScalar(0.8 + Math.random())
        .add(new THREE.Vector3(Math.random() - 0.5, Math.random() - 0.2, Math.random() - 0.5).multiplyScalar(0.7));
      this.puff(p, v, 0x5a0a08, 0.05, 0.35, 0.3 + Math.random() * 0.2, 0.75, 2);
    }
  }

  update(dt: number) {
    const cam = this.camera.position;
    for (const t of this.tracers) {
      if (!t.alive) continue;
      t.travelled += t.speed * dt;
      if (t.travelled >= t.dist) { t.alive = false; t.mesh.visible = false; continue; }
      const len = Math.min(t.travelled, 9);
      const head = t.from.clone().addScaledVector(t.dir, t.travelled);
      t.mesh.position.copy(head);
      t.mesh.lookAt(head.clone().add(t.dir));
      const w = Math.max(0.012, head.distanceTo(cam) * 0.0011);
      t.mesh.scale.set(w, w, len);
    }
    for (const f of this.flashes) {
      if (!f.alive) continue;
      f.age += dt;
      if (f.age > 0.05) { f.alive = false; f.mesh.visible = false; }
    }
    for (const l of this.lights) {
      l.age += dt;
      l.light.intensity = l.age < 0.06 ? 12 * (1 - l.age / 0.06) : 0;
    }
    for (const p of this.particles) {
      if (!p.alive) continue;
      p.age += dt;
      if (p.age >= p.life) { p.alive = false; p.sprite.visible = false; continue; }
      const k = p.age / p.life;
      p.vel.y -= p.gravity * dt;
      p.vel.multiplyScalar(Math.exp(-(p.gravity > 1 ? 1 : 3) * dt));
      p.sprite.position.addScaledVector(p.vel, dt);
      p.sprite.scale.setScalar(p.size + p.grow * k);
      p.sprite.material.opacity = p.alpha * (1 - k) * (1 - k);
    }
  }
}
