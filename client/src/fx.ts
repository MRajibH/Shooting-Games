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
interface Casing { mesh: THREE.Mesh; vel: THREE.Vector3; spin: THREE.Vector3; floor: number; age: number; bounced: boolean; alive: boolean; shell: boolean }

export class Fx {
  private tracers: Tracer[] = [];
  private particles: Particle[] = [];
  private flashes: Flash[] = [];
  private holes: THREE.Mesh[] = [];
  private holeNext = 0;
  private lights: { light: THREE.PointLight; age: number }[] = [];
  private casings: Casing[] = [];
  private teeth: Casing[] = [];
  private toothNext = 0;
  private splats: THREE.Mesh[] = [];
  private splatNext = 0;
  /** Called when a knocked-out tooth first hits the ground. */
  onTooth: (at: THREE.Vector3) => void = () => {};
  private casingNext = 0;
  /** Called when a casing first hits the ground (for the tink sound). */
  onCasing: (at: THREE.Vector3, shell: boolean) => void = () => {};
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
    // spent brass (rifle / SMG) and red plastic shells (shotgun)
    const brass = new THREE.MeshStandardMaterial({ color: 0xc9a043, metalness: 0.9, roughness: 0.3 });
    const shellMat = new THREE.MeshStandardMaterial({ color: 0x8a1f1a, roughness: 0.6 });
    const caseGeo = new THREE.CylinderGeometry(0.0048, 0.0048, 0.045, 8);
    const shellGeo = new THREE.CylinderGeometry(0.011, 0.011, 0.065, 10);
    for (let i = 0; i < 48; i++) {
      const shell = i % 6 === 5;
      const mesh = new THREE.Mesh(shell ? shellGeo : caseGeo, shell ? shellMat : brass);
      mesh.visible = false;
      mesh.castShadow = true;
      scene.add(mesh);
      this.casings.push({ mesh, vel: new THREE.Vector3(), spin: new THREE.Vector3(), floor: 0, age: 0, bounced: false, alive: false, shell });
    }
    // teeth: a small crown and root, ivory
    const enamel = new THREE.MeshStandardMaterial({ color: 0xeee6d2, roughness: 0.25, metalness: 0 });
    const tooth = new THREE.CylinderGeometry(0.0045, 0.0028, 0.016, 7);
    for (let i = 0; i < 24; i++) {
      const mesh = new THREE.Mesh(tooth, enamel);
      mesh.visible = false;
      mesh.castShadow = true;
      scene.add(mesh);
      this.teeth.push({ mesh, vel: new THREE.Vector3(), spin: new THREE.Vector3(), floor: 0, age: 0, bounced: false, alive: false, shell: false });
    }
    // blood stains on the ground
    const stainTex = (() => {
      const s = 64;
      const c = document.createElement('canvas');
      c.width = c.height = s;
      const g = c.getContext('2d')!;
      for (let k = 0; k < 9; k++) {
        const r = k === 0 ? s * 0.3 : s * (0.05 + Math.random() * 0.1);
        const x = k === 0 ? s / 2 : s / 2 + (Math.random() - 0.5) * s * 0.7;
        const y = k === 0 ? s / 2 : s / 2 + (Math.random() - 0.5) * s * 0.7;
        const gr = g.createRadialGradient(x, y, 0, x, y, r);
        gr.addColorStop(0, 'rgba(80,4,3,0.95)');
        gr.addColorStop(0.7, 'rgba(70,4,3,0.85)');
        gr.addColorStop(1, 'rgba(60,3,2,0)');
        g.fillStyle = gr;
        g.beginPath();
        g.arc(x, y, r, 0, Math.PI * 2);
        g.fill();
      }
      const t = new THREE.CanvasTexture(c);
      t.colorSpace = THREE.SRGBColorSpace;
      return t;
    })();
    const stainGeo = new THREE.PlaneGeometry(1, 1);
    for (let i = 0; i < 40; i++) {
      const m = new THREE.Mesh(stainGeo, new THREE.MeshStandardMaterial({ map: stainTex, transparent: true, depthWrite: false,
        roughness: 0.2, polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 }));
      m.visible = false;
      m.receiveShadow = true;
      scene.add(m);
      this.splats.push(m);
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

  private puff(at: THREE.Vector3, vel: THREE.Vector3, color: number, size: number, grow: number, life: number, alpha: number, gravity = -0.2, additive = false) {
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
    m.blending = additive ? THREE.AdditiveBlending : THREE.NormalBlending;
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
        this.puff(p, v, 0xffc070, 0.025, 0, 0.18 + Math.random() * 0.15, 1, 9.8, true);
      }
    }
  }

  /** Blood and gore on (setting). */
  gore = true;

  blood(p: THREE.Vector3, dir: THREE.Vector3) {
    if (!this.gore) return;
    for (let i = 0; i < 4; i++) {
      const v = dir.clone().multiplyScalar(0.8 + Math.random())
        .add(new THREE.Vector3(Math.random() - 0.5, Math.random() - 0.2, Math.random() - 0.5).multiplyScalar(0.7));
      this.puff(p, v, 0x5a0a08, 0.05, 0.35, 0.3 + Math.random() * 0.2, 0.75, 2);
    }
  }

  /**
   * A slap to the face: a spray of blood thrown along the swing, drops that fall and
   * leave splats on the ground, and teeth knocked out (`teeth` of them).
   */
  slapGore(face: THREE.Vector3, push: THREE.Vector3, floor: number, teeth: number, heavy: boolean) {
    if (!this.gore) return;
    const n = heavy ? 18 : 10;
    for (let i = 0; i < n; i++) {
      const v = push.clone().multiplyScalar(1.5 + Math.random() * 2.5)
        .add(new THREE.Vector3(Math.random() - 0.5, Math.random() * 1.2, Math.random() - 0.5).multiplyScalar(1.4));
      this.puff(face, v, i % 3 ? 0x6e0b08 : 0x9a1410, 0.02 + Math.random() * 0.03, 0.04, 0.5 + Math.random() * 0.4, 0.95, 9.8);
    }
    // a mist that hangs for a moment
    this.puff(face.clone().addScaledVector(push, 0.15), push.clone().multiplyScalar(0.8), 0x7a0d0a, 0.08, 0.35, 0.45, 0.5, 0.5);
    // splats where the drops come down
    const drops = heavy ? 5 : 3;
    for (let i = 0; i < drops; i++) {
      const reach = 0.4 + Math.random() * (heavy ? 1.6 : 1.0);
      const at = face.clone().addScaledVector(push, reach)
        .add(new THREE.Vector3(Math.random() - 0.5, 0, Math.random() - 0.5).multiplyScalar(0.5));
      at.y = floor;
      setTimeout(() => this.splat(at, 0.06 + Math.random() * (heavy ? 0.2 : 0.12)), 250 + Math.random() * 250);
    }
    for (let i = 0; i < teeth; i++) {
      const t = this.teeth[this.toothNext++ % this.teeth.length]!;
      t.mesh.position.copy(face).addScaledVector(push, 0.08);
      t.vel.copy(push).multiplyScalar(2.2 + Math.random() * 1.8)
        .add(new THREE.Vector3((Math.random() - 0.5) * 1.2, 1.5 + Math.random() * 1.5, (Math.random() - 0.5) * 1.2));
      t.spin.set(Math.random() * 40, Math.random() * 40, Math.random() * 40);
      t.floor = floor;
      t.age = 0;
      t.bounced = false;
      t.alive = true;
      t.mesh.visible = true;
    }
  }

  /** A blood stain on the ground. */
  private splat(at: THREE.Vector3, size: number) {
    const m = this.splats[this.splatNext++ % this.splats.length]!;
    m.position.set(at.x, at.y + 0.006, at.z);
    m.rotation.set(-Math.PI / 2, 0, Math.random() * Math.PI * 2);
    m.scale.set(size * (0.8 + Math.random() * 0.5), size, 1);
    m.visible = true;
    m.userData.age = 0;
  }

  /** Throw a spent case out of the ejection port to the right. */
  eject(at: THREE.Vector3, right: THREE.Vector3, floor: number, shell: boolean) {
    const pool = this.casings.filter((c) => c.shell === shell);
    const c = pool.find((x) => !x.alive) ?? pool[this.casingNext++ % pool.length]!;
    c.mesh.position.copy(at);
    c.vel.copy(right).multiplyScalar(1.8 + Math.random()).add(new THREE.Vector3(0, 1.4 + Math.random() * 0.8, 0))
      .add(new THREE.Vector3(Math.random() - 0.5, 0, Math.random() - 0.5).multiplyScalar(0.6));
    c.spin.set(Math.random() * 30, Math.random() * 30, Math.random() * 30);
    c.floor = floor;
    c.age = 0;
    c.bounced = false;
    c.alive = true;
    c.mesh.visible = true;
  }

  update(dt: number) {
    const cam = this.camera.position;
    for (const m of this.splats) {
      if (!m.visible) continue;
      m.userData.age = (m.userData.age as number) + dt;
      const mat = m.material as THREE.MeshStandardMaterial;
      mat.opacity = Math.min(1, Math.max(0, (40 - (m.userData.age as number)) / 10));
      if ((m.userData.age as number) > 40) m.visible = false;
    }
    for (const c of [...this.casings, ...this.teeth]) {
      if (!c.alive) continue;
      c.age += dt;
      const isTooth = this.teeth.includes(c);
      if (c.age > (isTooth ? 25 : 6)) { c.alive = false; c.mesh.visible = false; continue; }
      if (c.mesh.position.y > c.floor + 0.005 || c.vel.y > 0) {
        c.vel.y -= 9.81 * dt;
        c.mesh.position.addScaledVector(c.vel, dt);
        c.mesh.rotation.x += c.spin.x * dt;
        c.mesh.rotation.y += c.spin.y * dt;
        c.mesh.rotation.z += c.spin.z * dt;
        if (c.mesh.position.y <= c.floor + 0.005) {
          c.mesh.position.y = c.floor + 0.005;
          if (!c.bounced) (isTooth ? this.onTooth(c.mesh.position) : this.onCasing(c.mesh.position, c.shell));
          c.bounced = true;
          // bounce, losing most of the energy; lie flat once it stops
          c.vel.y = Math.abs(c.vel.y) > 0.6 ? -c.vel.y * 0.3 : 0;
          c.vel.x *= 0.5;
          c.vel.z *= 0.5;
          c.spin.multiplyScalar(0.5);
          if (c.vel.y === 0) { c.mesh.rotation.x = Math.PI / 2; c.vel.set(0, 0, 0); }
        }
      }
    }
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
